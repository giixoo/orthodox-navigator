//! Built-in editor (F4, Shift+F4 for a new file) with syntax colours.
//!
//! Keeps the file's encoding (UTF-8 with or without BOM, UTF-16, or Windows-1251), line endings (CRLF / LF)
//! and indentation style (tabs or spaces). Undo/redo, selection with Shift or the mouse,
//! clipboard (also sent to the terminal's clipboard with OSC 52), search, go to line.

use crate::app::{draw_key_bars, App, Input};
use crate::dialogs;
use crate::screen::Screen;
use crate::syntax::{self, Lang, State, Tok};
use crate::theme;
use crate::util::fit_left;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, UTF_8, WINDOWS_1251};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

static CLIPBOARD: Mutex<String> = Mutex::new(String::new());
const MAX_FILE: u64 = 16 << 20;
const TAB: usize = 4;

type Pos = (usize, usize); // (line, char index)

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Insert,
    Delete,
    Other,
}

struct Snap {
    lines: Vec<Vec<char>>,
    cx: usize,
    cy: usize,
}

struct Doc {
    path: PathBuf,
    lines: Vec<Vec<char>>,
    enc: &'static Encoding,
    bom: bool,
    crlf: bool,
    final_nl: bool,
    indent_tabs: bool,
    modified: bool,
    cx: usize,
    cy: usize,
    /// remembered visual column for ↑↓
    want: usize,
    top: usize,
    left: usize,
    anchor: Option<Pos>,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    last_edit: Option<(EditKind, usize)>,
    lang: Option<&'static Lang>,
    syn: bool,
    states: Vec<State>,
    search: String,
}

/// Screen column of char index `x` (tabs expand to the next multiple of TAB).
fn vcol(line: &[char], x: usize) -> usize {
    let mut v = 0;
    for &c in &line[..x.min(line.len())] {
        v = if c == '\t' { (v / TAB + 1) * TAB } else { v + 1 };
    }
    v
}

/// Char index at screen column `want`.
fn xcol(line: &[char], want: usize) -> usize {
    let mut v = 0;
    for (i, &c) in line.iter().enumerate() {
        let nv = if c == '\t' { (v / TAB + 1) * TAB } else { v + 1 };
        if nv > want {
            return i;
        }
        v = nv;
    }
    line.len()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn load(path: &Path) -> Result<Doc, String> {
    let mut d = Doc {
        path: path.to_path_buf(),
        lines: vec![vec![]],
        enc: UTF_8,
        bom: false,
        crlf: cfg!(windows),
        final_nl: true,
        indent_tabs: false,
        modified: false,
        cx: 0,
        cy: 0,
        want: 0,
        top: 0,
        left: 0,
        anchor: None,
        undo: vec![],
        redo: vec![],
        last_edit: None,
        lang: None,
        syn: false,
        states: vec![],
        search: String::new(),
    };
    if path.exists() {
        let len = fs::metadata(path).map_err(|e| e.to_string())?.len();
        if len > MAX_FILE {
            return Err(format!("{} is too big for the built-in editor (over 16 MB).\nAlt+F4 opens the external editor.", path.display()));
        }
        let data = fs::read(path).map_err(|e| e.to_string())?;
        let utf16 = crate::util::sniff_utf16(&data);
        if utf16.is_none() && data[..data.len().min(8192)].contains(&0) {
            return Err("This looks like a binary file.\nF3 shows it in hex; Alt+F4 opens the external editor.".into());
        }
        let text = if let Some((enc, bom_len)) = utf16 {
            d.enc = enc;
            d.bom = bom_len > 0;
            enc.decode_without_bom_handling(&data[bom_len..]).0.into_owned()
        } else {
            let (bytes, bom) = match data.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
                Some(rest) => (rest, true),
                None => (&data[..], false),
            };
            d.bom = bom;
            match std::str::from_utf8(bytes) {
                Ok(s) => s.to_string(),
                Err(_) => {
                    d.enc = WINDOWS_1251;
                    WINDOWS_1251.decode_without_bom_handling(bytes).0.into_owned()
                }
            }
        };
        if text.contains('\n') {
            d.crlf = text.contains("\r\n");
        }
        d.final_nl = text.is_empty() || text.ends_with('\n');
        let mut lines: Vec<Vec<char>> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).chars().collect()).collect();
        if text.ends_with('\n') {
            lines.pop();
        }
        if lines.is_empty() {
            lines.push(vec![]);
        }
        let tabs = lines.iter().take(5000).filter(|l| l.first() == Some(&'\t')).count();
        let spaces = lines.iter().take(5000).filter(|l| l.first() == Some(&' ')).count();
        d.indent_tabs = tabs > spaces;
        d.lines = lines;
    }
    let first: String = d.lines[0].iter().collect();
    d.lang = syntax::detect(path, &first);
    d.syn = d.lang.is_some();
    Ok(d)
}

