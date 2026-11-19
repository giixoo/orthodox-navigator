//! Developer tools (F2): Base64, URL, JSON and HTML escaping, JWT decode / sign, SHA-256, regex tester,
//! GUID, Lorem ipsum and password generators (F6 makes new ones).
//!
//! Pick a tool on the left; type or paste into Input (or F3 to load a file) and the Output follows.
//! Tab moves between the list, the pattern / secret field, Input and Output.

use crate::app::{draw_key_bar, App, Input};
use crate::dialogs::{self, LineEdit};
use crate::format;
use crate::screen::Screen;
use crate::theme;
use crate::util::{fit, fit_left, n0};
use crate::viewer::{draw_close_mark, is_close_mark};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha384, Sha512};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    B64Enc,
    B64Dec,
    UrlEnc,
    UrlDec,
    JsonEsc,
    JsonUnesc,
    HtmlEsc,
    HtmlUnesc,
    JwtDecode,
    JwtSign,
    Sha256,
    Regex,
    Guid,
    Lorem,
    Password,
}

/// What a tool setting is: a text box, a number box or a check box.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Number,
    Check,
}

/// One setting of a tool, shown above Input.
struct Field {
    label: &'static str,
    kind: Kind,
    default: &'static str,
    /// kept only while ON runs, never written to disk
    secret: bool,
}

const fn text(label: &'static str) -> Field {
    Field { label, kind: Kind::Text, default: "", secret: false }
}

const fn secret(label: &'static str) -> Field {
    Field { label, kind: Kind::Text, default: "", secret: true }
}

const fn number(label: &'static str, default: &'static str) -> Field {
    Field { label, kind: Kind::Number, default, secret: false }
}

const fn check(label: &'static str, on: bool) -> Field {
    Field { label, kind: Kind::Check, default: if on { "1" } else { "0" }, secret: false }
}

const NONE: &[Field] = &[];

/// Does the tool read Input? The generators don't: they get the whole height for Output.
fn uses_input(t: Tool) -> bool {
    !matches!(t, Tool::Guid | Tool::Lorem | Tool::Password)
}

/// Does the tool work on whole texts (rather than a token you type)? These start with the file under the
/// panel cursor in Input.
fn takes_file(t: Tool) -> bool {
    uses_input(t) && !matches!(t, Tool::JwtDecode | Tool::JwtSign)
}

/// Files up to this size are put into Input on their own; F3 loads bigger ones.
const AUTO_LOAD_MAX: u64 = 4 << 20;

/// A file for Input: its text, or for a binary file a note plus the bytes.
fn read_for_input(p: &Path) -> Result<(String, Option<Vec<u8>>), String> {
    let b = std::fs::read(p).map_err(|e| e.to_string())?;
    if b.len() > 16 << 20 {
        return Err("The file is over 16 MB.".into());
    }
    Ok(match String::from_utf8(b) {
        Ok(s) => (s, None),
        Err(e) => (format!("(binary file {}, {} bytes)", p.display(), n0(e.as_bytes().len() as u64)), Some(e.into_bytes())),
    })
}

const TOOLS: [(Tool, &str, &[Field]); 15] = [
    (Tool::B64Enc, "Base64 encode", NONE),
    (Tool::B64Dec, "Base64 decode", NONE),
    (Tool::UrlEnc, "URL encode", NONE),
    (Tool::UrlDec, "URL decode", NONE),
    (Tool::JsonEsc, "String escape (JSON)", NONE),
    (Tool::JsonUnesc, "String unescape (JSON)", NONE),
    (Tool::HtmlEsc, "HTML/XML escape", NONE),
    (Tool::HtmlUnesc, "HTML/XML unescape", NONE),
    (Tool::JwtDecode, "JWT decode / verify", &[secret("Secret (to verify, optional)")]),
    (Tool::JwtSign, "JWT sign (HS256)", &[secret("Secret")]),
    (Tool::Sha256, "SHA-256 hash", NONE),
    (Tool::Regex, "Regex tester", &[text("Pattern"), check("Ignore case", false), check("Multi-line", false)]),
    (Tool::Guid, "GUID generator", &[number("Count", "1"), check("Upper case", false), check("Braces", false), check("No dashes", false)]),
    (Tool::Lorem, "Lorem ipsum", &[number("Count", "3"), check("Words, not paragraphs", false)]),
    (Tool::Password, "Password generator", &[number("Length", "20"), number("Count", "5"), check("Symbols", true), check("No look-alikes (Il1O0o)", false)]),
];

enum Out {
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    /// the tool's n-th setting
    Param(usize),
    Input,
    Output,
}

// ------------------------------------------------------------------ text area

/// A small multi-line text box.
struct Area {
    lines: Vec<Vec<char>>,
    cx: usize,
    cy: usize,
    top: usize,
    left: usize,
}

impl Area {
    fn new(text: &str) -> Self {
        let mut a = Area { lines: vec![], cx: 0, cy: 0, top: 0, left: 0 };
        a.set(text);
        a
    }

    fn set(&mut self, text: &str) {
        self.lines = text.split('\n').map(|l| l.trim_end_matches('\r').chars().collect()).collect();
        self.cx = 0;
        self.cy = 0;
        self.top = 0;
        self.left = 0;
    }

