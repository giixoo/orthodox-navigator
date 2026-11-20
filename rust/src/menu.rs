//! DN-style pull-down menu (F10, or click the top row). Fully clickable.

use crate::app::{draw_clock, App, Cmd, Input};
use crate::panel::SortMode;
use crate::screen::Screen;
use crate::theme;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

pub struct Item {
    label: String,
    keys: &'static str,
    cmd: Cmd,
}

fn item(label: impl Into<String>, keys: &'static str, cmd: Cmd) -> Option<Item> {
    Some(Item { label: label.into(), keys, cmd })
}

const TITLES: [&str; 5] = ["File", "Disk", "Utilities", "Panel", "Options"];

fn title_x(i: usize) -> i32 {
    2 + TITLES[..i].iter().map(|t| t.len() as i32 + 4).sum::<i32>()
}

/// Which menu title is at column `x` of the top row, if any.
pub fn title_at(x: i32) -> Option<usize> {
    (0..TITLES.len()).find(|&i| x >= title_x(i) && x < title_x(i) + TITLES[i].len() as i32 + 4)
}

fn draw_bar(s: &mut Screen, open: usize) {
    let t = theme::current();
    s.fill(0, 0, s.w, 1, ' ', t.menu_fg, t.menu_bg);
    s.put(1, 0, '≡', t.menu_fg, t.menu_bg);
    for (i, title) in TITLES.iter().enumerate() {
        let on = i == open;
        let (fg, bg) = if on { (t.menu_sel_fg, t.menu_sel_bg) } else { (t.menu_fg, t.menu_bg) };
        s.text(title_x(i), 0, &format!("  {title}  "), fg, bg);
        s.put(title_x(i) + 2, 0, title.chars().next().unwrap_or(' '), if on { fg } else { t.menu_hot }, bg);
    }
    draw_clock(s);
}

fn build(i: usize) -> Vec<Option<Item>> {
    match i {
        0 => vec![
            item("View", "F3", Cmd::View),
            item("Edit", "F4", Cmd::Edit),
            item("Edit new file", "Shift+F4", Cmd::NewFile),
            item("Copy", "F5", Cmd::Copy),
            item("Rename/Move", "F6", Cmd::Move),
            item("Rename in place", "Shift+F6", Cmd::Rename),
            item("Batch rename", "Alt+F6", Cmd::BatchRename),
            item("Attributes", "Ctrl+A", Cmd::Attributes),
            item("Make folder", "F7", Cmd::Mkdir),
            item("Delete to Trash", "F8", Cmd::Delete),
            item("Wipe (delete permanently)", "Shift+F8", Cmd::Wipe),
            None,
            item("Select group", "Gray +", Cmd::SelectGroup),
            item("Unselect group", "Gray -", Cmd::UnselectGroup),
            item("Invert selection", "Gray *", Cmd::Invert),
            item("Add to Temp panel", "F5 to it", Cmd::AddToTemp),
            None,
            item("Exit", "Alt+X", Cmd::Quit),
        ],
        1 => vec![
            item("Drives: left panel", "Alt+F1", Cmd::DriveLeft),
            item("Drives: right panel", "Alt+F2", Cmd::DriveRight),
            item("Add folder to favourites", "Ctrl+D", Cmd::AddFavorite),
            item("Directory tree", "Alt+F10", Cmd::Tree),
            item("Folder history", "Alt+BkSp", Cmd::FolderHistory),
            item("Temp panel", "", Cmd::TempPanel),
            item("Remote connection...", "", Cmd::RemoteConnect),
            item("Find files", "Alt+F7", Cmd::Find),
            None,
            item("Folder sizes", "F3 on dir", Cmd::FolderSizes),
            item("Compare directories", "", Cmd::Compare),
            item("Compare files", "Shift+F3", Cmd::CompareFiles),
            item("Git diff with HEAD", "Alt+F3", Cmd::GitDiff),
        ],
        2 => vec![
            item("User menu", "F9", Cmd::UserMenu),
            item("Developer tools", "F2", Cmd::Tools),
            item("Calculator", "Ctrl+F2", Cmd::Calculator),
            item("System monitor", "Alt+F5", Cmd::SysMon),
            item("Orthodox game", "Ctrl+G", Cmd::Game),
            None,
            item("User screen", "Ctrl+O", Cmd::UserScreen),
        ],
        3 => vec![
            item("Brief / Full view", "Ctrl+B", Cmd::ToggleView),
            None,
            item("Sort by name", "Ctrl+F3", Cmd::Sort(SortMode::Name)),
            item("Sort by extension", "Ctrl+F4", Cmd::Sort(SortMode::Ext)),
            item("Sort by time", "Ctrl+F5", Cmd::Sort(SortMode::Time)),
            item("Sort by size", "Ctrl+F6", Cmd::Sort(SortMode::Size)),
            item("Unsorted", "", Cmd::Sort(SortMode::Unsorted)),
            None,
            item("Hidden files on/off", "Ctrl+H", Cmd::Hidden),
            item("Filter", "Ctrl+F", Cmd::Filter),
            item("Re-read", "Ctrl+R", Cmd::Reread),
            None,
            item("One / two panels", "Ctrl+P", Cmd::TogglePanel),
            item("Wider", "Ctrl+→", Cmd::Wider),
            item("Narrower", "Ctrl+←", Cmd::Narrower),
            item("Equal widths", "", Cmd::EqualWidths),
            item("Swap with other panel", "Ctrl+U", Cmd::Swap),
            item("Preview in other panel", "Ctrl+L", Cmd::Preview),
        ],
        _ => vec![
            item(format!("Colour theme: {}", theme::current().name), "Ctrl+T", Cmd::NextTheme),
            item("File colours", "", Cmd::EditColors),
            item("Commands for file types", "", Cmd::EditExtensions),
            item("Global user menu", "", Cmd::EditGlobalMenu),
            item("Change master password", "", Cmd::MasterPassword),
            item("Forget saved passwords", "", Cmd::ForgetPasswords),
            None,
            item("Help", "F1", Cmd::Help),
            item("About", "", Cmd::About),
        ],
    }
}

