//! Copy / move / delete / folder size, with overwrite prompts and Esc to cancel.

use crate::app::App;
use crate::archive::{self, ArcEntry};
use crate::dialogs;
use crate::panel::Entry;
use crate::platform;
use crate::util::{fit_left, human, is_inside, n0, normalize, same_path};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::fs;
use std::io;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::time::{Duration, Instant};

struct Ctx {
    title: &'static str,
    overwrite_all: bool,
    skip_all: bool,
    cancel: bool,
    skips: usize,
    last_draw: Option<Instant>,
}

impl Ctx {
    fn new(title: &'static str) -> Self {
        Ctx { title, overwrite_all: false, skip_all: false, cancel: false, skips: 0, last_draw: None }
    }
}

pub fn read_only(app: &mut App) {
    dialogs::message(
        app,
        "Archive",
        "Archives are read-only in this version.\nYou can browse them, view files (F3)\nand copy files out (F5).",
        false,
    );
}

pub fn copy_or_move(app: &mut App, mv: bool) {
    if app.active().arc.is_some() {
        if mv { read_only(app) } else { extract_out(app) }
        return;
    }
    let src = app.active().targets();
    if src.is_empty() {
        return;
    }
    let what = if src.len() == 1 { format!("\"{}\"", src[0].name) } else { format!("{} items", src.len()) };
    let mut def = app.passive().dir.to_string_lossy().to_string();
    if !def.ends_with(MAIN_SEPARATOR) {
        def.push(MAIN_SEPARATOR);
    }
    let (title, verb) = if mv { ("Rename/Move", "Move") } else { ("Copy", "Copy") };
    let Some(input) = dialogs::input(app, title, &format!("{verb} {what} to:"), &def) else { return };
    let input = input.trim().trim_matches('"').to_string();
    if input.is_empty() {
        return;
    }

    let full = normalize(&app.active().dir.join(&input));
    let into_dir = input.ends_with('/') || input.ends_with('\\') || full.is_dir() || src.len() > 1;
    let mut c = Ctx::new(if mv { "Move" } else { "Copy" });
    if into_dir {
        if let Err(e) = fs::create_dir_all(&full) {
            dialogs::message(app, "Error", &e.to_string(), true);
            return;
        }
    }

    for e in &src {
        if c.cancel {
            break;
        }
        // the file's own name (in search results `name` is a relative path)
        let own = e.path.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| e.name.clone().into());
        let dst = if into_dir { full.join(own) } else { full.clone() };
        let skips = c.skips;
        let r = if mv { move_entry(app, &mut c, &e.path, &dst, e.is_dir) } else { copy_entry(app, &mut c, &e.path, &dst, e.is_dir, true) };
        if let Err(msg) = r {
            if dialogs::choice(app, "Error", &[&e.name, &msg], &["Skip", "Cancel"], true) != Some(0) {
                c.cancel = true;
            }
            c.skips += 1;
        }
        if !c.cancel && c.skips == skips {
            unselect(app, &e.name);
        }
    }
    app.active().load(None);
    let focus = if !mv && !into_dir { full.file_name().map(|n| n.to_string_lossy().to_string()) } else { None };
    app.passive().load(focus.as_deref());
}

fn unselect(app: &mut App, name: &str) {
    if let Some(it) = app.active().items.iter_mut().find(|i| i.name == name) {
        it.selected = false;
    }
}

fn is_symlink(p: &Path) -> bool {
    fs::symlink_metadata(p).map(|m| m.file_type().is_symlink()).unwrap_or(false)
}

fn copy_entry(app: &mut App, c: &mut Ctx, src: &Path, dst: &Path, is_dir: bool, top: bool) -> Result<(), String> {
    if c.cancel {
        return Ok(());
    }
    if same_path(src, dst) {
        return Err("Cannot copy an item onto itself.".into());
    }
    if is_dir {
        if is_inside(dst, src) {
            return Err("Cannot copy a folder into itself.".into());
        }
        // don't follow links inside the tree (avoids loops)
        if !top && is_symlink(src) {
            return Ok(());
        }
        guard(app, c, src, || fs::create_dir_all(dst));
        let rd = fs::read_dir(src).map_err(|e| e.to_string())?;
        let (mut dirs, mut files) = (vec![], vec![]);
        for de in rd.flatten() {
            let p = de.path();
            if fs::metadata(&p).map(|m| m.is_dir()).unwrap_or(false) { dirs.push(p) } else { files.push(p) }
        }
        for d in dirs {
            let name = d.file_name().unwrap_or_default().to_owned();
            copy_entry(app, c, &d, &dst.join(name), true, false)?;
        }
        for f in files {
            let name = f.file_name().unwrap_or_default().to_owned();
            copy_entry(app, c, &f, &dst.join(name), false, false)?;
        }
    } else {
        progress(app, c, src, Some(dst));
        if c.cancel {
            return Ok(());
        }
        if dst.exists() && !ask_overwrite(app, c, dst) {
            return Ok(());
        }
        guard(app, c, src, || fs::copy(src, dst).map(|_| ()));
    }
    Ok(())
}

