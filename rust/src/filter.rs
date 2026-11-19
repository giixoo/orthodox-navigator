//! Ctrl+F: show only some files in the panel. Masks with `;` between them (`*.rs;*.toml`); a mask with `-`
//! in front hides (`-*.log`). Below, the extensions in the folder with how many files have each: Space
//! adds or removes one. Folders are always shown. An empty filter shows everything.

use crate::app::{App, Input};
use crate::dialogs::{frame_at, is_close_click, LineEdit};
use crate::theme;
use crate::util::fit;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The extensions of the files in a folder (`*.rs`, or the name for files without one) and their counts.
fn extensions(dir: &Path) -> Vec<(String, usize)> {
    let mut m: BTreeMap<String, usize> = BTreeMap::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for de in rd.flatten() {
            if de.file_type().map(|t| t.is_dir()).unwrap_or(false) || de.path().is_dir() {
                continue;
            }
            let name = de.file_name().to_string_lossy().to_string();
            let mask = match name.rfind('.') {
                Some(i) if i > 0 => format!("*{}", name[i..].to_lowercase()),
                _ => "*.".to_string(),
            };
            *m.entry(mask).or_default() += 1;
        }
    }
    let mut v: Vec<(String, usize)> = m.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

fn masks_of(text: &str) -> Vec<String> {
    text.split([';', ',']).map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect()
}

fn has(text: &str, mask: &str) -> bool {
    masks_of(text).iter().any(|m| m.eq_ignore_ascii_case(mask))
}

/// Add or remove one mask.
fn toggle(text: &str, mask: &str) -> String {
    let mut v = masks_of(text);
    if let Some(i) = v.iter().position(|m| m.eq_ignore_ascii_case(mask)) {
        v.remove(i);
    } else {
        v.push(mask.to_string());
    }
    v.join(";")
}

/// The dialog. Returns the new filter (None: show everything), or None when cancelled (outer None).
pub fn ask(app: &mut App, dir: &Path, current: Option<&str>) -> Option<Option<String>> {
    let exts = extensions(dir);
    let mut ed = LineEdit::new(current.unwrap_or(""));
    let mut in_list = false;
    let mut sel = 0usize;
    let mut top = 0usize;
    loop {
        let (sw, sh) = (app.screen.w, app.screen.h);
        let w = 56.min(sw - 2);
        let visible = (exts.len().max(1) as i32).min(sh - 12).max(1) as usize;
        let h = visible as i32 + 8;
        let (x, y) = (((sw - w) / 2).max(0), ((sh - 1 - h) / 2).max(0));
        if sel < top {
            top = sel;
        }
        if sel >= top + visible {
            top = sel + 1 - visible;
        }
        let text = ed.value();
        let (cur_in_list, cur_sel, cur_top, edr) = (in_list, sel, top, &mut ed);
        let inp = app.window_input(
            &mut |s| {
                let t = theme::current();
                frame_at(s, x, y, w, h, "Filter");
                let inner = (w - 8) as usize;
                s.text(x + 4, y + 2, &fit("Show files matching (; between, -mask hides):", inner), t.dlg_fg, t.dlg_bg);
                edr.draw(s, x + 4, y + 3, w - 8, t.input_fg, t.input_bg);
                if cur_in_list {
                    s.cursor = None;
                }
                if exts.is_empty() {
                    s.text(x + 4, y + 5, "(no files here)", t.dlg_fg, t.dlg_bg);
                }
                for r in 0..visible {
                    let Some((m, n)) = exts.get(cur_top + r) else { break };
                    let on = cur_in_list && cur_top + r == cur_sel;
                    let (fg, bg) = if on { (t.list_sel_fg, t.list_sel_bg) } else { (t.dlg_fg, t.dlg_bg) };
                    let mark = if has(&text, m) { "[x]" } else { "[ ]" };
                    let label = if m == "*." { "no extension".to_string() } else { m.clone() };
                    let line = format!(" {mark} {:<24} {n:>6} ", fit(&label, 24));
                    s.text(x + 4, y + 5 + r as i32, &fit(&line, inner), fg, bg);
                }
                s.text(x + 4, y + h - 2, &fit("Tab: the list, Space: tick, Enter: apply, Del: show all", inner), t.dlg_frame, t.dlg_bg);
                crate::app::draw_key_bar(s, &["Help", "", "", "", "", "", "", "All", "", ""]);
            },
            None,
        );
        match inp {
            Input::Key(k) => {
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Esc => return None,
                    KeyCode::Enter => {
                        let v = masks_of(&ed.value()).join(";");
                        return Some((!v.is_empty()).then_some(v));
                    }
                    KeyCode::F(8) => return Some(None),
                    KeyCode::Delete if in_list => return Some(None),
                    KeyCode::F(1) => crate::help::show(app, "panels"),
                    KeyCode::Tab | KeyCode::BackTab => in_list = !in_list && !exts.is_empty(),
                    KeyCode::Down if !in_list && !exts.is_empty() => in_list = true,
                    KeyCode::Up if in_list && sel == 0 => in_list = false,
                    KeyCode::Up if in_list => sel -= 1,
                    KeyCode::Down if in_list => sel = (sel + 1).min(exts.len() - 1),
                    KeyCode::PageUp if in_list => sel = sel.saturating_sub(visible),
                    KeyCode::PageDown if in_list => sel = (sel + visible).min(exts.len() - 1),
                    KeyCode::Char(' ') if in_list && !ctrl => {
                        let t = toggle(&ed.value(), &exts[sel].0);
                        ed.set(&t);
                    }
                    _ if !in_list => {
                        ed.handle(&k);
                    }
                    _ => {}
                }
            }
            Input::Mouse(m) => {
                let (mx, my) = (m.column as i32, m.row as i32);
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if mx < x || mx >= x + w || my < y || my >= y + h || is_close_click(x, y, mx, my) {
                            return None;
                        }
                        let r = my - y - 5;
                        if my == y + 3 {
                            in_list = false;
                        } else if r >= 0 && (r as usize) < visible && top + (r as usize) < exts.len() {
                            in_list = true;
                            sel = top + r as usize;
                            let t = toggle(&ed.value(), &exts[sel].0);
                            ed.set(&t);
                        }
                    }
                    MouseEventKind::ScrollUp if in_list => sel = sel.saturating_sub(1),
                    MouseEventKind::ScrollDown if in_list => sel = (sel + 1).min(exts.len().saturating_sub(1)),
                    _ => {}
                }
            }
            Input::Paste(t) if !in_list => ed.insert(t.trim()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::filter_passes;

    #[test]
    fn masks() {
        assert_eq!(toggle("*.rs", "*.toml"), "*.rs;*.toml");
        assert_eq!(toggle("*.rs; *.toml", "*.RS"), "*.toml");
        assert!(filter_passes("*.rs;*.toml", "main.rs"));
        assert!(!filter_passes("*.rs", "Cargo.lock"));
        assert!(filter_passes("-*.log", "a.txt"));
        assert!(!filter_passes("*;-*.log", "a.log"));
        assert!(filter_passes("*.", "Makefile"));
    }
}
