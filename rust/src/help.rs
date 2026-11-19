//! F1: help in the style of Far Manager's. Pages are Markdown files in `rust/help/`, built into the
//! program; links between them are ordinary Markdown links (`[Viewer](viewer.md)`).
//! ↑↓ scroll, Tab / Shift+Tab pick a link, Enter opens it, Backspace goes back, F1 the contents.

use crate::app::{draw_key_bar, App, Input};
use crate::screen::{Screen, C};
use crate::theme;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};

const TOPICS: &[(&str, &str)] = &[
    ("index", include_str!("../help/index.md")),
    ("help", include_str!("../help/help.md")),
    ("panels", include_str!("../help/panels.md")),
    ("cmdline", include_str!("../help/cmdline.md")),
    ("viewer", include_str!("../help/viewer.md")),
    ("editor", include_str!("../help/editor.md")),
    ("preview", include_str!("../help/preview.md")),
    ("tree", include_str!("../help/tree.md")),
    ("favorites", include_str!("../help/favorites.md")),
    ("find", include_str!("../help/find.md")),
    ("diff", include_str!("../help/diff.md")),
    ("tools", include_str!("../help/tools.md")),
    ("rename", include_str!("../help/rename.md")),
    ("usermenu", include_str!("../help/usermenu.md")),
    ("sysmon", include_str!("../help/sysmon.md")),
    ("calc", include_str!("../help/calc.md")),
    ("themes", include_str!("../help/themes.md")),
    ("game", include_str!("../help/game.md")),
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Sty {
    Text,
    Bold,
    Head,
    Link,
}

/// One character on a laid-out line, and the link it belongs to.
type Cell = (char, Sty, Option<usize>);

struct Page {
    title: String,
    lines: Vec<Vec<Cell>>,
    /// link targets (topic names), by link number
    links: Vec<String>,
}

fn source(topic: &str) -> Option<&'static str> {
    TOPICS.iter().find(|t| t.0 == topic).map(|t| t.1)
}

/// "viewer.md" -> "viewer"; external links (http...) are not followed.
fn topic_of(target: &str) -> Option<String> {
    if target.contains("://") {
        return None;
    }
    let t = target.split('#').next().unwrap_or("");
    let t = t.rsplit(['/', '\\']).next().unwrap_or(t);
    Some(t.strip_suffix(".md").unwrap_or(t).to_string())
}

/// Inline Markdown (`**bold**`, `` `code` ``, `[text](link)`, `\` escapes) to styled characters.
fn inline(s: &str, links: &mut Vec<String>, base: Sty) -> Vec<Cell> {
    let ch: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let (mut bold, mut code) = (false, false);
    let mut i = 0;
    while i < ch.len() {
        let c = ch[i];
        let sty = if bold || code { Sty::Bold } else { base };
        if code {
            if c == '`' {
                code = false;
            } else {
                out.push((c, sty, None));
            }
            i += 1;
            continue;
        }
        match c {
            '\\' if i + 1 < ch.len() && ch[i + 1].is_ascii_punctuation() => {
                out.push((ch[i + 1], sty, None));
                i += 2;
            }
            '`' => {
                code = true;
                i += 1;
            }
            '*' if ch.get(i + 1) == Some(&'*') => {
                bold = !bold;
                i += 2;
            }
            '[' => {
                // [text](target)
                let close = ch[i..].iter().position(|&c| c == ']').map(|p| p + i);
                let link = close.filter(|&j| ch.get(j + 1) == Some(&'(')).and_then(|j| ch[j..].iter().position(|&c| c == ')').map(|p| (j, p + j)));
                match link {
                    Some((j, k)) => {
                        let text: String = ch[i + 1..j].iter().collect();
                        let target: String = ch[j + 2..k].iter().collect();
                        match topic_of(&target) {
                            Some(t) => {
                                let id = links.len();
                                links.push(t);
                                out.extend(inline(&text, &mut Vec::new(), Sty::Link).into_iter().map(|(c, _, _)| (c, Sty::Link, Some(id))));
                            }
                            None => out.extend(inline(&text, links, Sty::Bold)),
                        }
                        i = k + 1;
                    }
                    None => {
                        out.push((c, sty, None));
                        i += 1;
                    }
                }
            }
            _ => {
                out.push((c, sty, None));
                i += 1;
            }
        }
    }
    out
}

