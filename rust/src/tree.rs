//! Directory tree (Alt+F10): browse folders as a tree, Enter jumps the panel there.
//! ↑↓ PgUp PgDn Home End move, → or + expand, ← or - collapse / go to parent,
//! Enter choose, Esc cancel. Mouse: click selects, double-click chooses, wheel scrolls.

use crate::app::{draw_key_bar, App, Input};
use crate::dialogs;
use crate::screen::Screen;
use crate::theme;
use crate::util::fit;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct Node {
    path: PathBuf,
    name: String,
    depth: usize,
    expanded: bool,
    /// has sub-folders (shows [+] / [-])
    has_kids: bool,
}

/// Does p contain at least one folder? (stops at the first one)
fn has_subdirs(p: &Path) -> bool {
    fs::read_dir(p).map(|rd| rd.flatten().any(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))).unwrap_or(false)
}

fn subdirs(p: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(p) else { return vec![] };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    v.sort_by_cached_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default());
    v
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string_lossy().to_string())
}

struct Tree {
    nodes: Vec<Node>,
    cursor: usize,
    top: usize,
}

impl Tree {
    /// Build the tree from the root of `start`'s drive, expanded down to `start`.
    fn new(start: &Path) -> Self {
        let root = start.ancestors().last().unwrap_or(start).to_path_buf();
        let mut t = Tree { nodes: vec![Node { name: name_of(&root), has_kids: true, path: root, depth: 0, expanded: false }], cursor: 0, top: 0 };
        let mut i = 0;
        let chain: Vec<&Path> = start.ancestors().collect::<Vec<_>>().into_iter().rev().skip(1).collect();
        for want in chain {
            t.expand(i);
            match (i + 1..t.nodes.len()).find(|&j| t.nodes[j].depth == t.nodes[i].depth + 1 && t.nodes[j].path == want) {
                Some(j) => i = j,
                None => break,
            }
        }
        t.cursor = i;
        t
    }

    fn expand(&mut self, i: usize) {
        if self.nodes[i].expanded {
            return;
        }
        self.nodes[i].expanded = true;
        let depth = self.nodes[i].depth + 1;
        let kids: Vec<Node> = subdirs(&self.nodes[i].path)
            .into_iter()
            .map(|p| Node { name: name_of(&p), has_kids: has_subdirs(&p), path: p, depth, expanded: false })
            .collect();
        self.nodes[i].has_kids = !kids.is_empty();
        self.nodes.splice(i + 1..i + 1, kids);
    }

    fn collapse(&mut self, i: usize) {
        if !self.nodes[i].expanded {
            return;
        }
        self.nodes[i].expanded = false;
        let d = self.nodes[i].depth;
        let end = (i + 1..self.nodes.len()).find(|&j| self.nodes[j].depth <= d).unwrap_or(self.nodes.len());
        self.nodes.drain(i + 1..end);
    }

    fn parent(&self, i: usize) -> Option<usize> {
        let d = self.nodes[i].depth;
        (0..i).rev().find(|&j| self.nodes[j].depth < d)
    }

    /// Is node `i` the last child of its parent? (decides └ vs ├)
    fn is_last(&self, i: usize) -> bool {
        let d = self.nodes[i].depth;
        for j in i + 1..self.nodes.len() {
            if self.nodes[j].depth < d {
                return true;
            }
            if self.nodes[j].depth == d {
                return false;
            }
        }
        true
    }

    /// "│ ├─" style prefix for node `i`.
    fn prefix(&self, i: usize) -> String {
        let d = self.nodes[i].depth;
        if d == 0 {
            return String::new();
        }
        let mut parts = vec![String::new(); d];
        parts[d - 1] = if self.is_last(i) { "└─".into() } else { "├─".into() };
        let mut j = i;
        for level in (0..d - 1).rev() {
            j = self.parent(j).unwrap_or(0);
            parts[level] = if self.is_last(j) { "  ".into() } else { "│ ".into() };
        }
        parts.concat()
    }

    /// [+] can be expanded, [-] can be collapsed, nothing for a folder without sub-folders.
    fn marker(&self, i: usize) -> &'static str {
        let n = &self.nodes[i];
        if !n.has_kids {
            "    "
        } else if n.expanded {
            "[-] "
        } else {
            "[+] "
        }
    }

    fn toggle(&mut self, i: usize) {
        if self.nodes[i].expanded {
            self.collapse(i);
        } else {
            self.expand(i);
        }
    }

    fn clamp(&mut self, rows: usize) {
        self.cursor = self.cursor.min(self.nodes.len().saturating_sub(1));
        if self.cursor < self.top {
            self.top = self.cursor;
        }
        if self.cursor >= self.top + rows {
            self.top = self.cursor + 1 - rows;
        }
    }
}