    fn text(&self) -> String {
        self.lines.iter().map(|l| l.iter().collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    fn insert(&mut self, s: &str) {
        let tail = self.lines[self.cy].split_off(self.cx);
        for ch in s.chars() {
            match ch {
                '\r' => {}
                '\n' => {
                    self.cy += 1;
                    self.lines.insert(self.cy, vec![]);
                    self.cx = 0;
                }
                c => {
                    self.lines[self.cy].push(c);
                    self.cx += 1;
                }
            }
        }
        self.lines[self.cy].extend(tail);
    }

    /// Handle a key; returns true if the text changed.
    fn key(&mut self, k: &KeyEvent, page: usize) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let last = self.lines.len() - 1;
        match k.code {
            KeyCode::Up => self.cy = self.cy.saturating_sub(1),
            KeyCode::Down => self.cy = (self.cy + 1).min(last),
            KeyCode::PageUp => self.cy = self.cy.saturating_sub(page),
            KeyCode::PageDown => self.cy = (self.cy + page).min(last),
            KeyCode::Left => {
                if self.cx > 0 {
                    self.cx -= 1;
                } else if self.cy > 0 {
                    self.cy -= 1;
                    self.cx = self.lines[self.cy].len();
                }
            }
            KeyCode::Right => {
                if self.cx < self.lines[self.cy].len() {
                    self.cx += 1;
                } else if self.cy < last {
                    self.cy += 1;
                    self.cx = 0;
                }
            }
            KeyCode::Home if ctrl => (self.cy, self.cx) = (0, 0),
            KeyCode::End if ctrl => (self.cy, self.cx) = (last, self.lines[last].len()),
            KeyCode::Home => self.cx = 0,
            KeyCode::End => self.cx = self.lines[self.cy].len(),
            KeyCode::Enter => {
                self.insert("\n");
                return true;
            }
            KeyCode::Backspace => {
                if self.cx > 0 {
                    self.cx -= 1;
                    self.lines[self.cy].remove(self.cx);
                } else if self.cy > 0 {
                    let line = self.lines.remove(self.cy);
                    self.cy -= 1;
                    self.cx = self.lines[self.cy].len();
                    self.lines[self.cy].extend(line);
                } else {
                    return false;
                }
                return true;
            }
            KeyCode::Delete => {
                if self.cx < self.lines[self.cy].len() {
                    self.lines[self.cy].remove(self.cx);
                } else if self.cy < last {
                    let line = self.lines.remove(self.cy + 1);
                    self.lines[self.cy].extend(line);
                } else {
                    return false;
                }
                return true;
            }
            // Ctrl+Y clears everything
            KeyCode::Char('y') if ctrl && !alt => {
                self.set("");
                return true;
            }
            KeyCode::Char(c) if !ctrl || alt => {
                self.insert(&c.to_string());
                return true;
            }
            _ => {}
        }
        self.cx = self.cx.min(self.lines[self.cy].len());
        false
    }

    /// Draw; `marks` are char ranges (in the joined text) shown highlighted.
    fn draw(&mut self, s: &mut Screen, x: i32, y: i32, w: i32, h: i32, focused: bool, marks: &[(usize, usize)]) {
        let t = theme::current();
        let (w_, h_) = (w.max(1) as usize, h.max(1) as usize);
        if self.cy < self.top {
            self.top = self.cy;
        }
        if self.cy >= self.top + h_ {
            self.top = self.cy + 1 - h_;
        }
        if self.cx < self.left {
            self.left = self.cx;
        }
        if self.cx >= self.left + w_ {
            self.left = self.cx + 1 - w_;
        }
        let mut off: usize = self.lines[..self.top.min(self.lines.len())].iter().map(|l| l.len() + 1).sum();
        for r in 0..h_ {
            let ry = y + r as i32;
            s.fill(x, ry, w, 1, ' ', t.file, t.panel_bg);
            let Some(line) = self.lines.get(self.top + r) else { continue };
            for (i, &ch) in line.iter().enumerate().skip(self.left).take(w_) {
                let g = off + i;
                let hit = marks.iter().any(|&(a, b)| g >= a && g < b);
                let (fg, bg) = if hit { (t.cursor_fg, t.cursor_bg) } else { (t.file, t.panel_bg) };
                s.put(x + (i - self.left) as i32, ry, if ch == '\t' { ' ' } else { ch }, fg, bg);
            }
            off += line.len() + 1;
        }
        if focused {
            s.cursor = Some((x + (self.cx - self.left) as i32, y + (self.cy - self.top) as i32));
        }
    }
}

// ------------------------------------------------------------------ the tools

pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            '=' => break,
            c if c.is_whitespace() => continue,
            c => return Err(format!("'{c}' is not a Base64 character.")),
        };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

