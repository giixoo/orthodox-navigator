//! VS Code colour themes: find the ones installed (built in and from extensions), read the
//! one VS Code is using, and turn a theme into ON's colours. Ctrl+T picks a theme with a live preview.

use crate::app::{draw_key_bar, App, Input};
use crate::dialogs;
use crate::screen::C;
use crate::theme::{self, Theme};
use crate::util::fit;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// The theme that follows whatever VS Code is using.
pub const FOLLOW: &str = "VS Code (current theme)";
const PREFIX: &str = "VS Code: ";

#[derive(Clone)]
pub struct ThemeRef {
    pub label: String,
    pub id: String,
    pub path: PathBuf,
}

/// JSON with comments and trailing commas (VS Code's JSONC) → plain JSON.
pub fn jsonc_to_json(text: &str) -> String {
    let c: Vec<char> = text.trim_start_matches('\u{feff}').chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '"' => {
                out.push('"');
                i += 1;
                while i < c.len() && c[i] != '"' {
                    if c[i] == '\\' && i + 1 < c.len() {
                        out.push(c[i]);
                        i += 1;
                    }
                    out.push(c[i]);
                    i += 1;
                }
                out.push('"');
            }
            '/' if c.get(i + 1) == Some(&'/') => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if c.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                    i += 1;
                }
                i += 1;
            }
            ',' => {
                // drop a comma that only has whitespace / comments before } or ]
                let mut j = i + 1;
                while j < c.len() && c[j].is_whitespace() {
                    j += 1;
                }
                if !matches!(c.get(j), Some('}') | Some(']')) {
                    out.push(',');
                }
            }
            ch => out.push(ch),
        }
        i += 1;
    }
    out
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&jsonc_to_json(&fs::read_to_string(path).ok()?)).ok()
}

/// Folders that hold VS Code extensions (built-in and installed), on this machine.
fn extension_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(home) = dirs::home_dir() {
        for d in [".vscode", ".vscode-insiders", ".vscode-oss", ".cursor"] {
            v.push(home.join(d).join("extensions"));
        }
    }
    let mut installs: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        if let Some(local) = dirs::data_local_dir() {
            installs.push(local.join("Programs").join("Microsoft VS Code"));
            installs.push(local.join("Programs").join("Microsoft VS Code Insiders"));
            installs.push(local.join("Programs").join("cursor"));
        }
        for pf in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(p) = std::env::var(pf) {
                installs.push(PathBuf::from(p).join("Microsoft VS Code"));
            }
        }
        // wherever `code` is on PATH: <install>\bin\code.cmd
        if let Ok(path) = std::env::var("PATH") {
            for d in std::env::split_paths(&path) {
                if d.join("code.cmd").exists() {
                    if let Some(p) = d.parent() {
                        installs.push(p.to_path_buf());
                    }
                }
            }
        }
        // newer installs keep resources under a version-hash folder
        for i in installs.clone() {
            if let Ok(rd) = fs::read_dir(&i) {
                installs.extend(rd.flatten().map(|e| e.path()).filter(|p| p.join("resources").is_dir()));
            }
        }
        v.extend(installs.iter().map(|i| i.join("resources").join("app").join("extensions")));
    } else {
        for d in [
            "/usr/share/code/resources/app/extensions",
            "/usr/lib/code/extensions",
            "/opt/visual-studio-code/resources/app/extensions",
            "/snap/code/current/usr/share/code/resources/app/extensions",
            "/usr/share/codium/resources/app/extensions",
            "/Applications/Visual Studio Code.app/Contents/Resources/app/extensions",
        ] {
            v.push(PathBuf::from(d));
        }
    }
    v.into_iter().filter(|d| d.is_dir()).collect()
}

/// Labels like "%colorTheme.label%" are looked up in package.nls.json.
fn localize(label: &str, ext_dir: &Path) -> String {
    let Some(key) = label.strip_prefix('%').and_then(|l| l.strip_suffix('%')) else { return label.to_string() };
    read_json(&ext_dir.join("package.nls.json"))
        .and_then(|n| n.get(key).and_then(|v| v.as_str().map(str::to_string).or_else(|| v.get("message")?.as_str().map(str::to_string))))
        .unwrap_or_else(|| label.to_string())
}

