//! Alt+Backspace: the folders visited before, newest first. Kept in <config>/orthodox-navigator/folders.txt.

use crate::picklist::{self, Opts, Row};
use crate::util::same_path;
use crossterm::event::KeyCode;
use std::fs;
use std::path::{Path, PathBuf};

const MAX: usize = 200;

fn file() -> Option<PathBuf> {
    crate::usermenu::config_dir().map(|d| d.join("folders.txt"))
}

pub fn load() -> Vec<PathBuf> {
    file().and_then(|f| fs::read_to_string(f).ok()).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from).collect()
}

pub fn save(list: &[PathBuf]) {
    let Some(f) = file() else { return };
    if let Some(d) = f.parent() {
        let _ = fs::create_dir_all(d);
    }
    let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    let _ = fs::write(f, text);
}

/// The history to save: the file as it is now (another ON window may have added to it), without the
/// folders this window removed, with this window's visits (oldest first) put on top.
pub fn merge(disk: Vec<PathBuf>, added: &[PathBuf], removed: &[PathBuf]) -> Vec<PathBuf> {
    let mut l = disk;
    l.retain(|p| !removed.iter().any(|r| same_path(p, r)));
    for a in added {
        visit(&mut l, a);
    }
    l
}

/// Take one folder off the saved history now.
fn remove_saved(dir: &Path) {
    let mut l = load();
    l.retain(|p| !same_path(p, dir));
    save(&l);
}

/// Put `dir` first (once).
pub fn visit(list: &mut Vec<PathBuf>, dir: &Path) {
    list.retain(|p| !same_path(p, dir));
    list.insert(0, dir.to_path_buf());
    list.truncate(MAX);
}

/// The history list: Enter goes to a folder, Del removes it, typing filters. Returns the folder.
pub fn pick(app: &mut crate::app::App, list: &mut Vec<PathBuf>, current: &Path) -> Option<PathBuf> {
    loop {
        let shown: Vec<usize> = (0..list.len()).filter(|&i| !same_path(&list[i], current)).collect();
        let rows: Vec<Row> = shown.iter().map(|&i| {
            let p = &list[i];
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string());
            Row::item(name, p.display().to_string())
        }).collect();
        let o = Opts {
            title: "Folder history",
            start: 0,
            keys: &[KeyCode::Delete, KeyCode::F(8)],
            bar: ["Help", "", "", "", "", "", "", "Delete", "", ""],
            help: "panels",
            filter: true,
        };
        let p = picklist::pick(app, &rows, &o)?;
        let Some(&i) = shown.get(p.row) else { continue };
        match p.key {
            KeyCode::Enter => return Some(list[i].clone()),
            _ => {
                let gone = list.remove(i);
                remove_saved(&gone);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_other_windows_visits() {
        let p = |s: &str| PathBuf::from(s);
        // on disk (another window saved it): a, b; this window visited c then d, and removed b
        let m = merge(vec![p("/a"), p("/b")], &[p("/c"), p("/d")], &[p("/b")]);
        assert_eq!(m, vec![p("/d"), p("/c"), p("/a")]);
    }
}