impl Doc {
    fn last_line(&self) -> usize {
        self.lines.len() - 1
    }

    /// Remember the text for undo. Typing (or deleting) on one line is one undo step.
    fn snapshot(&mut self, kind: EditKind) {
        let key = (kind, self.cy);
        if kind == EditKind::Other || self.last_edit != Some(key) {
            self.undo.push(Snap { lines: self.lines.clone(), cx: self.cx, cy: self.cy });
            let cap = if self.lines.len() > 20_000 { 20 } else { 200 };
            if self.undo.len() > cap {
                self.undo.remove(0);
            }
        }
        self.last_edit = if kind == EditKind::Other { None } else { Some(key) };
        self.redo.clear();
        self.modified = true;
    }

    fn restore(&mut self, from_undo: bool) {
        let (src, dst) = if from_undo { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        let Some(s) = src.pop() else { return };
        dst.push(Snap { lines: std::mem::replace(&mut self.lines, s.lines), cx: self.cx, cy: self.cy });
        self.cy = s.cy.min(self.last_line());
        self.cx = s.cx.min(self.lines[self.cy].len());
        self.want = vcol(&self.lines[self.cy], self.cx);
        self.states.clear();
        self.anchor = None;
        self.last_edit = None;
        self.modified = true;
    }

    /// Line `y` changed: syntax states after it must be recomputed.
    fn touched(&mut self, y: usize) {
        self.states.truncate(y + 1);
    }

    fn ensure_states(&mut self, upto: usize) {
        let Some(lang) = self.lang else { return };
        let upto = upto.min(self.last_line());
        if self.states.is_empty() {
            self.states.push(State::Normal);
        }
        let mut toks = Vec::new();
        while self.states.len() <= upto {
            let i = self.states.len() - 1;
            let line: String = self.lines[i].iter().collect();
            let st = syntax::scan(lang, &line, self.states[i], &mut toks);
            self.states.push(st);
        }
    }

    fn sel(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        let b = (self.cy, self.cx);
        if a == b {
            None
        } else if a < b {
            Some((a, b))
        } else {
            Some((b, a))
        }
    }

    fn text_of(&self, a: Pos, b: Pos) -> String {
        if a.0 == b.0 {
            return self.lines[a.0][a.1..b.1].iter().collect();
        }
        let mut s: String = self.lines[a.0][a.1..].iter().collect();
        for y in a.0 + 1..b.0 {
            s.push('\n');
            s.extend(self.lines[y].iter());
        }
        s.push('\n');
        s.extend(self.lines[b.0][..b.1].iter());
        s
    }

    fn remove(&mut self, a: Pos, b: Pos) {
        if a.0 == b.0 {
            self.lines[a.0].drain(a.1..b.1);
        } else {
            let tail = self.lines[b.0][b.1..].to_vec();
            self.lines[a.0].truncate(a.1);
            self.lines[a.0].extend(tail);
            self.lines.drain(a.0 + 1..=b.0);
        }
        self.cy = a.0;
        self.cx = a.1;
        self.touched(a.0);
    }

    fn delete_sel(&mut self) -> bool {
        let r = self.sel();
        self.anchor = None;
        match r {
            Some((a, b)) => {
                self.remove(a, b);
                true
            }
            None => false,
        }
    }

    fn insert(&mut self, s: &str) {
        let y0 = self.cy;
        let tail = self.lines[self.cy].split_off(self.cx);
        for ch in s.chars() {
            match ch {
                '\r' => {}
                '\n' => {
                    self.cy += 1;
                    self.lines.insert(self.cy, Vec::new());
                    self.cx = 0;
                }
                c => {
                    self.lines[self.cy].push(c);
                    self.cx += 1;
                }
            }
        }
        self.lines[self.cy].extend(tail);
        self.want = vcol(&self.lines[self.cy], self.cx);
        self.touched(y0);
    }

    fn move_to(&mut self, shift: bool, y: usize, x: usize, keep_want: bool) {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some((self.cy, self.cx));
            }
        } else {
            self.anchor = None;
        }
        self.cy = y.min(self.last_line());
        self.cx = x.min(self.lines[self.cy].len());
        if !keep_want {
            self.want = vcol(&self.lines[self.cy], self.cx);
        }
        self.last_edit = None;
    }

    fn vertical(&mut self, shift: bool, y: usize) {
        let y = y.min(self.last_line());
        let x = xcol(&self.lines[y], self.want);
        self.move_to(shift, y, x, true);
    }

    fn word_left(&self) -> Pos {
        let (y, mut x) = (self.cy, self.cx);
        if x == 0 {
            return if y > 0 { (y - 1, self.lines[y - 1].len()) } else { (0, 0) };
        }
        let l = &self.lines[y];
        while x > 0 && !is_word(l[x - 1]) {
            x -= 1;
        }
        while x > 0 && is_word(l[x - 1]) {
            x -= 1;
        }
        (y, x)
    }

    fn word_right(&self) -> Pos {
        let (y, mut x) = (self.cy, self.cx);
        let l = &self.lines[y];
        if x >= l.len() {
            return if y < self.last_line() { (y + 1, 0) } else { (y, x) };
        }
        while x < l.len() && is_word(l[x]) {
            x += 1;
        }
        while x < l.len() && !is_word(l[x]) {
            x += 1;
        }
        (y, x)
    }

    /// Lines covered by the selection (or the cursor line).
    fn sel_lines(&self) -> (usize, usize) {
        match self.sel() {
            Some((a, b)) => (a.0, if b.1 == 0 && b.0 > a.0 { b.0 - 1 } else { b.0 }),
            None => (self.cy, self.cy),
        }
    }

    fn indent_lines(&mut self, out: bool) {
        let (y1, y2) = self.sel_lines();
        self.snapshot(EditKind::Other);
        for y in y1..=y2 {
            let l = &mut self.lines[y];
            if out {
                if l.first() == Some(&'\t') {
                    l.remove(0);
                } else {
                    let n = l.iter().take(TAB).take_while(|c| **c == ' ').count();
                    l.drain(..n);
                }
            } else if !l.is_empty() {
                if self.indent_tabs {
                    l.insert(0, '\t');
                } else {
                    l.splice(0..0, std::iter::repeat_n(' ', TAB));
                }
            }
        }
        self.touched(y1);
        if y1 != y2 || self.anchor.is_some() {
            self.anchor = Some((y1, 0));
            self.cy = y2;
            self.cx = self.lines[y2].len();
        } else {
            self.cx = self.cx.min(self.lines[self.cy].len());
        }
        self.want = vcol(&self.lines[self.cy], self.cx);
    }

    fn scroll_into_view(&mut self, rows: usize, width: usize) {
        self.cy = self.cy.min(self.last_line());
        self.cx = self.cx.min(self.lines[self.cy].len());
        if self.cy < self.top {
            self.top = self.cy;
        }
        if self.cy >= self.top + rows {
            self.top = self.cy + 1 - rows;
        }
        let vx = vcol(&self.lines[self.cy], self.cx);
        if vx < self.left {
            self.left = vx.saturating_sub(width / 4);
        }
        if vx >= self.left + width {
            self.left = vx + 1 - width + width / 4;
        }
    }

    fn find_next(&mut self) -> bool {
        let needle: Vec<char> = self.search.chars().flat_map(char::to_lowercase).collect();
        if needle.is_empty() {
            return false;
        }
        let n = self.lines.len();
        for step in 0..=n {
            let y = (self.cy + step) % n;
            let hay: Vec<char> = self.lines[y].iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
            let from = if step == 0 { self.cx } else { 0 };
            let found = (from..=hay.len().saturating_sub(needle.len())).find(|&x| hay.len() >= needle.len() && hay[x..x + needle.len()] == needle[..]);
            if let Some(x) = found {
                self.anchor = Some((y, x));
                self.cy = y;
                self.cx = x + needle.len();
                self.want = vcol(&self.lines[y], self.cx);
                return true;
            }
        }
        false
    }
}

