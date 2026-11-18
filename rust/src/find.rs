//! Alt+F7: find files by name mask and, optionally, the text they contain.
//! The results replace the panel's list; Enter on one goes to it, Backspace returns to the folder.

use crate::app::App;
use crate::dialogs::{self, LineEdit};
use crate::panel::Entry;
use crate::platform;
use crate::theme;
use crate::util::{fit_left, n0, wildcard};
use chrono::DateTime;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

pub struct Query {
    pub mask: String,
    pub text: String,
}

impl Query {
    pub fn describe(&self) -> String {
        if self.text.is_empty() {
            format!("Find: {}", self.mask)
        } else {
            format!("Find: {} containing \"{}\"", self.mask, self.text)
        }
    }
}

/// The search form. Tab switches fields, Enter searches, Esc cancels.
pub fn ask(app: &mut App, last: &Query) -> Option<Query> {
    let mut mask = LineEdit::new(&last.mask);
    let mut text = LineEdit::new(&last.text);
    let mut field = 0;
    loop {
        let w = (app.screen.w - 4).min(70);
        let h = 9;
        let (x, y) = (((app.screen.w - w) / 2).max(0), ((app.screen.h - h) / 2).max(0));
        let cur = field;
        let (m, t) = (&mut mask, &mut text);
        let inp = app.modal_input(
            &mut |s| {
                let th = theme::current();
                dialogs::frame_at(s, x, y, w, h, "Find files");
                s.text(x + 4, y + 2, "File names (masks, ; between them):", th.dlg_fg, th.dlg_bg);
                s.text(x + 4, y + 4, "Containing text (optional, any case):", th.dlg_fg, th.dlg_bg);
                s.text(x + 4, y + 6, "Searches this folder and all below it. Enter starts.", th.dlg_fg, th.dlg_bg);
                // draw the unfocused field first so the cursor ends up in the focused one
                if cur == 0 {
                    t.draw(s, x + 4, y + 5, w - 8, th.input_fg, th.input_bg);
                    m.draw(s, x + 4, y + 3, w - 8, th.input_fg, th.input_bg);
                } else {
                    m.draw(s, x + 4, y + 3, w - 8, th.input_fg, th.input_bg);
                    t.draw(s, x + 4, y + 5, w - 8, th.input_fg, th.input_bg);
                }
            },
            None,
        );
        match inp {
            crate::app::Input::Key(k) => match k.code {
                KeyCode::Esc => return None,
                KeyCode::F(1) => crate::help::show(app, "find"),
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => field = 1 - field,
                KeyCode::Enter => {
                    let m = mask.value().trim().to_string();
                    return Some(Query { mask: if m.is_empty() { "*".into() } else { m }, text: text.value() });
                }
                _ => {
                    if field == 0 {
                        mask.handle(&k);
                    } else {
                        text.handle(&k);
                    }
                }
            },
            crate::app::Input::Mouse(mo) if mo.kind == MouseEventKind::Down(MouseButton::Left) => {
                let (mx, my) = (mo.column as i32, mo.row as i32);
                if dialogs::is_close_click(x, y, mx, my) {
                    return None;
                }
                if my == y + 3 {
                    field = 0;
                } else if my == y + 5 {
                    field = 1;
                }
            }
            crate::app::Input::Paste(p) => {
                if field == 0 {
                    mask.insert(&p);
                } else {
                    text.insert(&p);
                }
            }
            _ => {}
        }
    }
}

fn contains_text(path: &Path, needle_lower: &str) -> bool {
    let Ok(md) = fs::metadata(path) else { return false };
    if md.len() > 64 << 20 {
        return false;
    }
    let Ok(data) = fs::read(path) else { return false };
    if let Some((enc, _)) = crate::util::sniff_utf16(&data) {
        return enc.decode(&data).0.to_lowercase().contains(needle_lower);
    }
    if data[..data.len().min(8192)].contains(&0) {
        return false; // binary
    }
    String::from_utf8_lossy(&data).to_lowercase().contains(needle_lower)
}

/// Search below `root`. Shows progress; Esc stops (keeping what was found so far).
pub fn search(app: &mut App, root: &Path, q: &Query) -> Vec<Entry> {
    let masks: Vec<&str> = q.mask.split(';').map(str::trim).filter(|m| !m.is_empty()).collect();
    let needle = q.text.to_lowercase();
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut last = Instant::now() - Duration::from_secs(1);
    let mut scanned = 0u64;
    while let Some(dir) = stack.pop() {
        // progress and Esc, a few times a second
        if last.elapsed() > Duration::from_millis(120) {
            last = Instant::now();
            let lines = vec![
                fit_left(&dir.to_string_lossy(), 60),
                format!("{} files looked at, {} found.  Esc stops.", n0(scanned), n0(found.len() as u64)),
            ];
            dialogs::progress(app, "Find files", &lines);
            while event::poll(Duration::ZERO).unwrap_or(false) {
                if let Ok(Event::Key(k)) = event::read() {
                    if k.kind != KeyEventKind::Release && k.code == KeyCode::Esc {
                        return found;
                    }
                }
            }
        }
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let mut subdirs = Vec::new();
        for de in rd.flatten() {
            let Ok(md) = de.metadata() else { continue };
            let name = de.file_name().to_string_lossy().to_string();
            let path = de.path();
            let is_dir = md.is_dir();
            if is_dir && !md.file_type().is_symlink() && name != ".git" {
                subdirs.push(path.clone());
            }
            if !is_dir {
                scanned += 1;
            }
            if !masks.iter().any(|m| wildcard(m, &name)) {
                continue;
            }
            if !needle.is_empty() && (is_dir || !contains_text(&path, &needle)) {
                continue;
            }
            let rel = path.strip_prefix(root).map(|p| p.to_string_lossy().to_string()).unwrap_or(name.clone());
            found.push(Entry {
                name: rel.replace('/', std::path::MAIN_SEPARATOR_STR),
                is_dir,
                is_up: false,
                is_link: md.file_type().is_symlink(),
                selected: false,
                size: if is_dir { None } else { Some(md.len()) },
                modified: md.modified().ok().map(DateTime::from),
                hidden: platform::is_hidden(&name, &md),
                exec: platform::is_exec(&md),
                arc: None,
                path,
            });
        }
        subdirs.sort();
        stack.extend(subdirs.into_iter().rev());
    }
    found
}
