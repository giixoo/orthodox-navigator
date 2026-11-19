//! Batch rename (Alt+F6): new names for the selected files from a pattern, with a live preview.
//!
//! The name and extension patterns mix text with placeholders:
//! `[N]` name, `[N2-5]` its characters 2 to 5, `[E]` extension, `[C]` counter, `[P]` folder name,
//! `[DATE]` / `[TIME]` the file's modification date and time, and `[Y] [M] [D] [h] [m] [s]` their parts.
//! Then find / replace and the case change apply to the whole new name.

use crate::app::{draw_key_bar, App, Input};
use crate::dialogs::{self, LineEdit};
use crate::panel::Entry;
use crate::theme;
use crate::util::fit;
use crate::viewer::{draw_close_mark, is_close_mark};
use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

/// The settings, in the order of `FIELDS`.
#[derive(Clone, Debug, PartialEq)]
pub struct Opts {
    pub name: String,
    pub ext: String,
    pub find: String,
    pub replace: String,
    pub start: i64,
    pub step: i64,
    pub digits: usize,
    pub lower: bool,
    pub upper: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { name: "[N]".into(), ext: "[E]".into(), find: String::new(), replace: String::new(), start: 1, step: 1, digits: 1, lower: false, upper: false }
    }
}

/// One file to rename.
#[derive(Clone)]
pub struct Item {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub modified: Option<DateTime<Local>>,
}

/// One preview row: the new name, or why it can't be used.
#[derive(Debug, PartialEq)]
pub struct Row {
    pub new: String,
    pub err: Option<&'static str>,
}

fn split_ext(name: &str, is_dir: bool) -> (&str, &str) {
    if is_dir {
        return (name, "");
    }
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    }
}

/// Characters `a` to `b` (1-based, inclusive; `b` None = to the end) of `s`.
fn chars_range(s: &str, a: usize, b: Option<usize>) -> String {
    let a = a.max(1);
    let n = s.chars().count();
    let b = b.unwrap_or(n).min(n);
    if a > b {
        return String::new();
    }
    s.chars().skip(a - 1).take(b + 1 - a).collect()
}

/// Expand the placeholders of one pattern.
fn expand(pat: &str, it: &Item, counter: i64, digits: usize) -> String {
    let (stem, ext) = split_ext(&it.name, it.is_dir);
    let folder = it.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let t = it.modified.unwrap_or_else(Local::now);
    let mut out = String::new();
    let mut rest = pat;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let tag = &after[..close];
        let val = match tag {
            "N" => Some(stem.to_string()),
            "E" => Some(ext.to_string()),
            "C" => Some(format!("{:0w$}", counter, w = digits.max(1))),
            "P" => Some(folder.clone()),
            "DATE" => Some(t.format("%Y-%m-%d").to_string()),
            "TIME" => Some(t.format("%H-%M-%S").to_string()),
            "Y" => Some(t.format("%Y").to_string()),
            "M" => Some(t.format("%m").to_string()),
            "D" => Some(t.format("%d").to_string()),
            "h" => Some(t.format("%H").to_string()),
            "m" => Some(t.format("%M").to_string()),
            "s" => Some(t.format("%S").to_string()),
            _ => tag.strip_prefix('N').and_then(|r| {
                // [N3] one character, [N2-5] a range, [N2-] to the end
                let (a, b) = match r.split_once('-') {
                    Some((a, "")) => (a.parse().ok()?, None),
                    Some((a, b)) => (a.parse().ok()?, Some(b.parse().ok()?)),
                    None => {
                        let a: usize = r.parse().ok()?;
                        (a, Some(a))
                    }
                };
                Some(chars_range(stem, a, b))
            }),
        };
        match val {
            Some(v) => out.push_str(&v),
            None => out.push_str(&rest[open..open + close + 2]), // not a placeholder: keep as typed
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

fn bad_char(c: char) -> bool {
    c == '/' || c == '\0' || (cfg!(windows) && (c == '\\' || c == ':' || c == '*' || c == '?' || c == '"' || c == '<' || c == '>' || c == '|' || (c as u32) < 32))
}

/// File names compare without case on Windows (and macOS, usually).
fn key(name: &str) -> String {
    if cfg!(any(windows, target_os = "macos")) { name.to_lowercase() } else { name.to_string() }
}

/// The new names for `items`, with a reason for each one that can't be used.
pub fn plan(items: &[Item], o: &Opts) -> Vec<Row> {
    let mut rows: Vec<Row> = items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let c = o.start + o.step * i as i64;
            let stem = expand(&o.name, it, c, o.digits);
            let ext = expand(&o.ext, it, c, o.digits);
            let mut new = if ext.is_empty() { stem } else { format!("{stem}.{ext}") };
            if !o.find.is_empty() {
                new = new.replace(&o.find, &o.replace);
            }
            if o.upper {
                new = new.to_uppercase();
            } else if o.lower {
                new = new.to_lowercase();
            }
            let err = if new.trim().is_empty() {
                Some("empty name")
            } else if new.chars().any(bad_char) {
                Some("bad character")
            } else if new == "." || new == ".." || (cfg!(windows) && new.ends_with(['.', ' '])) {
                Some("bad name")
            } else {
                None
            };
            Row { new, err }
        })
        .collect();
    // two files can't get one name
    let mut count: HashMap<String, usize> = HashMap::new();
    for r in &rows {
        *count.entry(key(&r.new)).or_default() += 1;
    }
    // a name already taken by a file that isn't being renamed
    let sources: HashSet<String> = items.iter().map(|it| key(&it.name)).collect();
    for (r, it) in rows.iter_mut().zip(items) {
        if r.err.is_some() {
            continue;
        }
        if count[&key(&r.new)] > 1 {
            r.err = Some("same name twice");
        } else if !sources.contains(&key(&r.new)) && it.path.with_file_name(&r.new).symlink_metadata().is_ok() {
            r.err = Some("exists");
        }
    }
    rows
}