fn save(app: &mut App, d: &mut Doc) -> bool {
    let nl = if d.crlf { "\r\n" } else { "\n" };
    let mut text = d.lines.iter().map(|l| l.iter().collect::<String>()).collect::<Vec<_>>().join(nl);
    if d.final_nl && !(d.lines.len() == 1 && d.lines[0].is_empty()) {
        text.push_str(nl);
    }
    // encoding_rs can't write UTF-16 (it would give UTF-8), so that is done here
    let utf16 = d.enc == UTF_16LE || d.enc == UTF_16BE;
    let (bytes, _, unmappable) = if utf16 { (std::borrow::Cow::Owned(crate::util::encode_utf16(&text, d.enc == UTF_16BE)), d.enc, false) } else { d.enc.encode(&text) };
    let mut bytes = bytes.into_owned();
    if unmappable {
        let q = format!("Some characters can't be saved in {}.", d.enc.name());
        if dialogs::choice(app, "Save", &[&q, "Save the file as UTF-8?"], &["UTF-8", "Cancel"], true) != Some(0) {
            return false;
        }
        d.enc = UTF_8;
        bytes = text.into_bytes();
    }
    let mut out = Vec::with_capacity(bytes.len() + 3);
    if d.bom {
        if d.enc == UTF_8 {
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        } else if d.enc == UTF_16LE {
            out.extend_from_slice(&[0xFF, 0xFE]);
        } else if d.enc == UTF_16BE {
            out.extend_from_slice(&[0xFE, 0xFF]);
        }
    }
    out.extend_from_slice(&bytes);
    match fs::write(&d.path, out) {
        Ok(()) => {
            d.modified = false;
            true
        }
        Err(e) => {
            dialogs::message(app, "Save", &format!("Can't save {}:\n{e}", d.path.display()), true);
            false
        }
    }
}