/// x, y, width, height of the open drop-down.
fn drop_rect(screen_w: i32, menu: usize, items: &[Option<Item>]) -> (i32, i32, i32, i32) {
    let w = items.iter().flatten().map(|i| i.label.chars().count() + i.keys.chars().count()).max().unwrap_or(10) as i32 + 8;
    let x = title_x(menu).min(screen_w - w - 2).max(0);
    (x, 1, w, items.len() as i32 + 2)
}

/// Show the menu; returns the chosen command (the caller runs it after the menu closes).
pub fn run(app: &mut App, start: usize) -> Option<Cmd> {
    let mut menu = start.min(TITLES.len() - 1);
    let mut sel = 0usize;
    loop {
        let items = build(menu);
        while items[sel].is_none() {
            sel = (sel + 1) % items.len();
        }
        let (m, cur) = (menu, sel);
        let (dx, dy, dw, dh) = drop_rect(app.screen.w, menu, &items);
        let input = app.modal_input(
            &mut |s| {
                draw_bar(s, m);
                draw_drop(s, m, &items, cur);
            },
            None,
        );
        let n = items.len();
        match input {
            Input::Mouse(ev) => {
                let (mx, my) = (ev.column as i32, ev.row as i32);
                let inside = mx >= dx && mx < dx + dw && my > dy && my < dy + dh - 1;
                match ev.kind {
                    MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) if inside => {
                        let i = (my - dy - 1) as usize;
                        if items[i].is_some() {
                            return items[i].as_ref().map(|it| it.cmd);
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) if my == 0 => match title_at(mx) {
                        Some(t) if t != menu => {
                            menu = t;
                            sel = 0;
                        }
                        Some(_) => return None, // clicking the open title closes the menu
                        None => {}
                    },
                    MouseEventKind::Moved | MouseEventKind::Drag(_) if inside => {
                        let i = (my - dy - 1) as usize;
                        if items[i].is_some() {
                            sel = i;
                        }
                    }
                    MouseEventKind::Down(_) => return None, // click outside closes
                    _ => {}
                }
            }
            Input::Key(k) => match k.code {
                KeyCode::Left => {
                    menu = (menu + TITLES.len() - 1) % TITLES.len();
                    sel = 0;
                }
                KeyCode::Right => {
                    menu = (menu + 1) % TITLES.len();
                    sel = 0;
                }
                KeyCode::Up => loop {
                    sel = (sel + n - 1) % n;
                    if items[sel].is_some() {
                        break;
                    }
                },
                KeyCode::Down => loop {
                    sel = (sel + 1) % n;
                    if items[sel].is_some() {
                        break;
                    }
                },
                KeyCode::Enter => return items[sel].as_ref().map(|i| i.cmd),
                KeyCode::Esc => return None,
                KeyCode::Char(c) => {
                    let hit = items.iter().flatten().find(|it| it.label.chars().next().is_some_and(|f| f.eq_ignore_ascii_case(&c)));
                    if let Some(it) = hit {
                        return Some(it.cmd);
                    }
                }
                _ => {}
            },
            Input::Tick | Input::Paste(_) => {}
        }
    }
}

fn draw_drop(s: &mut Screen, menu: usize, items: &[Option<Item>], sel: usize) {
    let t = theme::current();
    let (x, y, w, h) = drop_rect(s.w, menu, items);
    s.cursor = None;
    s.fill(x, y, w, h, ' ', t.menu_fg, t.menu_bg);
    s.boxed(x, y, w, h, t.menu_fg, t.menu_bg, false);
    for (i, it) in items.iter().enumerate() {
        let ry = y + 1 + i as i32;
        let Some(it) = it else {
            s.put(x, ry, '├', t.menu_fg, t.menu_bg);
            s.hline(x + 1, ry, w - 2, '─', t.menu_fg, t.menu_bg);
            s.put(x + w - 1, ry, '┤', t.menu_fg, t.menu_bg);
            continue;
        };
        let on = i == sel;
        let (fg, bg) = if on { (t.menu_sel_fg, t.menu_sel_bg) } else { (t.menu_fg, t.menu_bg) };
        s.fill(x + 1, ry, w - 2, 1, ' ', fg, bg);
        s.text(x + 2, ry, &it.label, fg, bg);
        s.put(x + 2, ry, it.label.chars().next().unwrap_or(' '), if on { fg } else { t.menu_hot }, bg);
        s.text(x + w - 2 - it.keys.chars().count() as i32, ry, it.keys, fg, bg);
    }
    s.shadow(x, y, w, h);
}
