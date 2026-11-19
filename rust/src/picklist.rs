//! A centred list to pick from, used by the user menu (F9), folder history (Alt+Backspace) and others.

use crate::app::{draw_key_bar, App, Input};
use crate::dialogs::{frame_at, is_close_click};
use crate::theme;
use crate::util::{center, fit, fit_left};
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use std::time::Instant;

/// One row of a pick list.
pub struct Row {
    pub text: String,
    /// shown dimmed at the right (a command, a path, a count)
    pub right: String,
    /// a heading between groups: not selectable
    pub header: bool,
    /// a letter that picks this row at once (shown highlighted)
    pub hot: Option<char>,
}

impl Row {
    pub fn item(text: impl Into<String>, right: impl Into<String>) -> Self {
        Row { text: text.into(), right: right.into(), header: false, hot: None }
    }
    pub fn header(text: impl Into<String>) -> Self {
        Row { text: text.into(), right: String::new(), header: true, hot: None }
    }
}

/// What the user did: the row (an index into the rows given), and the key.
pub struct Picked {
    pub row: usize,
    /// Enter (also a double click or a row's hot letter), or one of the extra keys asked for
    pub key: KeyCode,
}

/// How the list looks and which keys it gives back.
pub struct Opts<'a> {
    pub title: &'a str,
    pub start: usize,
    /// extra keys returned with the row under the cursor (F4, Del, ...)
    pub keys: &'a [KeyCode],
    pub bar: [&'a str; 10],
    /// the help page for F1
    pub help: &'a str,
    /// typing narrows the list to the rows containing the text (instead of hot letters)
    pub filter: bool,
}