/// Word-wrap styled text to `width`; the first line starts at `first` columns in, the rest at `rest`.
fn wrap(cells: &[Cell], width: usize, first: usize, rest: usize) -> Vec<Vec<Cell>> {
    let mut lines = Vec::new();
    let mut line: Vec<Cell> = vec![(' ', Sty::Text, None); first];
    let mut words = cells.split(|c| c.0 == ' ').filter(|w| !w.is_empty()).peekable();
    let mut empty = true;
    while let Some(word) = words.next() {
        let room = width.max(rest + 1);
        if !empty && line.len() + 1 + word.len() > room {
            lines.push(std::mem::replace(&mut line, vec![(' ', Sty::Text, None); rest]));
            empty = true;
        }
        if !empty {
            // a space inside a link belongs to the link (so the highlight is continuous)
            let link = line.last().and_then(|c| c.2).filter(|&l| word.first().and_then(|c| c.2) == Some(l));
            line.push((' ', if link.is_some() { Sty::Link } else { Sty::Text }, link));
        }
        // a word longer than the line is cut
        for chunk in word.chunks(room.saturating_sub(rest).max(1)) {
            if line.len() + chunk.len() > room && !empty {
                lines.push(std::mem::replace(&mut line, vec![(' ', Sty::Text, None); rest]));
            }
            line.extend_from_slice(chunk);
            empty = false;
        }
        let _ = words.peek();
    }
    if !empty || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// The cells of a table row; `\|` stays inside a cell (escapes are kept for `inline`).
fn table_cells(row: &str) -> Vec<String> {
    let ch: Vec<char> = row.trim().chars().collect();
    let mut cells = vec![String::new()];
    let mut i = usize::from(ch.first() == Some(&'|'));
    while i < ch.len() {
        match ch[i] {
            '\\' if i + 1 < ch.len() => {
                cells.last_mut().unwrap().push('\\');
                cells.last_mut().unwrap().push(ch[i + 1]);
                i += 1;
            }
            '|' => cells.push(String::new()),
            c => cells.last_mut().unwrap().push(c),
        }
        i += 1;
    }
    if cells.len() > 1 && cells.last().is_some_and(|c| c.trim().is_empty()) {
        cells.pop();
    }
    cells.into_iter().map(|c| c.trim().to_string()).collect()
}
fn is_separator(row: &str) -> bool {
    row.contains('-') && row.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// Lay a page out for a window `width` columns wide.
fn layout(md: &str, width: usize) -> Page {
    let mut page = Page { title: String::new(), lines: Vec::new(), links: Vec::new() };
    let blank = |p: &mut Page| {
        if p.lines.last().is_some_and(|l| !l.is_empty()) {
            p.lines.push(Vec::new());
        }
    };
    let src: Vec<&str> = md.lines().collect();
    let mut i = 0;
    let mut para = String::new();
    let flush = |p: &mut Page, para: &mut String| {
        if !para.trim().is_empty() {
            let cells = inline(para.trim(), &mut p.links, Sty::Text);
            p.lines.extend(wrap(&cells, width, 0, 0));
        }
        para.clear();
    };
    while i < src.len() {
        let line = src[i];
        let t = line.trim();
        if t.starts_with('#') {
            flush(&mut page, &mut para);
            let text = t.trim_start_matches('#').trim();
            if page.title.is_empty() && t.starts_with("# ") {
                page.title = text.to_string();
            } else {
                blank(&mut page);
                let cells: Vec<Cell> = inline(text, &mut page.links, Sty::Head);
                page.lines.extend(wrap(&cells, width, 0, 0).into_iter().map(|l| l.into_iter().map(|(c, s, k)| (c, if s == Sty::Text { Sty::Head } else { s }, k)).collect()));
                page.lines.push(Vec::new());
            }
        } else if t.is_empty() {
            flush(&mut page, &mut para);
            blank(&mut page);
        } else if t.starts_with("```") {
            flush(&mut page, &mut para);
            i += 1;
            while i < src.len() && !src[i].trim().starts_with("```") {
                page.lines.push(src[i].chars().take(width).map(|c| (c, Sty::Bold, None)).collect());
                i += 1;
            }
        } else if t.starts_with("- ") || t.starts_with("* ") {
            flush(&mut page, &mut para);
            let mut item = t[2..].to_string();
            while i + 1 < src.len() && src[i + 1].starts_with("  ") && !src[i + 1].trim().is_empty() && !src[i + 1].trim().starts_with("- ") {
                i += 1;
                item.push(' ');
                item.push_str(src[i].trim());
            }
            let cells = inline(&item, &mut page.links, Sty::Text);
            let mut lines = wrap(&cells, width, 2, 2);
            if let Some(first) = lines.first_mut() {
                first[0] = ('•', Sty::Text, None);
            }
            page.lines.extend(lines);
        } else if t.starts_with('|') {
            flush(&mut page, &mut para);
            let mut rows: Vec<Vec<String>> = Vec::new();
            while i < src.len() && src[i].trim().starts_with('|') {
                if !is_separator(src[i].trim()) {
                    rows.push(table_cells(src[i]));
                }
                i += 1;
            }
            i -= 1;
            page.lines.extend(table(&rows, width, &mut page.links));
        } else {
            para.push(' ');
            para.push_str(t);
        }
        i += 1;
    }
    flush(&mut page, &mut para);
    while page.lines.last().is_some_and(|l| l.is_empty()) {
        page.lines.pop();
    }
    page
}

/// A table: the first row is the header; columns but the last get their natural width (up to a
/// third of the window), the last one wraps in what is left.
fn table(rows: &[Vec<String>], width: usize, links: &mut Vec<String>) -> Vec<Vec<Cell>> {
    let ncol = rows.iter().map(Vec::len).max().unwrap_or(0);
    if ncol == 0 {
        return vec![];
    }
    let parsed: Vec<Vec<Vec<Cell>>> = rows
        .iter()
        .enumerate()
        .map(|(r, row)| (0..ncol).map(|c| inline(row.get(c).map_or("", |s| s.as_str()), links, if r == 0 { Sty::Head } else { Sty::Text })).collect())
        .collect();
    let gap = 2;
    let mut widths: Vec<usize> = (0..ncol).map(|c| parsed.iter().map(|r| r[c].len()).max().unwrap_or(0).min(width / 3).max(1)).collect();
    let used: usize = widths[..ncol - 1].iter().sum::<usize>() + gap * (ncol - 1);
    widths[ncol - 1] = width.saturating_sub(used).max(8);
    let mut out = Vec::new();
    for row in &parsed {
        let cols: Vec<Vec<Vec<Cell>>> = row.iter().zip(&widths).map(|(cell, &w)| wrap(cell, w, 0, 0)).collect();
        let h = cols.iter().map(Vec::len).max().unwrap_or(1);
        for k in 0..h {
            let mut line: Vec<Cell> = Vec::new();
            for (c, col) in cols.iter().enumerate() {
                let start = line.len();
                if let Some(l) = col.get(k) {
                    line.extend_from_slice(l);
                }
                if c + 1 < ncol {
                    line.resize(start + widths[c] + gap, (' ', Sty::Text, None));
                }
            }
            while line.last().is_some_and(|c| c.0 == ' ' && c.2.is_none()) {
                line.pop();
            }
            out.push(line);
        }
    }
    out
}

fn load(topic: &str, width: usize) -> Page {
    match source(topic) {
        Some(md) => layout(md, width),
        None => layout(&format!("# Not found\n\nThere is no help page \"{topic}\".\n\nSee the [contents](index.md)."), width),
    }
}

/// The first line on which link `id` appears.
fn link_line(p: &Page, id: usize) -> Option<usize> {
    p.lines.iter().position(|l| l.iter().any(|c| c.2 == Some(id)))
}

/// The first link on lines `top..top+rows`.
fn first_visible_link(p: &Page, top: usize, rows: usize) -> Option<usize> {
    p.lines.iter().skip(top).take(rows).flat_map(|l| l.iter()).find_map(|c| c.2)
}

struct View {
    topic: String,
    top: usize,
    sel: Option<usize>,
}

fn colors(sty: Sty, selected: bool) -> (C, C) {
    let t = theme::current();
    match sty {
        Sty::Link if selected => (t.cursor_fg, t.cursor_bg),
        Sty::Link => (t.selected, t.panel_bg),
        Sty::Bold => (t.dir, t.panel_bg),
        Sty::Head => (t.header, t.panel_bg),
        Sty::Text => (t.file, t.panel_bg),
    }
}

/// The window: (x, y, w, h), and the text area inside it (x, y, w, rows).
/// x, y, width, height
type Rect = (i32, i32, i32, i32);

fn geometry(sw: i32, sh: i32) -> (Rect, Rect) {
    let (x, y, w, h) = (1, 1, sw - 2, sh - 2);
    ((x, y, w, h), (x + 2, y + 3, (w - 4).max(10), (h - 4).max(1)))
}

fn draw(s: &mut Screen, p: &Page, v: &View) {
    let t = theme::current();
    let ((x, y, w, h), (tx, ty, tw, rows)) = geometry(s.w, s.h);
    s.cursor = None;
    s.fill(x, y, w, h, ' ', t.file, t.panel_bg);
    s.boxed(x, y, w, h, t.frame, t.panel_bg, true);
    let title = " Help ";
    s.text(x + (w - title.len() as i32) / 2, y, title, t.title_fg, t.title_bg);
    s.text(tx, y + 1, &crate::util::fit(&p.title, tw as usize), t.header, t.panel_bg);
    s.put(x, y + 2, '╟', t.frame, t.panel_bg);
    s.hline(x + 1, y + 2, w - 2, '─', t.frame, t.panel_bg);
    s.put(x + w - 1, y + 2, '╢', t.frame, t.panel_bg);
    for (r, line) in p.lines.iter().skip(v.top).take(rows as usize).enumerate() {
        for (cx, &(ch, sty, link)) in line.iter().take(tw as usize).enumerate() {
            let (fg, bg) = colors(sty, link.is_some() && link == v.sel);
            s.put(tx + cx as i32, ty + r as i32, ch, fg, bg);
        }
    }
    // scroll bar on the right border
    let total = p.lines.len();
    if total > rows as usize {
        let bar_h = rows;
        let max_top = total - rows as usize;
        let pos = (v.top * (bar_h as usize - 1)) / max_top.max(1);
        for r in 0..bar_h {
            s.put(x + w - 1, ty + r, if r as usize == pos { '■' } else { '░' }, t.scroll_fg, t.scroll_bg);
        }
    }
    draw_key_bar(s, &["Index", "", "", "", "", "", "", "", "", ""]);
}

/// Show help on `topic` (see TOPICS), over whatever is on the screen.
pub fn show(app: &mut App, topic: &str) {
    app.over(|app| run(app, topic));
}

fn run(app: &mut App, topic: &str) {
    let mut history: Vec<View> = Vec::new();
    let mut v = View { topic: topic.to_string(), top: 0, sel: None };
    let mut width = 0;
    let mut page = load(&v.topic, 10);
    let mut fresh = true;
    loop {
        let (_, (_, _, tw, rows)) = geometry(app.screen.w, app.screen.h);
        let rows = rows as usize;
        if fresh || width != tw as usize {
            width = tw as usize;
            page = load(&v.topic, width);
            if fresh && v.sel.is_none() {
                v.sel = first_visible_link(&page, v.top, rows);
            }
            fresh = false;
        }
        let max_top = page.lines.len().saturating_sub(rows);
        v.top = v.top.min(max_top);
        let (pr, vr) = (&page, &v);
        let inp = app.modal_input(&mut |s| draw(s, pr, vr), None);

        let mut open: Option<String> = None;
        let mut scrolled = false;
        match inp {
            Input::Key(k) => match k.code {
                KeyCode::Esc => return,
                KeyCode::Up => {
                    v.top = v.top.saturating_sub(1);
                    scrolled = true;
                }
                KeyCode::Down => {
                    v.top = (v.top + 1).min(max_top);
                    scrolled = true;
                }
                KeyCode::PageUp => {
                    v.top = v.top.saturating_sub(rows.saturating_sub(1).max(1));
                    scrolled = true;
                }
                KeyCode::PageDown => {
                    v.top = (v.top + rows.saturating_sub(1).max(1)).min(max_top);
                    scrolled = true;
                }
                KeyCode::Home => {
                    v.top = 0;
                    scrolled = true;
                }
                KeyCode::End => {
                    v.top = max_top;
                    scrolled = true;
                }
                KeyCode::Tab | KeyCode::BackTab if !page.links.is_empty() => {
                    let back = k.code == KeyCode::BackTab || k.modifiers.contains(KeyModifiers::SHIFT);
                    let n = page.links.len();
                    let next = match v.sel {
                        Some(s) if back => (s + n - 1) % n,
                        Some(s) => (s + 1) % n,
                        None => first_visible_link(&page, v.top, rows).unwrap_or(0),
                    };
                    v.sel = Some(next);
                    if let Some(l) = link_line(&page, next) {
                        if l < v.top {
                            v.top = l;
                        } else if l >= v.top + rows {
                            v.top = (l + 1).saturating_sub(rows).max(l.saturating_sub(rows / 2)).min(max_top);
                        }
                    }
                }
                KeyCode::Enter => open = v.sel.and_then(|s| page.links.get(s).cloned()),
                KeyCode::Backspace => {
                    if let Some(prev) = history.pop() {
                        v = prev;
                        page = load(&v.topic, width);
                    }
                }
                KeyCode::F(1) if v.topic != "index" => open = Some("index".into()),
                _ => {}
            },
            Input::Mouse(m) => {
                let ((x, y, w, h), (tx, ty, _, _)) = geometry(app.screen.w, app.screen.h);
                let (mx, my) = (m.column as i32, m.row as i32);
                match m.kind {
                    MouseEventKind::ScrollUp => {
                        v.top = v.top.saturating_sub(3);
                        scrolled = true;
                    }
                    MouseEventKind::ScrollDown => {
                        v.top = (v.top + 3).min(max_top);
                        scrolled = true;
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if mx < x || mx >= x + w || my < y || my >= y + h {
                            return;
                        }
                        let line = page.lines.get(v.top + (my - ty).max(0) as usize).filter(|_| my >= ty);
                        let link = line.and_then(|l| l.get((mx - tx).max(0) as usize).filter(|_| mx >= tx)).and_then(|c| c.2);
                        if let Some(id) = link {
                            v.sel = Some(id);
                            open = page.links.get(id).cloned();
                        }
                    }
                    _ => {}
                }
            }
            Input::Tick | Input::Paste(_) => {}
        }
        // keep the selected link on the screen while scrolling
        if scrolled && v.sel.and_then(|s| link_line(&page, s)).is_none_or(|l| l < v.top || l >= v.top + rows) {
            v.sel = first_visible_link(&page, v.top, rows);
        }
        if let Some(t) = open {
            history.push(std::mem::replace(&mut v, View { topic: t, top: 0, sel: None }));
            fresh = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(l: &[Cell]) -> String {
        l.iter().map(|c| c.0).collect()
    }

    #[test]
    fn every_link_has_a_page() {
        for (name, md) in TOPICS {
            let p = layout(md, 70);
            assert!(!p.title.is_empty(), "{name} has no # title");
            for l in &p.links {
                assert!(source(l).is_some(), "{name} links to missing page {l}");
            }
        }
    }

    #[test]
    fn inline_styles_and_links() {
        let mut links = vec![];
        let c = inline("see **F3** and [the viewer](viewer.md), `a\\b` \\*", &mut links, Sty::Text);
        assert_eq!(c.iter().map(|c| c.0).collect::<String>(), "see F3 and the viewer, a\\b *");
        assert_eq!(links, vec!["viewer".to_string()]);
        assert_eq!(c[4].1, Sty::Bold);
        assert_eq!(c[11], ('t', Sty::Link, Some(0)));
    }

    #[test]
    fn tables_wrap_the_last_column() {
        let p = layout("# T\n\n| Key | What |\n|---|---|\n| **F3** | view the file under the cursor in the built-in viewer |\n| Ctrl+\\\\ | root |", 30);
        let lines: Vec<String> = p.lines.iter().map(|l| text(l)).collect();
        assert_eq!(lines[0], "Key     What");
        assert!(lines[1].starts_with("F3      view the file"), "{lines:?}");
        assert!(lines[2].starts_with("        "), "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("Ctrl+\\  root")), "{lines:?}");
        assert!(lines.iter().all(|l| l.chars().count() <= 30));
    }

    #[test]
    fn lists_and_paragraphs_wrap() {
        let p = layout("# T\n\nfirst line\ncontinues here\n\n- an item that is long enough to wrap\n  onto the next line", 20);
        let lines: Vec<String> = p.lines.iter().map(|l| text(l)).collect();
        assert_eq!(lines[0], "first line continues");
        assert_eq!(lines[1], "here");
        assert_eq!(lines[2], "");
        assert!(lines[3].starts_with("• an item"));
        assert!(lines[4].starts_with("  "));
    }
}
