//! Alt+F1 / Alt+F2: drives plus named favourite folders (Work, Movies, Docs, ...).
//!
//! Enter or double click goes there. F7 adds the panel's folder, F4 renames (and changes the path),
//! Del/F8 removes, Ctrl+↑/↓ moves a favourite. Letters pick a drive, digits 1-9 a favourite.
//! Saved in <config>/orthodox-navigator/favorites.txt as "name<TAB>path" lines.

use crate::app::{draw_key_bar, App, Input};
use crate::dialogs;
use crate::platform;
use crate::theme;
use crate::util::{fit, fit_left, human};
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct Fav {
    pub name: String,
    pub path: PathBuf,
}

pub enum Pick {
    /// a drive / mount point (the panel may return to the last folder it had there)
    Volume(PathBuf),
    Folder(PathBuf),
    /// the Temp panel
    Temp,
}

fn file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("orthodox-navigator").join("favorites.txt"))
}

pub fn load() -> Vec<Fav> {
    let Some(f) = file() else { return vec![] };
    match fs::read_to_string(&f) {
        Ok(text) => text
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(n, p)| Fav { name: n.to_string(), path: PathBuf::from(p) })
            .collect(),
        // first run: a few useful defaults
        Err(_) => [
            ("Home", dirs::home_dir()),
            ("Documents", dirs::document_dir()),
            ("Downloads", dirs::download_dir()),
            ("Desktop", dirs::desktop_dir()),
        ]
        .into_iter()
        .filter_map(|(n, p)| p.filter(|p| p.is_dir()).map(|path| Fav { name: n.into(), path }))
        .collect(),
    }
}

pub fn save(favs: &[Fav]) {
    let Some(f) = file() else { return };
    if let Some(dir) = f.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let text: String = favs.iter().map(|f| format!("{}\t{}\n", f.name.replace('\t', " "), f.path.display())).collect();
    let _ = fs::write(f, text);
}

/// Add `dir` to favourites, asking for a name.
pub fn add(app: &mut App, dir: &Path) {
    let mut favs = load();
    add_to(app, &mut favs, dir);
}

fn add_to(app: &mut App, favs: &mut Vec<Fav>, dir: &Path) -> bool {
    let default = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| dir.display().to_string());
    let Some(name) = dialogs::input(app, "Add to favourites", &format!("Name for {}", dir.display()), &default) else { return false };
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    favs.push(Fav { name: name.to_string(), path: dir.to_path_buf() });
    save(favs);
    true
}

enum Row {
    Vol(PathBuf, String),
    Temp,
    Sep,
    Fav(usize),
}

