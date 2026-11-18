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

const TOOLS: [(Tool, &str, Option<&str>); 15] = [
    (Tool::B64Enc, "Base64 encode", None),
    (Tool::B64Dec, "Base64 decode", None),
    (Tool::UrlEnc, "URL encode", None),
    (Tool::UrlDec, "URL decode", None),
    (Tool::JsonEsc, "String escape (JSON)", None),
    (Tool::JsonUnesc, "String unescape (JSON)", None),
    (Tool::HtmlEsc, "HTML/XML escape", None),
    (Tool::HtmlUnesc, "HTML/XML unescape", None),
    (Tool::JwtDecode, "JWT decode / verify", Some("Secret (to verify, optional)")),
    (Tool::JwtSign, "JWT sign (HS256)", Some("Secret")),
    (Tool::Sha256, "SHA-256 hash", None),
    (Tool::Regex, "Regex tester", Some("Pattern")),
    (Tool::Guid, "GUID generator", Some("Count, upper, braces, nodash")),
    (Tool::Lorem, "Lorem ipsum", Some("Paragraphs, or e.g. 50w for words")),
    (Tool::Password, "Password generator", Some("Length, count, nosym, noamb")),
];

enum Out {
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    Param,
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

/// Words of the option field: the numbers in order, and the rest lower-cased.
fn options(param: &str) -> (Vec<usize>, Vec<String>) {
    let (mut nums, mut words) = (vec![], vec![]);
    for w in param.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()) {
        match w.parse::<usize>() {
            Ok(n) => nums.push(n),
            Err(_) => words.push(w.to_lowercase()),
        }
    }
    (nums, words)
}

