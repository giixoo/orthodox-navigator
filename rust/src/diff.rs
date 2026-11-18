//! Side-by-side compare of two texts (Shift+F3 with the other panel, Alt+F3 against git HEAD).
//! Line diff with Myers' algorithm.

use crate::app::{draw_key_bars, App, Input};
use crate::dialogs;
use crate::screen::{Screen, C};
use crate::theme;
use crate::util::{fit, fit_left};
use crate::viewer::{draw_close_mark, is_close_mark};
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Same(usize, usize),
    Del(usize),
    Ins(usize),
}

/// Edit script from `a` to `b`. Very different inputs (over `MAX_D` edits) come back as delete-all + insert-all.
pub fn diff<T: PartialEq>(a: &[T], b: &[T]) -> Vec<Op> {
    const MAX_D: usize = 4000;
    // common prefix and suffix first: cheap and usually most of a file
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (am, bm) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let mut ops: Vec<Op> = (0..pre).map(|i| Op::Same(i, i)).collect();
    let mid = myers(am, bm, MAX_D).unwrap_or_else(|| (0..am.len()).map(Op::Del).chain((0..bm.len()).map(Op::Ins)).collect());
    ops.extend(mid.into_iter().map(|op| match op {
        Op::Same(i, j) => Op::Same(i + pre, j + pre),
        Op::Del(i) => Op::Del(i + pre),
        Op::Ins(j) => Op::Ins(j + pre),
    }));
    ops.extend((0..suf).map(|k| Op::Same(a.len() - suf + k, b.len() - suf + k)));
    ops
}

fn myers<T: PartialEq>(a: &[T], b: &[T], max_d: usize) -> Option<Vec<Op>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    if max == 0 {
        return Some(vec![]);
    }
    let off = max as isize;
    let mut v = vec![0isize; 2 * max + 2];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    'outer: for d in 0..=max.min(max_d) as isize {
        // keep only the k range used in this round
        trace.push(v[(off - d) as usize..=(off + d) as usize].to_vec());
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && v[(off + k - 1) as usize] < v[(off + k + 1) as usize]) {
                v[(off + k + 1) as usize]
            } else {
                v[(off + k - 1) as usize] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[(off + k) as usize] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'outer;
            }
            k += 2;
        }
    }
    found?;
    // walk back through the saved rounds
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..trace.len() as isize).rev() {
        let vd = &trace[d as usize];
        let get = |k: isize| vd[(k + d) as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && get(k - 1) < get(k + 1)) { k + 1 } else { k - 1 };
        let prev_x = if d == 0 { 0 } else { get(prev_k) };
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            ops.push(Op::Same(x as usize, y as usize));
        }
        if d > 0 {
            if x == prev_x {
                ops.push(Op::Ins(prev_y as usize));
            } else {
                ops.push(Op::Del(prev_x as usize));
            }
        }
        x = prev_x;
        y = prev_y;
    }
    ops.reverse();
    Some(ops)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Same,
    Changed,
    Deleted,
    Inserted,
}

struct Row {
    left: Option<usize>,
    right: Option<usize>,
    kind: RowKind,
}

/// Pair deletions with insertions so changed lines sit side by side.
fn rows(ops: &[Op]) -> Vec<Row> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < ops.len() {
        if let Op::Same(a, b) = ops[i] {
            out.push(Row { left: Some(a), right: Some(b), kind: RowKind::Same });
            i += 1;
            continue;
        }
        let (mut dels, mut ins) = (vec![], vec![]);
        while i < ops.len() {
            match ops[i] {
                Op::Del(a) => dels.push(a),
                Op::Ins(b) => ins.push(b),
                Op::Same(..) => break,
            }
            i += 1;
        }
        for k in 0..dels.len().max(ins.len()) {
            let (l, r) = (dels.get(k).copied(), ins.get(k).copied());
            let kind = match (l, r) {
                (Some(_), Some(_)) => RowKind::Changed,
                (Some(_), None) => RowKind::Deleted,
                _ => RowKind::Inserted,
            };
            out.push(Row { left: l, right: r, kind });
        }
    }
    out
}

