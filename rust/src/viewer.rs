//! Built-in F3 viewer: text (UTF-8 / Windows-1251 / DOS-866) with syntax colours, or hex.
//! JSON and XML can be shown formatted (F5). `TextDoc` is also used by the preview panel (Ctrl+L).

use crate::app::{draw_key_bar, App, Input};
use crate::dialogs;
use crate::format;
use crate::screen::Screen;
use crate::syntax::{self, Lang, State, Tok};
use crate::theme;
use crate::util::fit_left;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use encoding_rs::{Encoding, IBM866, UTF_16BE, UTF_16LE, UTF_8, WINDOWS_1251};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const ENCODINGS: [(&str, &Encoding); 5] =
    [("UTF-8", UTF_8), ("Win-1251", WINDOWS_1251), ("DOS-866", IBM866), ("UTF-16LE", UTF_16LE), ("UTF-16BE", UTF_16BE)];
const MAX_BYTES: u64 = 32 << 20;
const MAX_HIGHLIGHT: usize = 8 << 20;

/// A file loaded for viewing: its bytes, decoded lines and syntax state.
pub struct TextDoc {
    pub data: Vec<u8>,
    pub full_len: u64,
    pub lines: Vec<String>,
    pub enc: usize,
    pub hex: bool,
    pub lang: Option<&'static Lang>,
    pub syn: bool,
    /// JSON / XML shown pretty-printed
    pub formatted: bool,
    /// long lines continue on the next row instead of running off the right edge
    pub wrap: bool,
    states: Vec<State>,
    toks: Vec<Tok>,
}

impl TextDoc {
    /// Read up to `max` bytes of `path`.
    pub fn open(path: &Path, max: u64) -> std::io::Result<TextDoc> {
        let f = File::open(path)?;
        let full_len = f.metadata()?.len();
        let mut data = Vec::with_capacity(full_len.min(max) as usize);
        f.take(max).read_to_end(&mut data)?;
        let utf16 = crate::util::sniff_utf16(&data).and_then(|(e, _)| ENCODINGS.iter().position(|x| x.1 == e));
        let hex = utf16.is_none() && data[..data.len().min(8192)].contains(&0);
        let enc = match utf16 {
            Some(i) => i,
            None if std::str::from_utf8(&data).is_ok() || full_len > max => 0,
            None => 1,
        };
        let lines = split(&data, enc);
        let lang = syntax::detect(path, lines.first().map_or("", |l| l.as_str()));
        let mut d = TextDoc {
            data,
            full_len,
            lines,
            enc,
            hex,
            lang,
            syn: lang.is_some(),
            formatted: false,
            wrap: false,
            states: vec![],
            toks: vec![],
        };
        d.syn = d.syn && d.data.len() <= MAX_HIGHLIGHT;
        d.restate();
        Ok(d)
    }

    pub fn truncated(&self) -> bool {
        self.full_len > self.data.len() as u64
    }

    pub fn total(&self) -> usize {
        if self.hex {
            self.data.len().div_ceil(16)
        } else {
            self.lines.len()
        }
    }

    fn restate(&mut self) {
        self.states = match (self.lang, self.syn) {
            (Some(l), true) => syntax::line_states(l, &self.lines),
            _ => Vec::new(),
        };
    }

    pub fn toggle_syntax(&mut self) {
        if self.lang.is_some() && !self.hex {
            self.syn = !self.syn;
            self.restate();
        }
    }

    pub fn next_encoding(&mut self) {
        self.enc = (self.enc + 1) % ENCODINGS.len();
        self.hex = false;
        self.formatted = false;
        self.lines = split(&self.data, self.enc);
        self.restate();
    }

    /// Can F5 pretty-print this file?
    pub fn can_format(&self) -> bool {
        !self.hex && format::kind_of(self.lang).is_some()
    }

    /// Switch between the file as it is and formatted JSON / XML. Returns an error text if it isn't valid.
    pub fn toggle_format(&mut self) -> Result<(), String> {
        if self.formatted {
            self.formatted = false;
            self.lines = split(&self.data, self.enc);
        } else {
            let kind = format::kind_of(self.lang).ok_or("Only JSON and XML can be formatted.")?;
            let text = self.lines.join("\n");
            let pretty = format::pretty(kind, &text)?;
            self.lines = pretty.lines().map(str::to_string).collect();
            self.formatted = true;
        }
        self.restate();
        Ok(())
    }

