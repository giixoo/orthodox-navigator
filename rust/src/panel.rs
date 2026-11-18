//! One file panel: listing, sorting, selection, cursor and drawing.

use crate::archive::{self, ArcEntry};
use crate::platform;
use crate::screen::{Screen, C};
use crate::theme;
use crate::util::{center, fit, fit_left, human, n0, normalize, pad_left, size_col};
use chrono::{DateTime, Local};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::rc::Rc;

#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_up: bool,
    pub is_link: bool,
    pub selected: bool,
    /// None for folders until their size is calculated.
    pub size: Option<u64>,
    pub modified: Option<DateTime<Local>>,
    pub hidden: bool,
    pub exec: bool,
    /// Path inside the archive, for entries of a panel that browses an archive.
    pub arc: Option<String>,
}

impl Entry {
    /// Lower-case extension with the dot (".txt"), empty for folders and dot-files.
    pub fn ext(&self) -> String {
        if self.is_dir {
            return String::new();
        }
        match self.name.rfind('.') {
            Some(i) if i > 0 => self.name[i..].to_lowercase(),
            _ => String::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Name,
    Ext,
    Size,
    Time,
    Unsorted,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Full,
    Brief,
}

/// A panel that is browsing inside an archive.
#[derive(Clone)]
pub struct ArcView {
    pub file: PathBuf,
    /// Folder inside the archive: "" for the root, otherwise "dir/sub/".
    pub prefix: String,
    pub entries: Rc<Vec<ArcEntry>>,
}

/// Where the panel was last drawn, for mouse hit-testing.
#[derive(Default)]
struct Geom {
    list_top: i32,
    cols: Vec<(i32, i32)>,
}

pub struct Panel {
    pub dir: PathBuf,
    pub items: Vec<Entry>,
    pub cursor: usize,
    pub top: usize,
    pub sort: SortMode,
    pub sort_desc: bool,
    pub view: ViewMode,
    pub show_hidden: bool,
    pub error: Option<String>,
    pub free: Option<u64>,
    pub volume: String,
    pub rows: usize,
    cols: usize,
    pub quick_pos: (i32, i32),
    pub arc: Option<ArcView>,
    geom: Geom,
    /// git status mark per name (see git::status)
    pub git: std::collections::HashMap<String, char>,
    /// the git branch checked out here, if the folder is in a repository
    pub branch: Option<String>,
    /// Showing search results (Alt+F7) instead of the folder; the text describes the search.
    pub found: Option<String>,
}

/// Narrowest a name column may get, in characters.
const MIN_COL: i32 = 20;

const EXEC_EXT: &[&str] = &[".exe", ".com", ".bat", ".cmd", ".ps1", ".msi", ".sh"];
const ARCHIVE_EXT: &[&str] = &[".zip", ".rar", ".7z", ".arj", ".gz", ".tar", ".cab", ".lzh", ".tgz", ".ha", ".xz", ".bz2", ".zst"];
const DOC_EXT: &[&str] = &[".txt", ".diz", ".doc", ".docx", ".md", ".me", ".nfo", ".rtf", ".pdf", ".htm", ".html", ".ini"];
const IMAGE_EXT: &[&str] = &[".ico", ".bmp", ".gif", ".png", ".jpg", ".jpeg", ".pcx", ".svg", ".webp", ".tif"];
const JUNK_EXT: &[&str] = &[".bak", ".tmp", ".log", ".old"];

pub fn is_exec_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    EXEC_EXT.iter().any(|e| lower.ends_with(e))
}

fn color_of(e: &Entry) -> C {
    let t = theme::current();
    if e.is_up {
        return t.up;
    }
    if e.is_dir {
        return t.dir;
    }
    if e.hidden {
        return t.hidden;
    }
    let ext = e.ext();
    let ext = ext.as_str();
    if e.exec || EXEC_EXT.contains(&ext) {
        t.exec
    } else if ARCHIVE_EXT.contains(&ext) {
        t.archive
    } else if DOC_EXT.contains(&ext) {
        t.doc
    } else if IMAGE_EXT.contains(&ext) {
        t.image
    } else if JUNK_EXT.contains(&ext) {
        t.junk
    } else {
        t.file
    }
}

/// The full file name, cut to the column width.
fn show_name(e: &Entry, w: usize) -> String {
    fit(&e.name, w)
}

fn size_text(e: &Entry, w: usize) -> String {
    if e.is_up {
        "<UP-DIR>".into()
    } else if e.is_dir && e.size.is_none() {
        "<SUB-DIR>".into()
    } else {
        size_col(e.size.unwrap_or(0), w)
    }
}

impl Panel {
    pub fn new(dir: PathBuf) -> Self {
        Panel {
            dir: normalize(&dir),
            items: vec![],
            cursor: 0,
            top: 0,
            sort: SortMode::Name,
            sort_desc: false,
            view: ViewMode::Full,
            show_hidden: true,
            error: None,
            free: None,
            volume: String::new(),
            rows: 1,
            cols: 1,
            quick_pos: (0, 0),
            arc: None,
            geom: Geom::default(),
            git: Default::default(),
            branch: None,
            found: None,
        }
    }