/// Rename every item whose name changes. Names may swap or shift (a→b, b→c), so when a new name is one of
/// the old ones, everything goes through a temporary name first. Returns the errors.
pub fn apply(items: &[Item], rows: &[Row]) -> Vec<String> {
    let todo: Vec<(&Item, &str)> = items.iter().zip(rows).filter(|(it, r)| r.err.is_none() && r.new != it.name).map(|(it, r)| (it, r.new.as_str())).collect();
    let olds: HashSet<String> = todo.iter().map(|(it, _)| key(&it.name)).collect();
    let chained = todo.iter().any(|(it, new)| olds.contains(&key(new)) && key(new) != key(&it.name));
    let mut errors = Vec::new();
    let mut moved: Vec<(PathBuf, &Item, &str)> = Vec::new();
    for (i, (it, new)) in todo.iter().enumerate() {
        if chained {
            let tmp = it.path.with_file_name(format!(".on-rename-{}-{i}", std::process::id()));
            match std::fs::rename(&it.path, &tmp) {
                Ok(()) => moved.push((tmp, it, new)),
                Err(e) => errors.push(format!("{}: {e}", it.name)),
            }
        } else if let Err(e) = std::fs::rename(&it.path, it.path.with_file_name(new)) {
            errors.push(format!("{}: {e}", it.name));
        }
    }
    for (tmp, it, new) in moved {
        if let Err(e) = std::fs::rename(&tmp, it.path.with_file_name(new)) {
            // put it back under its old name rather than leave the temporary one
            let back = std::fs::rename(&tmp, &it.path).is_ok();
            errors.push(format!("{} -> {new}: {e}{}", it.name, if back { "" } else { " (left as a temporary name)" }));
        }
    }
    errors
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Number,
    Check,
}

/// The settings boxes: label, kind, row (0..3).
const FIELDS: [(&str, Kind, i32); 9] = [
    ("Name", Kind::Text, 0),
    ("Extension", Kind::Text, 0),
    ("Find", Kind::Text, 1),
    ("Replace with", Kind::Text, 1),
    ("[C] from", Kind::Number, 2),
    ("step", Kind::Number, 2),
    ("digits", Kind::Number, 2),
    ("lower case", Kind::Check, 2),
    ("UPPER CASE", Kind::Check, 2),
];

const HINT: &str = "[N] name  [N2-5] its letters 2-5  [E] extension  [C] counter  [P] folder  [DATE] [TIME]  [Y] [M] [D] [h] [m] [s]";

/// The settings of the last batch rename, while ON runs.
static LAST: Mutex<Option<Opts>> = Mutex::new(None);

fn to_edits(o: &Opts) -> Vec<LineEdit> {
    let b = |v: bool| if v { "1" } else { "0" };
    [o.name.clone(), o.ext.clone(), o.find.clone(), o.replace.clone(), o.start.to_string(), o.step.to_string(), o.digits.to_string(), b(o.lower).into(), b(o.upper).into()]
        .iter()
        .map(|s| LineEdit::new(s))
        .collect()
}

fn from_edits(e: &[LineEdit]) -> Opts {
    let num = |i: usize, d: i64| e[i].value().trim().parse::<i64>().unwrap_or(d);
    Opts {
        name: e[0].value(),
        ext: e[1].value(),
        find: e[2].value(),
        replace: e[3].value(),
        start: num(4, 1),
        step: num(5, 1),
        digits: num(6, 1).clamp(1, 12) as usize,
        lower: e[7].value() == "1",
        upper: e[8].value() == "1",
    }
}