/// ↑↓ PgUp PgDn Home End move, Enter or a double click picks, Esc or a click outside closes.
pub fn pick(app: &mut App, rows: &[Row], o: &Opts) -> Option<Picked> {
    let mut sel = o.start;
    let mut top = 0usize;
    let mut typed = String::new();
    let mut last_click: Option<(Instant, usize)> = None;
    loop {
        let low = typed.to_lowercase();
        let shown: Vec<usize> = (0..rows.len())
            .filter(|&i| low.is_empty() || (!rows[i].header && format!("{} {}", rows[i].text, rows[i].right).to_lowercase().contains(&low)))
            .collect();
        let selectable = |i: usize| shown.get(i).is_some_and(|&r| !rows[r].header);
        if !shown.is_empty() {
            sel = sel.min(shown.len() - 1);
            if !selectable(sel) {
                sel = (sel..shown.len()).chain((0..sel).rev()).find(|&i| selectable(i)).unwrap_or(sel);
            }
        }
        let (sw, sh) = (app.screen.w, app.screen.h);
        let want = rows
            .iter()
            .map(|r| r.text.chars().count() + if r.right.is_empty() { 0 } else { r.right.chars().count() + 3 })
            .max()
            .unwrap_or(20) as i32
            + 10;
        let w = want.max(o.title.chars().count() as i32 + 12).max(40).min(sw - 2).max(10);
        let visible = (shown.len().max(1) as i32).min(sh - 7 - i32::from(o.filter)).max(1) as usize;
        let h = visible as i32 + 4 + i32::from(o.filter);
        let (x, y) = (((sw - w) / 2).max(0), ((sh - 1 - h) / 2).max(0));
        if sel < top {
            top = sel;
        }
        if sel >= top + visible {
            top = sel + 1 - visible;
        }
        let (cur, cur_top, shown_ref, typed_ref) = (sel, top, &shown, &typed);
        let inp = app.window_input(
            &mut |s| {
                let t = theme::current();
                frame_at(s, x, y, w, h, o.title);
                let inner = (w - 6).max(1) as usize;
                if shown_ref.is_empty() {
                    let msg = if typed_ref.is_empty() { "(empty)" } else { "(nothing matches)" };
                    s.text(x + 3, y + 2, &center(msg, inner), t.dlg_fg, t.dlg_bg);
                }
                for r in 0..visible {
                    let Some(&ri) = shown_ref.get(cur_top + r) else { break };
                    let row = &rows[ri];
                    let ry = y + 2 + r as i32;
                    if row.header {
                        s.hline(x + 3, ry, inner as i32, '─', t.dlg_fg, t.dlg_bg);
                        s.text(x + 4, ry, &fit(&format!(" {} ", row.text), inner.saturating_sub(2)), t.dlg_title, t.dlg_bg);
                        continue;
                    }
                    let on = cur_top + r == cur;
                    let (fg, bg) = if on { (t.list_sel_fg, t.list_sel_bg) } else { (t.dlg_fg, t.dlg_bg) };
                    s.fill(x + 3, ry, inner as i32, 1, ' ', fg, bg);
                    let rw = row.right.chars().count().min(inner / 2);
                    let lw = inner.saturating_sub(rw + 2);
                    s.text(x + 3, ry, &fit(&format!(" {}", row.text), lw), fg, bg);
                    if let Some(hc) = row.hot {
                        if let Some(p) = row.text.chars().position(|c| c.eq_ignore_ascii_case(&hc)).filter(|&p| p + 1 < lw) {
                            s.put(x + 4 + p as i32, ry, row.text.chars().nth(p).unwrap_or(hc), if on { fg } else { t.dlg_hot }, bg);
                        }
                    }
                    if rw > 0 {
                        s.text(x + 3 + inner as i32 - rw as i32 - 1, ry, &fit_left(&row.right, rw), fg, bg);
                    }
                }
                if shown_ref.len() > visible {
                    s.put(x + w - 3, y + 2, if cur_top > 0 { '▲' } else { ' ' }, t.dlg_fg, t.dlg_bg);
                    s.put(x + w - 3, y + 1 + visible as i32, if cur_top + visible < shown_ref.len() { '▼' } else { ' ' }, t.dlg_fg, t.dlg_bg);
                }
                if o.filter {
                    let text = if typed_ref.is_empty() { "Type to filter".to_string() } else { format!("Filter: {typed_ref}") };
                    s.text(x + 4, y + h - 2, &fit(&text, inner.saturating_sub(2)), t.dlg_fg, t.dlg_bg);
                }
                draw_key_bar(s, &o.bar);
            },
            None,
        );
        let picked = |i: usize, key: KeyCode| shown.get(i).filter(|&&r| !rows[r].header).map(|&row| Picked { row, key });
        let step = |from: usize, down: bool| -> usize {
            let n = shown.len();
            let mut i = from;
            for _ in 0..n {
                i = if down { (i + 1) % n } else { (i + n - 1) % n };
                if selectable(i) {
                    return i;
                }
            }
            from
        };
        match inp {
            Input::Key(k) => {
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Esc if o.filter && !typed.is_empty() => typed.clear(),
                    KeyCode::Esc => return None,
                    KeyCode::Enter => {
                        if let Some(p) = picked(sel, KeyCode::Enter) {
                            return Some(p);
                        }
                    }
                    KeyCode::F(1) if !o.help.is_empty() => crate::help::show(app, o.help),
                    // an extra key works on the row under the cursor (usize::MAX: there is none)
                    c if o.keys.contains(&c) => return Some(picked(sel, c).unwrap_or(Picked { row: usize::MAX, key: c })),
                    KeyCode::Up => sel = step(sel, false),
                    KeyCode::Down => sel = step(sel, true),
                    KeyCode::PageUp => sel = sel.saturating_sub(visible),
                    KeyCode::PageDown => sel = (sel + visible).min(shown.len().saturating_sub(1)),
                    KeyCode::Home => sel = 0,
                    KeyCode::End => sel = shown.len().saturating_sub(1),
                    KeyCode::Backspace if o.filter => {
                        typed.pop();
                    }
                    KeyCode::Char(c) if o.filter && !ctrl => {
                        typed.push(c);
                        (sel, top) = (0, 0);
                    }
                    KeyCode::Char(c) if !ctrl => {
                        // a hot letter: the only row with it is picked, else the cursor goes to the next one
                        let hits: Vec<usize> = (0..shown.len()).filter(|&i| rows[shown[i]].hot.is_some_and(|h| h.eq_ignore_ascii_case(&c))).collect();
                        if hits.len() == 1 {
                            return picked(hits[0], KeyCode::Enter);
                        }
                        if let Some(&i) = hits.iter().find(|&&i| i > sel).or(hits.first()) {
                            sel = i;
                        }
                    }
                    _ => {}
                }
            }
            Input::Mouse(m) => {
                let (mx, my) = (m.column as i32, m.row as i32);
                match m.kind {
                    MouseEventKind::ScrollUp => sel = step(sel, false),
                    MouseEventKind::ScrollDown => sel = step(sel, true),
                    MouseEventKind::Down(MouseButton::Left) => {
                        if mx < x || mx >= x + w || my < y || my >= y + h || is_close_click(x, y, mx, my) {
                            return None;
                        }
                        let r = my - y - 2;
                        if r >= 0 && (r as usize) < visible {
                            let i = top + r as usize;
                            if selectable(i) {
                                let double = last_click.is_some_and(|(at, j)| j == i && at.elapsed().as_millis() < crate::platform::double_click_ms());
                                sel = i;
                                if double {
                                    return picked(i, KeyCode::Enter);
                                }
                                last_click = Some((Instant::now(), i));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Input::Paste(t) if o.filter => typed.push_str(t.trim()),
            _ => {}
        }
    }
}