fn guid() -> String {
    let mut b = random_bytes(16);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn guids(param: &str) -> String {
    let (nums, words) = options(param);
    let count = nums.first().copied().unwrap_or(1).clamp(1, 10_000);
    let has = |w: &str| words.iter().any(|x| x == w);
    (0..count)
        .map(|_| {
            let mut g = guid();
            if has("nodash") {
                g.retain(|c| c != '-');
            }
            if has("upper") {
                g = g.to_uppercase();
            }
            if has("braces") {
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

fn lorem(param: &str) -> String {
    let p = param.trim().to_lowercase();
    if let Some(n) = p.strip_suffix('w').and_then(|n| n.trim().parse::<usize>().ok()) {
        return lorem_words(n.clamp(1, 100_000), true);
    }
    let paras = options(param).0.first().copied().unwrap_or(3).clamp(1, 1000);
    (0..paras).map(|i| lorem_words(40 + random_below(40), i == 0)).collect::<Vec<_>>().join("\n\n")
}

fn passwords(param: &str) -> String {
    let (nums, words) = options(param);
    let len = nums.first().copied().unwrap_or(20).clamp(4, 1024);
    let count = nums.get(1).copied().unwrap_or(5).clamp(1, 10_000);
    let has = |w: &str| words.iter().any(|x| x == w);
    let mut sets = vec!["abcdefghijklmnopqrstuvwxyz", "ABCDEFGHIJKLMNOPQRSTUVWXYZ", "0123456789"];
    if !has("nosym") {
        sets.push("!@#$%^&*()-_=+[]{};:,.?/~");
    }
    let sets: Vec<Vec<char>> = sets.iter().map(|s| s.chars().filter(|c| !has("noamb") || !"Il1O0o".contains(*c)).collect()).collect();
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

fn run(tool: Tool, param: &str, input: &str, bytes: Option<&[u8]>, marks: &mut Vec<(usize, usize)>) -> Result<Out, String> {
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
        Tool::JwtDecode => Out::Text(jwt_decode(input, param)?),
        Tool::JwtSign => Out::Text(jwt_sign(input, param)?),
        Tool::Sha256 => {
            let h = Sha256::digest(raw);
            Out::Text(format!("{}\n\n{} bytes", h.iter().map(|b| format!("{b:02x}")).collect::<String>(), n0(raw.len() as u64)))
        }
        Tool::Regex => Out::Text(regex_test(param, input, marks)?),
        Tool::Guid => Out::Text(guids(param)),
        Tool::Lorem => Out::Text(lorem(param)),
        Tool::Password => Out::Text(passwords(param)),
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
    param_y: Option<i32>,
    in_y: i32,
    in_h: i32,
    out_y: i32,
    out_h: i32,
}

fn layout(w: i32, h: i32, has_param: bool) -> Layout {
    let lw = 26.min(w / 3);
    let top = if has_param { 3 } else { 1 };
    let avail = (h - 1 - top).max(6);
    let in_h = avail / 2;
    Layout { lw, param_y: has_param.then_some(1), in_y: top, in_h, out_y: top + in_h, out_h: avail - in_h }
}

/// Open the tools; `file` is the file under the panel cursor (offered by F3).
pub fn show(app: &mut App, file: Option<PathBuf>, dir: &Path) {
    let mut sel = 0usize;
    let mut focus = Focus::List;
    let mut param = LineEdit::new("");
    let mut area = Area::new("");
    let mut bytes: Option<Vec<u8>> = None; // a binary file loaded with F3
    let mut out_top = 0usize;
    let mut marks = Vec::new();
    let mut out = run(TOOLS[sel].0, "", "", None, &mut marks);
    let mut dirty = false;

    loop {
        if dirty {
            marks.clear();
            out = run(TOOLS[sel].0, &param.value(), &area.text(), bytes.as_deref(), &mut marks);
            out_top = 0;
            dirty = false;
        }
        let has_param = TOOLS[sel].2.is_some();
        let lay = layout(app.screen.w, app.screen.h, has_param);
        let lines = out_lines(&out);
        let failed = out.is_err();
        let (cur_sel, cur_focus, cur_top) = (sel, focus, out_top);
        let (area_ref, param_ref, marks_ref, lines_ref) = (&mut area, &mut param, &marks, &lines);
        let binary = bytes.is_some();
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
                // pattern / secret
                if let (Some(py), Some(label)) = (lay.param_y, TOOLS[cur_sel].2) {
                    let lab = format!(" {label}: ");
                    let ll = lab.chars().count() as i32;
                    s.text(rx + 1, py, &lab, t.header, t.panel_bg);
                    param_ref.draw(s, rx + 1 + ll, py, rw - ll - 3, t.input_fg, t.input_bg);
                    if cur_focus != Focus::Param {
                        s.cursor = None;
                    }
                }
                // input
                s.boxed(rx, lay.in_y, rw, lay.in_h, t.frame, t.panel_bg, cur_focus == Focus::Input);
                let ititle = if binary { " Input (binary file loaded) " } else { " Input " };
                s.text(rx + 2, lay.in_y, ititle, t.header, t.panel_bg);
                area_ref.draw(s, rx + 1, lay.in_y + 1, rw - 2, lay.in_h - 2, cur_focus == Focus::Input, marks_ref);
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
                draw_key_bar(s, &["Help", "Copy", "Load", "Swap", "Save", "New", "", "", "", ""]);
            },
            None,
        );

        let page = (lay.in_h - 2).max(1) as usize;
        let out_page = (lay.out_h - 2).max(1) as usize;
        match inp {
            Input::Paste(t) => {
                if focus == Focus::Param {
                    param.insert(&t.replace(['\r', '\n'], ""));
                } else {
                    focus = Focus::Input;
                    area.insert(&t);
                    bytes = None;
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
                        } else if lay.param_y == Some(my) {
                            focus = Focus::Param;
                        } else if my >= lay.in_y && my < lay.in_y + lay.in_h {
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
                KeyCode::F(3) => {
                    let init = file.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                    if let Some(name) = dialogs::input(app, "Load into Input", "File:", &init) {
                        let p = dir.join(name.trim());
                        match std::fs::read(&p) {
                            Ok(b) if b.len() > 16 << 20 => dialogs::message(app, "Load", "The file is over 16 MB.", true),
                            Ok(b) => {
                                match String::from_utf8(b) {
                                    Ok(s) => {
                                        area.set(&s);
                                        bytes = None;
                                    }
                                    Err(e) => {
                                        area.set(&format!("(binary file {}, {} bytes)", p.display(), n0(e.as_bytes().len() as u64)));
                                        bytes = Some(e.into_bytes());
                                    }
                                }
                                dirty = true;
                            }
                            Err(e) => dialogs::message(app, "Load", &e.to_string(), true),
                        }
                    }
                }
                KeyCode::F(4) => {
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
                    let order: Vec<Focus> = if TOOLS[sel].2.is_some() {
                        vec![Focus::List, Focus::Param, Focus::Input, Focus::Output]
                    } else {
                        vec![Focus::List, Focus::Input, Focus::Output]
                    };
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
                        KeyCode::Enter => focus = if TOOLS[sel].2.is_some() { Focus::Param } else { Focus::Input },
                        _ => {}
                    },
                    Focus::Param => {
                        if k.code == KeyCode::Enter {
                            focus = Focus::Input;
                        } else if param.handle(&k) {
                            dirty = true;
                        }
                    }
                    Focus::Input => {
                        if area.key(&k, page) {
                            bytes = None;
                            dirty = true;
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
        if focus == Focus::Param && TOOLS[sel].2.is_none() {
            focus = Focus::Input;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generators() {
        let g = guids("3 upper braces");
        let lines: Vec<&str> = g.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.len() == 38 && l.starts_with('{') && l.as_bytes()[15] == b'4'));
        assert_ne!(lines[0], lines[1]);
        assert_eq!(guids("nodash").len(), 32);
        let p = passwords("12 4 nosym noamb");
        let pw: Vec<&str> = p.lines().take(4).collect();
        assert!(pw.iter().all(|x| x.chars().count() == 12 && x.chars().all(|c| c.is_ascii_alphanumeric() && !"Il1O0o".contains(c))));
        assert!(pw.iter().all(|x| x.chars().any(|c| c.is_ascii_digit()) && x.chars().any(|c| c.is_ascii_uppercase())));
        let l = lorem("25w");
        assert!(l.starts_with("Lorem ipsum dolor sit amet"));
        assert_eq!(l.split_whitespace().count(), 25);
        assert_eq!(lorem("2").split("\n\n").count(), 2);
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