/// Put text on our clipboard and the system's (Windows), or the terminal's (OSC 52: most Linux terminals).
pub fn copy(text: &str) {
    if let Ok(mut c) = CLIPBOARD.lock() {
        *c = text.to_string();
    }
    if crate::platform::set_clipboard(text) {
        return;
    }
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", crate::util::base64(text.as_bytes()));
    let _ = out.flush();
}

fn draw(s: &mut Screen, d: &Doc, toks: &mut Vec<Tok>, rows: usize) {
    let t = theme::current();
    let hl = if d.syn { d.lang } else { None };
    s.fill(0, 0, s.w, 1, ' ', theme::current().bar_fg, theme::current().bar_bg);
    let enc = if d.enc == UTF_8 {
        if d.bom { "UTF-8 BOM" } else { "UTF-8" }
    } else if d.enc == UTF_16LE {
        "UTF-16LE"
    } else if d.enc == UTF_16BE {
        "UTF-16BE"
    } else {
        "Win-1251"
    };
    let right = format!(
        " {}{} {}  Ln {}/{}  Col {} ",
        hl.map_or(String::new(), |l| format!("{}  ", l.name)),
        enc,
        if d.crlf { "CRLF" } else { "LF" },
        d.cy + 1,
        d.lines.len(),
        vcol(&d.lines[d.cy], d.cx) + 1
    );
    let rlen = right.chars().count() as i32;
    let name = format!("{}{}", if d.modified { "*" } else { " " }, d.path.display());
    crate::viewer::draw_close_mark(s);
    s.text(4, 0, &fit_left(&name, (s.w - rlen - 5).max(4) as usize), theme::current().bar_fg, theme::current().bar_bg);
    s.text(s.w - rlen, 0, &right, theme::current().bar_fg, theme::current().bar_bg);

    let sel = d.sel();
    let width = s.w.max(1) as usize;
    for r in 0..rows {
        let y = d.top + r;
        let sy = r as i32 + 1;
        s.fill(0, sy, s.w, 1, ' ', t.viewer_fg, t.viewer_bg);
        let Some(line) = d.lines.get(y) else { continue };
        if let Some(l) = hl {
            let st = d.states.get(y).copied().unwrap_or_default();
            let text: String = line.iter().collect();
            syntax::scan(l, &text, st, toks);
        }
        let mut v = 0usize;
        for (x, &ch) in line.iter().enumerate() {
            let cw = if ch == '\t' { (v / TAB + 1) * TAB - v } else { 1 };
            let fg = if hl.is_some() { syntax::color(t, toks.get(x).copied().unwrap_or(Tok::Text)) } else { t.viewer_fg };
            let selected = sel.is_some_and(|(a, b)| (y, x) >= a && (y, x) < b);
            let (fg, bg) = if selected { (t.cursor_fg, t.cursor_bg) } else { (fg, t.viewer_bg) };
            for k in 0..cw {
                let vx = v + k;
                if vx >= d.left && vx < d.left + width {
                    s.put((vx - d.left) as i32, sy, if ch == '\t' { ' ' } else { ch }, fg, bg);
                }
            }
            v += cw;
            if v >= d.left + width {
                break;
            }
        }
        // the line break inside a selection
        if let Some((a, b)) = sel {
            let end = (y, line.len());
            if end >= a && end < b && v >= d.left && v < d.left + width {
                s.put((v - d.left) as i32, sy, ' ', t.cursor_fg, t.cursor_bg);
            }
        }
    }
    let vx = vcol(&d.lines[d.cy], d.cx);
    s.cursor = Some(((vx - d.left) as i32, (d.cy - d.top) as i32 + 1));
    let syn_label = if d.lang.is_some() { if d.syn { "Plain" } else { "Syntax" } } else { "" };
    draw_key_bars(
        s,
        &[
            (KeyModifiers::NONE, ["Help", "Save", "", "", "", syn_label, "Search", "", "", ""]),
            (KeyModifiers::SHIFT, ["", "", "", "", "", "", "Next", "", "", ""]),
        ],
    );
}

