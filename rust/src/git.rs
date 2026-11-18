//! Git: status marks for the panels, and a file's text at HEAD (for the diff).
//! Uses the `git` program; does nothing outside a repository or when git isn't installed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The repository folder containing `dir`, found by looking for `.git` upwards (no process started).
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find(|a| a.join(".git").exists()).map(Path::to_path_buf)
}

/// The checked-out branch of the repository containing `dir` (or a short commit id when detached).
/// Reads `.git/HEAD` directly, so it is cheap enough to call on every folder change.
pub fn branch(dir: &Path) -> Option<String> {
    let root = repo_root(dir)?;
    let dot = root.join(".git");
    // a worktree or submodule has a `.git` file: "gitdir: <path>"
    let git_dir = if dot.is_file() {
        let text = std::fs::read_to_string(&dot).ok()?;
        let p = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
        if p.is_absolute() { p } else { root.join(p) }
    } else {
        dot
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref:") {
        Some(r) => {
            let r = r.trim();
            Some(r.strip_prefix("refs/heads/").unwrap_or(r).to_string())
        }
        None => Some(head.chars().take(7).collect()),
    }
}

fn git(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

/// One mark per changed file or folder directly in `dir`:
/// M modified, A added, ? untracked, R renamed, U conflict; a folder with changes inside gets '•'.
pub fn status(dir: &Path) -> HashMap<String, char> {
    let mut map = HashMap::new();
    let Some(root) = repo_root(dir) else { return map };
    let Ok(out) = git(&root).args(["status", "--porcelain=v1", "-z"]).output() else { return map };
    if !out.status.success() {
        return map;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let rel_dir = dir.strip_prefix(&root).unwrap_or(Path::new(""));
    let mut fields = text.split('\0');
    while let Some(f) = fields.next() {
        if f.len() < 4 {
            continue;
        }
        let (x, y) = (f.as_bytes()[0] as char, f.as_bytes()[1] as char);
        if x == 'R' || x == 'C' {
            fields.next(); // the old name
        }
        let path = Path::new(&f[3..]);
        let mark = match (x, y) {
            ('?', _) => '?',
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => 'U',
            ('A', _) => 'A',
            ('R', _) => 'R',
            ('D', _) | (_, 'D') => 'D',
            _ => 'M',
        };
        let Ok(inside) = path.strip_prefix(rel_dir) else { continue };
        let mut comps = inside.components();
        let Some(first) = comps.next() else { continue };
        let name = first.as_os_str().to_string_lossy().to_string();
        if comps.next().is_some() {
            map.entry(name).or_insert('•');
        } else {
            map.insert(name, mark);
        }
    }
    map
}

/// The file's text as committed (HEAD), or None if it isn't in git.
pub fn head_text(file: &Path) -> Option<String> {
    let root = repo_root(file.parent()?)?;
    let rel = file.strip_prefix(&root).ok()?.to_string_lossy().replace('\\', "/");
    let out = git(&root).args(["show", &format!("HEAD:{rel}")]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