fn move_entry(app: &mut App, c: &mut Ctx, src: &Path, dst: &Path, is_dir: bool) -> Result<(), String> {
    if src == dst {
        return Ok(());
    }
    progress(app, c, src, Some(dst));
    // a rename that only changes letter case
    if same_path(src, dst) {
        return fs::rename(src, dst).map_err(|e| e.to_string());
    }
    if is_dir {
        if is_inside(dst, src) {
            return Err("Cannot move a folder into itself.".into());
        }
        if !dst.exists() && fs::rename(src, dst).is_ok() {
            return Ok(());
        }
        // merging into an existing folder, or another drive: copy, then delete what was copied
        let skips = c.skips;
        copy_entry(app, c, src, dst, true, true)?;
        if !c.cancel && c.skips == skips {
            guard(app, c, src, || fs::remove_dir_all(src));
        }
    } else {
        if dst.exists() && !ask_overwrite(app, c, dst) {
            return Ok(());
        }
        guard(app, c, src, || fs::rename(src, dst).or_else(|_| fs::copy(src, dst).and_then(|_| fs::remove_file(src))));
    }
    Ok(())
}

pub fn delete(app: &mut App, permanent: bool) {
    if app.active().arc.is_some() {
        return read_only(app);
    }
    let src = app.active().targets();
    if src.is_empty() {
        return;
    }
    let what = if src.len() == 1 { format!("\"{}\"?", src[0].name) } else { format!("{} items?", src.len()) };
    let verb = if permanent { "Permanently delete" } else { "Move to Trash" };
    if dialogs::choice(app, "Delete", &[verb, &what], &["Delete", "Cancel"], true) != Some(0) {
        return;
    }
    let mut c = Ctx::new("Delete");
    for e in &src {
        if c.cancel {
            break;
        }
        progress(app, &mut c, &e.path, None);
        guard(app, &mut c, &e.path, || remove(e, permanent));
    }
    app.active().load(None);
    app.passive().load(None);
}

#[allow(clippy::permissions_set_readonly_false)]
fn remove(e: &Entry, permanent: bool) -> io::Result<()> {
    if !permanent {
        return platform::move_to_trash(&e.path).map_err(io::Error::other);
    }
    if e.is_link {
        // remove the link, never what it points to
        return if cfg!(windows) && e.is_dir { fs::remove_dir(&e.path) } else { fs::remove_file(&e.path) };
    }
    if e.is_dir {
        fs::remove_dir_all(&e.path)
    } else {
        if let Ok(md) = fs::metadata(&e.path) {
            let mut p = md.permissions();
            if p.readonly() {
                p.set_readonly(false);
                let _ = fs::set_permissions(&e.path, p);
            }
        }
        fs::remove_file(&e.path)
    }
}

/// Calculate the size of the selected folders (F3 on a folder).
pub fn folder_sizes(app: &mut App) {
    let dirs: Vec<Entry> = app.active().targets().into_iter().filter(|e| e.is_dir).collect();
    if dirs.is_empty() {
        return;
    }
    if let Some(av) = app.active().arc.clone() {
        // inside an archive: add up the entries under each folder
        let (mut files, mut bytes) = (0u64, 0u64);
        for d in &dirs {
            let inner = d.arc.clone().unwrap_or_default() + "/";
            let mut sum = 0;
            for e in av.entries.iter().filter(|e| !e.is_dir && e.path.starts_with(&inner)) {
                files += 1;
                sum += e.size;
            }
            bytes += sum;
            if let Some(it) = app.active().items.iter_mut().find(|i| i.name == d.name) {
                it.size = Some(sum);
            }
        }
        let text = format!("{} bytes ({})\n{} files", n0(bytes), human(bytes), n0(files));
        return dialogs::message(app, "Folder size", &text, false);
    }
    let mut c = Ctx::new("Folder size");
    let (mut t_files, mut t_dirs, mut t_bytes) = (0u64, 0u64, 0u64);
    for d in &dirs {
        let (mut files, mut folders, mut bytes) = (0u64, 0u64, 0u64);
        let mut stack = vec![d.path.clone()];
        while let Some(cur) = stack.pop() {
            if c.cancel {
                break;
            }
            progress(app, &mut c, &cur, None);
            let Ok(rd) = fs::read_dir(&cur) else { continue };
            for de in rd.flatten() {
                let Ok(md) = de.metadata() else { continue }; // does not follow links
                if md.is_dir() {
                    folders += 1;
                    stack.push(de.path());
                } else {
                    files += 1;
                    if md.is_file() {
                        bytes += md.len();
                    }
                }
            }
        }
        if c.cancel {
            break;
        }
        if let Some(it) = app.active().items.iter_mut().find(|i| i.name == d.name) {
            it.size = Some(bytes);
        }
        t_files += files;
        t_dirs += folders;
        t_bytes += bytes;
    }
    if !c.cancel {
        let name = if dirs.len() == 1 { dirs[0].name.clone() } else { format!("{} folders", dirs.len()) };
        let text = format!("{name}\n\n{} bytes ({})\n{} files, {} folders", n0(t_bytes), human(t_bytes), n0(t_files), n0(t_dirs));
        dialogs::message(app, "Folder size", &text, false);
    }
}

