//! Line editor and DN-style dialogs (input, message box, pick list, progress).

use crate::app::{App, Input};
use crate::screen::{Screen, C};
use crate::theme;
use crate::util::{center, fit, fit_left};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};

/// Single-line text editor used by the command line and input dialogs.
pub struct LineEdit {
    text: Vec<char>,
    pos: usize,
    scroll: usize,
}

impl LineEdit {
    pub fn new(t: &str) -> Self {
        let text: Vec<char> = t.chars().collect();
        let pos = text.len();
        LineEdit { text, pos, scroll: 0 }
    }

    pub fn set(&mut self, t: &str) {
        *self = LineEdit::new(t);
    }

    pub fn insert(&mut self, t: &str) {
        for ch in t.chars() {
            self.text.insert(self.pos, ch);
            self.pos += 1;
        }
    }

    pub fn value(&self) -> String {
        self.text.iter().collect()
    }

    /// Cursor position, in characters.
    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn handle(&mut self, k: &KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        match k.code {
            KeyCode::Left => self.pos = self.pos.saturating_sub(1),
            KeyCode::Right => self.pos = (self.pos + 1).min(self.text.len()),
            KeyCode::Home => self.pos = 0,
            KeyCode::End => self.pos = self.text.len(),
            KeyCode::Backspace => {
                if self.pos > 0 {
                    self.pos -= 1;
                    self.text.remove(self.pos);
                }
            }
            KeyCode::Delete => {
                if self.pos < self.text.len() {
                    self.text.remove(self.pos);
                }
            }
            KeyCode::Char('y') if ctrl && !alt => self.set(""),
            // Ctrl+Alt is AltGr on many keyboards, which types characters
            KeyCode::Char(c) if !ctrl || alt => self.insert(&c.to_string()),
            _ => return false,
        }
        true
    }

    pub fn draw(&mut self, s: &mut Screen, x: i32, y: i32, w: i32, fg: C, bg: C) {
        if w < 1 {
            return;
        }
        let w = w as usize;
        if self.pos < self.scroll {
            self.scroll = self.pos;
        }
        if self.pos - self.scroll >= w {
            self.scroll = self.pos + 1 - w;
        }
        s.fill(x, y, w as i32, 1, ' ', fg, bg);
        let visible: String = self.text.iter().skip(self.scroll).take(w).collect();
        s.text(x, y, &visible, fg, bg);
        s.cursor = Some((x + (self.pos - self.scroll) as i32, y));
    }
}

/// Colours for one dialog: normal (theme) or warning (red).
struct Look {
    fg: C,
    bg: C,
    frame: C,
    title: C,
    hot: C,
}

fn normal() -> Look {
    let t = theme::current();
    Look { fg: t.dlg_fg, bg: t.dlg_bg, frame: t.dlg_frame, title: t.dlg_title, hot: t.dlg_hot }
}

fn warning() -> Look {
    Look { fg: C::White, bg: C::Red, frame: C::White, title: C::White, hot: C::Yellow }
}

/// Where a w×h dialog sits on the screen (centred).
fn place(s_w: i32, s_h: i32, w: i32, h: i32) -> (i32, i32) {
    (((s_w - w) / 2).max(0), ((s_h - h) / 2).max(0))
}

fn draw_frame(s: &mut Screen, x: i32, y: i32, w: i32, h: i32, title: &str, l: &Look) {
    s.cursor = None;
    s.fill(x, y, w, h, ' ', l.fg, l.bg);
    s.boxed(x + 1, y, w - 2, h, l.frame, l.bg, true);
    s.text(x + 3, y, "[", l.frame, l.bg);
    s.put(x + 4, y, '■', theme::current().dlg_close, l.bg);
    s.text(x + 5, y, "]", l.frame, l.bg);
    let t = format!(" {title} ");
    s.text(x + (w - t.chars().count() as i32) / 2, y, &t, l.title, l.bg);
    s.shadow(x, y, w, h);
}

/// DN dialog window at a given place: double frame, [■] close mark, centred title, shadow.
pub fn frame_at(s: &mut Screen, x: i32, y: i32, w: i32, h: i32, title: &str) {
    draw_frame(s, x, y, w, h, title, &normal());
}

/// Is the click on the [■] close mark of a dialog at (x, y)?
pub fn is_close_click(x: i32, y: i32, mx: i32, my: i32) -> bool {
    my == y && (x + 3..=x + 5).contains(&mx)
}

fn hot_index(text: &str) -> Option<usize> {
    text.chars().position(|c| c.is_uppercase()).or(if text.is_empty() { None } else { Some(0) })
}

/// Text where the first upper-case letter is the hotkey.
pub fn hot_text(s: &mut Screen, x: i32, y: i32, text: &str, fg: C, hot: C, bg: C) {
    s.text(x, y, text, fg, bg);
    if let Some(i) = hot_index(text) {
        s.put(x + i as i32, y, text.chars().nth(i).unwrap_or(' '), hot, bg);
    }
}

fn is_hotkey(text: &str, k: &KeyEvent) -> bool {
    let KeyCode::Char(c) = k.code else { return false };
    hot_index(text)
        .and_then(|i| text.chars().nth(i))
        .is_some_and(|h| h.to_lowercase().eq(c.to_lowercase()))
}

pub fn button_width(label: &str) -> i32 {
    label.chars().count() as i32 + 4
}

