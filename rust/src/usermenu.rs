//! F9: commands for the file under the cursor (by its extension) and the user menus: the folder's own
//! `.on-menu` (found in the folder or the ones above it) and the global one. Enter on a file runs its
//! "Enter" command, if it has one.
//!
//! Both files are plain text:
//!
//! ```text
//! # comment
//! &Build = cargo build --release        <- Name = command; & marks the hot letter
//! Test and lint =                       <- empty: the indented lines below are its commands
//!     cargo test
//!     cargo clippy
//! -                                     <- a separator
//! ```
//!
//! The extensions file has masks on their own lines, with the commands for them indented below:
//!
//! ```text
//! *.py;*.pyw
//!     Enter = python "!.!"
//!     Lint = ruff check "!.!"
//! ```
//!
//! In commands: `!.!` the file name, `!` the name without its extension, `!\` (or `!/`) its folder,
//! `!&` the selected files, `!@` a file listing them, `!!` a single `!`.

use crate::app::App;
use crate::dialogs;
use crate::editor;
use crate::panel::Entry;
use crate::picklist::{self, Opts, Row};
use crate::platform;
use crate::util::wildcard;
use crossterm::event::KeyCode;
use std::fs;
use std::path::{Path, PathBuf};

pub const LOCAL_NAME: &str = ".on-menu";

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// the label without the `&`
    pub label: String,
    pub hot: Option<char>,
    /// the commands, run one after another
    pub lines: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub struct Section {
    /// None in a menu file; the masks (`*.py;*.pyw`) in the extensions file
    pub masks: Option<String>,
    /// None is a separator
    pub items: Vec<Option<Item>>,
}

fn indent_of(l: &str) -> usize {
    l.chars().take_while(|c| c.is_whitespace()).count()
}

/// Parse a menu or extensions file. `sections`: unindented lines without `=` start a section of masks.
pub fn parse(text: &str, sections: bool) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    if !sections {
        out.push(Section { masks: None, items: vec![] });
    }
    // the item whose commands are the indented lines below it, and its indent
    let mut open: Option<usize> = None;
    for raw in text.lines() {
        let line = raw.trim_end();
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let ind = indent_of(line);
        if let (Some(base), Some(sec)) = (open, out.last_mut()) {
            if ind > base {
                if let Some(Some(it)) = sec.items.last_mut() {
                    it.lines.push(t.to_string());
                }
                continue;
            }
        }
        open = None;
        if t == "-" {
            if let Some(sec) = out.last_mut() {
                sec.items.push(None);
            }
            continue;
        }
        match t.split_once('=') {
            Some((label, cmd)) if !(sections && ind == 0) => {
                let Some(sec) = out.last_mut() else { continue };
                let label = label.trim();
                let hot = label.find('&').and_then(|i| label[i + 1..].chars().next());
                let label = label.replacen('&', "", 1);
                let cmd = cmd.trim();
                let lines = if cmd.is_empty() { vec![] } else { vec![cmd.to_string()] };
                if cmd.is_empty() {
                    open = Some(ind);
                }
                sec.items.push(Some(Item { label, hot, lines }));
            }
            _ if sections && ind == 0 => out.push(Section { masks: Some(t.to_string()), items: vec![] }),
            _ => {}
        }
    }
    out
}

fn masks_match(masks: &str, name: &str) -> bool {
    masks.split([';', ',']).map(str::trim).filter(|m| !m.is_empty()).any(|m| wildcard(m, name))
}

pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("orthodox-navigator"))
}

pub fn global_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("menu.txt"))
}

pub fn extensions_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("extensions.txt"))
}

/// The folder's menu: `.on-menu` here or in a folder above.
pub fn local_file(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().map(|d| d.join(LOCAL_NAME)).find(|f| f.is_file())
}

fn read(path: Option<&Path>) -> String {
    path.and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default()
}

