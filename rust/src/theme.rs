//! Colour schemes. "DN Gray" matches DOS Navigator's gray palette.

use crate::screen::C;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,

    // panels
    pub double_frame: bool,
    pub panel_bg: C, pub frame: C, pub col_line: C, pub header: C,
    pub file: C, pub dir: C, pub up: C, pub exec: C, pub archive: C, pub doc: C, pub image: C, pub hidden: C, pub junk: C,
    pub selected: C, pub cursor_fg: C, pub cursor_bg: C, pub info: C, pub status: C, pub free_num: C, pub free_label: C, pub error: C,
    pub title_fg: C, pub title_bg: C, pub active_title_fg: C, pub active_title_bg: C,
    pub scroll_fg: C, pub scroll_bg: C, pub drive_fg: C, pub drive_cur: C,

    // screen furniture
    pub menu_fg: C, pub menu_bg: C, pub menu_hot: C, pub menu_sel_fg: C, pub menu_sel_bg: C,
    pub clock_fg: C, pub clock_bg: C,
    /// title line of the viewer, editor and other full-screen windows
    pub bar_fg: C, pub bar_bg: C,
    pub key_num: C, pub key_label: C, pub key_bg: C,
    pub cmd_fg: C, pub cmd_bg: C,

    // dialogs
    pub dlg_fg: C, pub dlg_bg: C, pub dlg_frame: C, pub dlg_title: C, pub dlg_hot: C, pub dlg_close: C,
    pub input_fg: C, pub input_bg: C,
    pub btn_fg: C, pub btn_bg: C, pub btn_hot: C, pub btn_arrow: C,
    pub list_sel_fg: C, pub list_sel_bg: C,

    // viewer
    pub viewer_fg: C, pub viewer_bg: C,
    pub syn_keyword: C, pub syn_type: C, pub syn_string: C, pub syn_comment: C, pub syn_number: C, pub syn_meta: C,
}

use C::*;

pub static GRAY: Theme = Theme {
    name: "DN Gray",
    double_frame: false,
    panel_bg: DarkGray, frame: LightGray, col_line: White, header: Yellow,
    file: LightGray, dir: White, up: LightCyan, exec: LightCyan, archive: LightGreen,
    doc: Green, image: LightBlue, hidden: Cyan, junk: LightRed,
    selected: Yellow, cursor_fg: Black, cursor_bg: Cyan,
    info: LightCyan, status: Yellow, free_num: White, free_label: LightGray, error: LightRed,
    title_fg: White, title_bg: DarkGray, active_title_fg: White, active_title_bg: Cyan,
    scroll_fg: LightGray, scroll_bg: DarkGray, drive_fg: LightGray, drive_cur: Yellow,

    menu_fg: Black, menu_bg: LightGray, menu_hot: Red, menu_sel_fg: White, menu_sel_bg: Black,
    clock_fg: Blue, clock_bg: LightGray,
    bar_fg: Black, bar_bg: Cyan,
    key_num: Red, key_label: Black, key_bg: LightGray,
    cmd_fg: White, cmd_bg: Black,

    dlg_fg: Black, dlg_bg: LightGray, dlg_frame: White, dlg_title: White, dlg_hot: Yellow, dlg_close: LightGreen,
    input_fg: White, input_bg: Black,
    btn_fg: White, btn_bg: DarkGray, btn_hot: Yellow, btn_arrow: LightCyan,
    list_sel_fg: Black, list_sel_bg: Cyan,

    viewer_fg: LightGray, viewer_bg: DarkGray,
    syn_keyword: White, syn_type: Yellow, syn_string: LightGreen, syn_comment: Cyan, syn_number: LightMagenta, syn_meta: LightRed,
};

pub static BLUE: Theme = Theme {
    name: "Blue",
    double_frame: true,
    panel_bg: Blue, frame: LightCyan, col_line: LightCyan, header: Yellow,
    file: LightCyan, dir: White, up: White, exec: LightGreen, archive: LightMagenta,
    doc: LightCyan, image: LightCyan, hidden: Cyan, junk: LightGray,
    selected: Yellow, cursor_fg: Black, cursor_bg: Cyan,
    info: LightCyan, status: Yellow, free_num: White, free_label: LightCyan, error: LightRed,
    title_fg: LightCyan, title_bg: Blue, active_title_fg: Black, active_title_bg: Cyan,
    scroll_fg: Black, scroll_bg: Cyan, drive_fg: LightCyan, drive_cur: Yellow,

    menu_fg: Black, menu_bg: Cyan, menu_hot: Red, menu_sel_fg: White, menu_sel_bg: Black,
    clock_fg: Black, clock_bg: Cyan,
    bar_fg: Black, bar_bg: Cyan,
    key_num: White, key_label: Black, key_bg: Cyan,
    cmd_fg: LightGray, cmd_bg: Black,

    dlg_fg: Black, dlg_bg: LightGray, dlg_frame: Black, dlg_title: Black, dlg_hot: Red, dlg_close: Green,
    input_fg: Black, input_bg: Cyan,
    btn_fg: Black, btn_bg: Cyan, btn_hot: Red, btn_arrow: White,
    list_sel_fg: White, list_sel_bg: Black,

    viewer_fg: LightCyan, viewer_bg: Blue,
    syn_keyword: White, syn_type: Yellow, syn_string: LightGreen, syn_comment: LightGray, syn_number: LightMagenta, syn_meta: LightRed,
};

static BUILT_IN: [&Theme; 2] = [&GRAY, &BLUE];
/// Themes loaded while running (VS Code themes); never removed, so they can be 'static.
static LOADED: RwLock<Vec<&'static Theme>> = RwLock::new(Vec::new());
static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn get(i: usize) -> Option<&'static Theme> {
    if i < BUILT_IN.len() {
        Some(BUILT_IN[i])
    } else {
        LOADED.read().ok()?.get(i - BUILT_IN.len()).copied()
    }
}

pub fn current() -> &'static Theme {
    get(CURRENT.load(Ordering::Relaxed)).unwrap_or(&GRAY)
}

pub fn built_in_names() -> Vec<&'static str> {
    BUILT_IN.iter().map(|t| t.name).collect()
}

/// Make the theme with this name current. Returns false if no such theme is loaded.
pub fn set(name: &str) -> bool {
    let n = BUILT_IN.len() + LOADED.read().map(|l| l.len()).unwrap_or(0);
    match (0..n).find(|&i| get(i).is_some_and(|t| t.name.eq_ignore_ascii_case(name))) {
        Some(i) => {
            CURRENT.store(i, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// Add a theme (or replace the one with the same name) and make it current.
pub fn install(t: Theme) {
    let t: &'static Theme = Box::leak(Box::new(t));
    let Ok(mut l) = LOADED.write() else { return };
    let i = match l.iter().position(|x| x.name == t.name) {
        Some(i) => {
            l[i] = t;
            i
        }
        None => {
            l.push(t);
            l.len() - 1
        }
    };
    CURRENT.store(BUILT_IN.len() + i, Ordering::Relaxed);
}
