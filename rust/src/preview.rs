//! Ctrl+L: the other panel previews what's under the cursor: a file's text (with syntax colours),
//! or a summary of a folder. Tab focuses it: then it scrolls and has the F3 viewer's view modes.

use crate::panel::Entry;
use crate::screen::Screen;
use crate::theme;
use crate::util::{fit, fit_left, human, n0};
use crate::viewer::{self, DocKey, TextDoc};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_PREVIEW: u64 = 512 << 10;

enum Content {
    Text(TextDoc),
    Info(Vec<String>),
}

#[derive(Default)]
pub struct Preview {
    key: Option<PathBuf>,
    content: Option<Content>,
    pub top: usize,
    left: usize,
    /// Tab moved the keyboard here from the file panel
    pub focused: bool,
    /// the text area last drawn (width, height), for paging
    area: (usize, usize),
}

fn time_text(t: std::io::Result<std::time::SystemTime>) -> String {
    t.map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default()
}

fn folder_info(p: &Path) -> Vec<String> {
    let mut v = vec![format!("Folder   {}", p.display())];
    if let Ok(m) = fs::metadata(p) {
        v.push(format!("Modified {}", time_text(m.modified())));
    }
    match fs::read_dir(p) {
        Ok(rd) => {
            let (mut dirs, mut files, mut hidden, mut bytes) = (0u64, 0u64, 0u64, 0u64);
            for e in rd.flatten() {
                let Ok(m) = e.metadata() else { continue };
                if crate::platform::is_hidden(&e.file_name().to_string_lossy(), &m) {
                    hidden += 1;
                }
                if m.is_dir() {
                    dirs += 1;
                } else {
                    files += 1;
                    bytes += m.len();
                }
            }
            v.push(String::new());
            v.push(format!("Folders  {}", n0(dirs)));
            v.push(format!("Files    {}  ({} bytes, {})", n0(files), n0(bytes), human(bytes)));
            if hidden > 0 {
                v.push(format!("Hidden   {}", n0(hidden)));
            }
            v.push(String::new());
            v.push("Sizes of files directly inside; F3 on the".into());
            v.push("folder counts everything below it.".into());
        }
        Err(e) => v.push(format!("Can't read it: {e}")),
    }
    if let Some(b) = crate::git::branch(p) {
        v.push(String::new());
        v.push(format!("Git branch {b}"));
    }
    v
}

impl Preview {
    /// Make sure the content is for `entry`.
    fn load(&mut self, entry: Option<&Entry>, in_archive: bool, dir: &Path) {
        let key = match entry {
            Some(e) if !e.is_up => e.path.clone(),
            _ => dir.to_path_buf(),
        };
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.top = 0;
        self.left = 0;
        self.content = Some(match entry {
            Some(e) if in_archive => Content::Info(vec![
                format!("Name     {}", e.name),
                format!("Size     {} bytes", n0(e.size.unwrap_or(0))),
                String::new(),
                "Inside an archive: F3 to view it.".into(),
            ]),
            Some(e) if !e.is_up && !e.is_dir => match TextDoc::open(&e.path, MAX_PREVIEW) {
                Ok(mut d) => {
                    d.wrap = true; // a panel is narrow
                    Content::Text(d)
                }
                Err(err) => Content::Info(vec![format!("Can't read {}:", e.name), err.to_string()]),
            },
            Some(e) if !e.is_up => Content::Info(folder_info(&e.path)),
            _ => Content::Info(folder_info(dir)),
        });
        self.key = Some(key);
    }

    pub fn forget(&mut self) {
        self.key = None;
        self.content = None;
    }

    pub fn scroll(&mut self, delta: isize) {
        self.top = self.top.saturating_add_signed(delta);
    }

    /// A key while the preview has the focus.
    pub fn key(&mut self, code: crossterm::event::KeyCode) -> DocKey {
        let (w, h) = self.area;
        match &mut self.content {
            Some(Content::Text(d)) => viewer::doc_key(d, code, &mut self.top, &mut self.left, w, h),
            _ => DocKey::No,
        }
    }

    /// The key bar while the preview has the focus.
    pub fn key_labels(&self) -> [&'static str; 10] {
        let mut l = match &self.content {
            Some(Content::Text(d)) => viewer::doc_key_labels(d),
            _ => ["Help", "", "", "", "", "", "", "", "", ""],
        };
        l[2] = "View";
        l[9] = "Menu";
        l
    }

    pub fn draw(&mut self, s: &mut Screen, x: i32, y: i32, w: i32, h: i32, entry: Option<&Entry>, in_archive: bool, dir: &Path) {
        self.load(entry, in_archive, dir);
        let t = theme::current();
        s.fill(x, y, w, h, ' ', t.file, t.panel_bg);
        s.boxed(x, y, w, h, t.frame, t.panel_bg, t.double_frame);
        let name = match entry {
            Some(e) if !e.is_up => e.name.clone(),
            _ => dir.display().to_string(),
        };
        let (iw, ih) = (w - 2, h - 2);
        match &mut self.content {
            Some(Content::Text(d)) => {
                self.area = (iw.max(1) as usize, ih.max(1) as usize);
                self.top = self.top.min(d.last_top(self.area.0, self.area.1));
                d.draw(s, x + 1, y + 1, iw, ih, self.top, self.left);
                let mut info = format!(" {}{}{} ", if d.wrap && !d.hex { "Wrap  " } else { "" }, if d.formatted { "Formatted  " } else { "" }, d.encoding_name());
                if d.truncated() {
                    info = format!(" first {} of {} ", human(d.data.len() as u64), human(d.full_len));
                }
                let il = info.chars().count() as i32;
                s.text(x + w - il - 2, y + h - 1, &info, t.frame, t.panel_bg);
            }
            Some(Content::Info(lines)) => {
                for (i, l) in lines.iter().enumerate().take(ih.max(0) as usize) {
                    s.text(x + 2, y + 1 + i as i32, &fit(l, (iw - 2).max(0) as usize), t.info, t.panel_bg);
                }
            }
            None => {}
        }
        let title = format!(" Preview: {} ", fit_left(&name, (w - 16).max(4) as usize));
        let (tf, tb) = if self.focused { (t.active_title_fg, t.active_title_bg) } else { (t.title_fg, t.title_bg) };
        s.text(x + (w - title.chars().count() as i32) / 2, y, &title, tf, tb);
    }
}