    pub fn encoding_name(&self) -> &'static str {
        if self.hex {
            "HEX"
        } else {
            ENCODINGS[self.enc].0
        }
    }

    /// Draw lines `top..` into the area (x, y, w, h), scrolled `left` columns (ignored when wrapping).
    pub fn draw(&mut self, s: &mut Screen, x: i32, y: i32, w: i32, h: i32, top: usize, left: usize) {
        let t = theme::current();
        let highlight = if self.syn && !self.hex { self.lang } else { None };
        let wrap = self.wrap && !self.hex;
        let total = self.total();
        let wu = w.max(1) as usize;
        s.fill(x, y, w.max(0), h.max(0), ' ', t.viewer_fg, t.viewer_bg);
        let mut idx = top;
        let mut row = 0;
        while row < h && idx < total {
            let line = if self.hex { hex_line(&self.data, idx) } else { self.lines[idx].clone() };
            let colors: Vec<_> = match highlight {
                Some(l) => {
                    let st = self.states.get(idx).copied().unwrap_or_default();
                    syntax::scan(l, &line, st, &mut self.toks);
                    self.toks.iter().map(|&k| syntax::color(t, k)).collect()
                }
                None => Vec::new(),
            };
            let chars: Vec<char> = line.chars().collect();
            // the slices of the line shown on each screen row
            let pieces: Vec<(usize, usize)> = if wrap {
                if chars.is_empty() { vec![(0, 0)] } else { (0..chars.len()).step_by(wu).map(|a| (a, (a + wu).min(chars.len()))).collect() }
            } else {
                let a = left.min(chars.len());
                vec![(a, (a + wu).min(chars.len()))]
            };
            for (a, b) in pieces {
                if row >= h {
                    break;
                }
                for (cx, k) in (a..b).enumerate() {
                    let fg = colors.get(k).copied().unwrap_or(t.viewer_fg);
                    s.put(x + cx as i32, y + row, chars[k], fg, t.viewer_bg);
                }
                row += 1;
            }
            idx += 1;
        }
    }

    /// Number of screen rows line `idx` takes at width `w`.
    fn rows_of(&self, idx: usize, w: usize) -> usize {
        if !self.wrap || self.hex {
            return 1;
        }
        self.lines.get(idx).map_or(1, |l| l.chars().count().div_ceil(w.max(1)).max(1))
    }

    /// The first line to show so that the end of the file fills the last row of an area `w` × `h`.
    pub fn last_top(&self, w: usize, h: usize) -> usize {
        let total = self.total();
        if !self.wrap || self.hex {
            return total.saturating_sub(h);
        }
        let mut rows = 0;
        let mut i = total;
        while i > 0 {
            rows += self.rows_of(i - 1, w);
            if rows > h {
                break;
            }
            i -= 1;
        }
        i
    }

    /// Lines that fill one area of `h` rows starting at `top` (at least 1), for paging.
    pub fn lines_on_page(&self, top: usize, w: usize, h: usize) -> usize {
        let (mut rows, mut n) = (0, 0);
        while top + n < self.total() {
            rows += self.rows_of(top + n, w);
            if rows > h {
                break;
            }
            n += 1;
        }
        n.max(1)
    }
}
/// The DN close mark at the left of a full-screen window's title line; click it to close.
pub fn draw_close_mark(s: &mut Screen) {
    s.text(0, 0, "[", theme::current().bar_fg, theme::current().bar_bg);
    s.put(1, 0, '■', theme::current().dlg_close, theme::current().bar_bg);
    s.text(2, 0, "]", theme::current().bar_fg, theme::current().bar_bg);
}

pub fn is_close_mark(col: u16, row: u16) -> bool {
    row == 0 && col <= 2
}

/// What a key did to a document (see `doc_key`).
pub enum DocKey {
    Done,
    /// F5 couldn't format it; show this message
    Error(String),
    /// not a document key
    No,
}

/// Keys shared by the F3 viewer and the preview panel: scrolling and the F2/F4/F5/F6/F8 view modes.
/// `w` × `h` is the text area.
pub fn doc_key(d: &mut TextDoc, code: KeyCode, top: &mut usize, left: &mut usize, w: usize, h: usize) -> DocKey {
    let page = d.lines_on_page(*top, w, h);
    match code {
        KeyCode::Up => *top = top.saturating_sub(1),
        KeyCode::Down => *top += 1,
        KeyCode::PageUp => *top = top.saturating_sub(h.max(1)),
        KeyCode::PageDown => *top += page,
        KeyCode::Home => {
            *top = 0;
            *left = 0;
        }
        KeyCode::End => *top = d.last_top(w, h),
        KeyCode::Left => *left = left.saturating_sub(8),
        KeyCode::Right if !d.wrap || d.hex => *left += 8,
        KeyCode::F(2) => {
            d.wrap = !d.wrap;
            *left = 0;
        }
        KeyCode::F(4) => {
            d.hex = !d.hex;
            if d.hex && d.formatted {
                let _ = d.toggle_format();
            }
            *top = 0;
            *left = 0;
        }
        KeyCode::F(5) if d.can_format() => match d.toggle_format() {
            Ok(()) => {
                *top = 0;
                *left = 0;
            }
            Err(e) => return DocKey::Error(e),
        },
        KeyCode::F(6) => d.toggle_syntax(),
        KeyCode::F(8) => d.next_encoding(),
        _ => return DocKey::No,
    }
    *top = (*top).min(d.last_top(w, h));
    DocKey::Done
}

