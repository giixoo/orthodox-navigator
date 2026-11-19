//! The Temp panel: a list of files and folders from anywhere, gathered to work on them together (copy,
//! view, edit, pack, delete...). Removing an item (F8) only takes it off the list. The list is kept in
//! <config>/orthodox-navigator/temp-panel.txt.

use crate::panel::Entry;
use crate::util::same_path;
use chrono::DateTime;
use std::fs;
use std::path::PathBuf;

fn file() -> Option<PathBuf> {
    crate::usermenu::config_dir().map(|d| d.join("temp-panel.txt"))
}

pub fn load() -> Vec<PathBuf> {
    file().and_then(|f| fs::read_to_string(f).ok()).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from).collect()
}

pub fn save(list: &[PathBuf]) {
    let Some(f) = file() else { return };
    if let Some(d) = f.parent() {
        let _ = fs::create_dir_all(d);
    }
    let _ = fs::write(f, list.iter().map(|p| format!("{}\n", p.display())).collect::<String>());
}

/// Add paths not on the list yet; returns how many were added.
pub fn add(list: &mut Vec<PathBuf>, paths: &[PathBuf]) -> usize {
    let mut n = 0;
    for p in paths {
        if !list.iter().any(|q| same_path(q, p)) {
            list.push(p.clone());
            n += 1;
        }
    }
    n
}

pub fn remove(list: &mut Vec<PathBuf>, paths: &[PathBuf]) {
    list.retain(|q| !paths.iter().any(|p| same_path(p, q)));
}

/// Panel entries for the paths that still exist (named by their full path).
pub fn entries(list: &[PathBuf]) -> Vec<Entry> {
    list.iter()
        .filter_map(|p| {
            let own = fs::symlink_metadata(p).ok()?;
            let md = fs::metadata(p).unwrap_or(own.clone());
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            Some(Entry {
                name: p.display().to_string(),
                path: p.clone(),
                is_dir: md.is_dir(),
                is_up: false,
                is_link: own.file_type().is_symlink(),
                selected: false,
                size: (!md.is_dir()).then_some(md.len()),
                modified: md.modified().ok().map(DateTime::from),
                hidden: crate::platform::is_hidden(&name, &own),
                exec: crate::platform::is_exec(&md),
                arc: None,
            })
        })
        .collect()
}

pub fn title(n: usize) -> String {
    format!("Temp panel ({n})")
}