pub fn show(app: &mut App, start: &Path) -> Option<PathBuf> {
    let mut t = Tree::new(start);
    let mut last_click: Option<(Instant, usize)> = None;
    loop {
        let (sw, sh) = (app.screen.w, app.screen.h);
        let w = (sw - 4).min(76);
        let h = (sh - 3).max(8);
        let (x, y) = (((sw - w) / 2).max(0), 0);
        let rows = (h - 4).max(1) as usize;
        t.clamp(rows);
        let tr = &t;
        let input = app.window_input(
            &mut |s: &mut Screen| {
                let th = theme::current();
                dialogs::frame_at(s, x, y, w, h, "Directory tree");
                s.fill(x + 2, y + 1, w - 4, h - 2, ' ', th.file, th.panel_bg);
                s.boxed(x + 2, y + 1, w - 4, h - 2, th.frame, th.panel_bg, th.double_frame);
                let path = tr.nodes[tr.cursor].path.to_string_lossy().to_string();
                s.text(x + 4, y + 1, &format!(" {} ", crate::util::fit_left(&path, (w - 10) as usize)), th.active_title_fg, th.active_title_bg);
                for r in 0..rows {
                    let i = tr.top + r;
                    let Some(n) = tr.nodes.get(i) else { break };
                    let cur = i == tr.cursor;
                    let prefix = format!("{}{}", tr.prefix(i), tr.marker(i));
                    let line = format!("{prefix}{}", n.name);
                    let (fg, bg) = if cur { (th.cursor_fg, th.cursor_bg) } else { (th.dir, th.panel_bg) };
                    s.text(x + 4, y + 2 + r as i32, &fit(&line, (w - 8) as usize), fg, bg);
                    if !cur {
                        // recolour only the tree lines; `fit` would pad with spaces over the name
                        let lines: String = prefix.chars().take((w - 8).max(0) as usize).collect();
                        s.text(x + 4, y + 2 + r as i32, &lines, th.frame, th.panel_bg);
                    }
                }
                s.cursor = None;
                draw_key_bar(s, &["Help", "", "", "", "", "", "", "", "", ""]);
            },
            None,
        );
        match input {
            Input::Key(k) => match k.code {
                KeyCode::Esc => return None,
                KeyCode::Enter => return Some(t.nodes[t.cursor].path.clone()),
                KeyCode::Up => t.cursor = t.cursor.saturating_sub(1),
                KeyCode::Down => t.cursor += 1,
                KeyCode::PageUp => t.cursor = t.cursor.saturating_sub(rows),
                KeyCode::PageDown => t.cursor += rows,
                KeyCode::Home => t.cursor = 0,
                KeyCode::End => t.cursor = t.nodes.len() - 1,
                KeyCode::Right | KeyCode::Char('+') => {
                    if t.nodes[t.cursor].expanded {
                        if t.nodes.get(t.cursor + 1).is_some_and(|n| n.depth > t.nodes[t.cursor].depth) {
                            t.cursor += 1;
                        }
                    } else {
                        t.expand(t.cursor);
                    }
                }
                KeyCode::Left | KeyCode::Char('-') => {
                    if t.nodes[t.cursor].expanded {
                        t.collapse(t.cursor);
                    } else if let Some(p) = t.parent(t.cursor) {
                        t.cursor = p;
                    }
                }
                KeyCode::F(1) => crate::help::show(app, "tree"),
                _ => {}
            },
            Input::Mouse(m) => {
                let (mx, my) = (m.column as i32, m.row as i32);
                match m.kind {
                    MouseEventKind::ScrollUp => t.cursor = t.cursor.saturating_sub(3),
                    MouseEventKind::ScrollDown => t.cursor += 3,
                    MouseEventKind::Down(MouseButton::Left) if mx > x + 2 && mx < x + w - 3 && my >= y + 2 && my < y + 2 + rows as i32 => {
                        let i = t.top + (my - y - 2) as usize;
                        // a click on [+] / [-] expands / collapses
                        let mark_x = x + 4 + t.prefix(i.min(t.nodes.len().saturating_sub(1))).chars().count() as i32;
                        if i < t.nodes.len() && t.nodes[i].has_kids && (mark_x..mark_x + 3).contains(&mx) {
                            t.cursor = i;
                            t.toggle(i);
                            continue;
                        }
                        if i < t.nodes.len() {
                            let double = last_click.is_some_and(|(at, j)| j == i && at.elapsed().as_millis() < 400);
                            t.cursor = i;
                            if double {
                                return Some(t.nodes[i].path.clone());
                            }
                            last_click = Some((Instant::now(), i));
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) if dialogs::is_close_click(x, y, mx, my) => return None,
                    _ => {}
                }
            }
            Input::Tick | Input::Paste(_) => {}
        }
    }
}