/// Every colour theme VS Code has here, sorted by name.
pub fn discover() -> Vec<ThemeRef> {
    let mut out: Vec<ThemeRef> = Vec::new();
    for dir in extension_dirs() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        // folders starting with '.' are removed or half-installed extensions
        for ext in rd.flatten().map(|e| e.path()).filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))) {
            let Some(pkg) = read_json(&ext.join("package.json")) else { continue };
            let Some(themes) = pkg.pointer("/contributes/themes").and_then(|t| t.as_array()) else { continue };
            for t in themes {
                let (Some(label), Some(path)) = (t.get("label").and_then(|v| v.as_str()), t.get("path").and_then(|v| v.as_str())) else { continue };
                let label = localize(label, &ext);
                let id = t.get("id").and_then(|v| v.as_str()).unwrap_or(&label).to_string();
                if !out.iter().any(|o| o.label == label) {
                    out.push(ThemeRef { label, id, path: ext.join(path) });
                }
            }
        }
    }
    out.sort_by_key(|t| t.label.to_lowercase());
    out
}

/// The theme VS Code is set to (settings.json "workbench.colorTheme"), or its default.
pub fn current_vscode_theme() -> String {
    let mut files = Vec::new();
    if let Some(cfg) = dirs::config_dir() {
        for app in ["Code", "Code - Insiders", "VSCodium", "Cursor"] {
            files.push(cfg.join(app).join("User").join("settings.json"));
        }
    }
    for f in files {
        if let Some(v) = read_json(&f) {
            if let Some(t) = v.get("workbench.colorTheme").and_then(|t| t.as_str()) {
                return t.to_string();
            }
            return "Dark Modern".into(); // settings exist without a theme: VS Code's default
        }
    }
    "Dark Modern".into()
}

/// A theme file with its "include" chain merged (included first, then this file's own entries).
fn load_theme_json(path: &Path, depth: usize) -> (serde_json::Map<String, Value>, Vec<Value>) {
    let mut colors = serde_json::Map::new();
    let mut tokens = Vec::new();
    let Some(v) = read_json(path) else { return (colors, tokens) };
    if depth < 5 {
        if let Some(inc) = v.get("include").and_then(|i| i.as_str()) {
            if let Some(dir) = path.parent() {
                let (c, t) = load_theme_json(&dir.join(inc), depth + 1);
                colors = c;
                tokens = t;
            }
        }
    }
    if let Some(c) = v.get("colors").and_then(|c| c.as_object()) {
        for (k, val) in c {
            colors.insert(k.clone(), val.clone());
        }
    }
    if let Some(t) = v.get("tokenColors").and_then(|t| t.as_array()) {
        tokens.extend(t.iter().cloned());
    }
    (colors, tokens)
}

/// "#rgb", "#rgba", "#rrggbb", "#rrggbbaa" → colour, blended over `base` when it has alpha.
fn parse_color(s: &str, base: (u8, u8, u8)) -> Option<C> {
    let h = s.strip_prefix('#')?;
    let digits: Vec<u8> = match h.len() {
        3 | 4 => h.chars().map(|c| c.to_digit(16).map(|d| (d * 17) as u8)).collect::<Option<_>>()?,
        6 | 8 => (0..h.len() / 2).map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()).collect::<Option<_>>()?,
        _ => return None,
    };
    let a = digits.get(3).copied().unwrap_or(255) as u32;
    let mix = |c: u8, b: u8| ((c as u32 * a + b as u32 * (255 - a)) / 255) as u8;
    Some(C::Rgb(mix(digits[0], base.0), mix(digits[1], base.1), mix(digits[2], base.2)))
}

/// Difference in lightness of two colours, 0..1 (enough to read text: about 0.35 or more).
fn contrast(a: C, b: C) -> f32 {
    let l = |c: C| {
        let (r, g, b) = c.rgb();
        (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0
    };
    (l(a) - l(b)).abs()
}

fn blend(a: C, b: C, t: f32) -> C {
    let (a, b) = (a.rgb(), b.rgb());
    let m = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t) as u8;
    C::Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// The foreground a theme gives to a TextMate scope (most specific selector wins).
fn token_color(tokens: &[Value], scope: &str, base: (u8, u8, u8)) -> Option<C> {
    let mut best: Option<(usize, C)> = None;
    for t in tokens {
        let Some(fg) = t.pointer("/settings/foreground").and_then(|f| f.as_str()) else { continue };
        let selectors: Vec<String> = match t.get("scope") {
            Some(Value::String(s)) => s.split(',').map(|x| x.trim().to_string()).collect(),
            Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).collect(),
            _ => continue,
        };
        for sel in selectors {
            if sel.contains(' ') {
                continue; // descendant selectors: too specific for us
            }
            if scope == sel || scope.starts_with(&format!("{sel}.")) {
                if let Some(c) = parse_color(fg, base) {
                    if best.is_none_or(|(len, _)| sel.len() >= len) {
                        best = Some((sel.len(), c));
                    }
                }
            }
        }
    }
    best.map(|(_, c)| c)
}