/// The commands the extensions file has for this file name.
pub fn file_commands(name: &str) -> Vec<Item> {
    let text = read(extensions_file().as_deref());
    parse(&text, true)
        .into_iter()
        .filter(|s| s.masks.as_deref().is_some_and(|m| masks_match(m, name)))
        .flat_map(|s| s.items.into_iter().flatten())
        .collect()
}

/// What `!` macros stand for.
pub struct Ctx {
    /// the file under the cursor, as shown (a relative path in search results)
    pub name: String,
    pub dir: PathBuf,
    pub selected: Vec<String>,
    pub shell: String,
}

/// Replace the `!` macros in one command.
pub fn expand(cmd: &str, c: &Ctx) -> String {
    let stem = match c.name.rfind('.') {
        Some(i) if i > 0 && !c.name[i..].contains(['/', '\\']) => &c.name[..i],
        _ => c.name.as_str(),
    };
    let mut dir = c.dir.to_string_lossy().to_string();
    if !dir.ends_with(std::path::MAIN_SEPARATOR) {
        dir.push(std::path::MAIN_SEPARATOR);
    }
    let chars: Vec<char> = cmd.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '!' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let next = chars.get(i + 1).copied();
        match next {
            Some('!') => {
                out.push('!');
                i += 2;
            }
            Some('.') if chars.get(i + 2) == Some(&'!') => {
                out += &c.name;
                i += 3;
            }
            Some('\\') | Some('/') => {
                out += &dir;
                i += 2;
            }
            Some('&') => {
                let names: Vec<String> = c.selected.iter().map(|n| platform::quote(&c.shell, n)).collect();
                out += &names.join(" ");
                i += 2;
            }
            Some('@') => {
                let list = std::env::temp_dir().join(format!("on-list-{}.txt", std::process::id()));
                let _ = fs::write(&list, c.selected.iter().map(|n| format!("{n}\n")).collect::<String>());
                out += &platform::quote(&c.shell, &list.to_string_lossy());
                i += 2;
            }
            _ => {
                out += stem;
                i += 1;
            }
        }
    }
    out
}

/// The commands of an item as one line for the shell.
pub fn command_line(it: &Item, c: &Ctx) -> String {
    let sep = if platform::is_cmd(&c.shell) { " & " } else { "; " };
    it.lines.iter().map(|l| expand(l, c)).collect::<Vec<_>>().join(sep)
}

/// The macro context for the active panel.
fn context(app: &mut App) -> Ctx {
    let shell = app.shell_name();
    let p = app.active();
    let name = p.current().filter(|e| !e.is_up).map(|e| e.name.clone()).unwrap_or_default();
    let targets: Vec<Entry> = p.targets();
    Ctx { name, dir: p.dir.clone(), selected: targets.into_iter().map(|e| e.name).collect(), shell }
}

/// Enter on a file: its extension's "Enter" command as a shell line, if there is one.
pub fn enter_command(app: &mut App, e: &Entry) -> Option<String> {
    let it = file_commands(&e.name).into_iter().find(|it| it.label.eq_ignore_ascii_case("enter") && !it.lines.is_empty())?;
    let c = context(app);
    Some(command_line(&it, &c))
}

const MENU_TEMPLATE: &str = "\
# Orthodox Navigator user menu (F9). One command per line:   Name = command
# For several commands leave the right side empty and indent them below the name.
# & marks the hot letter; a line with just - is a separator; # starts a comment.
# In commands: !.! the file name, ! the name without extension, !\\ its folder,
# !& the selected files, !@ a file listing them, !! a single !.
#
# &Build = cargo build --release
# &Test and lint =
#     cargo test
#     cargo clippy
";

const EXT_TEMPLATE: &str = "\
# Orthodox Navigator: commands by file name (F9 on a file shows them; Enter runs the one named Enter).
# A line of masks (; between them), then its commands indented:   Name = command
# Same macros as the user menu: !.! the file name, ! the name without extension, !\\ its folder,
# !& the selected files, !@ a file listing them, !! a single !.
#
# *.py;*.pyw
#     Enter = python \"!.!\"
#     &Lint = ruff check \"!.!\"
";

