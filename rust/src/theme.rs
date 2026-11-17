//! Colour schemes. "DN Gray" matches DOS Navigator's gray palette.

use crate::screen::C;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    pub key_num: C, pub key_label: C, pub key_bg: C,
    pub cmd_fg: C, pub cmd_bg: C,

    // dialogs
    pub dlg_fg: C, pub dlg_bg: C, pub dlg_frame: C, pub dlg_title: C, pub dlg_hot: C, pub dlg_close: C,
    pub input_fg: C, pub input_bg: C,
    pub btn_fg: C, pub btn_bg: C, pub btn_hot: C, pub btn_arrow: C,
    pub list_sel_fg: C, pub list_sel_bg: C,

    // viewer
    pub viewer_fg: C, pub viewer_bg: C,
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
    key_num: Red, key_label: Black, key_bg: LightGray,
    cmd_fg: White, cmd_bg: Black,

    dlg_fg: Black, dlg_bg: LightGray, dlg_frame: White, dlg_title: White, dlg_hot: Yellow, dlg_close: LightGreen,
    input_fg: White, input_bg: Black,
    btn_fg: White, btn_bg: DarkGray, btn_hot: Yellow, btn_arrow: LightCyan,
    list_sel_fg: Black, list_sel_bg: Cyan,

    viewer_fg: LightGray, viewer_bg: DarkGray,
};

pub static BLUE: Theme = Theme {
    name: "Blue",
    double_frame: true,
    panel_bg: Blue, frame: LightCyan, col_line: LightCyan, header: Yellow,
    file: LightCyan, dir: White, up: White, exec: LightGreen, archive: LightMagenta,
    doc: LightCyan, image: LightCyan, hidden: Cyan, junk: LightGray,
    selected: Yellow, cursor_fg: Black, cursor_bg: Cyan,
    info: LightCyan, status: Yellow, free_num: LightCyan, free_label: LightCyan, error: LightRed,
    title_fg: LightCyan, title_bg: Blue, active_title_fg: Black, active_title_bg: Cyan,
    scroll_fg: Black, scroll_bg: Cyan, drive_fg: LightCyan, drive_cur: Yellow,

    menu_fg: Black, menu_bg: Cyan, menu_hot: Red, menu_sel_fg: White, menu_sel_bg: Black,
    clock_fg: Black, clock_bg: Cyan,
    key_num: White, key_label: Black, key_bg: Cyan,
    cmd_fg: LightGray, cmd_bg: Black,

    dlg_fg: Black, dlg_bg: LightGray, dlg_frame: Black, dlg_title: Black, dlg_hot: Red, dlg_close: Green,
    input_fg: Black, input_bg: Cyan,
    btn_fg: Black, btn_bg: Cyan, btn_hot: Red, btn_arrow: White,
    list_sel_fg: White, list_sel_bg: Black,

    viewer_fg: LightCyan, viewer_bg: Blue,
};

pub static ALL: [&Theme; 2] = [&GRAY, &BLUE];
static CURRENT: AtomicUsize = AtomicUsize::new(0);

pub fn current() -> &'static Theme {
    ALL[CURRENT.load(Ordering::Relaxed) % ALL.len()]
}

pub fn next() {
    CURRENT.store((CURRENT.load(Ordering::Relaxed) + 1) % ALL.len(), Ordering::Relaxed);
}

pub fn set(name: &str) {
    let i = ALL.iter().position(|t| t.name.eq_ignore_ascii_case(name)).unwrap_or(0);
    CURRENT.store(i, Ordering::Relaxed);
}