fn base64url(bytes: &[u8]) -> String {
    crate::util::base64(bytes).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn url_decode(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3).ok_or("A '%' at the end is missing its two hex digits.")?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| format!("'%{hex}' is not a valid escape."))?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "The decoded bytes are not UTF-8 text.".into())
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_unescape(s: &str) -> Result<String, String> {
    let t = s.trim();
    let t = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t);
    let c: Vec<char> = t.chars().collect();
    let mut out = String::with_capacity(t.len());
    let mut i = 0;
    let hex4 = |i: usize| -> Result<u32, String> {
        let h: String = c.get(i..i + 4).ok_or("\\u needs four hex digits.")?.iter().collect();
        u32::from_str_radix(&h, 16).map_err(|_| format!("\\u{h} is not valid."))
    };
    while i < c.len() {
        if c[i] != '\\' {
            out.push(c[i]);
            i += 1;
            continue;
        }
        let e = *c.get(i + 1).ok_or("A '\\' at the end has nothing to escape.")?;
        i += 2;
        match e {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            '0' => out.push('\0'),
            'u' => {
                let mut v = hex4(i)?;
                i += 4;
                // surrogate pair
                if (0xD800..0xDC00).contains(&v) && c.get(i) == Some(&'\\') && c.get(i + 1) == Some(&'u') {
                    let lo = hex4(i + 2)?;
                    v = 0x10000 + ((v - 0xD800) << 10) + (lo - 0xDC00);
                    i += 6;
                }
                out.push(char::from_u32(v).unwrap_or('\u{fffd}'));
            }
            other => out.push(other), // \" \\ \/ and anything else as itself
        }
    }
    Ok(out)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn html_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let end = rest[..rest.len().min(12)].find(';');
        let decoded = end.and_then(|e| {
            let ent = &rest[1..e];
            let ch = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => ent
                    .strip_prefix("#x")
                    .or_else(|| ent.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            ch.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn hmac_sign(alg: &str, key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    Some(match alg {
        "HS256" => {
            let mut m = Hmac::<Sha256>::new_from_slice(key).ok()?;
            m.update(data);
            m.finalize().into_bytes().to_vec()
        }
        "HS384" => {
            let mut m = Hmac::<Sha384>::new_from_slice(key).ok()?;
            m.update(data);
            m.finalize().into_bytes().to_vec()
        }
        "HS512" => {
            let mut m = Hmac::<Sha512>::new_from_slice(key).ok()?;
            m.update(data);
            m.finalize().into_bytes().to_vec()
        }
        _ => return None,
    })
}

fn date_of(v: &serde_json::Value) -> Option<String> {
    let secs = v.as_i64()?;
    let t = chrono::DateTime::from_timestamp(secs, 0)?;
    Some(t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S %Z").to_string())
}

fn jwt_decode(token: &str, secret: &str) -> Result<String, String> {
    let token: String = token.split_whitespace().collect();
    let token = token.strip_prefix("Bearer").unwrap_or(&token);
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(format!("A JWT has 3 parts separated by '.', this has {}.", parts.len()));
    }
    let part = |i: usize, name: &str| -> Result<String, String> {
        let bytes = base64_decode(parts[i]).map_err(|e| format!("{name}: {e}"))?;
        String::from_utf8(bytes).map_err(|_| format!("{name} is not text."))
    };
    let header = part(0, "Header")?;
    let payload = part(1, "Payload")?;
    let mut out = format!("Header\n{}\n\nPayload\n{}\n", format::pretty_json(&header)?, format::pretty_json(&payload)?);

    if let Ok(serde_json::Value::Object(p)) = serde_json::from_str::<serde_json::Value>(&payload) {
        let now = chrono::Utc::now().timestamp();
        for (k, label) in [("iat", "Issued"), ("nbf", "Valid from"), ("exp", "Expires")] {
            if let Some(d) = p.get(k).and_then(date_of) {
                out.push_str(&format!("\n{label:<11} {d}"));
                if k == "exp" && p.get(k).and_then(|v| v.as_i64()).is_some_and(|e| e < now) {
                    out.push_str("   EXPIRED");
                }
            }
        }
        out.push('\n');
    }
    let alg = serde_json::from_str::<serde_json::Value>(&header).ok().and_then(|h| h.get("alg")?.as_str().map(str::to_string)).unwrap_or_default();
    out.push_str(&format!("\nAlgorithm   {alg}\n"));
    if secret.is_empty() {
        out.push_str("Signature   not checked (enter the secret above to verify it)");
    } else {
        let signed = format!("{}.{}", parts[0], parts[1]);
        match hmac_sign(&alg, secret.as_bytes(), signed.as_bytes()) {
            Some(sig) if base64url(&sig) == parts[2] => out.push_str("Signature   VALID"),
            Some(_) => out.push_str("Signature   INVALID (wrong secret, or the token was changed)"),
            None => out.push_str(&format!("Signature   can't check {alg} here (only HS256, HS384, HS512)")),
        }
    }
    Ok(out)
}

fn jwt_sign(payload: &str, secret: &str) -> Result<String, String> {
    if secret.is_empty() {
        return Err("Enter the secret above.".into());
    }
    let v: serde_json::Value = serde_json::from_str(payload).map_err(|e| format!("The payload is not valid JSON: {e}"))?;
    if !v.is_object() {
        return Err("The payload must be a JSON object { ... }.".into());
    }
    let compact = format::compact_json(payload);
    let head = base64url(br#"{"alg":"HS256","typ":"JWT"}"#);
    let body = base64url(compact.as_bytes());
    let signed = format!("{head}.{body}");
    let sig = hmac_sign("HS256", secret.as_bytes(), signed.as_bytes()).ok_or("Can't sign.")?;
    Ok(format!("{signed}.{}", base64url(&sig)))
}

/// Byte offset → char offset in `s`.
fn char_offsets(s: &str) -> impl Fn(usize) -> usize + '_ {
    move |b| s[..b].chars().count()
}

fn regex_test(pattern: &str, text: &str, marks: &mut Vec<(usize, usize)>) -> Result<String, String> {
    if pattern.is_empty() {
        return Err("Enter a pattern above, e.g. (?i)(\\w+)@(\\w+)\\.com".into());
    }
    let re = regex::Regex::new(pattern).map_err(|e| e.to_string())?;
    let to_char = char_offsets(text);
    let names: Vec<Option<&str>> = re.capture_names().collect();
    let mut out = String::new();
    let mut n = 0;
    for caps in re.captures_iter(text).take(1000) {
        let m = caps.get(0).unwrap();
        n += 1;
        let line = text[..m.start()].matches('\n').count() + 1;
        let col = m.start() - text[..m.start()].rfind('\n').map_or(0, |p| p + 1);
        let col = text[m.start() - col..m.start()].chars().count() + 1;
        marks.push((to_char(m.start()), to_char(m.end())));
        out.push_str(&format!("{n}. line {line}, col {col}: {:?}\n", m.as_str()));
        for (gi, g) in caps.iter().enumerate().skip(1) {
            let name = names.get(gi).copied().flatten().map_or(format!("${gi}"), |s| format!("${gi} {s}"));
            match g {
                Some(g) => out.push_str(&format!("     {name} = {:?}\n", g.as_str())),
                None => out.push_str(&format!("     {name} (no match)\n")),
            }
        }
    }
    Ok(if n == 0 { "No match.".into() } else { format!("{n} match{}\n\n{out}", if n == 1 { "" } else { "es" }) })
}

// ------------------------------------------------------------------ generators

/// n random bytes from the operating system.
fn random_bytes(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    if getrandom::fill(&mut b).is_err() {
        // never expected; fall back to the std hasher's random keys
        use std::hash::{BuildHasher, Hasher};
        for (i, x) in b.iter_mut().enumerate() {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_usize(i);
            *x = h.finish() as u8;
        }
    }
    b
}

/// A uniform random number in 0..n (n > 0).
fn random_below(n: usize) -> usize {
    let n = n as u32;
    let limit = u32::MAX - u32::MAX % n;
    loop {
        let b = random_bytes(4);
        let v = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        if v < limit {
            return (v % n) as usize;
        }
    }
}

/// A number setting, or `default` when it is empty or not a number; kept within lo..=hi.
fn num(v: &str, default: usize, lo: usize, hi: usize) -> usize {
    v.trim().parse().unwrap_or(default).clamp(lo, hi)
}

/// Setting i of a tool as text ("" if missing).
fn arg(v: &[String], i: usize) -> &str {
    v.get(i).map_or("", |s| s.as_str())
}

/// Is check box i of a tool on?
fn on(v: &[String], i: usize) -> bool {
    v.get(i).is_some_and(|x| x == "1")
}

fn guid() -> String {
    let mut b = random_bytes(16);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn guids(count: usize, upper: bool, braces: bool, nodash: bool) -> String {
    (0..count)
        .map(|_| {
            let mut g = guid();
            if nodash {
                g.retain(|c| c != '-');
            }
            if upper {
                g = g.to_uppercase();
            }
            if braces {
                g = format!("{{{g}}}");
            }
            g
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const LOREM: &str = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ut enim ad minim veniam quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur excepteur sint occaecat cupidatat non proident sunt in culpa qui officia deserunt mollit anim id est laborum curabitur pretium tincidunt lacus nulla gravida orci a odio nullam varius turpis et commodo pharetra est eros bibendum elit nec luctus magna felis sollicitudin mauris integer in mauris eu nibh euismod gravida";

/// Sentences of random Lorem words, `words` long in total; the first starts "Lorem ipsum dolor sit amet".
fn lorem_words(words: usize, first: bool) -> String {
    let dict: Vec<&str> = LOREM.split(' ').collect();
    let mut out = String::new();
    let mut left = words;
    let mut start = first;
    while left > 0 {
        let len = (6 + random_below(10)).min(left);
        let opening = start;
        let mut s: Vec<String> = if start { dict[..5.min(len)].iter().map(|w| w.to_string()).collect() } else { vec![] };
        start = false;
        while s.len() < len {
            s.push(dict[random_below(dict.len())].to_string());
        }
        if s.len() > 5 {
            // the classic opening reads "Lorem ipsum dolor sit amet, ..."
            let comma = if opening { 4 } else { 2 + random_below(s.len() - 3) };
            s[comma].push(',');
        }
        let mut sentence = s.join(" ");
        if let Some(c) = sentence.get(..1) {
            sentence = c.to_uppercase() + &sentence[1..];
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&sentence);
        out.push('.');
        left -= len;
    }
    out
}

/// `count` paragraphs, or `count` words.
fn lorem(count: usize, words: bool) -> String {
    if words {
        return lorem_words(count, true);
    }
    (0..count).map(|i| lorem_words(40 + random_below(40), i == 0)).collect::<Vec<_>>().join("\n\n")
}

/// `count` passwords of `len` characters: letters and digits, symbols if asked, without look-alikes if asked.
fn passwords(len: usize, count: usize, symbols: bool, noamb: bool) -> String {
    let mut sets = vec!["abcdefghijklmnopqrstuvwxyz", "ABCDEFGHIJKLMNOPQRSTUVWXYZ", "0123456789"];
    if symbols {
        sets.push("!@#$%^&*()-_=+[]{};:,.?/~");
    }
    let sets: Vec<Vec<char>> = sets.iter().map(|s| s.chars().filter(|c| !noamb || !"Il1O0o".contains(*c)).collect()).collect();
    let all: Vec<char> = sets.concat();
    let one = || {
        // one character from every set, the rest from all of them, then shuffled
        let mut p: Vec<char> = sets.iter().map(|s| s[random_below(s.len())]).collect();
        while p.len() < len {
            p.push(all[random_below(all.len())]);
        }
        for i in (1..p.len()).rev() {
            p.swap(i, random_below(i + 1));
        }
        p.into_iter().collect::<String>()
    };
    let bits = (len as f64) * (all.len() as f64).log2();
    let list: Vec<String> = (0..count).map(|_| one()).collect();
    format!("{}\n\n{len} characters from {}, about {bits:.0} bits each", list.join("\n"), all.len())
}

fn run(tool: Tool, p: &[String], input: &str, bytes: Option<&[u8]>, marks: &mut Vec<(usize, usize)>) -> Result<Out, String> {
    let raw = bytes.unwrap_or(input.as_bytes());
    Ok(match tool {
        Tool::B64Enc => Out::Text(crate::util::base64(raw)),
        Tool::B64Dec => {
            let b = base64_decode(input)?;
            match String::from_utf8(b) {
                Ok(s) => Out::Text(s),
                Err(e) => Out::Bytes(e.into_bytes()),
            }
        }
        Tool::UrlEnc => Out::Text(url_encode(input)),
        Tool::UrlDec => Out::Text(url_decode(input)?),
        Tool::JsonEsc => Out::Text(json_escape(input)),
        Tool::JsonUnesc => Out::Text(json_unescape(input)?),
        Tool::HtmlEsc => Out::Text(html_escape(input)),
        Tool::HtmlUnesc => Out::Text(html_unescape(input)),
        Tool::JwtDecode => Out::Text(jwt_decode(input, arg(p, 0))?),
        Tool::JwtSign => Out::Text(jwt_sign(input, arg(p, 0))?),
        Tool::Sha256 => {
            let h = Sha256::digest(raw);
            Out::Text(format!("{}\n\n{} bytes", h.iter().map(|b| format!("{b:02x}")).collect::<String>(), n0(raw.len() as u64)))
        }
        Tool::Regex => {
            let flags = format!("{}{}", if on(p, 1) { "i" } else { "" }, if on(p, 2) { "m" } else { "" });
            let pattern = arg(p, 0);
            let full = if flags.is_empty() || pattern.is_empty() { pattern.to_string() } else { format!("(?{flags}){pattern}") };
            Out::Text(regex_test(&full, input, marks)?)
        }
        Tool::Guid => Out::Text(guids(num(arg(p, 0), 1, 1, 10_000), on(p, 1), on(p, 2), on(p, 3))),
        Tool::Lorem => {
            let words = on(p, 1);
            Out::Text(lorem(num(arg(p, 0), 3, 1, if words { 100_000 } else { 1000 }), words))
        }
        Tool::Password => Out::Text(passwords(num(arg(p, 0), 20, 4, 1024), num(arg(p, 1), 5, 1, 10_000), on(p, 2), on(p, 3))),
    })
}

fn out_lines(out: &Result<Out, String>) -> Vec<String> {
    match out {
        Ok(Out::Text(t)) => t.lines().map(str::to_string).collect(),
        Ok(Out::Bytes(b)) => {
            let mut v = vec![format!("Binary data, {} bytes (F5 saves it to a file):", n0(b.len() as u64)), String::new()];
            for (i, chunk) in b.chunks(16).take(4096).enumerate() {
                let hex: String = chunk.iter().map(|x| format!("{x:02X} ")).collect();
                let asc: String = chunk.iter().map(|&x| if (32..127).contains(&x) { x as char } else { '.' }).collect();
                v.push(format!("{:08X}: {hex:<48} {asc}", i * 16));
            }
            v
        }
        Err(e) => e.lines().map(|l| format!("! {l}")).collect(),
    }
}

// ------------------------------------------------------------------ the screen

struct Layout {
    lw: i32,
    /// (x, y, width) of each setting field
    places: Vec<(i32, i32, i32)>,
    in_y: i32,
    in_h: i32,
    out_y: i32,
    out_h: i32,
}

fn field_width(f: &Field) -> i32 {
    let l = f.label.chars().count() as i32;
    match f.kind {
        Kind::Check => l + 4,
        Kind::Number => l + 2 + 7,
        Kind::Text => l + 2 + 20,
    }
}

/// Lay the fields out in rows from (x0, y0), `width` columns wide; text boxes take the spare width.
fn field_places(fields: &[Field], x0: i32, y0: i32, width: i32) -> Vec<(i32, i32, i32)> {
    const GAP: i32 = 3;
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = 0;
    for (i, f) in fields.iter().enumerate() {
        let w = field_width(f);
        match rows.last_mut() {
            Some(row) if used + GAP + w <= width => {
                row.push(i);
                used += GAP + w;
            }
            _ => {
                rows.push(vec![i]);
                used = w;
            }
        }
    }
    let mut out = vec![(0, 0, 0); fields.len()];
    for (r, row) in rows.iter().enumerate() {
        let natural: i32 = row.iter().map(|&i| field_width(&fields[i])).sum::<i32>() + GAP * (row.len() as i32 - 1);
        let spare = (width - natural).max(0);
        let texts = row.iter().filter(|&&i| fields[i].kind == Kind::Text).count() as i32;
        let mut x = x0;
        for &i in row {
            let mut w = field_width(&fields[i]);
            if fields[i].kind == Kind::Text && texts > 0 {
                w += spare / texts;
            }
            let w = w.min(width);
            out[i] = (x, y0 + r as i32, w);
            x += w + GAP;
        }
    }
    out
}

fn layout(w: i32, h: i32, fields: &[Field], input: bool) -> Layout {
    let lw = 26.min(w / 3);
    let places = field_places(fields, lw + 1, 2, w - lw - 2);
    let top = places.iter().map(|p| p.1 + 2).max().unwrap_or(1);
    let avail = (h - 1 - top).max(6);
    let in_h = if input { avail / 2 } else { 0 };
    Layout { lw, places, in_y: top, in_h, out_y: top + in_h, out_h: avail - in_h }
}

// ------------------------------------------------------------------ settings

/// Secrets typed this run, by tool and field.
static SECRETS: std::sync::Mutex<Vec<Vec<String>>> = std::sync::Mutex::new(Vec::new());

fn settings_file() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    dirs::config_dir().map(|d| d.join("orthodox-navigator").join("tools.txt"))
}

/// Every tool's settings: from tools.txt ("Tool name.Field label=value"), defaults, and this run's secrets.
fn load_settings() -> Vec<Vec<String>> {
    let text = settings_file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    let secrets = SECRETS.lock().map(|s| s.clone()).unwrap_or_default();
    TOOLS
        .iter()
        .enumerate()
        .map(|(i, (_, name, fields))| {
            fields
                .iter()
                .enumerate()
                .map(|(j, f)| {
                    if f.secret {
                        return secrets.get(i).and_then(|v| v.get(j)).cloned().unwrap_or_default();
                    }
                    let key = format!("{name}.{}=", f.label);
                    text.lines().find_map(|l| l.strip_prefix(&key)).map_or(f.default.to_string(), str::to_string)
                })
                .collect()
        })
        .collect()
}

fn save_settings(values: &[Vec<String>]) {
    if let Ok(mut s) = SECRETS.lock() {
        *s = values.to_vec();
    }
    let Some(f) = settings_file() else { return };
    let mut text = String::new();
    for ((_, name, fields), vals) in TOOLS.iter().zip(values) {
        for (f, v) in fields.iter().zip(vals).filter(|(f, _)| !f.secret) {
            text.push_str(&format!("{name}.{}={v}\n", f.label));
        }
    }
    let _ = std::fs::create_dir_all(f.parent().unwrap_or(&f));
    let _ = std::fs::write(f, text);
}

fn values(edits: &[LineEdit]) -> Vec<String> {
    edits.iter().map(LineEdit::value).collect()
}

fn toggle(ed: &mut LineEdit) {
    let v = if ed.value() == "1" { "0" } else { "1" };
    ed.set(v);
}

/// Open the tools; `file` is the file under the panel cursor. The text tools start with it in Input (until
/// you type or load something else), and F3 offers it.
pub fn show(app: &mut App, file: Option<PathBuf>, dir: &Path) {
    let mut sel = 0usize;
    let mut focus = Focus::List;
    // every tool keeps its own settings (pattern, secret, generator options)
    let mut edits: Vec<Vec<LineEdit>> = load_settings().iter().map(|v| v.iter().map(|s| LineEdit::new(s)).collect()).collect();
    let mut saved: Vec<Vec<String>> = edits.iter().map(|e| values(e)).collect();
    let mut area = Area::new("");
    let mut bytes: Option<Vec<u8>> = None; // a binary file loaded with F3
    let mut out_top = 0usize;
    let mut marks = Vec::new();
    let mut out = run(TOOLS[sel].0, &values(&edits[sel]), "", None, &mut marks);
    let mut dirty = false;
    // Input holds the file under the cursor, put there on its own; `manual` once you change Input yourself
    let mut auto: Option<String> = None;
    let mut manual = false;

    loop {
        // the text tools get the file under the cursor; the others don't keep it
        if !manual {
            if let Some(f) = file.as_ref() {
                let want = takes_file(TOOLS[sel].0);
                if want && auto.is_none() {
                    let small = std::fs::metadata(f).is_ok_and(|m| m.len() <= AUTO_LOAD_MAX);
                    match read_for_input(f).ok().filter(|_| small) {
                        Some((t, b)) => {
                            area.set(&t);
                            bytes = b;
                            auto = Some(f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                            dirty = true;
                        }
                        None => manual = true, // too big or unreadable: leave Input to you (F3)
                    }
                } else if !want && auto.is_some() {
                    area.set("");
                    bytes = None;
                    auto = None;
                    dirty = true;
                }
            }
        }
        let now: Vec<Vec<String>> = edits.iter().map(|e| values(e)).collect();
        if now != saved {
            save_settings(&now);
            saved = now;
        }
        if dirty {
            marks.clear();
            out = run(TOOLS[sel].0, &values(&edits[sel]), &area.text(), bytes.as_deref(), &mut marks);
            out_top = 0;
            dirty = false;
        }
        let fields = TOOLS[sel].2;
        let input_on = uses_input(TOOLS[sel].0);
        let lay = layout(app.screen.w, app.screen.h, fields, input_on);
        let lines = out_lines(&out);
        let failed = out.is_err();
        let (cur_sel, cur_focus, cur_top) = (sel, focus, out_top);
        let (area_ref, edits_ref, marks_ref, lines_ref) = (&mut area, &mut edits[sel], &marks, &lines);
        let binary = bytes.is_some();
        let auto_ref = &auto;
        let inp = app.window_input(
            &mut |s| {
                let t = theme::current();
                let (w, h) = (s.w, s.h);
                s.cursor = None;
                s.fill(0, 0, w, h - 1, ' ', t.file, t.panel_bg);
                s.fill(0, 0, w, 1, ' ', theme::current().bar_fg, theme::current().bar_bg);
                draw_close_mark(s);
                s.text(4, 0, "Developer tools", theme::current().bar_fg, theme::current().bar_bg);
                // tool list
                s.boxed(0, 1, lay.lw, h - 2, t.frame, t.panel_bg, cur_focus == Focus::List);
                s.text(2, 1, " Tool ", t.header, t.panel_bg);
                for (i, (_, name, _)) in TOOLS.iter().enumerate() {
                    let on = i == cur_sel;
                    let (fg, bg) = if on { (t.cursor_fg, t.cursor_bg) } else { (t.file, t.panel_bg) };
                    s.text(1, 2 + i as i32, &fit(&format!(" {name}"), (lay.lw - 2) as usize), fg, bg);
                }
                let (rx, rw) = (lay.lw, w - lay.lw);
                // the tool's settings: text / number boxes and check boxes
                let mut cursor = None;
                for (j, (f, &(fx, fy, fw))) in fields.iter().zip(&lay.places).enumerate() {
                    let focused = cur_focus == Focus::Param(j);
                    let ed = &mut edits_ref[j];
                    if f.kind == Kind::Check {
                        let mark = if ed.value() == "1" { "[x]" } else { "[ ]" };
                        s.text(fx, fy, mark, t.input_fg, t.input_bg);
                        s.text(fx + 4, fy, &fit(f.label, (fw - 4).max(0) as usize), t.header, t.panel_bg);
                        if focused {
                            cursor = Some((fx + 1, fy));
                        }
                    } else {
                        let lab = format!("{}: ", f.label);
                        let ll = lab.chars().count() as i32;
                        s.text(fx, fy, &lab, t.header, t.panel_bg);
                        ed.draw(s, fx + ll, fy, fw - ll, t.input_fg, t.input_bg);
                        if focused {
                            cursor = s.cursor;
                        }
                    }
                }
                s.cursor = cursor;
                // input
                if lay.in_h > 0 {
                    s.boxed(rx, lay.in_y, rw, lay.in_h, t.frame, t.panel_bg, cur_focus == Focus::Input);
                    let ititle = match (auto_ref, binary) {
                        (Some(n), true) => format!(" Input: {n} (binary) "),
                        (Some(n), false) => format!(" Input: {n} "),
                        (None, true) => " Input (binary file loaded) ".into(),
                        (None, false) => " Input ".into(),
                    };
                    let ititle = fit(&ititle, (rw - 4).max(0) as usize);
                    s.text(rx + 2, lay.in_y, &ititle, t.header, t.panel_bg);
                    area_ref.draw(s, rx + 1, lay.in_y + 1, rw - 2, lay.in_h - 2, cur_focus == Focus::Input, marks_ref);
                }
                // output
                s.boxed(rx, lay.out_y, rw, lay.out_h, t.frame, t.panel_bg, cur_focus == Focus::Output);
                s.text(rx + 2, lay.out_y, " Output ", t.header, t.panel_bg);
                let oh = (lay.out_h - 2).max(0) as usize;
                for (r, l) in lines_ref.iter().skip(cur_top).take(oh).enumerate() {
                    let fg = if failed { t.error } else { t.file };
                    s.text(rx + 1, lay.out_y + 1 + r as i32, &fit(l, (rw - 2) as usize), fg, t.panel_bg);
                }
                if lines_ref.len() > oh {
                    let info = format!(" {}/{} ", (cur_top + oh).min(lines_ref.len()), lines_ref.len());
                    s.text(rx + rw - 2 - info.chars().count() as i32, lay.out_y + lay.out_h - 1, &info, t.frame, t.panel_bg);
                }
                draw_key_bar(s, &["Help", "Copy", if input_on { "Load" } else { "" }, if input_on { "Swap" } else { "" }, "Save", "New", "", "", "", ""]);
            },
            None,
        );

        let page = (lay.in_h - 2).max(1) as usize;
        let out_page = (lay.out_h - 2).max(1) as usize;
        match inp {
            Input::Paste(t) => {
                match focus {
                    Focus::Param(j) if fields[j].kind != Kind::Check => {
                        let t = t.replace(['\r', '\n'], "");
                        let t: String = if fields[j].kind == Kind::Number { t.chars().filter(|c| c.is_ascii_digit()).collect() } else { t };
                        edits[sel][j].insert(&t);
                    }
                    Focus::Param(_) => {}
                    _ if !input_on => {}
                    _ => {
                        focus = Focus::Input;
                        area.insert(&t);
                        bytes = None;
                        (manual, auto) = (true, None);
                    }
                }
                dirty = true;
            }
            Input::Mouse(m) => {
                let (mx, my) = (m.column as i32, m.row as i32);
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if is_close_mark(m.column, m.row) {
                            return;
                        }
                        if mx < lay.lw {
                            let i = my - 2;
                            if i >= 0 && (i as usize) < TOOLS.len() {
                                sel = i as usize;
                                dirty = true;
                            }
                            focus = Focus::List;
                        } else if let Some(j) = lay.places.iter().position(|&(fx, fy, fw)| my == fy && mx >= fx && mx < fx + fw) {
                            focus = Focus::Param(j);
                            if fields[j].kind == Kind::Check {
                                toggle(&mut edits[sel][j]);
                                dirty = true;
                            }
                        } else if lay.in_h > 0 && my >= lay.in_y && my < lay.in_y + lay.in_h {
                            focus = Focus::Input;
                            let y = area.top + (my - lay.in_y - 1).max(0) as usize;
                            area.cy = y.min(area.lines.len() - 1);
                            area.cx = (area.left + (mx - lay.lw - 1).max(0) as usize).min(area.lines[area.cy].len());
                        } else if my >= lay.out_y {
                            focus = Focus::Output;
                        }
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        let d: isize = if m.kind == MouseEventKind::ScrollUp { -3 } else { 3 };
                        if my >= lay.out_y && mx >= lay.lw {
                            out_top = out_top.saturating_add_signed(d).min(lines.len().saturating_sub(out_page));
                        } else if mx >= lay.lw {
                            area.top = area.top.saturating_add_signed(d).min(area.lines.len().saturating_sub(1));
                            area.cy = area.cy.clamp(area.top, (area.top + page - 1).min(area.lines.len() - 1));
                            area.cx = area.cx.min(area.lines[area.cy].len());
                        }
                    }
                    _ => {}
                }
            }
            Input::Tick => {}
            Input::Key(k) => match k.code {
                KeyCode::Esc => return,
                KeyCode::F(1) => crate::help::show(app, "tools"),
                KeyCode::F(2) => match &out {
                    Ok(Out::Text(t)) => crate::editor::copy(t),
                    Ok(Out::Bytes(_)) => dialogs::message(app, "Copy", "The output is binary; F5 saves it to a file.", false),
                    Err(_) => {}
                },
                KeyCode::F(3) if input_on => {
                    let init = file.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                    if let Some(name) = dialogs::input(app, "Load into Input", "File:", &init) {
                        let p = dir.join(name.trim());
                        match read_for_input(&p) {
                            Ok((t, b)) => {
                                area.set(&t);
                                bytes = b;
                                (manual, auto) = (true, None);
                                dirty = true;
                            }
                            Err(e) => dialogs::message(app, "Load", &e, true),
                        }
                    }
                }
                KeyCode::F(4) if input_on => {
                    (manual, auto) = (true, None);
                    match &out {
                        Ok(Out::Text(t)) => {
                            area.set(t);
                            bytes = None;
                        }
                        Ok(Out::Bytes(b)) => {
                            area.set(&format!("(binary data, {} bytes)", n0(b.len() as u64)));
                            bytes = Some(b.clone());
                        }
                        Err(_) => {}
                    }
                    dirty = true;
                }
                KeyCode::F(5) => {
                    if let Some(name) = dialogs::input(app, "Save the output", "File:", "") {
                        let name = name.trim();
                        if !name.is_empty() {
                            let p = dir.join(name);
                            let data = match &out {
                                Ok(Out::Text(t)) => t.clone().into_bytes(),
                                Ok(Out::Bytes(b)) => b.clone(),
                                Err(_) => vec![],
                            };
                            let go = !p.exists()
                                || dialogs::choice(app, "Save", &[&format!("{} exists. Overwrite it?", fit_left(&p.display().to_string(), 50))], &["Overwrite", "Cancel"], true) == Some(0);
                            if go {
                                if let Err(e) = std::fs::write(&p, data) {
                                    dialogs::message(app, "Save", &e.to_string(), true);
                                }
                            }
                        }
                    }
                }
                // the generators make new values; the others just run again
                KeyCode::F(6) => dirty = true,
                KeyCode::Tab | KeyCode::BackTab => {
                    let mut order = vec![Focus::List];
                    order.extend((0..fields.len()).map(Focus::Param));
                    if input_on {
                        order.push(Focus::Input);
                    }
                    order.push(Focus::Output);
                    let i = order.iter().position(|f| *f == focus).unwrap_or(0);
                    let n = order.len();
                    focus = order[if k.code == KeyCode::Tab { (i + 1) % n } else { (i + n - 1) % n }];
                }
                _ => match focus {
                    Focus::List => match k.code {
                        KeyCode::Up => {
                            sel = (sel + TOOLS.len() - 1) % TOOLS.len();
                            dirty = true;
                        }
                        KeyCode::Down => {
                            sel = (sel + 1) % TOOLS.len();
                            dirty = true;
                        }
                        KeyCode::Enter => focus = if !fields.is_empty() { Focus::Param(0) } else if input_on { Focus::Input } else { Focus::Output },
                        _ => {}
                    },
                    Focus::Param(j) => {
                        let ed = &mut edits[sel][j];
                        match (fields[j].kind, k.code) {
                            // Space / Enter: tick or untick
                            (Kind::Check, KeyCode::Char(' ') | KeyCode::Enter) => {
                                toggle(ed);
                                dirty = true;
                            }
                            (Kind::Check, _) => {}
                            (_, KeyCode::Enter) => focus = if input_on { Focus::Input } else { Focus::Output },
                            // ↑ ↓ change a number by one
                            (Kind::Number, KeyCode::Up | KeyCode::Down) => {
                                let n = ed.value().trim().parse::<i64>().unwrap_or(0) + if k.code == KeyCode::Up { 1 } else { -1 };
                                ed.set(&n.max(0).to_string());
                                dirty = true;
                            }
                            (Kind::Number, KeyCode::Char(c)) if !c.is_ascii_digit() => {}
                            _ => {
                                if ed.handle(&k) {
                                    dirty = true;
                                }
                            }
                        }
                    }
                    Focus::Input => {
                        if area.key(&k, page) {
                            bytes = None;
                            dirty = true;
                            (manual, auto) = (true, None);
                        }
                    }
                    Focus::Output => match k.code {
                        KeyCode::Up => out_top = out_top.saturating_sub(1),
                        KeyCode::Down => out_top = (out_top + 1).min(lines.len().saturating_sub(out_page)),
                        KeyCode::PageUp => out_top = out_top.saturating_sub(out_page),
                        KeyCode::PageDown => out_top = (out_top + out_page).min(lines.len().saturating_sub(out_page)),
                        KeyCode::Home => out_top = 0,
                        KeyCode::End => out_top = lines.len().saturating_sub(out_page),
                        _ => {}
                    },
                },
            },
        }
        // a different tool keeps the input, so switching tools recomputes
        let input_now = uses_input(TOOLS[sel].0);
        if let Focus::Param(j) = focus {
            if j >= TOOLS[sel].2.len() {
                focus = if input_now { Focus::Input } else { Focus::Output };
            }
        }
        if focus == Focus::Input && !input_now {
            focus = Focus::Output;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generators() {
        let g = guids(3, true, true, false);
        let lines: Vec<&str> = g.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.len() == 38 && l.starts_with('{') && l.as_bytes()[15] == b'4'));
        assert_ne!(lines[0], lines[1]);
        assert_eq!(guids(1, false, false, true).len(), 32);
        let p = passwords(12, 4, false, true);
        let pw: Vec<&str> = p.lines().take(4).collect();
        assert!(pw.iter().all(|x| x.chars().count() == 12 && x.chars().all(|c| c.is_ascii_alphanumeric() && !"Il1O0o".contains(c))));
        assert!(pw.iter().all(|x| x.chars().any(|c| c.is_ascii_digit()) && x.chars().any(|c| c.is_ascii_uppercase())));
        let l = lorem(25, true);
        assert!(l.starts_with("Lorem ipsum dolor sit amet"));
        assert_eq!(l.split_whitespace().count(), 25);
        assert_eq!(lorem(2, false).split("\n\n").count(), 2);
    }

    #[test]
    fn settings_and_fields() {
        // no settings file in tests: every field starts at its default
        let all = load_settings();
        let pw = TOOLS.iter().position(|t| t.0 == Tool::Password).unwrap();
        assert_eq!(all[pw], ["20", "5", "1", "0"]);
        let out = run(Tool::Password, &all[pw], "", None, &mut vec![]).ok().unwrap();
        let Out::Text(t) = out else { panic!() };
        assert_eq!(t.lines().take_while(|l| !l.is_empty()).count(), 5);
        // check boxes share a row with the number boxes when there is room, wrap when not
        let wide = field_places(TOOLS[pw].2, 0, 1, 200);
        assert!(wide.iter().all(|p| p.1 == 1));
        let narrow = field_places(TOOLS[pw].2, 0, 1, 30);
        assert!(narrow.iter().map(|p| p.1).max().unwrap() > 1);
        // regex flags
        let mut marks = vec![];
        let rx = run(Tool::Regex, &["abc".into(), "1".into(), "0".into()], "ABC", None, &mut marks).ok().unwrap();
        assert!(matches!(rx, Out::Text(t) if t.starts_with("1 match")));
    }

    #[test]
    fn base64_roundtrip() {
        let s = "héllo, world ✓";
        assert_eq!(String::from_utf8(base64_decode(&crate::util::base64(s.as_bytes())).unwrap()).unwrap(), s);
        assert_eq!(base64_decode("aGk").unwrap(), b"hi"); // no padding, url-safe ok
    }

    #[test]
    fn escapes() {
        assert_eq!(url_encode("a b&ü"), "a%20b%26%C3%BC");
        assert_eq!(url_decode("a%20b%26%C3%BC").unwrap(), "a b&ü");
        let s = "line\n\"q\"\t\\ ✓";
        assert_eq!(json_unescape(&json_escape(s)).unwrap(), s);
        assert_eq!(json_unescape(r#""😀""#).unwrap(), "😀");
        assert_eq!(html_unescape(&html_escape("<a href=\"x\">'&'</a>")), "<a href=\"x\">'&'</a>");
        assert_eq!(html_unescape("&#65;&#x42;&bogus; & x"), "AB&bogus; & x");
    }

    #[test]
    fn jwt() {
        // jwt.io's example token, secret "your-256-bit-secret"
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let out = jwt_decode(token, "your-256-bit-secret").unwrap();
        assert!(out.contains("\"name\": \"John Doe\""));
        assert!(out.contains("VALID") && !out.contains("INVALID"));
        assert!(jwt_decode(token, "wrong").unwrap().contains("INVALID"));
        let signed = jwt_sign(r#"{ "sub": "1234567890", "name": "John Doe", "iat": 1516239022 }"#, "your-256-bit-secret").unwrap();
        assert_eq!(signed, token);
    }

    #[test]
    fn regex() {
        let mut marks = vec![];
        let out = regex_test(r"(\d+)-(?<x>\w)", "a 12-b\nzz 3-c", &mut marks).unwrap();
        assert!(out.starts_with("2 matches"));
        assert!(out.contains("line 2, col 4"));
        assert!(out.contains("$2 x = \"c\""));
        assert_eq!(marks, vec![(2, 6), (10, 13)]);
    }
}
