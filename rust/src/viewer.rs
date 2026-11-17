//! Built-in F3 viewer: text (UTF-8 / Windows-1251 / DOS-866) or hex.

use crate::app::{draw_key_bar, App, Input};
use crate::dialogs;
use crate::screen::C;
use crate::theme;
use crate::util::fit_left;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use encoding_rs::{Encoding, IBM866, UTF_8, WINDOWS_1251};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const ENCODINGS: [(&str, &Encoding); 3] = [("UTF-8", UTF_8), ("Win-1251", WINDOWS_1251), ("DOS-866", IBM866)];
const MAX_BYTES: u64 = 32 << 20;

pub fn show(app: &mut App, path: &Path) {
    let (data, full_len) = match read(path) {
        Ok(v) => v,
        Err(e) => {
            dialogs::message(app, "Viewer", &e.to_string(), true);
            return;
        }
    };
    let mut hex = data[..data.len().min(8192)].contains(&0);
    let mut enc = if std::str::from_utf8(&data).is_ok() { 0 } else { 1 };
    let mut lines = split(&data, enc);
    let (mut top, mut left) = (0usize, 0usize);
    let path_text = path.to_string_lossy().to_string();

    loop {
        let total = if hex { data.len().div_ceil(16) } else { lines.len() };
        let (cur_top, cur_left, cur_hex, cur_enc) = (top, left, hex, enc);
        let lines_ref = &lines;
        let data_ref = &data;
        let inp = app.modal_input(&mut |s| {
            let t = theme::current();
            let rows = (s.h - 2).max(1) as usize;
            s.cursor = None;
            s.fill(0, 0, s.w, 1, ' ', C::Black, C::Cyan);
            let pct = if total <= rows { 100 } else { ((cur_top + rows) * 100 / total).min(100) };
            let mut right = format!(
                " {}  Line {}/{}  {:>3}% ",
                if cur_hex { "HEX" } else { ENCODINGS[cur_enc].0 },
                cur_top + 1,
                total,
                pct
            );
            if full_len > data_ref.len() as u64 {
                right = format!(" [first 32M] {right}");
            }
            let rlen = right.chars().count() as i32;
            s.text(0, 0, &format!(" {}", fit_left(&path_text, (s.w - rlen - 2).max(4) as usize)), C::Black, C::Cyan);
            s.text(s.w - rlen, 0, &right, C::Black, C::Cyan);
            for r in 0..rows {
                let idx = cur_top + r;
                let line = if cur_hex {
                    if idx < total { hex_line(data_ref, idx) } else { String::new() }
                } else {
                    lines_ref.get(idx).cloned().unwrap_or_default()
                };
                let visible: String = line.chars().skip(cur_left).take(s.w as usize).collect();
                let pad = s.w as usize - visible.chars().count();
                s.text(0, r as i32 + 1, &format!("{visible}{}", " ".repeat(pad)), t.viewer_fg, t.viewer_bg);
            }
            draw_key_bar(s, &["Help", "", "Quit", if cur_hex { "Text" } else { "Hex" }, "", "", "", "Codepg", "", "Quit"]);
        }, None);

        let page = (app.screen.h - 2).max(1) as usize;
        let max = total.saturating_sub(page);
        let code = match inp {
            Input::Key(k) => k.code,
            Input::Mouse(m) => match m.kind {
                MouseEventKind::ScrollUp => {
                    top = top.saturating_sub(3);
                    continue;
                }
                MouseEventKind::ScrollDown => {
                    top = (top + 3).min(max);
                    continue;
                }
                MouseEventKind::Down(MouseButton::Right) => return,
                _ => continue,
            },
            Input::Tick => continue,
        };
        match code {
            KeyCode::Up => top = top.saturating_sub(1),
            KeyCode::Down => top += 1,
            KeyCode::PageUp => top = top.saturating_sub(page),
            KeyCode::PageDown => top += page,
            KeyCode::Home => {
                top = 0;
                left = 0;
            }
            KeyCode::End => top = max,
            KeyCode::Left => left = left.saturating_sub(8),
            KeyCode::Right => left += 8,
            KeyCode::F(4) => {
                hex = !hex;
                top = 0;
                left = 0;
            }
            KeyCode::F(8) => {
                enc = (enc + 1) % ENCODINGS.len();
                lines = split(&data, enc);
                hex = false;
            }
            KeyCode::Esc | KeyCode::F(3) | KeyCode::F(10) => return,
            _ => {}
        }
        top = top.min(max);
    }
}

fn read(path: &Path) -> std::io::Result<(Vec<u8>, u64)> {
    let f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut data = Vec::with_capacity(len.min(MAX_BYTES) as usize);
    f.take(MAX_BYTES).read_to_end(&mut data)?;
    Ok((data, len))
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