fn lines_of(text: &str) -> Vec<String> {
    text.lines().map(|l| l.replace('\t', "    ")).collect()
}

/// Show `a` (left) and `b` (right) side by side.
pub fn show(app: &mut App, title_a: &str, a: &str, title_b: &str, b: &str) {
    let (la, lb) = (lines_of(a), lines_of(b));
    let rows = rows(&diff(&la, &lb));
    // first row of each block of changes, for F7 / Shift+F7
    let starts: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].kind != RowKind::Same && (i == 0 || rows[i - 1].kind == RowKind::Same)).collect();
    if starts.is_empty() {
        dialogs::message(app, "Compare", "The files are identical (apart from line endings).", false);
        return;
    }
    let (mut top, mut left) = (0usize, 0usize);
    if let Some(&s0) = starts.first() {
        top = s0.saturating_sub(3);
    }
    loop {
        let page = (app.screen.h - 3).max(1) as usize;
        let (cur_top, cur_left) = (top, left);
        let (rows_ref, la_ref, lb_ref) = (&rows, &la, &lb);
        let n_changes = starts.len();
        let inp = app.window_input(
            &mut |s: &mut Screen| {
                let t = theme::current();
                let (w, h) = (s.w, s.h);
                s.cursor = None;
                s.fill(0, 0, w, 1, ' ', theme::current().bar_fg, theme::current().bar_bg);
                draw_close_mark(s);
                let right = format!(" {n_changes} change{}  Line {}/{} ", if n_changes == 1 { "" } else { "s" }, cur_top + 1, rows_ref.len());
                let rl = right.chars().count() as i32;
                s.text(4, 0, "Compare", theme::current().bar_fg, theme::current().bar_bg);
                s.text(w - rl, 0, &right, theme::current().bar_fg, theme::current().bar_bg);
                let half = w / 2;
                let num_w = (rows_ref.len().max(1).ilog10() + 1).max(3) as i32;
                // file names
                s.fill(0, 1, w, 1, ' ', t.header, t.panel_bg);
                s.text(1, 1, &fit_left(title_a, (half - 2) as usize), t.header, t.panel_bg);
                s.text(half + 1, 1, &fit_left(title_b, (w - half - 2) as usize), t.header, t.panel_bg);
                for r in 0..(h - 3).max(0) as usize {
                    let y = r as i32 + 2;
                    s.fill(0, y, w, 1, ' ', t.viewer_fg, t.viewer_bg);
                    s.put(half - 1, y, '│', t.frame, t.viewer_bg);
                    let Some(row) = rows_ref.get(cur_top + r) else { continue };
                    let (fg_l, fg_r, bg_l, bg_r) = match row.kind {
                        RowKind::Same => (t.viewer_fg, t.viewer_fg, t.viewer_bg, t.viewer_bg),
                        RowKind::Changed => (t.syn_type, t.syn_type, t.viewer_bg, t.viewer_bg),
                        RowKind::Deleted => (t.error, t.viewer_fg, t.viewer_bg, t.viewer_bg),
                        RowKind::Inserted => (t.viewer_fg, t.syn_string, t.viewer_bg, t.viewer_bg),
                    };
                    let mut side = |x: i32, width: i32, idx: Option<usize>, lines: &[String], fg: C, bg: C, mark: char| {
                        if let Some(i) = idx {
                            s.text(x, y, &format!("{:>num_w$}", i + 1, num_w = num_w as usize), t.frame, t.viewer_bg);
                            if row.kind != RowKind::Same {
                                s.put(x + num_w, y, mark, fg, bg);
                            }
                            let text: String = lines[i].chars().skip(cur_left).collect();
                            s.text(x + num_w + 1, y, &fit(&text, (width - num_w - 1).max(0) as usize), fg, bg);
                        } else {
                            s.fill(x, y, width, 1, '░', t.frame, t.viewer_bg);
                        }
                    };
                    let mark_l = if row.kind == RowKind::Deleted { '-' } else { '~' };
                    let mark_r = if row.kind == RowKind::Inserted { '+' } else { '~' };
                    side(0, half - 1, row.left, la_ref, fg_l, bg_l, mark_l);
                    side(half, w - half, row.right, lb_ref, fg_r, bg_r, mark_r);
                }
                draw_key_bars(
                    s,
                    &[
                        (KeyModifiers::NONE, ["Help", "", "", "", "", "", "Next", "", "", ""]),
                        (KeyModifiers::SHIFT, ["", "", "", "", "", "", "Prev", "", "", ""]),
                    ],
                );
            },
            None,
        );
        let max = rows.len().saturating_sub(page);
        match inp {
            Input::Key(k) => match k.code {
                KeyCode::Esc => return,
                KeyCode::Up => top = top.saturating_sub(1),
                KeyCode::Down => top += 1,
                KeyCode::PageUp => top = top.saturating_sub(page),
                KeyCode::PageDown => top += page,
                KeyCode::Home => (top, left) = (0, 0),
                KeyCode::End => top = max,
                KeyCode::Left => left = left.saturating_sub(8),
                KeyCode::Right => left += 8,
                KeyCode::F(7) if k.modifiers.contains(KeyModifiers::SHIFT) => {
                    if let Some(&s0) = starts.iter().rev().find(|&&s0| s0.saturating_sub(3) < top) {
                        top = s0.saturating_sub(3);
                    }
                }
                KeyCode::F(7) => {
                    if let Some(&s0) = starts.iter().find(|&&s0| s0.saturating_sub(3) > top) {
                        top = s0.saturating_sub(3);
                    }
                }
                KeyCode::F(1) => crate::help::show(app, "diff"),
                _ => {}
            },
            Input::Mouse(m) => match m.kind {
                MouseEventKind::ScrollUp => top = top.saturating_sub(3),
                MouseEventKind::ScrollDown => top += 3,
                MouseEventKind::Down(MouseButton::Left) if is_close_mark(m.column, m.row) => return,
                _ => {}
            },
            _ => {}
        }
        top = top.min(max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(a: &[&str], b: &[&str], ops: &[Op]) -> Vec<String> {
        // rebuild b from the script and check every Same really is equal
        let mut out = vec![];
        for op in ops {
            match *op {
                Op::Same(i, j) => {
                    assert_eq!(a[i], b[j]);
                    out.push(b[j].to_string());
                }
                Op::Ins(j) => out.push(b[j].to_string()),
                Op::Del(_) => {}
            }
        }
        out
    }

    #[test]
    fn diffs() {
        let a = ["a", "b", "c", "a", "b", "b", "a"];
        let b = ["c", "b", "a", "b", "a", "c"];
        let ops = diff(&a, &b);
        assert_eq!(apply(&a, &b, &ops), b);
        let dels = ops.iter().filter(|o| matches!(o, Op::Del(_))).count();
        let ins = ops.iter().filter(|o| matches!(o, Op::Ins(_))).count();
        assert_eq!(dels + ins, 5); // the shortest edit script
        assert_eq!(diff::<&str>(&[], &[]), vec![]);
        let one = diff(&["x"], &["y"]);
        assert_eq!(one, vec![Op::Del(0), Op::Ins(0)]);
    }

    #[test]
    fn side_by_side() {
        let ops = diff(&["a", "b", "c"], &["a", "B", "c", "d"]);
        let r = rows(&ops);
        let kinds: Vec<_> = r.iter().map(|r| r.kind).collect();
        assert!(kinds == vec![RowKind::Same, RowKind::Changed, RowKind::Same, RowKind::Inserted]);
    }
}