/// Space between two fields in a row.
const GAP: i32 = 2;

/// Where each field is drawn: (x, y, width) of the whole field, label included.
fn places(w: i32) -> Vec<(i32, i32, i32)> {
    let mut out = Vec::new();
    for row in 0..3 {
        let idx: Vec<usize> = (0..FIELDS.len()).filter(|&i| FIELDS[i].2 == row).collect();
        let fixed = |i: usize| match FIELDS[i].1 {
            Kind::Check => 4 + FIELDS[i].0.len() as i32,
            Kind::Number => FIELDS[i].0.len() as i32 + 2 + 5,
            Kind::Text => 0,
        };
        let texts = idx.iter().filter(|&&i| FIELDS[i].1 == Kind::Text).count() as i32;
        let used: i32 = idx.iter().map(|&i| fixed(i)).sum::<i32>() + GAP * (idx.len() as i32 - 1);
        let each = if texts > 0 { ((w - 4 - used) / texts).max(12) } else { 0 };
        let mut x = 2;
        for i in idx {
            let fw = if FIELDS[i].1 == Kind::Text { each } else { fixed(i) };
            out.push((x, 2 + row, fw));
            x += fw + GAP;
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Field(usize),
    List,
}

/// Open batch rename for the selected files (or the one under the cursor) of the active panel.
pub fn show(app: &mut App) {
    if app.active().arc.is_some() {
        return crate::fileops::read_only(app);
    }
    let entries: Vec<Entry> = app.active().targets().into_iter().filter(|e| !e.is_up).collect();
    if entries.is_empty() {
        return;
    }
    let items: Vec<Item> = entries.iter().map(|e| Item { name: e.name.clone(), path: e.path.clone(), is_dir: e.is_dir, modified: e.modified }).collect();
    let mut edits = to_edits(&LAST.lock().ok().and_then(|l| l.clone()).unwrap_or_default());
    let mut focus = Focus::Field(0);
    let mut top = 0usize;
    loop {
        let o = from_edits(&edits);
        let rows = plan(&items, &o);
        let changed = items.iter().zip(&rows).filter(|(it, r)| r.new != it.name).count();
        let bad = rows.iter().filter(|r| r.err.is_some()).count();
        let pl = places(app.screen.w);
        let list_y = 7;
        let list_h = (app.screen.h - 1 - list_y).max(3);
        let page = (list_h - 2).max(1) as usize;
        top = top.min(items.len().saturating_sub(page));
        let (cur_focus, cur_top, edits_ref, rows_ref, items_ref) = (focus, top, &mut edits, &rows, &items);
        let inp = app.window_input(
            &mut |s| {
                let t = theme::current();
                let (w, h) = (s.w, s.h);
                s.cursor = None;
                s.fill(0, 0, w, h - 1, ' ', t.file, t.panel_bg);
                s.fill(0, 0, w, 1, ' ', t.bar_fg, t.bar_bg);
                draw_close_mark(s);
                s.text(4, 0, "Batch rename", t.bar_fg, t.bar_bg);
                let mut cursor = None;
                for (i, (&(label, kind, _), &(fx, fy, fw))) in FIELDS.iter().zip(&pl).enumerate() {
                    let ed = &mut edits_ref[i];
                    let focused = cur_focus == Focus::Field(i);
                    if kind == Kind::Check {
                        s.text(fx, fy, if ed.value() == "1" { "[x]" } else { "[ ]" }, t.input_fg, t.input_bg);
                        s.text(fx + 4, fy, label, t.header, t.panel_bg);
                        if focused {
                            cursor = Some((fx + 1, fy));
                        }
                    } else {
                        let lab = format!("{label}: ");
                        let ll = lab.chars().count() as i32;
                        s.text(fx, fy, &lab, t.header, t.panel_bg);
                        ed.draw(s, fx + ll, fy, fw - ll, t.input_fg, t.input_bg);
                        if focused {
                            cursor = s.cursor;
                        }
                    }
                }
                s.cursor = cursor;
                s.text(2, 5, &fit(HINT, (w - 4).max(0) as usize), t.free_label, t.panel_bg);
                // preview: old name -> new name
                s.boxed(0, list_y, w, list_h, t.frame, t.panel_bg, cur_focus == Focus::List);
                let title = if bad > 0 { format!(" {} files, {changed} to rename, {bad} can't ", items_ref.len()) } else { format!(" {} files, {changed} to rename ", items_ref.len()) };
                s.text(2, list_y, &title, if bad > 0 { t.error } else { t.header }, t.panel_bg);
                let half = (w - 2) / 2;
                for (r, (it, row)) in items_ref.iter().zip(rows_ref).skip(cur_top).take(page).enumerate() {
                    let y = list_y + 1 + r as i32;
                    s.text(1, y, &fit(&it.name, (half - 3) as usize), t.file, t.panel_bg);
                    s.text(half - 1, y, "→", t.frame, t.panel_bg);
                    let (txt, fg) = match row.err {
                        Some(e) => (format!("{}  ({e})", row.new), t.error),
                        None if row.new == it.name => (row.new.clone(), t.free_label),
                        None => (row.new.clone(), t.free_num),
                    };
                    s.text(half + 1, y, &fit(&txt, (w - half - 2).max(0) as usize), fg, t.panel_bg);
                }
                draw_key_bar(s, &["Help", "Rename", "", "", "", "", "", "", "", ""]);
            },
            None,
        );
        let mut go = false;
        match inp {
            Input::Paste(p) => {
                if let Focus::Field(i) = focus {
                    if FIELDS[i].1 != Kind::Check {
                        let p = p.replace(['\r', '\n'], "");
                        let p: String = if FIELDS[i].1 == Kind::Number { p.chars().filter(|c| c.is_ascii_digit() || *c == '-').collect() } else { p };
                        edits[i].insert(&p);
                    }
                }
            }
            Input::Mouse(m) => {
                let (mx, my) = (m.column as i32, m.row as i32);
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if is_close_mark(m.column, m.row) {
                            return;
                        }
                        if let Some(i) = pl.iter().position(|&(fx, fy, fw)| my == fy && mx >= fx && mx < fx + fw) {
                            focus = Focus::Field(i);
                            if FIELDS[i].1 == Kind::Check {
                                toggle(&mut edits, i);
                            }
                        } else if my >= list_y {
                            focus = Focus::List;
                        }
                    }
                    MouseEventKind::ScrollUp => top = top.saturating_sub(3),
                    MouseEventKind::ScrollDown => top += 3,
                    _ => {}
                }
            }
            Input::Tick => {}
            Input::Key(k) => match k.code {
                KeyCode::Esc => return,
                KeyCode::F(1) => crate::help::show(app, "rename"),
                KeyCode::F(2) | KeyCode::Enter => go = true,
                KeyCode::Tab | KeyCode::BackTab => {
                    let mut order: Vec<Focus> = (0..FIELDS.len()).map(Focus::Field).collect();
                    order.push(Focus::List);
                    let i = order.iter().position(|f| *f == focus).unwrap_or(0);
                    let n = order.len();
                    focus = order[if k.code == KeyCode::Tab { (i + 1) % n } else { (i + n - 1) % n }];
                }
                _ => match focus {
                    Focus::List => match k.code {
                        KeyCode::Up => top = top.saturating_sub(1),
                        KeyCode::Down => top += 1,
                        KeyCode::PageUp => top = top.saturating_sub(page),
                        KeyCode::PageDown => top += page,
                        KeyCode::Home => top = 0,
                        KeyCode::End => top = items.len(),
                        _ => {}
                    },
                    Focus::Field(i) => match (FIELDS[i].1, k.code) {
                        (Kind::Check, KeyCode::Char(' ')) => toggle(&mut edits, i),
                        (Kind::Check, _) => {}
                        (Kind::Number, KeyCode::Up | KeyCode::Down) => {
                            let n = edits[i].value().trim().parse::<i64>().unwrap_or(0) + if k.code == KeyCode::Up { 1 } else { -1 };
                            edits[i].set(&n.to_string());
                        }
                        (Kind::Number, KeyCode::Char(c)) if !(c.is_ascii_digit() || c == '-') => {}
                        // ↑ ↓ in a text box: the box above / below
                        (Kind::Text, KeyCode::Up) => focus = Focus::Field(above(i, -1)),
                        (Kind::Text, KeyCode::Down) => focus = Focus::Field(above(i, 1)),
                        _ => {
                            edits[i].handle(&k);
                        }
                    },
                },
            },
        }
        if go {
            if bad > 0 {
                dialogs::message(app, "Batch rename", "Some new names can't be used (shown in red). Change the pattern first.", true);
                continue;
            }
            if let Ok(mut l) = LAST.lock() {
                *l = Some(o.clone());
            }
            if changed == 0 {
                return;
            }
            let errors = apply(&items, &rows);
            // keep the cursor on the file it was on, under its new name
            let cur = app.active().current().map(|e| e.name.clone());
            let focus_name = cur.and_then(|c| items.iter().zip(&rows).find(|(it, _)| it.name == c).map(|(_, r)| r.new.clone()));
            app.active().load(focus_name.as_deref());
            app.passive().load(None);
            if !errors.is_empty() {
                let shown: Vec<&str> = errors.iter().take(12).map(String::as_str).collect();
                let more = if errors.len() > 12 { format!("\n... and {} more", errors.len() - 12) } else { String::new() };
                dialogs::message(app, "Batch rename", &format!("{}{more}", shown.join("\n")), true);
            }
            return;
        }
    }
}