/// Build an ON theme from a VS Code theme file.
pub fn load(r: &ThemeRef, name: &str) -> Option<Theme> {
    let (colors, tokens) = load_theme_json(&r.path, 0);
    if colors.is_empty() && tokens.is_empty() {
        return None;
    }
    let dark = !r.path.to_string_lossy().to_lowercase().contains("light");
    let ed_bg0 = colors.get("editor.background").and_then(|v| v.as_str()).and_then(|s| parse_color(s, (0, 0, 0)));
    let ed_bg = ed_bg0.unwrap_or(if dark { C::Rgb(30, 30, 30) } else { C::Rgb(255, 255, 255) });
    let base = ed_bg.rgb();
    let col = |keys: &[&str]| keys.iter().find_map(|k| colors.get(*k).and_then(|v| v.as_str()).and_then(|s| parse_color(s, base)));
    let tok = |scopes: &[&str]| scopes.iter().find_map(|s| token_color(&tokens, s, base));

    let fg = col(&["editor.foreground", "foreground"]).unwrap_or(if dark { C::Rgb(212, 212, 212) } else { C::Rgb(30, 30, 30) });
    let panel_bg = col(&["sideBar.background"]).unwrap_or(ed_bg);
    let file = col(&["sideBar.foreground", "foreground"]).unwrap_or(fg);
    let dim = col(&["descriptionForeground"]).unwrap_or(blend(fg, panel_bg, 0.4));
    let frame = col(&["sideBarSectionHeader.border", "panel.border", "editorGroup.border", "contrastBorder"]).unwrap_or(blend(fg, panel_bg, 0.6));
    let accent = col(&["textLink.foreground", "focusBorder", "button.background"]).unwrap_or(fg);
    let keyword = tok(&["keyword.control", "keyword", "storage"]).unwrap_or(accent);
    let string = tok(&["string"]).unwrap_or(fg);
    let ty = tok(&["entity.name.type", "support.type", "storage.type"]).unwrap_or(keyword);
    let func = tok(&["entity.name.function", "support.function"]).unwrap_or(ty);
    let number = tok(&["constant.numeric", "constant"]).unwrap_or(string);
    let comment = tok(&["comment"]).unwrap_or(dim);
    let warn = col(&["editorWarning.foreground", "gitDecoration.modifiedResourceForeground"]).unwrap_or(C::Rgb(220, 180, 60));
    let error = col(&["errorForeground", "editorError.foreground"]).unwrap_or(C::Rgb(240, 80, 80));
    let sel_bg = col(&["list.activeSelectionBackground", "editor.selectionBackground"]).unwrap_or(blend(accent, panel_bg, 0.5));
    let sel_fg = col(&["list.activeSelectionForeground"]).unwrap_or(fg);
    let status_bg = col(&["statusBar.background"]).unwrap_or(panel_bg);
    let status_fg = col(&["statusBar.foreground"]).unwrap_or(fg);
    let menu_bg = col(&["menu.background", "editorWidget.background", "dropdown.background"]).unwrap_or(panel_bg);
    let menu_fg = col(&["menu.foreground", "dropdown.foreground"]).unwrap_or(fg);
    let menu_sel_bg = col(&["menu.selectionBackground", "list.activeSelectionBackground"]).unwrap_or(sel_bg);
    let menu_sel_fg = col(&["menu.selectionForeground", "list.activeSelectionForeground"]).unwrap_or(sel_fg);
    let widget_bg = col(&["editorWidget.background", "quickInput.background"]).unwrap_or(menu_bg);
    let widget_fg = col(&["editorWidget.foreground", "foreground"]).unwrap_or(fg);
    let title_bg = col(&["titleBar.activeBackground", "editorGroupHeader.tabsBackground"]).unwrap_or(status_bg);
    let title_fg = col(&["titleBar.activeForeground", "tab.activeForeground"]).unwrap_or(fg);
    // the info lines (file under the cursor, git branch, free space) stand out, like DN's cyan
    let info = [accent, ty, fg].into_iter().find(|c| contrast(*c, panel_bg) >= 0.35).unwrap_or(fg);

    Some(Theme {
        name: Box::leak(name.to_string().into_boxed_str()),
        double_frame: false,
        panel_bg,
        frame,
        col_line: frame,
        header: accent,
        file,
        dir: ty,
        up: ty,
        exec: func,
        archive: number,
        doc: string,
        image: keyword,
        hidden: dim,
        junk: error,
        selected: warn,
        cursor_fg: sel_fg,
        cursor_bg: sel_bg,
        info,
        status: warn,
        free_num: fg,
        free_label: dim,
        error,
        title_fg: file,
        title_bg: panel_bg,
        active_title_fg: sel_fg,
        active_title_bg: sel_bg,
        scroll_fg: col(&["scrollbarSlider.activeBackground", "scrollbarSlider.background"]).unwrap_or(frame),
        scroll_bg: panel_bg,
        drive_fg: file,
        drive_cur: warn,
        menu_fg,
        menu_bg,
        menu_hot: accent,
        menu_sel_fg,
        menu_sel_bg,
        clock_fg: title_fg,
        clock_bg: title_bg,
        bar_fg: title_fg,
        bar_bg: title_bg,
        key_num: accent,
        key_label: status_fg,
        key_bg: status_bg,
        cmd_fg: col(&["terminal.foreground"]).unwrap_or(fg),
        cmd_bg: col(&["terminal.background", "panel.background"]).unwrap_or(ed_bg),
        dlg_fg: widget_fg,
        dlg_bg: widget_bg,
        dlg_frame: col(&["editorWidget.border", "widget.border", "focusBorder"]).unwrap_or(frame),
        dlg_title: widget_fg,
        dlg_hot: accent,
        dlg_close: string,
        input_fg: col(&["input.foreground"]).unwrap_or(fg),
        input_bg: col(&["input.background"]).unwrap_or(ed_bg),
        btn_fg: col(&["button.foreground"]).unwrap_or(fg),
        btn_bg: col(&["button.background"]).unwrap_or(sel_bg),
        btn_hot: col(&["button.foreground"]).unwrap_or(fg),
        btn_arrow: col(&["button.foreground"]).unwrap_or(fg),
        list_sel_fg: sel_fg,
        list_sel_bg: sel_bg,
        viewer_fg: fg,
        viewer_bg: ed_bg,
        syn_keyword: keyword,
        syn_type: ty,
        syn_string: string,
        syn_comment: comment,
        syn_number: number,
        syn_meta: func,
    })
}

