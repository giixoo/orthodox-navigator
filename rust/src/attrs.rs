//! Ctrl+A: attributes of the selected files (or the one under the cursor): read-only, hidden, system and
//! archive on Windows, read / write / run for owner, group and others elsewhere; and the time the file
//! was last changed. With several files a box that differs between them shows [?] and stays as it is.

use crate::app::{App, Input};
use crate::dialogs::{self, button, button_width, frame_at, is_close_click, LineEdit};
use crate::panel::Entry;
use crate::theme;
use crate::util::fit;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tri {
    Off,
    On,
    Mixed,
}

/// The attribute bits shown: Windows FILE_ATTRIBUTE_* or Unix mode bits.
#[cfg(windows)]
const BITS: [(u32, &str); 4] = [(0x1, "Read-only"), (0x2, "Hidden"), (0x4, "System"), (0x20, "Archive")];
#[cfg(not(windows))]
const BITS: [(u32, &str); 9] = [
    (0o400, "owner read"), (0o200, "owner write"), (0o100, "owner run"),
    (0o040, "group read"), (0o020, "group write"), (0o010, "group run"),
    (0o004, "others read"), (0o002, "others write"), (0o001, "others run"),
];

#[cfg(windows)]
fn get_bits(p: &Path) -> Option<u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetFileAttributesW, INVALID_FILE_ATTRIBUTES};
    let w: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let a = unsafe { GetFileAttributesW(w.as_ptr()) };
    (a != INVALID_FILE_ATTRIBUTES).then_some(a)
}

#[cfg(windows)]
fn set_bits(p: &Path, old: u32, mask: u32, value: u32) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::SetFileAttributesW;
    let new = (old & !mask) | (value & mask);
    if new == old {
        return Ok(());
    }
    let w: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    // only the bits SetFileAttributes takes (read-only, hidden, system, archive, temporary, offline,
    // not indexed); none of them is "normal"
    let set = new & 0x3127;
    let ok = unsafe { SetFileAttributesW(w.as_ptr(), if set == 0 { 0x80 } else { set }) };
    if ok == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn get_bits(p: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).ok().map(|m| m.permissions().mode())
}

#[cfg(not(windows))]
fn set_bits(p: &Path, old: u32, mask: u32, value: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let new = (old & !mask) | (value & mask);
    if new == old {
        return Ok(());
    }
    fs::set_permissions(p, fs::Permissions::from_mode(new & 0o7777)).map_err(|e| e.to_string())
}

/// Set the time a file (or folder) was last changed.
fn set_modified(p: &Path, t: DateTime<Local>) -> Result<(), String> {
    #[cfg(windows)]
    let f = {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_WRITE_ATTRIBUTES works on read-only files too; BACKUP_SEMANTICS opens folders
        fs::OpenOptions::new().access_mode(0x100).custom_flags(0x0200_0000).open(p)
    };
    #[cfg(not(windows))]
    let f = fs::File::open(p);
    let f = f.map_err(|e| e.to_string())?;
    f.set_modified(t.into()).map_err(|e| e.to_string())
}

const TIME_FMT: &str = "%Y-%m-%d %H:%M:%S";

fn parse_time(s: &str) -> Option<DateTime<Local>> {
    let s = s.trim();
    let n = NaiveDateTime::parse_from_str(s, TIME_FMT)
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M"))
        .or_else(|_| NaiveDateTime::parse_from_str(&format!("{s} 00:00:00"), TIME_FMT))
        .ok()?;
    Local.from_local_datetime(&n).earliest()
}

/// The controls in focus order: the boxes, the time, Set, Cancel.
fn n_focus() -> usize {
    BITS.len() + 3
}