    pub fn current(&self) -> Option<&Entry> {
        self.items.get(self.cursor)
    }

    pub fn selected_count(&self) -> usize {
        self.items.iter().filter(|e| e.selected).count()
    }

    pub fn page_size(&self) -> usize {
        (self.rows * self.cols).max(1)
    }

    /// Selected items, or the item under the cursor if nothing is selected.
    pub fn targets(&self) -> Vec<Entry> {
        let sel: Vec<Entry> = self.items.iter().filter(|e| e.selected).cloned().collect();
        if !sel.is_empty() {
            return sel;
        }
        match self.current() {
            Some(e) if !e.is_up => vec![e.clone()],
            _ => vec![],
        }
    }

    pub fn change_dir(&mut self, dir: &Path, focus: Option<&str>) -> bool {
        let full = normalize(dir);
        if !full.is_dir() {
            return false;
        }
        self.arc = None;
        self.found = None;
        self.dir = full;
        self.items.clear();
        self.top = 0;
        self.cursor = 0;
        self.load(focus);
        true
    }

    pub fn load(&mut self, focus: Option<&str>) {
        let focus: Option<String> = focus.map(str::to_string).or_else(|| self.current().map(|e| e.name.clone()));
        let keep: HashSet<String> = self.items.iter().filter(|e| e.selected).map(|e| e.name.to_lowercase()).collect();
        let mut list = Vec::new();
        self.error = None;
        if self.arc.is_some() {
            self.load_archive(focus, &keep);
            return;
        }
        if self.found.is_some() {
            // search results: drop what no longer exists
            self.items.retain(|e| e.is_up || e.path.exists());
            self.clamp();
            return;
        }

        if let Some(parent) = self.dir.parent() {
            list.push(Entry {
                name: "..".into(),
                path: parent.to_path_buf(),
                is_dir: true,
                is_up: true,
                is_link: false,
                selected: false,
                size: None,
                modified: fs::metadata(&self.dir).and_then(|m| m.modified()).ok().map(DateTime::from),
                hidden: false,
                exec: false,
                arc: None,
            });
        }

        match fs::read_dir(&self.dir) {
            Ok(rd) => {
                for de in rd.flatten() {
                    let name = de.file_name().to_string_lossy().to_string();
                    let Ok(own) = de.metadata() else { continue };
                    let is_link = own.file_type().is_symlink();
                    // follow links so a link to a folder behaves like a folder
                    let md = if is_link { fs::metadata(de.path()).unwrap_or(own.clone()) } else { own.clone() };
                    let hidden = platform::is_hidden(&name, &own);
                    if hidden && !self.show_hidden {
                        continue;
                    }
                    let is_dir = md.is_dir();
                    list.push(Entry {
                        selected: keep.contains(&name.to_lowercase()),
                        path: de.path(),
                        is_dir,
                        is_up: false,
                        is_link,
                        size: if is_dir { None } else { Some(md.len()) },
                        modified: md.modified().ok().map(DateTime::from),
                        hidden,
                        exec: platform::is_exec(&md),
                        arc: None,
                        name,
                    });
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }

        self.git = crate::git::status(&self.dir);
        self.branch = crate::git::branch(&self.dir);
        self.free = platform::free_space(&self.dir);
        self.volume = platform::volume_of(&self.dir);
        self.items = list;
        self.apply_sort();
        self.cursor = focus
            .and_then(|f| self.items.iter().position(|e| e.name.eq_ignore_ascii_case(&f) || e.name == f))
            .unwrap_or(0);
        self.clamp();
    }

    /// Start browsing inside an archive file.
    pub fn enter_archive(&mut self, file: &Path) -> Result<(), String> {
        let entries = archive::list(file).map_err(|e| e.to_string())?;
        self.dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        self.arc = Some(ArcView { file: file.to_path_buf(), prefix: String::new(), entries: Rc::new(entries) });
        self.items.clear();
        self.top = 0;
        self.cursor = 0;
        self.load(None);
        Ok(())
    }

    /// Open a folder inside the archive ("dir/sub").
    pub fn archive_cd(&mut self, inner: &str, focus: Option<&str>) {
        if let Some(av) = self.arc.as_mut() {
            av.prefix = if inner.is_empty() { String::new() } else { format!("{}/", inner.trim_end_matches('/')) };
        }
        self.items.clear();
        self.top = 0;
        self.cursor = 0;
        self.load(focus);
    }

    /// ".." inside an archive: up one folder, or out of the archive at its root.
    pub fn archive_up(&mut self) {
        let Some(av) = self.arc.clone() else { return };
        if av.prefix.is_empty() {
            let name = av.file.file_name().map(|n| n.to_string_lossy().to_string());
            let parent = av.file.parent().map(Path::to_path_buf).unwrap_or_default();
            self.change_dir(&parent, name.as_deref());
            return;
        }
        let p = av.prefix.trim_end_matches('/');
        let (up, focus) = match p.rfind('/') {
            Some(i) => (p[..i].to_string(), p[i + 1..].to_string()),
            None => (String::new(), p.to_string()),
        };
        self.archive_cd(&up, Some(&focus));
    }

    pub fn find_arc_entry(&self, inner: &str) -> Option<&ArcEntry> {
        self.arc.as_ref()?.entries.iter().find(|e| e.path == inner)
    }

    fn load_archive(&mut self, focus: Option<String>, keep: &HashSet<String>) {
        let av = self.arc.clone().expect("archive view");
        let mut list = vec![Entry {
            name: "..".into(),
            path: av.file.clone(),
            is_dir: true,
            is_up: true,
            is_link: false,
            selected: false,
            size: None,
            modified: None,
            hidden: false,
            exec: false,
            arc: None,
        }];
        let mut seen: HashSet<String> = HashSet::new();
        for ae in av.entries.iter() {
            let Some(rest) = ae.path.strip_prefix(av.prefix.as_str()) else { continue };
            if rest.is_empty() {
                continue;
            }
            let (first, deeper) = match rest.find('/') {
                Some(i) => (&rest[..i], true),
                None => (rest, false),
            };
            if !seen.insert(first.to_string()) {
                continue;
            }
            let inner = format!("{}{}", av.prefix, first);
            let is_dir = deeper || ae.is_dir;
            list.push(Entry {
                name: first.to_string(),
                path: av.file.join(inner.replace('/', std::path::MAIN_SEPARATOR_STR)),
                is_dir,
                is_up: false,
                is_link: false,
                selected: keep.contains(&first.to_lowercase()),
                size: if is_dir { None } else { Some(ae.size) },
                modified: if deeper { None } else { ae.modified },
                hidden: false,
                exec: false,
                arc: Some(inner),
            });
        }
        self.git = crate::git::status(&self.dir);
        self.branch = crate::git::branch(&self.dir);
        self.free = platform::free_space(&self.dir);
        self.volume = platform::volume_of(&self.dir);
        self.items = list;
        self.apply_sort();
        self.cursor = focus.and_then(|f| self.items.iter().position(|e| e.name == f)).unwrap_or(0);
        self.clamp();
    }

    /// The path shown in the title: the folder, or "archive.zip/inner/path" inside an archive.
    /// Show search results (paths relative to this panel's folder) instead of the folder.
    pub fn set_results(&mut self, title: String, results: Vec<Entry>) {
        let mut list = vec![Entry {
            name: "..".into(),
            path: self.dir.clone(),
            is_dir: true,
            is_up: true,
            is_link: false,
            selected: false,
            size: None,
            modified: None,
            hidden: false,
            exec: false,
            arc: None,
        }];
        list.extend(results);
        self.arc = None;
        self.found = Some(title);
        self.items = list;
        self.git.clear();
        self.apply_sort();
        self.top = 0;
        self.cursor = 0;
        self.clamp();
    }

    /// Leave search results, back to the folder.
    pub fn close_results(&mut self) {
        self.found = None;
        self.load(None);
    }

    pub fn title_path(&self) -> String {
        if let Some(f) = &self.found {
            return f.clone();
        }
        match &self.arc {
            Some(av) => {
                let inner = av.prefix.trim_end_matches('/').replace('/', std::path::MAIN_SEPARATOR_STR);
                if inner.is_empty() {
                    av.file.to_string_lossy().to_string()
                } else {
                    format!("{}{}{}", av.file.to_string_lossy(), MAIN_SEPARATOR, inner)
                }
            }
            None => self.dir.to_string_lossy().to_string(),
        }
    }

    /// The item at a screen position (as last drawn), if any.
    pub fn hit(&self, mx: i32, my: i32) -> Option<usize> {
        let g = &self.geom;
        if my < g.list_top || my >= g.list_top + self.rows as i32 {
            return None;
        }
        for (c, (cx, cw)) in g.cols.iter().enumerate() {
            if mx >= *cx && mx < cx + cw {
                let idx = self.top + c * self.rows + (my - g.list_top) as usize;
                return (idx < self.items.len()).then_some(idx);
            }
        }
        None
    }

    pub fn set_sort(&mut self, m: SortMode) {
        if self.sort == m {
            self.sort_desc = !self.sort_desc;
        } else {
            self.sort = m;
            self.sort_desc = false;
        }
        let name = self.current().map(|e| e.name.clone());
        self.apply_sort();
        if let Some(n) = name {
            self.cursor = self.items.iter().position(|e| e.name == n).unwrap_or(0);
        }
        self.clamp();
    }

    pub fn toggle_view(&mut self) {
        self.view = if self.view == ViewMode::Full { ViewMode::Brief } else { ViewMode::Full };
        self.clamp();
    }

    fn apply_sort(&mut self) {
        let (mut up, mut dirs, mut files) = (vec![], vec![], vec![]);
        for e in self.items.drain(..) {
            if e.is_up {
                up.push(e)
            } else if e.is_dir {
                dirs.push(e)
            } else {
                files.push(e)
            }
        }
        self.order(&mut dirs, true);
        self.order(&mut files, false);
        up.extend(dirs);
        up.extend(files);
        self.items = up;
    }

    fn order(&self, v: &mut [Entry], dirs: bool) {
        use std::cmp::Reverse;
        match self.sort {
            SortMode::Name => v.sort_by_cached_key(|e| e.name.to_lowercase()),
            SortMode::Ext => v.sort_by_cached_key(|e| (e.ext(), e.name.to_lowercase())),
            SortMode::Size if dirs => v.sort_by_cached_key(|e| e.name.to_lowercase()),
            SortMode::Size => v.sort_by_cached_key(|e| (Reverse(e.size.unwrap_or(0)), e.name.to_lowercase())),
            SortMode::Time => v.sort_by_cached_key(|e| (Reverse(e.modified), e.name.to_lowercase())),
            SortMode::Unsorted => {}
        }
        if self.sort_desc {
            v.reverse();
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let max = self.items.len() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, max) as usize;
        self.clamp();
    }

    pub fn move_to(&mut self, i: usize) {
        self.cursor = i;
        self.clamp();
    }

    /// Shift+movement: select (or unselect) the files the cursor passes over, then move.
    /// The first file decides: unselected -> select them all, selected -> unselect them all.
    /// `to_edge` (Shift+Home / End) includes the file the cursor lands on.
    pub fn select_by(&mut self, delta: isize, to_edge: bool) {
        if self.items.is_empty() {
            return;
        }
        let from = self.cursor;
        let to = (from as isize + delta).clamp(0, self.items.len() as isize - 1) as usize;
        let state = !self.items[from].selected || self.items[from].is_up;
        // at the first / last file the cursor can't move, so that file itself is toggled
        let (a, b) = if to == from || to_edge {
            (from.min(to), from.max(to))
        } else if to > from {
            (from, to - 1)
        } else {
            (to + 1, from)
        };
        for e in &mut self.items[a..=b] {
            if !e.is_up {
                e.selected = state;
            }
        }
        self.move_to(to);
    }

    pub fn toggle_current(&mut self) {
        if let Some(e) = self.items.get_mut(self.cursor) {
            if !e.is_up {
                e.selected = !e.selected;
            }
        }
        self.move_by(1);
    }

    fn clamp(&mut self) {
        if self.items.is_empty() {
            self.cursor = 0;
            self.top = 0;
            return;
        }
        self.cursor = self.cursor.min(self.items.len() - 1);
        let cap = self.page_size();
        if self.view == ViewMode::Brief {
            let rows = self.rows.max(1);
            let col = self.cursor / rows;
            let mut top_col = self.top / rows;
            if col < top_col {
                top_col = col;
            }
            if col >= top_col + self.cols {
                top_col = col + 1 - self.cols;
            }
            self.top = top_col * rows;
        } else {
            if self.cursor < self.top {
                self.top = self.cursor;
            }
            if self.cursor >= self.top + cap {
                self.top = self.cursor + 1 - cap;
            }
        }
    }

    // ------------------------------------------------------------------ drawing

    /// Git status at the right end of the name: M modified, A added, ? new, U conflict, • changes inside.
    fn draw_git_mark(&self, s: &mut Screen, e: &Entry, x: i32, y: i32, cur: bool, bg: C) {
        if self.arc.is_some() || e.is_up {
            return;
        }
        let Some(&m) = self.git.get(&e.name) else { return };
        let th = theme::current();
        let fg = if cur {
            th.cursor_fg
        } else {
            match m {
                '?' | 'A' => th.syn_string,
                'U' | 'D' => th.error,
                _ => th.syn_type,
            }
        };
        s.put(x, y, m, fg, bg);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, s: &mut Screen, x: i32, y: i32, w: i32, h: i32, active: bool, quick: Option<&str>) {
        let th = theme::current();
        let (bg, frame, dbl) = (th.panel_bg, th.frame, th.double_frame);
        s.fill(x, y, w, h, ' ', frame, bg);
        s.boxed(x, y, w, h, frame, bg, dbl);
        let iw = w - 2;
        let list_top = y + 2;
        let sep_y = y + h - 5; // separator, then 3 info lines, then the bottom border
        let col_line_h = sep_y - (y + 1);
        self.rows = (h - 7).max(1) as usize;
        self.geom = Geom { list_top, cols: vec![] };

        s.put(x, sep_y, if dbl { '╟' } else { '├' }, frame, bg);
        s.hline(x + 1, sep_y, iw, '─', frame, bg);
        s.put(x + w - 1, sep_y, if dbl { '╢' } else { '┤' }, frame, bg);

        let column_line = |s: &mut Screen, vx: i32| {
            s.vline(vx, y + 1, col_line_h, '│', th.col_line, bg);
            s.put(vx, y, if dbl { '╤' } else { '┬' }, frame, bg);
            s.put(vx, sep_y, '┴', frame, bg);
        };
        let header = |s: &mut Screen, hx: i32, hw: i32, text: &str| {
            s.text(hx, y + 1, &center(text, hw.max(0) as usize), th.header, bg);
        };

        if self.view == ViewMode::Full {
            self.cols = 1;
            // the name keeps at least MIN_COL; Time goes first, then Date, then Size
            let (mut size_w, mut date_w, mut time_w) = (10, 8, 5);
            let used = |a: i32, b: i32, c: i32| [a, b, c].iter().filter(|&&v| v > 0).map(|v| v + 1).sum::<i32>();
            if iw - used(size_w, date_w, time_w) < MIN_COL {
                time_w = 0;
            }
            if iw - used(size_w, date_w, time_w) < MIN_COL {
                date_w = 0;
            }
            if iw - used(size_w, date_w, time_w) < MIN_COL {
                size_w = 0;
            }
            let name_w = iw - used(size_w, date_w, time_w);
            let nx = x + 1;
            let sx = nx + name_w + 1;
            let dx = sx + size_w + 1;
            let tx = dx + date_w + 1;
            let lines: Vec<i32> = [(size_w, sx), (date_w, dx), (time_w, tx)].iter().filter(|c| c.0 > 0).map(|c| c.1 - 1).collect();
            self.geom.cols.push((x + 1, iw));
            header(s, nx, name_w, "Name");
            for &vx in &lines {
                column_line(s, vx);
            }
            if size_w > 0 {
                header(s, sx, size_w, "Size");
            }
            if date_w > 0 {
                header(s, dx, date_w, "Date");
            }
            if time_w > 0 {
                header(s, tx, time_w, "Time");
            }
            for r in 0..self.rows {
                let idx = self.top + r;
                let Some(e) = self.items.get(idx) else { break };
                let cur = active && idx == self.cursor;
                let rbg = if cur { th.cursor_bg } else { bg };
                let fg = if e.selected { th.selected } else if cur { th.cursor_fg } else { color_of(e) };
                let ry = list_top + r as i32;
                s.text(nx, ry, &show_name(e, name_w as usize), fg, rbg);
                self.draw_git_mark(s, e, nx + name_w - 1, ry, cur, rbg);
                if size_w > 0 {
                    s.text(sx, ry, &pad_left(&size_text(e, size_w as usize), size_w as usize), fg, rbg);
                }
                let (d, t) = match e.modified {
                    Some(m) => (m.format("%d-%m-%y").to_string(), m.format("%H:%M").to_string()),
                    None => (String::new(), String::new()),
                };
                if date_w > 0 {
                    s.text(dx, ry, &pad_left(&d, date_w as usize), fg, rbg);
                }
                if time_w > 0 {
                    s.text(tx, ry, &pad_left(&t, time_w as usize), fg, rbg);
                }
                if cur {
                    for &vx in &lines {
                        s.put(vx, ry, '│', th.cursor_fg, rbg);
                    }
                }
            }
        } else {
            // as many columns as fit at MIN_COL each (with a line between), at most 3
            self.cols = ((iw + 1) / (MIN_COL + 1)).clamp(1, 3) as usize;
            let cols = self.cols as i32;
            let cw = (iw - (cols - 1)) / cols;
            for c in 0..cols {
                let cx = x + 1 + c * (cw + 1);
                let width = if c == cols - 1 { x + w - 1 - cx } else { cw };
                self.geom.cols.push((cx, width));
                header(s, cx, width, "Name");
                if c > 0 {
                    column_line(s, cx - 1);
                }
                for r in 0..self.rows {
                    let idx = self.top + c as usize * self.rows + r;
                    let Some(e) = self.items.get(idx) else { break };
                    let cur = active && idx == self.cursor;
                    let rbg = if cur { th.cursor_bg } else { bg };
                    let fg = if e.selected { th.selected } else if cur { th.cursor_fg } else { color_of(e) };
                    s.text(cx, list_top + r as i32, &show_name(e, width.max(0) as usize), fg, rbg);
                    self.draw_git_mark(s, e, cx + width - 1, list_top + r as i32, cur, rbg);
                }
            }
        }

        let iwu = iw.max(0) as usize;
        // git branch, on the right of the separator line
        if let Some(b) = self.branch.as_ref().filter(|_| self.found.is_none()) {
            // "git:" like a label, the branch like a value (the free space line's colours)
            let name = format!("{} ", fit(b, iwu.saturating_sub(12).max(1)).trim_end());
            let lx = x + w - 2 - 6 - name.chars().count() as i32;
            if lx > x + 1 {
                s.text(lx, sep_y, " git: ", th.free_label, bg);
                s.text(lx + 6, sep_y, &name, th.free_num, bg);
            }
        }

        // info line 1: the file under the cursor
        let info_y = sep_y + 1;
        if let Some(ce) = self.current() {
            let mut right = if ce.is_up {
                "<UP-DIR>".to_string()
            } else if ce.is_dir && ce.size.is_none() {
                "<SUB-DIR>".to_string()
            } else {
                n0(ce.size.unwrap_or(0))
            };
            if let Some(m) = ce.modified {
                right += &format!(" {:>8} {:>5}", m.format("%-d-%m-%y").to_string(), m.format("%-H:%M").to_string());
            }
            let rlen = right.chars().count();
            let name_w = iwu.saturating_sub(rlen + 1).max(1);
            let line = format!("{} {}", fit(&ce.name, name_w), fit(&right, iwu.saturating_sub(name_w + 1)));
            s.text(x + 1, info_y, &line, th.info, bg);
        }

        // info line 2: selection (or an error)
        if let Some(err) = &self.error {
            s.text(x + 1, info_y + 1, &center(err, iwu), th.error, bg);
        } else {
            let n = self.selected_count();
            let bytes: u64 = self.items.iter().filter(|e| e.selected).filter_map(|e| e.size).sum();
            let text = if n == 0 {
                "No files selected".to_string()
            } else {
                format!("{} bytes in {} selected file{}", n0(bytes), n, if n == 1 { "" } else { "s" })
            };
            s.text(x + 1, info_y + 1, &center(&text, iwu), th.status, bg);
        }

        // info line 3: free space
        if let Some(free) = self.free {
            let mut num = if free >= 1_000_000_000 { format!("{}K", n0(free >> 10)) } else { n0(free) };
            let mut label = if cfg!(windows) {
                format!(" free bytes on drive {}", self.volume)
            } else {
                format!(" free bytes on {}", self.volume)
            };
            if num.chars().count() + label.chars().count() > iwu {
                num = human(free);
                label = format!(" free on {}", self.volume);
            }
            let total = (num.chars().count() + label.chars().count()) as i32;
            let fx = x + 1 + ((iw - total) / 2).max(0);
            s.text(fx, info_y + 2, &num, th.free_num, bg);
            let rest = (x + w - 1 - fx - num.chars().count() as i32).max(0) as usize;
            s.text(fx + num.chars().count() as i32, info_y + 2, &fit(&label, rest), th.free_label, bg);
        }

        // title: path, highlighted for the active panel
        let title = format!(" {} ", fit_left(&self.title_path(), (w - 8).max(4) as usize));
        let tx0 = x + (w - title.chars().count() as i32) / 2;
        let (tf, tb) = if active { (th.active_title_fg, th.active_title_bg) } else { (th.title_fg, th.title_bg) };
        s.text(tx0, y, &title, tf, tb);

        // sort marker in the corner (only when not the default name order)
        if self.sort != SortMode::Name || self.sort_desc {
            let mark = match self.sort {
                SortMode::Name => 'n',
                SortMode::Ext => 'x',
                SortMode::Size => 's',
                SortMode::Time => 't',
                SortMode::Unsorted => 'u',
            };
            let mark = if self.sort_desc { mark.to_ascii_uppercase() } else { mark };
            s.text(x + 1, y, &format!("[{mark}]"), frame, bg);
        }

        // drive strip on the bottom border: [ A C D ]  (Windows)
        let letters = platform::drive_letters();
        if !letters.is_empty() {
            let mut dxs = x + 1;
            let dmax = x + w - 2;
            s.put(dxs, y + h - 1, '[', frame, bg);
            dxs += 1;
            let here = self.volume.chars().next().map(|c| c.to_ascii_uppercase());
            for letter in letters {
                if dxs + 4 > dmax {
                    break;
                }
                s.put(dxs, y + h - 1, ' ', frame, bg);
                s.put(dxs + 1, y + h - 1, letter, if Some(letter) == here { th.drive_cur } else { th.drive_fg }, bg);
                dxs += 2;
            }
            s.text(dxs, y + h - 1, " ]", frame, bg);
        }

        // scroll bar on the right edge of the active panel
        if active && col_line_h >= 3 {
            let (top, bottom) = (y + 1, sep_y - 1);
            let track = bottom - top - 1;
            s.put(x + w - 1, top, '▲', th.scroll_fg, th.scroll_bg);
            s.put(x + w - 1, bottom, '▼', th.scroll_fg, th.scroll_bg);
            s.vline(x + w - 1, top + 1, track, '░', th.scroll_fg, th.scroll_bg);
            if track > 0 && !self.items.is_empty() {
                let pos = if self.items.len() <= 1 { 0 } else { (self.cursor as i64 * (track as i64 - 1) / (self.items.len() as i64 - 1)) as i32 };
                s.put(x + w - 1, top + 1 + pos, '■', th.scroll_fg, th.scroll_bg);
            }
        }

        // quick search box
        if let Some(q) = quick {
            let text = format!(" Search: {q} ");
            s.text(x + 2, y + h - 1, &text, theme::current().bar_fg, theme::current().bar_bg);
            self.quick_pos = (x + 2 + text.chars().count() as i32 - 1, y + h - 1);
        }
    }
}

#[cfg(test)]
mod select_tests {
    use super::*;

    #[test]
    fn shift_moves_select() {
        let dir = std::env::temp_dir().join(format!("on-select-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["a", "b", "c", "d", "e"] {
            std::fs::write(dir.join(n), n).unwrap();
        }
        let mut p = Panel::new(dir.clone());
        p.load(None);
        let names = |p: &Panel| p.items.iter().filter(|e| e.selected).map(|e| e.name.clone()).collect::<Vec<_>>();
        let first = p.items.iter().position(|e| e.name == "a").unwrap();
        p.move_to(first);
        p.select_by(1, false); // a, cursor on b
        p.select_by(1, false); // b, cursor on c
        assert_eq!(names(&p), ["a", "b"]);
        p.select_by(-1, false); // c is not selected: selects c, back to b
        assert_eq!(names(&p), ["a", "b", "c"]);
        p.select_by(-1, false); // b is selected: unselects it, on a
        assert_eq!(names(&p), ["a", "c"]);
        p.select_by(100, true); // Shift+End: a .. e by a's state (selected -> unselect all)
        assert!(names(&p).is_empty());
        assert_eq!(p.items[p.cursor].name, "e");
        p.select_by(1, false); // at the last file: toggles it
        assert_eq!(names(&p), ["e"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