/// Key bar labels for `doc_key`'s keys (F1 is Help; callers fill F3, F9, F10 if they use them).
pub fn doc_key_labels(d: &TextDoc) -> [&'static str; 10] {
    let wrap = if d.hex { "" } else if d.wrap { "Unwrap" } else { "Wrap" };
    let syn = if d.lang.is_some() && !d.hex { if d.syn { "Plain" } else { "Syntax" } } else { "" };
    let fmt = if d.can_format() { if d.formatted { "Raw" } else { "Format" } } else { "" };
    ["Help", wrap, "", if d.hex { "Text" } else { "Hex" }, fmt, syn, "", "Codepg", "", ""]
}

pub fn show(app: &mut App, path: &Path) {
    let mut d = match TextDoc::open(path, MAX_BYTES) {
        Ok(d) => d,
        Err(e) => {
            dialogs::message(app, "Viewer", &e.to_string(), true);
            return;
        }
    };
    let (mut top, mut left) = (0usize, 0usize);
    let path_text = path.to_string_lossy().to_string();

    loop {
        let total = d.total();
        let (cur_top, cur_left) = (top, left);
        let dr = &mut d;
        let inp = app.window_input(
            &mut |s| {
                let rows = (s.h - 2).max(1) as usize;
                s.cursor = None;
                s.fill(0, 0, s.w, 1, ' ', theme::current().bar_fg, theme::current().bar_bg);
                draw_close_mark(s);
                let pct = if total <= rows { 100 } else { ((cur_top + rows) * 100 / total).min(100) };
                let mut right = format!(
                    " {}{}{}{}  Line {}/{}  {:>3}% ",
                    if dr.wrap && !dr.hex { "Wrap  " } else { "" },
                    if dr.formatted { "Formatted  " } else { "" },
                    if dr.syn && !dr.hex { dr.lang.map_or(String::new(), |l| format!("{}  ", l.name)) } else { String::new() },
                    dr.encoding_name(),
                    cur_top + 1,
                    total,
                    pct
                );
                if dr.truncated() {
                    right = format!(" [first 32M] {right}");
                }
                let rlen = right.chars().count() as i32;
                s.text(4, 0, &fit_left(&path_text, (s.w - rlen - 5).max(4) as usize), theme::current().bar_fg, theme::current().bar_bg);
                s.text(s.w - rlen, 0, &right, theme::current().bar_fg, theme::current().bar_bg);
                dr.draw(s, 0, 1, s.w, rows as i32, cur_top, cur_left);
                draw_key_bar(s, &doc_key_labels(dr));
            },
            None,
        );

        let (w, h) = (app.screen.w.max(1) as usize, (app.screen.h - 2).max(1) as usize);
        let code = match inp {
            Input::Key(k) => k.code,
            Input::Mouse(m) => {
                match m.kind {
                    MouseEventKind::ScrollUp => top = top.saturating_sub(3),
                    MouseEventKind::ScrollDown => top = (top + 3).min(d.last_top(w, h)),
                    MouseEventKind::Down(MouseButton::Left) if is_close_mark(m.column, m.row) => return,
                    _ => {}
                }
                continue;
            }
            Input::Tick | Input::Paste(_) => continue,
        };
        match code {
            KeyCode::Esc => return,
            KeyCode::F(1) => crate::help::show(app, "viewer"),
            _ => {
                if let DocKey::Error(e) = doc_key(&mut d, code, &mut top, &mut left, w, h) {
                    dialogs::message(app, "Format", &e, true);
                }
            }
        }
    }
}
fn split(data: &[u8], enc: usize) -> Vec<String> {
    let (text, _, _) = ENCODINGS[enc].1.decode(data);
    let text = text.trim_start_matches('\u{feff}');
    let mut out = Vec::new();
    let mut line = String::new();
    let mut col = 0usize;
    for ch in text.chars() {
        match ch {
            '\n' => {
                out.push(std::mem::take(&mut line));
                col = 0;
            }
            '\r' => {}
            '\t' => {
                let n = 8 - col % 8;
                line.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            c => {
                line.push(if c.is_control() { '·' } else { c });
                col += 1;
            }
        }
    }
    out.push(line);
    out
}

fn hex_line(d: &[u8], row: usize) -> String {
    let off = row * 16;
    let mut s = format!("{off:08X}: ");
    for i in 0..16 {
        if i == 8 {
            s.push_str("│ ");
        }
        match d.get(off + i) {
            Some(b) => s.push_str(&format!("{b:02X} ")),
            None => s.push_str("   "),
        }
    }
    s.push(' ');
    for b in d.iter().skip(off).take(16) {
        s.push(if (32..127).contains(b) { *b as char } else { '.' });
    }
    s
}