/// The drive / favourites list. `current` is the panel's folder (for Ins and the initial cursor).
pub fn pick(app: &mut App, title: &str, current: &Path) -> Option<Pick> {
    let mut vols = platform::volumes();
    if !cfg!(windows) {
        vols.insert(0, platform::home());
    }
    let vol_rows: Vec<(PathBuf, String)> = vols
        .into_iter()
        .map(|v| {
            let name = v.to_string_lossy().trim_end_matches('\\').to_string();
            let free = platform::free_space(&v).map(|f| format!("{} free", human(f))).unwrap_or_else(|| "not ready".into());
            (v, format!("{name}\t{free}"))
        })
        .collect();
    let mut favs = load();
    let cur_vol = platform::volume_of(current);
    let mut sel: Option<usize> = None;
    let mut top = 0usize;
    let mut last_click: Option<(Instant, usize)> = None;

    loop {
        let mut rows: Vec<Row> = vol_rows.iter().map(|(p, l)| Row::Vol(p.clone(), l.clone())).collect();
        rows.push(Row::Temp);
        rows.push(Row::Sep);
        rows.extend((0..favs.len()).map(Row::Fav));
        let sel_i = *sel.get_or_insert_with(|| {
            // start on the favourite we're in, else the current drive
            (0..favs.len())
                .find(|&i| crate::util::same_path(&favs[i].path, current))
                .map(|i| vol_rows.len() + 2 + i)
                .or_else(|| vol_rows.iter().position(|(v, _)| platform::volume_of(v) == cur_vol && (cfg!(windows) || v != &platform::home())))
                .unwrap_or(0)
        });
        let sel_i = sel_i.min(rows.len() - 1);
        sel = Some(sel_i);

        let (sw, sh) = (app.screen.w, app.screen.h);
        let w = (sw - 2).clamp(30, 70);
        let visible = (rows.len() as i32).min(sh - 6).max(3) as usize;
        let h = visible as i32 + 4;
        let (x, y) = (((sw - w) / 2).max(0), ((sh - 1 - h) / 2).max(0));
        if sel_i < top {
            top = sel_i;
        }
        if sel_i >= top + visible {
            top = sel_i + 1 - visible;
        }
        let (favs_ref, rows_ref, cur_top) = (&favs, &rows, top);
        let inp = app.window_input(
            &mut |s| {
                let t = theme::current();
                dialogs::frame_at(s, x, y, w, h, title);
                let inner = (w - 6) as usize;
                for r in 0..visible {
                    let i = cur_top + r;
                    let Some(row) = rows_ref.get(i) else { break };
                    let ry = y + 2 + r as i32;
                    let on = i == sel_i;
                    let (fg, bg) = if on { (t.list_sel_fg, t.list_sel_bg) } else { (t.dlg_fg, t.dlg_bg) };
                    match row {
                        Row::Sep => {
                            s.hline(x + 3, ry, w - 6, '─', t.dlg_fg, t.dlg_bg);
                            s.text(x + 5, ry, " Favourites  (F7 add, F4 edit, F8 remove) ", t.dlg_fg, t.dlg_bg);
                        }
                        Row::Temp => {
                            let n = crate::temp::load().len();
                            let line = format!(" {:<w2$}{:>12} ", "Temp panel", format!("{n} items"), w2 = inner.saturating_sub(14));
                            s.text(x + 3, ry, &fit(&line, inner), fg, bg);
                        }
                        Row::Vol(_, label) => {
                            let (name, free) = label.split_once('\t').unwrap_or((label, ""));
                            let line = format!(" {:<w2$}{:>12} ", fit_left(name, inner.saturating_sub(14)), free, w2 = inner.saturating_sub(14));
                            s.text(x + 3, ry, &fit(&line, inner), fg, bg);
                        }
                        Row::Fav(k) => {
                            let f = &favs_ref[*k];
                            let num = if *k < 9 { format!("{}", k + 1) } else { " ".into() };
                            let nw = 16usize.min(inner / 2);
                            let line = format!(" {num} {:<nw$} {}", fit(&f.name, nw), fit_left(&f.path.to_string_lossy(), inner.saturating_sub(nw + 5)));
                            s.text(x + 3, ry, &fit(&line, inner), fg, bg);
                            if !on {
                                s.put(x + 4, ry, num.chars().next().unwrap_or(' '), t.dlg_hot, bg);
                            }
                        }
                    }
                }
                if rows_ref.len() > visible {
                    s.put(x + w - 3, y + 2, if cur_top > 0 { '▲' } else { ' ' }, t.dlg_fg, t.dlg_bg);
                    s.put(x + w - 3, y + 1 + visible as i32, if cur_top + visible < rows_ref.len() { '▼' } else { ' ' }, t.dlg_fg, t.dlg_bg);
                }
                draw_key_bar(s, &["Help", "", "", "Edit", "", "", "Add", "Delete", "", ""]);
            },
            None,
        );

        let choose = |i: usize| -> Option<Pick> {
            match &rows[i] {
                Row::Vol(p, _) => Some(Pick::Volume(p.clone())),
                Row::Fav(k) => Some(Pick::Folder(favs[*k].path.clone())),
                Row::Temp => Some(Pick::Temp),
                Row::Sep => None,
            }
        };
        let fav_at = |i: usize| match rows.get(i) {
            Some(Row::Fav(k)) => Some(*k),
            _ => None,
        };
        let step = |from: usize, down: bool| -> usize {
            let mut i = from;
            loop {
                i = if down { (i + 1) % rows.len() } else { (i + rows.len() - 1) % rows.len() };
                if !matches!(rows[i], Row::Sep) {
                    return i;
                }
            }
        };

        match inp {
            Input::Key(k) => {
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Esc => return None,
                    KeyCode::Enter => {
                        if let Some(p) = choose(sel_i) {
                            return Some(p);
                        }
                    }
                    KeyCode::Up if ctrl => {
                        if let Some(f) = fav_at(sel_i).filter(|&f| f > 0) {
                            favs.swap(f, f - 1);
                            save(&favs);
                            sel = Some(sel_i - 1);
                        }
                    }
                    KeyCode::Down if ctrl => {
                        if let Some(f) = fav_at(sel_i).filter(|&f| f + 1 < favs.len()) {
                            favs.swap(f, f + 1);
                            save(&favs);
                            sel = Some(sel_i + 1);
                        }
                    }
                    KeyCode::Up => sel = Some(step(sel_i, false)),
                    KeyCode::Down => sel = Some(step(sel_i, true)),
                    KeyCode::PageUp => sel = Some(sel_i.saturating_sub(visible)),
                    KeyCode::PageDown => sel = Some((sel_i + visible).min(rows.len() - 1)),
                    KeyCode::Home => sel = Some(0),
                    KeyCode::End => sel = Some(rows.len() - 1),
                    KeyCode::F(7) => {
                        if add_to(app, &mut favs, current) {
                            sel = Some(vol_rows.len() + 1 + favs.len());
                        }
                    }
                    KeyCode::Delete | KeyCode::F(8) => {
                        if let Some(f) = fav_at(sel_i) {
                            let q = format!("Remove \"{}\" from favourites?", favs[f].name);
                            if dialogs::choice(app, "Favourites", &[&q], &["Remove", "Cancel"], false) == Some(0) {
                                favs.remove(f);
                                save(&favs);
                                if favs.is_empty() || f == favs.len() {
                                    sel = Some(sel_i - 1);
                                }
                            }
                        }
                    }
                    KeyCode::F(4) => {
                        if let Some(f) = fav_at(sel_i) {
                            if let Some(name) = dialogs::input(app, "Edit favourite", "Name", &favs[f].name) {
                                if let Some(path) = dialogs::input(app, "Edit favourite", "Folder", &favs[f].path.to_string_lossy()) {
                                    if !name.trim().is_empty() && !path.trim().is_empty() {
                                        favs[f] = Fav { name: name.trim().to_string(), path: PathBuf::from(path.trim()) };
                                        save(&favs);
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::F(1) => crate::help::show(app, "favorites"),
                    KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                        let f = c as usize - '1' as usize;
                        if f < favs.len() {
                            return Some(Pick::Folder(favs[f].path.clone()));
                        }
                    }
                    KeyCode::Char(c) => {
                        let hit = rows.iter().position(|r| match r {
                            Row::Vol(_, l) => l.trim_start_matches('/').chars().next().is_some_and(|f| f.eq_ignore_ascii_case(&c)),
                            _ => false,
                        });
                        if let Some(i) = hit {
                            return choose(i);
                        }
                    }
                    _ => {}
                }
            }
            Input::Mouse(m) => {
                let (mx, my) = (m.column as i32, m.row as i32);
                match m.kind {
                    MouseEventKind::ScrollUp => sel = Some(step(sel_i, false)),
                    MouseEventKind::ScrollDown => sel = Some(step(sel_i, true)),
                    MouseEventKind::Down(MouseButton::Left) => {
                        if mx < x || mx >= x + w || my < y || my >= y + h || dialogs::is_close_click(x, y, mx, my) {
                            return None;
                        }
                        let r = my - y - 2;
                        if r >= 0 && (r as usize) < visible && mx > x + 2 && mx < x + w - 3 {
                            let i = top + r as usize;
                            if i < rows.len() && !matches!(rows[i], Row::Sep) {
                                let double = last_click.is_some_and(|(at, j)| j == i && at.elapsed().as_millis() < 400);
                                sel = Some(i);
                                if double {
                                    return choose(i);
                                }
                                last_click = Some((Instant::now(), i));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Input::Tick | Input::Paste(_) => {}
        }
    }
}