/// Ask whether to save a modified file. Returns true when the editor may close.
fn confirm_close(app: &mut App, d: &mut Doc, saved: &mut bool) -> bool {
    if !d.modified {
        return true;
    }
    let q = format!("{} has been modified.", d.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    match dialogs::choice(app, "Editor", &[&q, "Save it?"], &["Save", "Don't save", "Cancel"], false) {
        Some(0) => {
            let ok = save(app, d);
            *saved |= ok;
            ok
        }
        Some(1) => true,
        _ => false,
    }
}

/// Edit `path` (it may not exist yet). Returns true if the file was saved.
pub fn edit(app: &mut App, path: &Path) -> bool {
    let mut d = match load(path) {
        Ok(d) => d,
        Err(e) => {
            dialogs::message(app, "Editor", &e, true);
            return false;
        }
    };
    let mut saved = false;
    let mut toks = Vec::new();
    let mut drag_from: Option<Pos> = None;
    loop {
        let rows = (app.screen.h - 2).max(1) as usize;
        let width = app.screen.w.max(1) as usize;
        d.scroll_into_view(rows, width);
        if d.syn {
            d.ensure_states(d.top + rows);
        }
        let dr = &d;
        let tk = &mut toks;
        let inp = app.window_input(&mut |s| draw(s, dr, tk, rows), None);
        // more input already waiting = text is being pasted: no auto-indent
        let pasting = crossterm::event::poll(Duration::ZERO).unwrap_or(false);
        match inp {
            Input::Key(k) => {
                if !key(app, &mut d, k, rows, pasting, &mut saved) {
                    return saved;
                }
            }
            Input::Paste(text) => {
                d.snapshot(EditKind::Other);
                d.delete_sel();
                d.insert(&text);
            }
            Input::Mouse(m) => {
                let (mx, my) = (m.column as usize, m.row as i32);
                let pos_at = |d: &Doc, my: i32| {
                    let y = (d.top as i64 + my as i64 - 1).clamp(0, d.last_line() as i64) as usize;
                    (y, xcol(&d.lines[y], d.left + mx))
                };
                match m.kind {
                    MouseEventKind::ScrollUp => {
                        d.top = d.top.saturating_sub(3);
                        let y = d.cy.saturating_sub(3);
                        d.vertical(false, y);
                    }
                    MouseEventKind::ScrollDown => {
                        d.top = (d.top + 3).min(d.last_line());
                        let y = d.cy + 3;
                        d.vertical(false, y);
                    }
                    MouseEventKind::Down(MouseButton::Left) if crate::viewer::is_close_mark(m.column, m.row) => {
                        if confirm_close(app, &mut d, &mut saved) {
                            return saved;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) if my >= 1 && my <= rows as i32 => {
                        let (y, x) = pos_at(&d, my);
                        let shift = m.modifiers.contains(KeyModifiers::SHIFT);
                        d.move_to(shift, y, x, false);
                        drag_from = if shift { None } else { Some((y, x)) };
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        if let Some(from) = drag_from {
                            let (y, x) = pos_at(&d, my);
                            d.anchor = Some(from);
                            d.cy = y;
                            d.cx = x;
                            d.want = vcol(&d.lines[y], x);
                        }
                    }
                    MouseEventKind::Up(MouseButton::Left) => drag_from = None,
                    _ => {}
                }
            }
            Input::Tick => {}
        }
    }
}

/// Handle one key; returns false when the editor should close.
fn key(app: &mut App, d: &mut Doc, k: KeyEvent, rows: usize, pasting: bool, saved: &mut bool) -> bool {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let shift = k.modifiers.contains(KeyModifiers::SHIFT);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let cmd = ctrl && !alt; // Ctrl+Alt is AltGr (types characters)
    match k.code {
        KeyCode::Esc => return !confirm_close(app, d, saved),
        KeyCode::F(2) => *saved |= save(app, d),
        KeyCode::F(1) => crate::help::show(app, "editor"),
        KeyCode::F(6) => d.syn = d.lang.is_some() && !d.syn,
        KeyCode::F(7) if shift => {
            if !d.find_next() {
                dialogs::message(app, "Search", &format!("\"{}\" not found.", d.search), false);
            }
        }
        KeyCode::F(7) => {
            let init = d.sel().filter(|(a, b)| a.0 == b.0).map(|(a, b)| d.text_of(a, b)).unwrap_or_else(|| d.search.clone());
            if let Some(s) = dialogs::input(app, "Search", "Find:", &init) {
                if !s.is_empty() {
                    d.search = s;
                    if let Some((a, _)) = d.sel() {
                        d.cy = a.0;
                        d.cx = a.1;
                    }
                    if !d.find_next() {
                        dialogs::message(app, "Search", &format!("\"{}\" not found.", d.search), false);
                    }
                }
            }
        }
        KeyCode::Char('g') if cmd => {
            if let Some(s) = dialogs::input(app, "Go to line", &format!("Line (1-{}):", d.lines.len()), "") {
                if let Ok(n) = s.trim().parse::<usize>() {
                    d.move_to(false, n.saturating_sub(1), 0, false);
                    d.top = d.cy.saturating_sub(rows / 2);
                }
            }
        }
        KeyCode::Char('a') if cmd => {
            d.anchor = Some((0, 0));
            d.cy = d.last_line();
            d.cx = d.lines[d.cy].len();
        }
        KeyCode::Char('c') if cmd => copy(&clip_text(d)),
        KeyCode::Char('x') if cmd => cut(d),
        KeyCode::Char('v') if cmd => paste(d),
        KeyCode::Char('z') | KeyCode::Char('Z') if cmd => d.restore(!shift),
        KeyCode::Char('y') if cmd => {
            d.snapshot(EditKind::Other);
            d.anchor = None;
            if d.lines.len() > 1 {
                d.lines.remove(d.cy);
            } else {
                d.lines[0].clear();
            }
            d.cy = d.cy.min(d.last_line());
            d.cx = 0;
            d.touched(d.cy);
        }
        KeyCode::Char('d') if cmd => {
            d.snapshot(EditKind::Other);
            let l = d.lines[d.cy].clone();
            d.lines.insert(d.cy + 1, l);
            d.touched(d.cy);
            d.cy += 1;
        }

        // movement (Shift extends the selection)
        KeyCode::Up => d.vertical(shift, d.cy.saturating_sub(1)),
        KeyCode::Down => d.vertical(shift, d.cy + 1),
        KeyCode::PageUp => {
            d.top = d.top.saturating_sub(rows);
            d.vertical(shift, d.cy.saturating_sub(rows));
        }
        KeyCode::PageDown => {
            d.top = (d.top + rows).min(d.last_line());
            d.vertical(shift, d.cy + rows);
        }
        KeyCode::Left if ctrl => {
            let (y, x) = d.word_left();
            d.move_to(shift, y, x, false);
        }
        KeyCode::Right if ctrl => {
            let (y, x) = d.word_right();
            d.move_to(shift, y, x, false);
        }
        KeyCode::Left => {
            let (y, x) = if d.cx > 0 { (d.cy, d.cx - 1) } else if d.cy > 0 { (d.cy - 1, d.lines[d.cy - 1].len()) } else { (0, 0) };
            d.move_to(shift, y, x, false);
        }
        KeyCode::Right => {
            let (y, x) = if d.cx < d.lines[d.cy].len() { (d.cy, d.cx + 1) } else if d.cy < d.last_line() { (d.cy + 1, 0) } else { (d.cy, d.cx) };
            d.move_to(shift, y, x, false);
        }
        KeyCode::Home if ctrl => d.move_to(shift, 0, 0, false),
        KeyCode::End if ctrl => {
            let y = d.last_line();
            d.move_to(shift, y, d.lines[y].len(), false);
        }
        KeyCode::Home => {
            // first press: start of the text, second: column 1
            let first = d.lines[d.cy].iter().take_while(|c| c.is_whitespace()).count();
            let x = if d.cx == first { 0 } else { first };
            d.move_to(shift, d.cy, x, false);
        }
        KeyCode::End => d.move_to(shift, d.cy, d.lines[d.cy].len(), false),

        // editing
        KeyCode::Enter => {
            d.snapshot(EditKind::Other);
            d.delete_sel();
            let indent: String = if pasting { String::new() } else { d.lines[d.cy].iter().take(d.cx).take_while(|c| **c == ' ' || **c == '\t').collect() };
            d.insert(&format!("\n{indent}"));
        }
        KeyCode::Backspace => {
            if d.sel().is_some() {
                d.snapshot(EditKind::Other);
                d.delete_sel();
            } else if d.cx > 0 {
                d.snapshot(EditKind::Delete);
                let l = &d.lines[d.cy];
                // in leading spaces, go back to the previous indent stop
                let n = if !pasting && l[..d.cx].iter().all(|c| *c == ' ') { (d.cx - 1) % TAB + 1 } else { 1 };
                d.remove((d.cy, d.cx - n), (d.cy, d.cx));
            } else if d.cy > 0 {
                d.snapshot(EditKind::Other);
                let px = d.lines[d.cy - 1].len();
                d.remove((d.cy - 1, px), (d.cy, 0));
            }
            d.want = vcol(&d.lines[d.cy], d.cx);
        }
        KeyCode::Delete => {
            if d.sel().is_some() {
                d.snapshot(EditKind::Other);
                d.delete_sel();
            } else if d.cx < d.lines[d.cy].len() {
                d.snapshot(EditKind::Delete);
                d.remove((d.cy, d.cx), (d.cy, d.cx + 1));
            } else if d.cy < d.last_line() {
                d.snapshot(EditKind::Other);
                d.remove((d.cy, d.cx), (d.cy + 1, 0));
            }
        }
        KeyCode::BackTab => d.indent_lines(true),
        KeyCode::Tab if d.sel().is_some_and(|(a, b)| a.0 != b.0) => d.indent_lines(false),
        KeyCode::Tab => {
            d.snapshot(EditKind::Insert);
            d.delete_sel();
            if d.indent_tabs {
                d.insert("\t");
            } else {
                let n = TAB - vcol(&d.lines[d.cy], d.cx) % TAB;
                d.insert(&" ".repeat(n));
            }
        }
        KeyCode::Char(c) if !ctrl || alt => {
            if d.sel().is_some() {
                d.snapshot(EditKind::Other);
                d.delete_sel();
            } else {
                d.snapshot(EditKind::Insert);
            }
            d.insert(&c.to_string());
        }
        _ => {}
    }
    true
}

/// The selection, or the whole current line when nothing is selected.
fn clip_text(d: &Doc) -> String {
    match d.sel() {
        Some((a, b)) => d.text_of(a, b),
        None => {
            let mut s: String = d.lines[d.cy].iter().collect();
            s.push('\n');
            s
        }
    }
}

fn cut(d: &mut Doc) {
    copy(&clip_text(d));
    d.snapshot(EditKind::Other);
    if !d.delete_sel() {
        if d.lines.len() > 1 {
            d.lines.remove(d.cy);
        } else {
            d.lines[0].clear();
        }
        d.cy = d.cy.min(d.last_line());
        d.cx = 0;
        d.touched(d.cy);
    }
}

fn paste(d: &mut Doc) {
    let text = CLIPBOARD.lock().map(|c| c.clone()).unwrap_or_default();
    if text.is_empty() {
        return;
    }
    d.snapshot(EditKind::Other);
    let replaced = d.delete_sel();
    // a whole copied line goes above the cursor line, like in most editors
    if !replaced && text.ends_with('\n') && !text[..text.len() - 1].contains('\n') {
        d.cx = 0;
    }
    d.insert(&text);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Doc {
        let dir = std::env::temp_dir().join(format!("on-ed-{}-{}", std::process::id(), text.len()));
        fs::write(&dir, text).unwrap();
        let d = load(&dir).unwrap();
        let _ = fs::remove_file(&dir);
        d
    }

    #[test]
    fn insert_remove_undo() {
        let mut d = doc("one\r\ntwo\r\n");
        assert!(d.crlf && d.final_nl);
        assert_eq!(d.lines.len(), 2);
        d.cx = 3;
        d.snapshot(EditKind::Other);
        d.insert("!\nnew");
        assert_eq!(d.lines.len(), 3);
        assert_eq!(d.lines[1].iter().collect::<String>(), "new");
        assert_eq!((d.cy, d.cx), (1, 3));
        d.remove((0, 2), (1, 1));
        assert_eq!(d.lines[0].iter().collect::<String>(), "onew");
        d.restore(true);
        assert_eq!(d.lines.len(), 2);
        assert_eq!(d.lines[0].iter().collect::<String>(), "one");
    }

    #[test]
    fn tabs_and_columns() {
        let l: Vec<char> = "\tab\tc".chars().collect();
        assert_eq!(vcol(&l, 1), 4);
        assert_eq!(vcol(&l, 4), 8);
        assert_eq!(xcol(&l, 5), 2);
        assert_eq!(xcol(&l, 2), 0);
    }

    #[test]
    fn search_wraps() {
        let mut d = doc("Alpha\nbeta\nALPHA\n");
        d.search = "alpha".into();
        d.cy = 1;
        assert!(d.find_next());
        assert_eq!((d.cy, d.cx), (2, 5));
        assert!(d.find_next());
        assert_eq!((d.cy, d.cx), (0, 5));
    }
}