fn ask_overwrite(app: &mut App, c: &mut Ctx, dst: &Path) -> bool {
    if c.overwrite_all {
        return true;
    }
    if c.skip_all {
        c.skips += 1;
        return false;
    }
    let info = match fs::metadata(dst) {
        Ok(md) => {
            let when = md.modified().ok().map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%d.%m.%Y %H:%M").to_string());
            format!("{} bytes  {}", n0(md.len()), when.unwrap_or_default())
        }
        Err(_) => String::new(),
    };
    let path = fit_left(&dst.to_string_lossy(), 60);
    let r = dialogs::choice(app, "Warning", &["File already exists:", &path, &info], &["Overwrite", "All", "Skip", "sKip all", "Cancel"], true);
    match r {
        Some(0) => true,
        Some(1) => {
            c.overwrite_all = true;
            true
        }
        Some(2) => {
            c.skips += 1;
            false
        }
        Some(3) => {
            c.skip_all = true;
            c.skips += 1;
            false
        }
        _ => {
            c.cancel = true;
            false
        }
    }
}

/// Run an action; on failure ask Retry / Skip / Cancel.
fn guard(app: &mut App, c: &mut Ctx, item: &Path, mut f: impl FnMut() -> io::Result<()>) {
    while !c.cancel {
        match f() {
            Ok(()) => return,
            Err(e) => {
                let name = fit_left(&item.to_string_lossy(), 60);
                match dialogs::choice(app, "Error", &[&name, &e.to_string()], &["Retry", "Skip", "Cancel"], true) {
                    Some(0) => continue,
                    Some(1) => {
                        c.skips += 1;
                        return;
                    }
                    _ => c.cancel = true,
                }
            }
        }
    }
}

fn progress(app: &mut App, c: &mut Ctx, a: &Path, b: Option<&Path>) {
    while event::poll(Duration::ZERO).unwrap_or(false) {
        if let Ok(Event::Key(k)) = event::read() {
            if k.kind != KeyEventKind::Release
                && k.code == KeyCode::Esc
                && dialogs::choice(app, c.title, &["Cancel the operation?"], &["Yes", "No"], true) == Some(0)
            {
                c.cancel = true;
            }
        }
    }
    if c.last_draw.is_some_and(|t| t.elapsed() < Duration::from_millis(50)) {
        return;
    }
    c.last_draw = Some(Instant::now());
    let mut lines = vec![a.to_string_lossy().to_string()];
    if let Some(b) = b {
        lines.push("to".into());
        lines.push(b.to_string_lossy().to_string());
    }
    dialogs::progress(app, c.title, &lines);
}

/// F5 in a panel that browses an archive: extract the selected entries into a folder.
fn extract_out(app: &mut App) {
    let Some(av) = app.active().arc.clone() else { return };
    let src = app.active().targets();
    if src.is_empty() {
        return;
    }
    let what = if src.len() == 1 { format!("\"{}\"", src[0].name) } else { format!("{} items", src.len()) };
    let mut def = app.passive().dir.to_string_lossy().to_string();
    if !def.ends_with(MAIN_SEPARATOR) {
        def.push(MAIN_SEPARATOR);
    }
    let Some(input) = dialogs::input(app, "Extract", &format!("Extract {what} to:"), &def) else { return };
    let input = input.trim().trim_matches('"').to_string();
    if input.is_empty() {
        return;
    }
    let dest = normalize(&app.active().dir.join(&input));

    // every file to write: (entry in the archive, file on disk)
    let mut jobs: Vec<(ArcEntry, PathBuf)> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for e in &src {
        let inner = e.arc.clone().unwrap_or_default();
        let base = dest.join(&e.name);
        if e.is_dir {
            dirs.push(base.clone());
            let prefix = format!("{inner}/");
            for ae in av.entries.iter().filter(|a| a.path.starts_with(&prefix)) {
                let rel = &ae.path[prefix.len()..];
                let target = base.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
                if ae.is_dir { dirs.push(target) } else { jobs.push((ae.clone(), target)) }
            }
        } else if let Some(ae) = av.entries.iter().find(|a| a.path == inner) {
            jobs.push((ae.clone(), base));
        }
    }

    let mut c = Ctx::new("Extract");
    let mut keep = Vec::with_capacity(jobs.len());
    for job in jobs {
        if c.cancel {
            return;
        }
        if !job.1.exists() || ask_overwrite(app, &mut c, &job.1) {
            keep.push(job);
        }
    }
    for d in &dirs {
        let _ = fs::create_dir_all(d);
    }
    let file = av.file.clone();
    let result = archive::extract_many(&file, &keep, |name| {
        progress(app, &mut c, Path::new(name), None);
        !c.cancel
    });
    if let Err(e) = result {
        dialogs::message(app, "Error", &e.to_string(), true);
    }
    for e in &src {
        if let Some(it) = app.active().items.iter_mut().find(|i| i.name == e.name) {
            it.selected = false;
        }
    }
    app.passive().load(None);
}