/// Open a menu file in the editor, creating it from the template first.
fn edit_file(app: &mut App, path: &Path, template: &str, append: Option<String>) {
    if let Some(d) = path.parent() {
        let _ = fs::create_dir_all(d);
    }
    if !path.exists() {
        if let Err(e) = fs::write(path, template) {
            return dialogs::message(app, "User menu", &format!("Cannot create {}:\n{e}", path.display()), true);
        }
    }
    if let Some(extra) = append {
        let mut text = fs::read_to_string(path).unwrap_or_default();
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text += &extra;
        let _ = fs::write(path, text);
    }
    if editor::edit(app, path) {
        app.reload_panels();
    }
}

/// What a row of the F9 list is.
enum Kind {
    Run(Item),
    /// F4 / Enter: open this file in the editor (made from the template if missing), adding the text
    Edit(PathBuf, &'static str, Option<String>),
    Header,
}

/// F9: the commands for the file under the cursor, the folder's menu and the global menu.
pub fn show(app: &mut App) {
    let mut start = 0usize;
    loop {
        let in_arc = app.active().arc.is_some();
        let dir = app.active().dir.clone();
        let cur: Option<Entry> = app.active().current().filter(|e| !e.is_up && !e.is_dir && !in_arc).cloned();
        let mut rows: Vec<Row> = Vec::new();
        let mut kinds: Vec<(Kind, Option<PathBuf>)> = Vec::new();
        let push = |rows: &mut Vec<Row>, kinds: &mut Vec<(Kind, Option<PathBuf>)>, r: Row, k: Kind, src: Option<PathBuf>| {
            rows.push(r);
            kinds.push((k, src));
        };
        let add_items = |rows: &mut Vec<Row>, kinds: &mut Vec<(Kind, Option<PathBuf>)>, items: Vec<Option<Item>>, src: &Path| {
            for it in items {
                match it {
                    Some(it) => {
                        let mut r = Row::item(it.label.clone(), it.lines.first().cloned().unwrap_or_default() + if it.lines.len() > 1 { " ..." } else { "" });
                        r.hot = it.hot;
                        rows.push(r);
                        kinds.push((Kind::Run(it), Some(src.to_path_buf())));
                    }
                    None => {
                        rows.push(Row::header(""));
                        kinds.push((Kind::Header, None));
                    }
                }
            }
        };

        // 1. the file under the cursor
        if let (Some(e), Some(ext_file)) = (&cur, extensions_file()) {
            push(&mut rows, &mut kinds, Row::header(format!("For {}", e.name)), Kind::Header, None);
            let items = file_commands(&e.name);
            if items.is_empty() {
                let mask = match e.name.rfind('.') {
                    Some(i) if i > 0 => format!("*{}", &e.name[i..]),
                    _ => e.name.clone(),
                };
                let add = format!("\n{mask}\n    Enter = \n");
                let r = Row::item(format!("Add commands for {mask} files..."), "F4");
                push(&mut rows, &mut kinds, r, Kind::Edit(ext_file.clone(), EXT_TEMPLATE, Some(add)), None);
            } else {
                add_items(&mut rows, &mut kinds, items.into_iter().map(Some).collect(), &ext_file);
            }
        }
        // 2. this folder's menu
        match local_file(&dir) {
            Some(f) => {
                push(&mut rows, &mut kinds, Row::header(format!("Menu: {}", f.display())), Kind::Header, None);
                let items = parse(&read(Some(&f)), false).into_iter().flat_map(|s| s.items).collect::<Vec<_>>();
                if items.is_empty() {
                    push(&mut rows, &mut kinds, Row::item("(empty: F4 to edit)", ""), Kind::Edit(f.clone(), MENU_TEMPLATE, None), None);
                }
                add_items(&mut rows, &mut kinds, items, &f);
            }
            None if !in_arc => {
                push(&mut rows, &mut kinds, Row::header("This folder"), Kind::Header, None);
                let r = Row::item("Make a menu for this folder...", "F4");
                push(&mut rows, &mut kinds, r, Kind::Edit(dir.join(LOCAL_NAME), MENU_TEMPLATE, None), None);
            }
            None => {}
        }
        // 3. the global menu
        if let Some(g) = global_file() {
            push(&mut rows, &mut kinds, Row::header("Global menu"), Kind::Header, None);
            let items = parse(&read(Some(&g)), false).into_iter().flat_map(|s| s.items).collect::<Vec<_>>();
            if items.iter().flatten().next().is_none() {
                let r = Row::item(if g.exists() { "(empty: F4 to edit)" } else { "Make the global menu..." }, "F4");
                push(&mut rows, &mut kinds, r, Kind::Edit(g.clone(), MENU_TEMPLATE, None), None);
            }
            add_items(&mut rows, &mut kinds, items, &g);
        }

        let o = Opts {
            title: "User menu",
            start,
            keys: &[KeyCode::F(4)],
            bar: ["Help", "", "", "Edit", "", "", "", "", "", ""],
            help: "usermenu",
            filter: false,
        };
        let Some(p) = picklist::pick(app, &rows, &o) else { return };
        start = p.row.min(rows.len().saturating_sub(1));
        let Some((kind, src)) = kinds.into_iter().nth(p.row) else { continue };
        match (kind, p.key) {
            (Kind::Edit(path, tpl, add), _) => edit_file(app, &path, tpl, add),
            (Kind::Run(_), KeyCode::F(4)) => {
                if let Some(src) = src {
                    edit_file(app, &src, MENU_TEMPLATE, None);
                }
            }
            (Kind::Run(it), _) => {
                if it.lines.is_empty() {
                    dialogs::message(app, "User menu", &format!("\"{}\" has no commands.", it.label), false);
                    continue;
                }
                let c = context(app);
                let line = command_line(&it, &c);
                app.run_line(&line);
                return;
            }
            (Kind::Header, _) => {}
        }
    }
}

/// Options menu: edit the global menu or the extensions file.
pub fn edit_global(app: &mut App) {
    if let Some(g) = global_file() {
        edit_file(app, &g, MENU_TEMPLATE, None);
    }
}

pub fn edit_extensions(app: &mut App) {
    if let Some(f) = extensions_file() {
        edit_file(app, &f, EXT_TEMPLATE, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_menu() {
        let text = "# c\n&Build = cargo build\nTest =\n    cargo test\n    cargo clippy --all=x\n-\nRun=x=1\n";
        let s = parse(text, false);
        assert_eq!(s.len(), 1);
        let items: Vec<_> = s[0].items.clone();
        assert_eq!(items.len(), 4);
        let b = items[0].as_ref().unwrap();
        assert_eq!((b.label.as_str(), b.hot, b.lines.clone()), ("Build", Some('B'), vec!["cargo build".to_string()]));
        assert_eq!(items[1].as_ref().unwrap().lines, ["cargo test", "cargo clippy --all=x"]);
        assert!(items[2].is_none());
        assert_eq!(items[3].as_ref().unwrap().lines, ["x=1"]);
    }

    #[test]
    fn parse_extensions() {
        let text = "*.py;*.pyw\n    Enter = python \"!.!\"\n    Lint =\n        ruff check\n*.rs\n  Run = cargo run\n";
        let s = parse(text, true);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].masks.as_deref(), Some("*.py;*.pyw"));
        assert_eq!(s[0].items.len(), 2);
        assert_eq!(s[0].items[1].as_ref().unwrap().lines, ["ruff check"]);
        assert_eq!(s[1].items[0].as_ref().unwrap().label, "Run");
        assert!(masks_match("*.py;*.pyw", "A.PYW"));
    }

    #[test]
    fn macros() {
        let c = Ctx { name: "my file.tar.gz".into(), dir: PathBuf::from("/w"), selected: vec!["a".into(), "b c".into()], shell: "bash".into() };
        assert_eq!(expand("x \"!.!\" ! !! !&", &c), "x \"my file.tar.gz\" my file.tar ! a 'b c'");
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(expand("cd !/", &c), format!("cd {}{sep}", PathBuf::from("/w").display()));
    }
}
