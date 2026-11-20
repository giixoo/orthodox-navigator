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
    file().map(|f| load_at(&f)).unwrap_or_default()
}

fn load_at(f: &std::path::Path) -> Vec<PathBuf> {
    fs::read_to_string(f).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from).collect()
}

/// Change the list on disk: read it as it is now (another ON window may have changed it), apply
/// `change`, write it back. Returns the new list and what `change` returned.
pub fn update<T>(change: impl FnOnce(&mut Vec<PathBuf>) -> T) -> (Vec<PathBuf>, T) {
    match file() {
        Some(f) => update_at(&f, change),
        None => {
            let mut l = vec![];
            let r = change(&mut l);
            (l, r)
        }
    }
}

pub fn update_at<T>(f: &std::path::Path, change: impl FnOnce(&mut Vec<PathBuf>) -> T) -> (Vec<PathBuf>, T) {
    let mut list = load_at(f);
    let r = change(&mut list);
    if let Some(d) = f.parent() {
        let _ = fs::create_dir_all(d);
    }
    let _ = fs::write(f, list.iter().map(|p| format!("{}\n", p.display())).collect::<String>());
    (list, r)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_windows_keep_each_others_files() {
        let f = std::env::temp_dir().join(format!("on-temp-test-{}.txt", std::process::id()));
        let _ = fs::remove_file(&f);
        // window A and window B each add a file; neither loses the other's
        update_at(&f, |l| add(l, &[PathBuf::from("/a/one")]));
        let (list, _) = update_at(&f, |l| add(l, &[PathBuf::from("/b/two")]));
        assert_eq!(list, vec![PathBuf::from("/a/one"), PathBuf::from("/b/two")]);
        let (list, _) = update_at(&f, |l| remove(l, &[PathBuf::from("/a/one")]));
        assert_eq!(list, vec![PathBuf::from("/b/two")]);
        let _ = fs::remove_file(&f);
    }
}