/// DN button: dark gray block with a black half-block shadow; the focused one shows ►label◄.
pub fn button(s: &mut Screen, x: i32, y: i32, label: &str, focused: bool, dialog_bg: C) {
    let t = theme::current();
    let w = button_width(label);
    s.fill(x, y, w, 1, ' ', t.btn_fg, t.btn_bg);
    hot_text(s, x + 2, y, label, t.btn_fg, t.btn_hot, t.btn_bg);
    if focused {
        s.put(x + 1, y, '►', t.btn_arrow, t.btn_bg);
        s.put(x + w - 2, y, '◄', t.btn_arrow, t.btn_bg);
    }
    s.put(x + w, y, '▄', C::Black, dialog_bg);
    s.hline(x + 1, y + 1, w, '▀', C::Black, dialog_bg);
}

fn is_left_click(i: &Input) -> Option<(i32, i32)> {
    match i {
        Input::Mouse(m) if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) => Some((m.column as i32, m.row as i32)),
        _ => None,
    }
}

pub fn input(app: &mut App, title: &str, prompt: &str, initial: &str) -> Option<String> {
    app.over(|app| input_over(app, title, prompt, initial))
}

fn input_over(app: &mut App, title: &str, prompt: &str, initial: &str) -> Option<String> {
    let mut ed = LineEdit::new(initial);
    loop {
        let w = (app.screen.w - 4).min(64.max(prompt.chars().count() as i32 + 10));
        let (x, y) = place(app.screen.w, app.screen.h, w, 6);
        let inp = app.modal_input(
            &mut |s| {
                let l = normal();
                draw_frame(s, x, y, w, 6, title, &l);
                hot_text(s, x + 4, y + 2, &fit(prompt, (w - 8) as usize), l.fg, l.hot, l.bg);
                let t = theme::current();
                ed.draw(s, x + 4, y + 3, w - 8, t.input_fg, t.input_bg);
            },
            None,
        );
        if let Some((mx, my)) = is_left_click(&inp) {
            let outside = mx < x || mx >= x + w || my < y || my >= y + 6;
            if outside || is_close_click(x, y, mx, my) {
                return None;
            }
        }
        if let Input::Key(k) = inp {
            match k.code {
                KeyCode::Enter => return Some(ed.value()),
                KeyCode::Esc => return None,
                _ => {
                    ed.handle(&k);
                }
            }
        }
    }
}

/// Message box with buttons. Returns the button index, or None on Esc.
/// A button's hotkey is its first upper-case letter ("sKip all" -> K). Buttons are clickable.
pub fn choice(app: &mut App, title: &str, lines: &[&str], buttons: &[&str], warn: bool) -> Option<usize> {
    app.over(|app| choice_over(app, title, lines, buttons, warn))
}

fn choice_over(app: &mut App, title: &str, lines: &[&str], buttons: &[&str], warn: bool) -> Option<usize> {
    let mut sel = 0usize;
    const GAP: i32 = 2;
    let btn_w: i32 = buttons.iter().map(|b| button_width(b)).sum::<i32>() + GAP * (buttons.len() as i32 - 1) + 1;
    let text_w = lines.iter().map(|l| l.chars().count() as i32).max().unwrap_or(0);
    loop {
        let w = (app.screen.w - 2).min(btn_w.max(text_w) + 10);
        let h = lines.len() as i32 + 6;
        let (x, y) = place(app.screen.w, app.screen.h, w, h);
        let (bx0, by) = (x + (w - btn_w) / 2, y + h - 3);
        let cur = sel;
        let inp = app.modal_input(
            &mut |s| {
                let l = if warn { warning() } else { normal() };
                draw_frame(s, x, y, w, h, title, &l);
                for (i, line) in lines.iter().enumerate() {
                    s.text(x + 4, y + 2 + i as i32, &center(line, (w - 8) as usize), l.fg, l.bg);
                }
                let mut bx = bx0;
                for (i, b) in buttons.iter().enumerate() {
                    button(s, bx, by, b, i == cur, l.bg);
                    bx += button_width(b) + GAP;
                }
            },
            None,
        );
        if let Some((mx, my)) = is_left_click(&inp) {
            if is_close_click(x, y, mx, my) {
                return None;
            }
            if my == by {
                let mut bx = bx0;
                for (i, b) in buttons.iter().enumerate() {
                    if mx >= bx && mx < bx + button_width(b) {
                        return Some(i);
                    }
                    bx += button_width(b) + GAP;
                }
            }
            continue;
        }
        let Input::Key(k) = inp else { continue };
        match k.code {
            KeyCode::Left | KeyCode::BackTab => sel = (sel + buttons.len() - 1) % buttons.len(),
            KeyCode::Right | KeyCode::Tab => sel = (sel + 1) % buttons.len(),
            KeyCode::Enter => return Some(sel),
            KeyCode::Esc => return None,
            _ => {
                if let Some(i) = buttons.iter().position(|b| is_hotkey(b, &k)) {
                    return Some(i);
                }
            }
        }
    }
}

pub fn message(app: &mut App, title: &str, text: &str, error: bool) {
    let lines: Vec<&str> = text.split('\n').collect();
    choice(app, title, &lines, &["OK"], error);
}

/// Draw a non-interactive progress box once (used during long operations).
pub fn progress(app: &mut App, title: &str, lines: &[String]) {
    app.screen.resize();
    app.draw_backdrop();
    let s = &mut app.screen;
    let w = (s.w - 2).min(70);
    let h = lines.len() as i32 + 5;
    let (x, y) = place(s.w, s.h, w, h);
    let l = normal();
    draw_frame(s, x, y, w, h, title, &l);
    let iw = (w - 8) as usize;
    for (i, line) in lines.iter().enumerate() {
        s.text(x + 4, y + 2 + i as i32, &fit(&fit_left(line, iw), iw), l.fg, l.bg);
    }
    s.text(x + 4, y + 2 + lines.len() as i32, &center("Esc to cancel", iw), C::DarkGray, l.bg);
    s.flush();
}