/// Load and switch to a theme by its saved name ("VS Code: Dark Modern" or the follow-VS-Code entry).
/// Returns false if it can't be found.
pub fn apply(name: &str) -> bool {
    if theme::set(name) && name != FOLLOW {
        return true;
    }
    apply_from(&discover(), name)
}

/// Like `apply`, with the themes already found.
fn apply_from(all: &[ThemeRef], name: &str) -> bool {
    if theme::set(name) && name != FOLLOW {
        return true;
    }
    let wanted = if name == FOLLOW { current_vscode_theme() } else { name.strip_prefix(PREFIX).unwrap_or(name).to_string() };
    let Some(r) = all.iter().find(|t| t.label.eq_ignore_ascii_case(&wanted) || t.id.eq_ignore_ascii_case(&wanted)) else { return false };
    match load(r, name) {
        Some(t) => {
            theme::install(t);
            true
        }
        None => false,
    }
}

/// Ctrl+T: choose a colour theme; moving the cursor previews it, Enter keeps it, Esc goes back.
pub fn pick(app: &mut App) {
    let before = theme::current().name;
    let vs = discover();
    let mut names: Vec<String> = theme::built_in_names().into_iter().map(str::to_string).collect();
    if !vs.is_empty() {
        names.push(FOLLOW.to_string());
        names.extend(vs.iter().map(|t| format!("{PREFIX}{}", t.label)));
    }
    let mut sel = names.iter().position(|n| n == before).unwrap_or(0);
    let mut top = 0usize;
    let mut shown = String::new();
    loop {
        if names[sel] != shown {
            shown = names[sel].clone();
            if !apply_from(&vs, &shown) {
                theme::set(before);
            }
        }
        let (sw, sh) = (app.screen.w, app.screen.h);
        let w = 56.min(sw - 4);
        let visible = (names.len() as i32).min(sh - 8).max(3) as usize;
        let h = visible as i32 + 4;
        let (x, y) = ((sw - w) / 2, ((sh - 1 - h) / 2).max(0));
        if sel < top {
            top = sel;
        }
        if sel >= top + visible {
            top = sel + 1 - visible;
        }
        let (names_ref, cur, cur_top) = (&names, sel, top);
        let note = if vs.is_empty() { "No VS Code themes found on this computer." } else { "" };
        let inp = app.window_input(
            &mut |s| {
                let t = theme::current();
                dialogs::frame_at(s, x, y, w, h, "Colour theme");
                for r in 0..visible {
                    let i = cur_top + r;
                    let Some(n) = names_ref.get(i) else { break };
                    let (fg, bg) = if i == cur { (t.list_sel_fg, t.list_sel_bg) } else { (t.dlg_fg, t.dlg_bg) };
                    s.text(x + 3, y + 2 + r as i32, &fit(&format!(" {n}"), (w - 6) as usize), fg, bg);
                }
                if !note.is_empty() {
                    s.text(x + 3, y + h - 2, &fit(note, (w - 6) as usize), t.dlg_fg, t.dlg_bg);
                }
                draw_key_bar(s, &["Help", "", "", "", "", "", "", "", "", ""]);
            },
            None,
        );
        match inp {
            Input::Key(k) => match k.code {
                KeyCode::Esc => {
                    theme::set(before);
                    return;
                }
                KeyCode::Enter => return,
                KeyCode::Up => sel = sel.saturating_sub(1),
                KeyCode::Down => sel = (sel + 1).min(names.len() - 1),
                KeyCode::PageUp => sel = sel.saturating_sub(visible),
                KeyCode::PageDown => sel = (sel + visible).min(names.len() - 1),
                KeyCode::Home => sel = 0,
                KeyCode::End => sel = names.len() - 1,
                KeyCode::F(1) => crate::help::show(app, "themes"),
                _ => {}
            },
            Input::Mouse(m) => match m.kind {
                MouseEventKind::ScrollUp => sel = sel.saturating_sub(1),
                MouseEventKind::ScrollDown => sel = (sel + 1).min(names.len() - 1),
                MouseEventKind::Down(MouseButton::Left) => {
                    let (mx, my) = (m.column as i32, m.row as i32);
                    if dialogs::is_close_click(x, y, mx, my) || mx < x || mx >= x + w || my < y || my >= y + h {
                        theme::set(before);
                        return;
                    }
                    let r = my - y - 2;
                    if r >= 0 && (r as usize) < visible && top + (r as usize) < names.len() {
                        let i = top + r as usize;
                        if i == sel {
                            return; // second click on the same one keeps it
                        }
                        sel = i;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc() {
        let j = jsonc_to_json("{\n // c\n \"a\": \"x//y\", /* b */ \"c\": [1, 2,],\n}");
        let v: Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["a"], "x//y");
        assert_eq!(v["c"][1], 2);
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("#fff", (0, 0, 0)), Some(C::Rgb(255, 255, 255)));
        assert_eq!(parse_color("#10203040", (0, 0, 0)), Some(C::Rgb(4, 8, 12)));
        let tokens: Vec<Value> = serde_json::from_str(r##"[
            {"scope": "keyword", "settings": {"foreground": "#ff0000"}},
            {"scope": ["keyword.control", "x"], "settings": {"foreground": "#00ff00"}},
            {"scope": "string, comment", "settings": {"foreground": "#0000ff"}}
        ]"##).unwrap();
        assert_eq!(token_color(&tokens, "keyword.control.rust", (0, 0, 0)), Some(C::Rgb(0, 255, 0)));
        assert_eq!(token_color(&tokens, "keyword.other", (0, 0, 0)), Some(C::Rgb(255, 0, 0)));
        assert_eq!(token_color(&tokens, "comment.line", (0, 0, 0)), Some(C::Rgb(0, 0, 255)));
        assert_eq!(token_color(&tokens, "variable", (0, 0, 0)), None);
    }

    #[test]
    #[ignore]
    fn discover_here() {
        let t0 = std::time::Instant::now();
        let all = discover();
        println!("discover took {:?}", t0.elapsed());
        println!("{} themes; current: {}", all.len(), current_vscode_theme());
        for t in all.iter().take(60) {
            println!("  {} | {}", t.label, t.path.display());
        }
        let cur = current_vscode_theme();
        let r = all.iter().find(|t| t.label == cur || t.id == cur).expect("current theme found");
        let th = load(r, "x").expect("loads");
        println!("panel {:?} file {:?} cursor {:?}/{:?} kw {:?} str {:?} status {:?}", th.panel_bg, th.file, th.cursor_fg, th.cursor_bg, th.syn_keyword, th.syn_string, th.key_bg);
    }
}