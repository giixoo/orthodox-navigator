//! Command history for the command line (Ctrl+E previous, Ctrl+Shift+E next).
//! Starts from the shell's own history file (PSReadLine, bash or zsh), so the commands
//! you typed in the shell are there too. Commands run from ON are also kept in ON's own file, because
//! ON restarts after each command under the shell wrapper and bash writes its history only on exit.

use std::path::PathBuf;

/// Most commands kept.
const MAX: usize = 1000;

pub struct History {
    items: Vec<String>,
    loaded: bool,
    /// Index of the command shown on the command line, None = not browsing
    pos: Option<usize>,
    /// What was typed before browsing started, restored after the newest command
    draft: String,
}

impl History {
    pub fn new() -> Self {
        History { items: Vec::new(), loaded: false, pos: None, draft: String::new() }
    }

    fn load(&mut self, shell: &str) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let mut items = history_file(shell).and_then(|f| std::fs::read(f).ok()).map(|b| parse(&String::from_utf8_lossy(&b), shell.contains("zsh"))).unwrap_or_default();
        let own = std::fs::read_to_string(own_file().unwrap_or_default()).unwrap_or_default();
        let own: Vec<&str> = own.lines().filter(|l| !l.trim().is_empty()).collect();
        if own.len() > 2 * MAX {
            if let Some(f) = own_file() {
                let _ = std::fs::write(f, own[own.len() - MAX..].join("\n") + "\n");
            }
        }
        items.extend(own.iter().map(|l| l.trim().to_string()));
        // commands run before the first Ctrl+E come after the files'
        items.append(&mut self.items);
        self.items = items;
        self.tidy();
    }

    /// Drop older copies of repeated commands and keep the newest MAX.
    fn tidy(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut v: Vec<String> = self.items.drain(..).rev().filter(|c| seen.insert(c.clone())).take(MAX).collect();
        v.reverse();
        self.items = v;
    }

    /// A command was run: add it and stop browsing.
    pub fn add(&mut self, command: &str) {
        let c = command.trim();
        self.pos = None;
        if c.is_empty() {
            return;
        }
        self.items.retain(|x| x != c);
        self.items.push(c.to_string());
        if let Some(f) = own_file() {
            use std::io::Write;
            let _ = std::fs::create_dir_all(f.parent().unwrap_or(&f));
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&f) {
                let _ = writeln!(file, "{}", c.replace(['\r', '\n'], " "));
            }
        }
    }

    /// The line was edited by hand: the next Ctrl+E starts from the newest command again.
    pub fn reset(&mut self) {
        self.pos = None;
    }

    /// The previous (older) command, given the current line.
    pub fn prev(&mut self, shell: &str, line: &str) -> Option<String> {
        self.load(shell);
        if self.items.is_empty() {
            return None;
        }
        let p = match self.pos {
            None => {
                self.draft = line.to_string();
                self.items.len() - 1
            }
            Some(p) => p.saturating_sub(1),
        };
        self.pos = Some(p);
        Some(self.items[p].clone())
    }

    /// The next (newer) command; after the newest, what was typed before browsing.
    pub fn next(&mut self) -> Option<String> {
        let p = self.pos?;
        if p + 1 < self.items.len() {
            self.pos = Some(p + 1);
            Some(self.items[p + 1].clone())
        } else {
            self.pos = None;
            Some(std::mem::take(&mut self.draft))
        }
    }
}

fn own_file() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    dirs::config_dir().map(|d| d.join("orthodox-navigator").join("history.txt"))
}

fn history_file(shell: &str) -> Option<PathBuf> {
    if crate::platform::is_powershell(shell) {
        // PSReadLine keeps one file for Windows PowerShell and pwsh
        let base = if cfg!(windows) {
            PathBuf::from(std::env::var_os("APPDATA")?).join("Microsoft").join("Windows").join("PowerShell")
        } else {
            PathBuf::from(std::env::var_os("HOME")?).join(".local").join("share").join("powershell")
        };
        return Some(base.join("PSReadLine").join("ConsoleHost_history.txt"));
    }
    if let Some(f) = std::env::var_os("HISTFILE") {
        return Some(PathBuf::from(f));
    }
    let home = PathBuf::from(std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?);
    Some(home.join(if shell.contains("zsh") { ".zsh_history" } else { ".bash_history" }))
}

/// One command per line. PSReadLine ends continued lines with a backtick; zsh's extended
/// format starts each line with ": <time>:<duration>;".
fn parse(text: &str, zsh: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        let mut l = line;
        if zsh && l.starts_with(": ") {
            if let Some(i) = l.find(';') {
                l = &l[i + 1..];
            }
        }
        if let Some(head) = l.strip_suffix('`').or_else(|| l.strip_suffix('\\')) {
            cur.push_str(head);
            cur.push(' ');
            continue;
        }
        cur.push_str(l);
        let c = cur.trim();
        if !c.is_empty() {
            out.push(c.to_string());
        }
        cur.clear();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_browses() {
        assert_eq!(parse("ls\ngit status `\n  -s\n\n", false), vec!["ls", "git status    -s"]);
        assert_eq!(parse(": 1700000000:0;make\nls\n", true), vec!["make", "ls"]);

        let mut h = History { items: vec!["a".into(), "b".into(), "a".into(), "c".into()], loaded: true, pos: None, draft: String::new() };
        h.tidy();
        assert_eq!(h.items, vec!["b", "a", "c"]);
        assert_eq!(h.prev("bash", "typed").as_deref(), Some("c"));
        assert_eq!(h.prev("bash", "").as_deref(), Some("a"));
        assert_eq!(h.prev("bash", "").as_deref(), Some("b"));
        assert_eq!(h.prev("bash", "").as_deref(), Some("b"));
        assert_eq!(h.next().as_deref(), Some("a"));
        assert_eq!(h.next().as_deref(), Some("c"));
        assert_eq!(h.next().as_deref(), Some("typed"));
        assert_eq!(h.next(), None);
        h.add("b");
        assert_eq!(h.items, vec!["a", "c", "b"]);
    }
}