fn toggle(edits: &mut [LineEdit], i: usize) {
    let on = edits[i].value() != "1";
    edits[i].set(if on { "1" } else { "0" });
    // lower and UPPER exclude each other
    if on {
        match FIELDS[i].0 {
            "lower case" => edits[8].set("0"),
            "UPPER CASE" => edits[7].set("0"),
            _ => {}
        }
    }
}

/// The first field of the row above (`d` = -1) or below (+1) field `i`, wrapping.
fn above(i: usize, d: i32) -> usize {
    let row = (FIELDS[i].2 + d).rem_euclid(3);
    FIELDS.iter().position(|f| f.2 == row).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::path::Path;

    fn item(dir: &Path, name: &str) -> Item {
        Item { name: name.into(), path: dir.join(name), is_dir: false, modified: Some(Local.with_ymd_and_hms(2026, 11, 19, 8, 5, 9).unwrap()) }
    }

    #[test]
    fn patterns() {
        let d = std::env::temp_dir().join("on-rename-none");
        let it = item(&d, "Holiday Photo.JPG");
        assert_eq!(expand("[N]", &it, 1, 1), "Holiday Photo");
        assert_eq!(expand("[E]", &it, 1, 1), "JPG");
        assert_eq!(expand("[N1-7]_[C]", &it, 7, 3), "Holiday_007");
        assert_eq!(expand("[N9-]", &it, 1, 1), "Photo");
        assert_eq!(expand("[N1]", &it, 1, 1), "H");
        assert_eq!(expand("[DATE] [TIME]", &it, 1, 1), "2026-11-19 08-05-09");
        assert_eq!(expand("[Y][M][D]_[h][m][s]", &it, 1, 1), "20261119_080509");
        assert_eq!(expand("[P]", &it, 1, 1), "on-rename-none");
        assert_eq!(expand("a[x]b[", &it, 1, 1), "a[x]b[");
        let items = vec![item(&d, "a.txt"), item(&d, "b.txt"), item(&d, "c")];
        let o = Opts { name: "file [C]".into(), digits: 2, start: 9, ..Opts::default() };
        let rows = plan(&items, &o);
        assert_eq!(rows.iter().map(|r| r.new.as_str()).collect::<Vec<_>>(), ["file 09.txt", "file 10.txt", "file 11"]);
        assert!(rows.iter().all(|r| r.err.is_none()));
        // same name twice, upper case, find / replace
        let o = Opts { name: "x".into(), ..Opts::default() };
        assert_eq!(plan(&items, &o)[0].err, Some("same name twice"));
        let o = Opts { find: "txt".into(), replace: "md".into(), upper: true, ..Opts::default() };
        assert_eq!(plan(&items, &o)[1].new, "B.MD");
        let o = Opts { name: "a/b".into(), ..Opts::default() };
        assert_eq!(plan(&items[..1], &o)[0].err, Some("bad character"));
    }

    #[test]
    fn rename_on_disk_with_swaps() {
        let d = std::env::temp_dir().join(format!("on-rename-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for (n, body) in [("1.txt", "one"), ("2.txt", "two"), ("other.txt", "x")] {
            std::fs::write(d.join(n), body).unwrap();
        }
        // shift: 1 -> 2, 2 -> 3 (2 is also a source, so this goes through temporary names)
        let items = vec![item(&d, "1.txt"), item(&d, "2.txt")];
        let o = Opts { name: "[C]".into(), start: 2, ..Opts::default() };
        let rows = plan(&items, &o);
        assert!(rows.iter().all(|r| r.err.is_none()), "{rows:?}");
        assert!(apply(&items, &rows).is_empty());
        assert_eq!(std::fs::read_to_string(d.join("2.txt")).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(d.join("3.txt")).unwrap(), "two");
        assert!(!d.join("1.txt").exists());
        // a name taken by a file that isn't renamed
        let items = vec![item(&d, "2.txt")];
        let o = Opts { name: "other".into(), ..Opts::default() };
        assert_eq!(plan(&items, &o)[0].err, Some("exists"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