pub fn show(app: &mut App) {
    if app.active().arc.is_some() {
        return crate::fileops::read_only(app);
    }
    let items: Vec<Entry> = app.active().targets().into_iter().filter(|e| !e.is_up).collect();
    if items.is_empty() {
        return;
    }
    let olds: Vec<Option<u32>> = items.iter().map(|e| get_bits(&e.path)).collect();
    let mut state: Vec<Tri> = BITS
        .iter()
        .map(|(b, _)| {
            let on = olds.iter().flatten().filter(|o| *o & b != 0).count();
            match on {
                0 => Tri::Off,
                n if n == olds.iter().flatten().count() => Tri::On,
                _ => Tri::Mixed,
            }
        })
        .collect();
    let mixed: Vec<bool> = state.iter().map(|t| *t == Tri::Mixed).collect();
    let times: Vec<Option<DateTime<Local>>> = items.iter().map(|e| e.modified).collect();
    let same_time = times.windows(2).all(|w| w[0].map(|t| t.timestamp()) == w[1].map(|t| t.timestamp()));
    let time0 = if same_time { times[0].map(|t| t.format(TIME_FMT).to_string()).unwrap_or_default() } else { String::new() };
    let mut time = LineEdit::new(&time0);
    let what = if items.len() == 1 { items[0].name.clone() } else { format!("{} items", items.len()) };
    let mut focus = 0usize;
    let unix = !cfg!(windows);
    // rows: name, blank, the boxes (4 rows, or a header + 3 rows), mode line (unix), blank, time, blank, buttons
    let box_rows: i32 = if unix { 4 } else { BITS.len() as i32 };
    let h = 2 + 2 + box_rows + i32::from(unix) + 1 + 1 + 1 + 2 + 1;
    let w = 52;
    loop {
        let (x, y) = (((app.screen.w - w) / 2).max(0), ((app.screen.h - h) / 2).max(0));
        let by = y + h - 3;
        let (bset, bcancel) = (x + 12, x + 12 + button_width("Set") + 3);
        let ty = y + 4 + box_rows + i32::from(unix) + 1;
        // where each box is drawn
        let box_at = |i: usize| -> (i32, i32) {
            if unix {
                (x + 16 + (i % 3) as i32 * 8, y + 5 + (i / 3) as i32)
            } else {
                (x + 5, y + 4 + i as i32)
            }
        };
        let (cur, st, tm) = (focus, &state, &mut time);
        let inp = app.window_input(
            &mut |s| {
                let t = theme::current();
                frame_at(s, x, y, w, h, "Attributes");
                s.cursor = None;
                s.text(x + 4, y + 2, &fit(&what, (w - 8) as usize), t.dlg_title, t.dlg_bg);
                if unix {
                    s.text(x + 15, y + 4, "Read    Write   Run", t.dlg_fg, t.dlg_bg);
                    for (r, who) in ["Owner", "Group", "Others"].iter().enumerate() {
                        s.text(x + 5, y + 5 + r as i32, who, t.dlg_fg, t.dlg_bg);
                    }
                    if st.iter().all(|v| *v != Tri::Mixed) {
                        let mode: u32 = BITS.iter().zip(st).filter(|(_, v)| **v == Tri::On).map(|((b, _), _)| b).sum();
                        s.text(x + 5, y + 8, &format!("Mode {mode:03o}"), t.dlg_frame, t.dlg_bg);
                    }
                }
                for (i, (_, label)) in BITS.iter().enumerate() {
                    let (bx, byy) = box_at(i);
                    let mark = match st[i] {
                        Tri::On => "[x]",
                        Tri::Off => "[ ]",
                        Tri::Mixed => "[?]",
                    };
                    let (fg, bg) = if cur == i { (t.list_sel_fg, t.list_sel_bg) } else { (t.dlg_fg, t.dlg_bg) };
                    s.text(bx, byy, mark, fg, bg);
                    if !unix {
                        s.text(bx + 4, byy, label, t.dlg_fg, t.dlg_bg);
                    }
                }
                s.text(x + 5, ty, "Changed:", t.dlg_fg, t.dlg_bg);
                tm.draw(s, x + 14, ty, 21, t.input_fg, t.input_bg);
                if cur != BITS.len() {
                    s.cursor = None;
                }
                let hint = if same_time { "" } else { "(empty: keep)" };
                s.text(x + 36, ty, hint, t.dlg_frame, t.dlg_bg);
                button(s, bset, by, "Set", cur == BITS.len() + 1, t.dlg_bg);
                button(s, bcancel, by, "Cancel", cur == BITS.len() + 2, t.dlg_bg);
            },
            None,
        );
        let toggle = |state: &mut Vec<Tri>, i: usize| {
            state[i] = match (state[i], mixed[i]) {
                (Tri::Off, _) => Tri::On,
                (Tri::On, true) => Tri::Mixed,
                (Tri::On, false) => Tri::Off,
                (Tri::Mixed, _) => Tri::Off,
            };
        };
        let mut apply = false;
        match inp {
            Input::Key(k) => {
                let shift = k.modifiers.contains(KeyModifiers::SHIFT);
                match k.code {
                    KeyCode::Esc => return,
                    KeyCode::Enter if focus == BITS.len() + 2 => return,
                    KeyCode::Enter => apply = true,
                    KeyCode::F(1) => crate::help::show(app, "panels"),
                    KeyCode::Tab if !shift => focus = (focus + 1) % n_focus(),
                    KeyCode::BackTab | KeyCode::Tab => focus = (focus + n_focus() - 1) % n_focus(),
                    KeyCode::Down if unix && focus + 3 < BITS.len() => focus += 3,
                    KeyCode::Up if unix && (3..BITS.len()).contains(&focus) => focus -= 3,
                    KeyCode::Up if unix && focus == BITS.len() => focus = BITS.len() - 3,
                    KeyCode::Down if unix && focus < BITS.len() => focus = BITS.len(),
                    KeyCode::Down => focus = (focus + 1) % n_focus(),
                    KeyCode::Up => focus = (focus + n_focus() - 1) % n_focus(),
                    KeyCode::Right if focus < BITS.len() || focus > BITS.len() => focus = (focus + 1) % n_focus(),
                    KeyCode::Left if focus < BITS.len() || focus > BITS.len() => focus = (focus + n_focus() - 1) % n_focus(),
                    KeyCode::Char(' ') if focus < BITS.len() => toggle(&mut state, focus),
                    _ if focus == BITS.len() => {
                        time.handle(&k);
                    }
                    _ => {}
                }
            }
            Input::Mouse(m) if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) => {
                let (mx, my) = (m.column as i32, m.row as i32);
                if is_close_click(x, y, mx, my) || mx < x || mx >= x + w || my < y || my >= y + h {
                    return;
                }
                if let Some(i) = (0..BITS.len()).find(|&i| {
                    let (bx, byy) = box_at(i);
                    my == byy && mx >= bx && mx < bx + if unix { 3 } else { 4 + BITS[i].1.len() as i32 }
                }) {
                    focus = i;
                    toggle(&mut state, i);
                } else if my == ty && mx >= x + 14 && mx < x + 35 {
                    focus = BITS.len();
                } else if my == by && mx >= bset && mx < bset + button_width("Set") {
                    apply = true;
                } else if my == by && mx >= bcancel && mx < bcancel + button_width("Cancel") {
                    return;
                }
            }
            _ => {}
        }
        if !apply {
            continue;
        }
        let new_time = time.value();
        let when = if new_time.trim().is_empty() || new_time == time0 {
            None
        } else {
            match parse_time(&new_time) {
                Some(t) => Some(t),
                None => {
                    dialogs::message(app, "Attributes", "Write the time as 2026-11-19 08:05:09.", true);
                    focus = BITS.len();
                    continue;
                }
            }
        };
        let (mut mask, mut value) = (0u32, 0u32);
        for (i, (b, _)) in BITS.iter().enumerate() {
            match state[i] {
                Tri::On => {
                    mask |= b;
                    value |= b;
                }
                Tri::Off => mask |= b,
                Tri::Mixed => {}
            }
        }
        let mut errors = Vec::new();
        for (e, old) in items.iter().zip(&olds) {
            // the time first: a read-only file can't be changed after it becomes read-only (on some systems)
            if let Some(t) = when {
                if let Err(err) = set_modified(&e.path, t) {
                    errors.push(format!("{}: {err}", e.name));
                }
            }
            if let Some(old) = old {
                if let Err(err) = set_bits(&e.path, *old, mask, value) {
                    errors.push(format!("{}: {err}", e.name));
                }
            }
        }
        app.reload_panels();
        if !errors.is_empty() {
            let more = if errors.len() > 8 { format!("\n... and {} more", errors.len() - 8) } else { String::new() };
            let text = errors.iter().take(8).cloned().collect::<Vec<_>>().join("\n") + &more;
            dialogs::message(app, "Attributes", &text, true);
        }
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_bits() {
        assert!(parse_time("2026-11-19 08:05:09").is_some());
        assert!(parse_time("2026-11-19 08:05").is_some());
        assert!(parse_time("2026-11-19").is_some());
        assert!(parse_time("19.11.2026").is_none());
        let dir = std::env::temp_dir().join(format!("on-attrs-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let f = dir.join("a.txt");
        fs::write(&f, "x").unwrap();
        let t = parse_time("2020-01-02 03:04:05").unwrap();
        set_modified(&f, t).unwrap();
        let m: DateTime<Local> = fs::metadata(&f).unwrap().modified().unwrap().into();
        assert_eq!(m.timestamp(), t.timestamp());
        let old = get_bits(&f).unwrap();
        let ro = BITS.iter().find(|(_, l)| l.contains("write") || l.contains("Read-only")).unwrap().0;
        // read-only on (Windows) / owner write off (Unix), then back
        let on = if cfg!(windows) { ro } else { 0 };
        set_bits(&f, old, ro, on).unwrap();
        assert!(fs::metadata(&f).unwrap().permissions().readonly());
        set_modified(&f, t).unwrap(); // still possible when read-only
        let now = get_bits(&f).unwrap();
        set_bits(&f, now, ro, if cfg!(windows) { 0 } else { ro }).unwrap();
        assert!(!fs::metadata(&f).unwrap().permissions().readonly());
        let _ = fs::remove_dir_all(&dir);
    }
}
