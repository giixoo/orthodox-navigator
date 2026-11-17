//! Main loop, key and mouse handling, panel layout, command line and shell integration.

use crate::archive;
use crate::calc;
use crate::dialogs::{self, LineEdit};
use crate::fileops;
use crate::game;
use crate::menu;
use crate::panel::{self, Panel, SortMode, ViewMode};
use crate::platform;
use crate::screen::Screen;
use crate::theme;
use crate::tree;
use crate::util::{fit, fit_left, human, normalize, same_path, wildcard};
use crate::viewer;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crossterm::terminal;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub const MAX_PANELS: usize = 2;
const MIN_PANEL_W: i32 = 16;

/// Commands reachable from the menu.
#[derive(Clone, Copy)]
pub enum Cmd {
    ToggleView,
    Sort(SortMode),
    Hidden,
    Reread,
    DriveActive,
    DriveOther,
    Tree,
    View,
    Edit,
    Copy,
    Move,
    Rename,
    Mkdir,
    Delete,
    Wipe,
    SelectGroup,
    UnselectGroup,
    Invert,
    Compare,
    FolderSizes,
    Swap,
    UserScreen,
    Quit,
    Help,
    About,
    NextTheme,
    Calculator,
    Game,
    NewPanel,
    ClosePanel,
    Wider,
    Narrower,
    EqualWidths,
}

/// One thing that happened while a screen was waiting.
pub enum Input {
    Key(KeyEvent),
    Mouse(MouseEvent),
    /// The timeout given to `modal_input` ran out.
    Tick,
}

/// How a panel should start: its folder, and optionally an archive and the item to focus.
struct PanelInit {
    arc: Option<(PathBuf, String)>,
    focus: Option<String>,
}

pub struct App {
    pub screen: Screen,
    pub panels: Vec<Panel>,
    fracs: Vec<f64>,
    active: usize,
    prev: usize,
    cmd: LineEdit,
    quick: Option<String>,
    quit: bool,
    // shell integration (see shell/on.ps1 and shell/on.sh)
    cwd_file: Option<PathBuf>,
    history_file: Option<PathBuf>,
    session_file: Option<PathBuf>,
    action_file: Option<PathBuf>,
    pending_run: Option<String>,
    shell: String,
    /// Last folder visited on each drive / mount point, so the drive menu returns you there.
    last_dir: HashMap<String, PathBuf>,
    last_click: Option<(Instant, usize, usize)>,
    drag: Option<usize>,
    init: Vec<PanelInit>,
    host_relaunch: bool,
}

fn state_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("orthodox-navigator").join("state.txt"))
}

fn read_kv(path: &Path) -> HashMap<String, String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim().to_string(), v.to_string())))
        .collect()
}

fn sort_name(m: SortMode) -> &'static str {
    match m {
        SortMode::Name => "name",
        SortMode::Ext => "ext",
        SortMode::Size => "size",
        SortMode::Time => "time",
        SortMode::Unsorted => "unsorted",
    }
}

fn sort_from(s: &str) -> SortMode {
    match s {
        "ext" => SortMode::Ext,
        "size" => SortMode::Size,
        "time" => SortMode::Time,
        "unsorted" => SortMode::Unsorted,
        _ => SortMode::Name,
    }
}

fn parse_fracs(s: Option<&String>, n: usize) -> Vec<f64> {
    let v: Vec<f64> = s.map(|s| s.split(',').filter_map(|f| f.trim().parse().ok()).collect()).unwrap_or_default();
    if v.len() == n && v.iter().all(|f| *f > 0.0) {
        let sum: f64 = v.iter().sum();
        v.iter().map(|f| f / sum).collect()
    } else {
        vec![1.0 / n as f64; n]
    }
}

/// The clock in the top-right corner, over the panel frame (as in DN).
pub fn draw_clock(s: &mut Screen) {
    let t = theme::current();
    let clock = format!(" {} ", chrono::Local::now().format("%H:%M:%S"));
    s.text(s.w - clock.chars().count() as i32, 0, &clock, t.clock_fg, t.clock_bg);
}

/// DN-style key bar: "F1 Help  F2 ...  F10 Menu". Clicking a slot presses that key.
pub fn draw_key_bar(s: &mut Screen, labels: &[&str; 10]) {
    let t = theme::current();
    let y = s.h - 1;
    let slot = s.w / 10;
    s.fill(0, y, s.w, 1, ' ', t.key_label, t.key_bg);
    for (i, label) in labels.iter().enumerate() {
        let x = i as i32 * slot;
        let n = format!("F{}", i + 1);
        let w = if i == 9 { s.w - x } else { slot } - n.len() as i32;
        s.text(x, y, &n, t.key_num, t.key_bg);
        s.text(x + n.len() as i32, y, &fit(&format!(" {label}"), w.max(0) as usize), t.key_label, t.key_bg);
    }
}

impl App {
    pub fn new(args: &[String]) -> Self {
        let (mut cwd_file, mut history_file, mut session_file, mut action_file) = (None, None, None, None);
        let mut shell = platform::default_shell();
        let mut i = 0;
        while i + 1 < args.len() {
            let v = PathBuf::from(&args[i + 1]);
            match args[i].as_str() {
                "--cwd-file" => cwd_file = Some(v),
                "--history-file" => history_file = Some(v),
                "--session-file" => session_file = Some(v),
                "--action-file" => action_file = Some(v),
                "--shell" => shell = args[i + 1].clone(),
                _ => {
                    i += 1;
                    continue;
                }
            }
            i += 2;
        }

        let cwd = std::env::current_dir().unwrap_or_else(|_| platform::home());
        let state = state_file().map(|f| read_kv(&f)).unwrap_or_default();
        if let Some(t) = state.get("theme") {
            theme::set(t);
        }

        // inside the `on` loop we come back after each command: restore exactly where we were
        let session = session_file.as_ref().map(|f| read_kv(f)).unwrap_or_default();
        let host_relaunch = !session.is_empty();
        let src = if host_relaunch { &session } else { &state };

        let n: usize = src.get("panels").and_then(|v| v.parse().ok()).unwrap_or(2).clamp(1, MAX_PANELS);
        let mut panels = Vec::new();
        let mut init = Vec::new();
        for i in 0..n {
            let saved = src.get(&format!("p{i}.dir")).map(PathBuf::from).filter(|p| p.is_dir());
            let dir = if host_relaunch {
                saved.unwrap_or_else(|| cwd.clone())
            } else if i == 0 {
                cwd.clone()
            } else {
                saved.unwrap_or_else(platform::home)
            };
            let mut p = Panel::new(dir);
            if let Some(v) = src.get(&format!("p{i}.sort")) {
                p.sort = sort_from(v);
            }
            p.sort_desc = src.get(&format!("p{i}.desc")).is_some_and(|v| v == "1");
            if src.get(&format!("p{i}.view")).is_some_and(|v| v == "brief") {
                p.view = ViewMode::Brief;
            }
            if src.get(&format!("p{i}.hidden")).is_some_and(|v| v == "0") {
                p.show_hidden = false;
            }
            let arc = if host_relaunch {
                src.get(&format!("p{i}.arc"))
                    .map(PathBuf::from)
                    .filter(|f| f.is_file())
                    .map(|f| (f, src.get(&format!("p{i}.prefix")).cloned().unwrap_or_default()))
            } else {
                None
            };
            let focus = if host_relaunch { src.get(&format!("p{i}.cursor")).cloned() } else { None };
            panels.push(p);
            init.push(PanelInit { arc, focus });
        }
        let fracs = parse_fracs(src.get("fracs"), n);
        let active = if host_relaunch { src.get("active").and_then(|v| v.parse().ok()).unwrap_or(0).min(n - 1) } else { 0 };
        let prev = if host_relaunch { src.get("prev").and_then(|v| v.parse().ok()).unwrap_or(0).min(n - 1) } else { 1.min(n - 1) };

        App {
            screen: Screen::new(),
            panels,
            fracs,
            active,
            prev,
            cmd: LineEdit::new(""),
            quick: None,
            quit: false,
            cwd_file,
            history_file,
            session_file,
            action_file,
            pending_run: None,
            shell,
            last_dir: HashMap::new(),
            last_click: None,
            drag: None,
            init,
            host_relaunch,
        }
    }

    // ------------------------------------------------------------- panels

    pub fn active(&mut self) -> &mut Panel {
        &mut self.panels[self.active]
    }

    fn passive_idx(&self) -> usize {
        let n = self.panels.len();
        if self.prev != self.active && self.prev < n {
            self.prev
        } else if n > 1 {
            (self.active + 1) % n
        } else {
            self.active
        }
    }

    /// The panel F5/F6 copy and move to: the one you used last.
    pub fn passive(&mut self) -> &mut Panel {
        let i = self.passive_idx();
        &mut self.panels[i]
    }

    fn set_active(&mut self, i: usize) {
        if i != self.active && i < self.panels.len() {
            self.prev = self.active;
            self.active = i;
        }
    }

    /// x and width of each panel.
    fn layout(&self) -> Vec<(i32, i32)> {
        let w = self.screen.w;
        let n = self.panels.len();
        let mut xs = Vec::with_capacity(n + 1);
        let mut acc = 0.0;
        for f in &self.fracs {
            xs.push((acc * w as f64).round() as i32);
            acc += f;
        }
        xs.push(w);
        (0..n).map(|i| (xs[i], xs[i + 1] - xs[i])).collect()
    }

    fn panel_at(&self, mx: i32) -> Option<usize> {
        self.layout().iter().position(|(x, w)| mx >= *x && mx < x + w)
    }

    fn equal_widths(&mut self) {
        let n = self.panels.len();
        self.fracs = vec![1.0 / n as f64; n];
    }

    /// Make the active panel wider (delta > 0) or narrower, taking from its neighbour.
    fn resize_active(&mut self, delta: i32) {
        let n = self.panels.len();
        if n < 2 {
            return;
        }
        let nb = if self.active + 1 < n { self.active + 1 } else { self.active - 1 };
        let w = self.screen.w as f64;
        let (d, min) = (delta as f64 / w, MIN_PANEL_W as f64 / w);
        let (a, b) = (self.fracs[self.active] + d, self.fracs[nb] - d);
        if a >= min && b >= min {
            self.fracs[self.active] = a;
            self.fracs[nb] = b;
        }
    }

    /// Mouse drag: put the border between panel j and j+1 at column x.
    fn set_border(&mut self, j: usize, x: i32) {
        let lay = self.layout();
        let left = lay[j].0;
        let right = lay[j + 1].0 + lay[j + 1].1;
        if right - left < 2 * MIN_PANEL_W {
            return;
        }
        let x = x.clamp(left + MIN_PANEL_W, right - MIN_PANEL_W);
        let w = self.screen.w as f64;
        self.fracs[j] = (x - left) as f64 / w;
        self.fracs[j + 1] = (right - x) as f64 / w;
    }

    fn new_panel(&mut self) {
        if self.panels.len() >= MAX_PANELS {
            return; // already showing both panels
        }
        let dir = self.active().dir.clone();
        let at = self.active + 1;
        let half = self.fracs[self.active] / 2.0;
        self.fracs[self.active] = half;
        self.fracs.insert(at, half);
        self.panels.insert(at, Panel::new(dir.clone()));
        if self.prev >= at {
            self.prev += 1;
        }
        if half * (self.screen.w as f64) < MIN_PANEL_W as f64 {
            self.equal_widths();
        }
        self.go(at, &dir, None);
        self.set_active(at);
    }

    fn close_panel(&mut self) {
        if self.panels.len() <= 1 {
            return;
        }
        let i = self.active;
        self.panels.remove(i);
        let f = self.fracs.remove(i);
        let nb = if i < self.panels.len() { i } else { i - 1 };
        self.fracs[nb] += f;
        self.active = nb;
        self.prev = if self.prev > i { self.prev - 1 } else if self.prev == i { nb } else { self.prev };
        self.quick = None;
    }

    fn swap_panels(&mut self) {
        let p = self.passive_idx();
        if p == self.active {
            return;
        }
        self.panels.swap(self.active, p);
        (self.active, self.prev) = (p, self.active);
    }

    // ---------------------------------------------------------- lifecycle

    pub fn run(&mut self) -> io::Result<()> {
        self.screen.init()?;
        let init = std::mem::take(&mut self.init);
        for (i, pi) in init.into_iter().enumerate() {
            let dir = self.panels[i].dir.clone();
            self.go(i, &dir, pi.focus.as_deref());
            if let Some((file, prefix)) = pi.arc {
                if self.panels[i].enter_archive(&file).is_ok() && !prefix.is_empty() {
                    self.panels[i].archive_cd(&prefix, pi.focus.as_deref());
                }
            }
        }
        // after a command in the real shell, follow a `cd` it may have done
        if self.host_relaunch {
            if let Ok(cwd) = std::env::current_dir() {
                if !same_path(&cwd, &self.panels[self.active].dir) {
                    let a = self.active;
                    self.go(a, &cwd, None);
                }
            }
        }

        while !self.quit {
            match self.modal_input(&mut |_| {}, None) {
                Input::Key(k) => self.handle(k),
                Input::Mouse(m) => self.handle_mouse(m),
                Input::Tick => {}
            }
        }

        self.save_state();
        if self.action_file.is_some() {
            self.save_session();
        }
        if let Some(f) = self.cwd_file.clone() {
            let dir = self.active().dir.to_string_lossy().to_string();
            let _ = fs::write(f, dir);
        }
        if let Some(f) = self.action_file.clone() {
            let action = match &self.pending_run {
                Some(c) => format!("run\n{c}"),
                None => "exit".to_string(),
            };
            let _ = fs::write(f, action);
        }
        Ok(())
    }

    fn save_state(&self) {
        let Some(f) = state_file() else { return };
        let mut out = format!("theme={}\npanels={}\n", theme::current().name, self.panels.len());
        out += &format!("fracs={}\n", self.fracs.iter().map(|f| format!("{f:.4}")).collect::<Vec<_>>().join(","));
        for (i, p) in self.panels.iter().enumerate() {
            out += &format!("p{i}.dir={}\np{i}.sort={}\np{i}.desc={}\n", p.dir.display(), sort_name(p.sort), p.sort_desc as u8);
            out += &format!("p{i}.view={}\np{i}.hidden={}\n", if p.view == ViewMode::Brief { "brief" } else { "full" }, p.show_hidden as u8);
        }
        let _ = fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
        let _ = fs::write(f, out);
    }

    /// Everything needed to come back exactly as we were after a command runs in the shell.
    fn save_session(&self) {
        let Some(f) = &self.session_file else { return };
        let mut out = format!("panels={}\nactive={}\nprev={}\n", self.panels.len(), self.active, self.prev);
        out += &format!("fracs={}\n", self.fracs.iter().map(|f| format!("{f:.4}")).collect::<Vec<_>>().join(","));
        for (i, p) in self.panels.iter().enumerate() {
            out += &format!("p{i}.dir={}\np{i}.sort={}\np{i}.desc={}\n", p.dir.display(), sort_name(p.sort), p.sort_desc as u8);
            out += &format!("p{i}.view={}\np{i}.hidden={}\n", if p.view == ViewMode::Brief { "brief" } else { "full" }, p.show_hidden as u8);
            if let Some(c) = p.current() {
                out += &format!("p{i}.cursor={}\n", c.name);
            }
            if let Some(av) = &p.arc {
                out += &format!("p{i}.arc={}\np{i}.prefix={}\n", av.file.display(), av.prefix.trim_end_matches('/'));
            }
        }
        let _ = fs::write(f, out);
    }

    /// Change a panel's folder and remember it as the last folder on that drive.
    fn go(&mut self, i: usize, dir: &Path, focus: Option<&str>) -> bool {
        let p = &mut self.panels[i];
        if !p.change_dir(dir, focus) {
            return false;
        }
        let d = p.dir.clone();
        self.last_dir.insert(platform::volume_of(&d), d);
        true
    }

    /// Redraw (base screen + overlay) and wait for a key or mouse event.
    /// Redraws every second for the clock and at once on resize. With a timeout, returns Tick when it runs out.
    pub fn modal_input(&mut self, overlay: &mut dyn FnMut(&mut Screen), timeout: Option<Duration>) -> Input {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            self.screen.resize();
            self.draw_base();
            overlay(&mut self.screen);
            self.screen.flush();
            let redraw_at = Instant::now() + Duration::from_secs(1);
            loop {
                let now = Instant::now();
                if deadline.is_some_and(|d| now >= d) {
                    return Input::Tick;
                }
                let until = deadline.map_or(redraw_at, |d| d.min(redraw_at));
                if now >= until {
                    break;
                }
                match event::poll(until - now) {
                    Ok(true) => match event::read() {
                        Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => return Input::Key(k),
                        Ok(Event::Mouse(m)) if m.kind != MouseEventKind::Moved => return Input::Mouse(m),
                        Ok(Event::Resize(..)) => break,
                        _ => {}
                    },
                    Ok(false) => {}
                    Err(_) => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        }
    }

    fn prompt(&self) -> String {
        let dir = self.panels[self.active].title_path();
        let dir = fit_left(&dir, (self.screen.w / 2) as usize);
        if platform::is_powershell(&self.shell) {
            format!("PS {dir}>")
        } else if cfg!(windows) {
            format!("{dir}>")
        } else {
            format!("{dir}$ ")
        }
    }

    pub fn draw_base(&mut self) {
        let t = theme::current();
        let prompt = self.prompt();
        let (w, h) = (self.screen.w, self.screen.h);
        self.screen.cursor = None;
        self.screen.fill(0, 0, w, h, ' ', t.cmd_fg, t.cmd_bg);
        let lay = self.layout();
        let q = self.quick.as_deref();
        for (i, (x, pw)) in lay.into_iter().enumerate() {
            let active = i == self.active;
            self.panels[i].draw(&mut self.screen, x, 0, pw, h - 2, active, if active { q } else { None });
        }
        draw_clock(&mut self.screen);

        let plen = prompt.chars().count() as i32;
        self.screen.text(0, h - 2, &prompt, t.cmd_fg, t.cmd_bg);
        self.cmd.draw(&mut self.screen, plen, h - 2, w - plen - 1, t.cmd_fg, t.cmd_bg);
        if self.quick.is_some() {
            self.screen.cursor = Some(self.panels[self.active].quick_pos);
        }
        draw_key_bar(&mut self.screen, &["Help", "Menu", "View", "Edit", "Copy", "Ren", "MkDir", "Del", "", "Menu"]);
    }

    // --------------------------------------------------------------- mouse

    fn handle_mouse(&mut self, m: MouseEvent) {
        let (mx, my) = (m.column as i32, m.row as i32);
        let (w, h) = (self.screen.w, self.screen.h);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.quick = None;
                // key bar: click = press that F-key
                if my == h - 1 {
                    let slot = (mx / (w / 10).max(1)).min(9) as u8;
                    return self.handle(KeyEvent::new(KeyCode::F(slot + 1), KeyModifiers::NONE));
                }
                // top row: open the menu at the title under the mouse
                if my == 0 && mx < w - 10 {
                    let start = menu::title_at(mx).unwrap_or(0);
                    if let Some(c) = menu::run(self, start) {
                        self.run_cmd(c);
                    }
                    return;
                }
                if my >= h - 2 {
                    return;
                }
                // border between two panels: start dragging it
                let lay = self.layout();
                for j in 0..lay.len().saturating_sub(1) {
                    let bx = lay[j + 1].0;
                    if mx == bx || mx == bx - 1 {
                        self.drag = Some(j);
                        return;
                    }
                }
                let Some(i) = self.panel_at(mx) else { return };
                self.set_active(i);
                if let Some(idx) = self.panels[i].hit(mx, my) {
                    let double = self.last_click.is_some_and(|(at, pi, pidx)| pi == i && pidx == idx && at.elapsed().as_millis() < 400);
                    self.panels[i].move_to(idx);
                    if double {
                        self.last_click = None;
                        if let Some(e) = self.panels[i].current().cloned() {
                            self.open(e);
                        }
                    } else {
                        self.last_click = Some((Instant::now(), i, idx));
                    }
                }
            }
            // right click selects / unselects, like DN
            MouseEventKind::Down(MouseButton::Right) => {
                let Some(i) = self.panel_at(mx) else { return };
                self.set_active(i);
                if let Some(idx) = self.panels[i].hit(mx, my) {
                    let p = &mut self.panels[i];
                    p.move_to(idx);
                    if let Some(e) = p.items.get_mut(idx) {
                        if !e.is_up {
                            e.selected = !e.selected;
                        }
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(j) = self.drag {
                    self.set_border(j, mx);
                }
            }
            MouseEventKind::Up(_) => self.drag = None,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                if my >= h - 2 {
                    return;
                }
                let Some(i) = self.panel_at(mx) else { return };
                self.set_active(i);
                let d = if m.kind == MouseEventKind::ScrollUp { -3 } else { 3 };
                self.panels[i].move_by(d);
            }
            _ => {}
        }
    }

    // ---------------------------------------------------------------- keys

    fn handle(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let cmd_empty = self.cmd.is_empty();

        // quick search: Alt+letter starts it, plain letters continue it
        if let Some(q) = self.quick.clone() {
            match k.code {
                KeyCode::Backspace => {
                    let mut q = q;
                    q.pop();
                    self.quick = Some(q);
                    return;
                }
                KeyCode::Char(c) if !ctrl => {
                    self.quick_search(&format!("{q}{c}"));
                    return;
                }
                _ => {
                    self.quick = None;
                    if k.code == KeyCode::Esc {
                        return;
                    }
                }
            }
        }

        if alt && !ctrl {
            match k.code {
                KeyCode::F(1) => return self.run_cmd(Cmd::DriveActive),
                KeyCode::F(2) => return self.run_cmd(Cmd::DriveOther),
                KeyCode::F(10) => return self.run_cmd(Cmd::Tree),
                KeyCode::Char('x') | KeyCode::Char('X') => return self.cmd_quit(),
                KeyCode::Char(c) => {
                    self.quick = Some(String::new());
                    self.quick_search(&c.to_string());
                    return;
                }
                _ => {}
            }
        }

        if ctrl && !alt {
            match k.code {
                KeyCode::F(2) => return self.run_cmd(Cmd::Calculator),
                KeyCode::F(3) => return self.run_cmd(Cmd::Sort(SortMode::Name)),
                KeyCode::F(4) => return self.run_cmd(Cmd::Sort(SortMode::Ext)),
                KeyCode::F(5) => return self.run_cmd(Cmd::Sort(SortMode::Time)),
                KeyCode::F(6) => return self.run_cmd(Cmd::Sort(SortMode::Size)),
                KeyCode::Char('r') => return self.run_cmd(Cmd::Reread),
                KeyCode::Char('h') => return self.run_cmd(Cmd::Hidden),
                KeyCode::Char('b') => return self.run_cmd(Cmd::ToggleView),
                KeyCode::Char('u') => return self.run_cmd(Cmd::Swap),
                KeyCode::Char('o') => return self.run_cmd(Cmd::UserScreen),
                KeyCode::Char('t') => return self.run_cmd(Cmd::NextTheme),
                KeyCode::Char('n') => return self.run_cmd(Cmd::NewPanel),
                KeyCode::Char('w') => return self.run_cmd(Cmd::ClosePanel),
                KeyCode::Char('e') => return self.run_cmd(Cmd::EqualWidths),
                KeyCode::Right => return self.run_cmd(Cmd::Wider),
                KeyCode::Left => return self.run_cmd(Cmd::Narrower),
                KeyCode::Char('\\') | KeyCode::Char('4') => return self.go_root(),
                KeyCode::PageUp => return self.go_parent(),
                KeyCode::PageDown => {
                    if let Some(e) = self.active().current().filter(|e| e.is_dir || archive::is_archive(&e.name)).cloned() {
                        self.open(e);
                    }
                    return;
                }
                KeyCode::Enter | KeyCode::Char('j') => {
                    if let Some(e) = self.active().current().filter(|e| !e.is_up).cloned() {
                        let q = platform::quote(&self.shell, &e.name);
                        self.cmd.insert(&format!("{q} "));
                    }
                    return;
                }
                _ => {}
            }
        }

        let n = self.panels.len();
        let p = &mut self.panels[self.active];
        let page = p.page_size() as isize;
        let step = if p.view == ViewMode::Brief { p.rows as isize } else { page };
        let handled = match k.code {
            KeyCode::Tab => {
                self.set_active((self.active + 1) % n);
                true
            }
            KeyCode::BackTab => {
                self.set_active((self.active + n - 1) % n);
                true
            }
            KeyCode::Up => {
                p.move_by(-1);
                true
            }
            KeyCode::Down => {
                p.move_by(1);
                true
            }
            KeyCode::PageUp => {
                p.move_by(-page);
                true
            }
            KeyCode::PageDown => {
                p.move_by(page);
                true
            }
            KeyCode::Home if cmd_empty => {
                p.move_to(0);
                true
            }
            KeyCode::End if cmd_empty => {
                let last = p.items.len().saturating_sub(1);
                p.move_to(last);
                true
            }
            KeyCode::Left if cmd_empty => {
                p.move_by(-step);
                true
            }
            KeyCode::Right if cmd_empty => {
                p.move_by(step);
                true
            }
            KeyCode::Enter => {
                let text = self.cmd.value();
                if !text.trim().is_empty() {
                    self.cmd.set("");
                    self.execute(&text);
                } else if let Some(e) = self.active().current().cloned() {
                    self.open(e);
                }
                true
            }
            KeyCode::Esc => {
                self.cmd.set("");
                true
            }
            KeyCode::Backspace if cmd_empty => {
                self.go_parent();
                true
            }
            KeyCode::Insert => {
                p.toggle_current();
                true
            }
            KeyCode::Char('+') if cmd_empty && !ctrl && !alt => {
                self.select_group(true);
                true
            }
            KeyCode::Char('-') if cmd_empty && !ctrl && !alt => {
                self.select_group(false);
                true
            }
            KeyCode::Char('*') if cmd_empty && !ctrl && !alt => {
                self.invert_selection();
                true
            }
            KeyCode::F(1) => {
                self.help();
                true
            }
            KeyCode::F(2) | KeyCode::F(9) | KeyCode::F(10) => {
                if let Some(c) = menu::run(self, 0) {
                    self.run_cmd(c);
                }
                true
            }
            KeyCode::F(3) => {
                self.cmd_view();
                true
            }
            KeyCode::F(4) => {
                self.cmd_edit();
                true
            }
            KeyCode::F(5) => {
                fileops::copy_or_move(self, false);
                true
            }
            KeyCode::F(6) => {
                if shift { self.cmd_rename() } else { fileops::copy_or_move(self, true) }
                true
            }
            KeyCode::F(7) => {
                self.cmd_mkdir();
                true
            }
            KeyCode::F(8) => {
                fileops::delete(self, shift);
                true
            }
            KeyCode::Delete if cmd_empty => {
                fileops::delete(self, shift);
                true
            }
            _ => false,
        };
        if !handled {
            self.cmd.handle(&k);
        }
    }

    pub fn run_cmd(&mut self, c: Cmd) {
        match c {
            Cmd::ToggleView => self.active().toggle_view(),
            Cmd::Sort(m) => self.active().set_sort(m),
            Cmd::Hidden => {
                let p = self.active();
                p.show_hidden = !p.show_hidden;
                p.load(None);
            }
            Cmd::Reread => self.active().load(None),
            Cmd::DriveActive => self.change_drive(self.active),
            Cmd::DriveOther => self.change_drive(self.passive_idx()),
            Cmd::Tree => self.cmd_tree(),
            Cmd::View => self.cmd_view(),
            Cmd::Edit => self.cmd_edit(),
            Cmd::Copy => fileops::copy_or_move(self, false),
            Cmd::Move => fileops::copy_or_move(self, true),
            Cmd::Rename => self.cmd_rename(),
            Cmd::Mkdir => self.cmd_mkdir(),
            Cmd::Delete => fileops::delete(self, false),
            Cmd::Wipe => fileops::delete(self, true),
            Cmd::SelectGroup => self.select_group(true),
            Cmd::UnselectGroup => self.select_group(false),
            Cmd::Invert => self.invert_selection(),
            Cmd::Compare => self.compare(),
            Cmd::FolderSizes => fileops::folder_sizes(self),
            Cmd::Swap => self.swap_panels(),
            Cmd::UserScreen => self.user_screen(),
            Cmd::Quit => self.cmd_quit(),
            Cmd::Help => self.help(),
            Cmd::About => dialogs::message(self, "About", "Orthodox Navigator\nprototype 0.2 (Rust)\n\nIn memory of DOS Navigator", false),
            Cmd::NextTheme => theme::next(),
            Cmd::Calculator => {
                if let Some(text) = calc::show(self) {
                    self.cmd.insert(&text);
                }
            }
            Cmd::Game => game::show(self),
            Cmd::NewPanel => self.new_panel(),
            Cmd::ClosePanel => self.close_panel(),
            Cmd::Wider => self.resize_active(2),
            Cmd::Narrower => self.resize_active(-2),
            Cmd::EqualWidths => self.equal_widths(),
        }
    }

    fn quick_search(&mut self, text: &str) {
        let p = self.active();
        let lower = text.to_lowercase();
        let pos = p
            .items
            .iter()
            .position(|e| !e.is_up && e.name.to_lowercase().starts_with(&lower))
            .or_else(|| p.items.iter().position(|e| !e.is_up && wildcard(&format!("{text}*"), &e.name)));
        if let Some(i) = pos {
            p.move_to(i);
            self.quick = Some(text.to_string());
        } else {
            let _ = io::stdout().write_all(b"\x07");
        }
    }

    // ---------------------------------------------------------- navigation

    fn open(&mut self, e: panel::Entry) {
        let i = self.active;
        if self.panels[i].arc.is_some() {
            if e.is_up {
                self.panels[i].archive_up();
            } else if e.is_dir {
                self.panels[i].archive_cd(e.arc.as_deref().unwrap_or(""), None);
            } else if let Some(tmp) = self.extract_temp(&e) {
                if let Err(err) = platform::open_default(&tmp) {
                    dialogs::message(self, "Error", &err.to_string(), true);
                }
            }
            return;
        }
        if e.is_up {
            return self.go_parent();
        }
        if e.is_dir {
            if !self.go(i, &e.path, None) {
                dialogs::message(self, "Error", &format!("Cannot open {}", e.path.display()), true);
            }
            return;
        }
        if archive::is_archive(&e.name) {
            if let Err(err) = self.panels[i].enter_archive(&e.path) {
                dialogs::message(self, "Archive", &format!("Cannot open {}:\n{err}", e.name), true);
            }
            return;
        }
        if e.exec || (cfg!(windows) && panel::is_exec_name(&e.name)) {
            let line = platform::run_file(&self.shell, &e.name);
            self.run_shell(&line);
        } else if let Err(err) = platform::open_default(&e.path) {
            dialogs::message(self, "Error", &err.to_string(), true);
        }
    }

    /// Extract a file from the archive in the active panel to a temporary folder.
    fn extract_temp(&mut self, e: &panel::Entry) -> Option<PathBuf> {
        let p = &self.panels[self.active];
        let file = p.arc.as_ref()?.file.clone();
        let ae = p.find_arc_entry(e.arc.as_deref()?)?.clone();
        match archive::extract_to_temp(&file, &ae) {
            Ok(t) => Some(t),
            Err(err) => {
                dialogs::message(self, "Archive", &err.to_string(), true);
                None
            }
        }
    }

    fn go_parent(&mut self) {
        let i = self.active;
        if self.panels[i].arc.is_some() {
            return self.panels[i].archive_up();
        }
        let dir = self.panels[i].dir.clone();
        if let Some(parent) = dir.parent() {
            let name = dir.file_name().map(|n| n.to_string_lossy().to_string());
            self.go(i, parent, name.as_deref());
        }
    }

    fn go_root(&mut self) {
        let i = self.active;
        let dir = self.panels[i].dir.clone();
        if let Some(root) = dir.ancestors().last() {
            self.go(i, root, None);
        }
    }

    pub fn change_drive(&mut self, panel: usize) {
        let mut vols = platform::volumes();
        if !cfg!(windows) {
            vols.insert(0, platform::home());
        }
        let labels: Vec<String> = vols
            .iter()
            .map(|v| {
                let name = v.to_string_lossy().trim_end_matches('\\').to_string();
                let free = platform::free_space(v).map(|f| format!("{:>8} free", human(f))).unwrap_or_else(|| "   not ready".into());
                format!("{:<24} {free}", fit_left(&name, 24))
            })
            .collect();
        let cur = platform::volume_of(&self.panels[panel].dir);
        let init = vols.iter().position(|v| platform::volume_of(v) == cur && (cfg!(windows) || v != &platform::home())).unwrap_or(0);
        let Some(i) = dialogs::list_menu(self, "Drive", &labels, init) else { return };
        let root = vols[i].clone();
        let target = if root == platform::home() {
            root.clone()
        } else {
            self.last_dir.get(&platform::volume_of(&root)).filter(|p| p.is_dir()).cloned().unwrap_or(root.clone())
        };
        if !self.go(panel, &target, None) {
            dialogs::message(self, "Error", &format!("{} is not ready.", root.display()), true);
        }
        self.set_active(panel);
    }

    // ------------------------------------------------------------ commands

    fn cmd_view(&mut self) {
        let Some(e) = self.active().current().cloned() else { return };
        if e.is_dir {
            fileops::folder_sizes(self);
        } else if self.active().arc.is_some() {
            if let Some(tmp) = self.extract_temp(&e) {
                viewer::show(self, &tmp);
            }
        } else {
            viewer::show(self, &e.path);
        }
    }

    fn cmd_edit(&mut self) {
        if self.active().arc.is_some() {
            return fileops::read_only(self);
        }
        let Some(e) = self.active().current().filter(|e| !e.is_dir).cloned() else { return };
        let file = platform::quote(&self.shell, &e.path.to_string_lossy());
        match std::env::var("EDITOR") {
            Ok(ed) if !ed.trim().is_empty() => self.run_shell(&format!("{ed} {file}")),
            _ if cfg!(windows) => {
                if let Err(err) = Command::new("notepad.exe").arg(&e.path).spawn() {
                    dialogs::message(self, "Error", &err.to_string(), true);
                }
            }
            _ => self.run_shell(&format!("vi {file}")),
        }
    }

    fn cmd_rename(&mut self) {
        if self.active().arc.is_some() {
            return fileops::read_only(self);
        }
        let Some(e) = self.active().current().filter(|e| !e.is_up).cloned() else { return };
        let Some(name) = dialogs::input(self, "Rename", "New name:", &e.name) else { return };
        let name = name.trim().to_string();
        if name.is_empty() || name == e.name {
            return;
        }
        let dst = self.active().dir.join(&name);
        match fs::rename(&e.path, &dst) {
            Ok(()) => {
                let focus = dst.file_name().map(|n| n.to_string_lossy().to_string());
                self.active().load(focus.as_deref());
                self.passive().load(None);
            }
            Err(err) => dialogs::message(self, "Error", &err.to_string(), true),
        }
    }

    fn cmd_mkdir(&mut self) {
        if self.active().arc.is_some() {
            return fileops::read_only(self);
        }
        let Some(name) = dialogs::input(self, "Make folder", "Create the folder:", "") else { return };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let full = self.active().dir.join(&name);
        match fs::create_dir_all(&full) {
            Ok(()) => {
                // focus the first new path segment, e.g. "a" for "a/b/c"
                let first = Path::new(&name).components().next().map(|c| c.as_os_str().to_string_lossy().to_string());
                self.active().load(first.as_deref());
                self.passive().load(None);
            }
            Err(err) => dialogs::message(self, "Error", &err.to_string(), true),
        }
    }

    fn cmd_tree(&mut self) {
        let start = self.active().dir.clone();
        if let Some(dir) = tree::show(self, &start) {
            let i = self.active;
            self.go(i, &dir, None);
        }
    }

    fn select_group(&mut self, select: bool) {
        let title = if select { "Select" } else { "Unselect" };
        let Some(mask) = dialogs::input(self, title, "Files matching:", "*.*") else { return };
        let patterns: Vec<String> = mask
            .split([';', ',', ' '])
            .filter(|m| !m.is_empty())
            .map(|m| if m == "*.*" { "*".to_string() } else { m.to_string() })
            .collect();
        for e in self.active().items.iter_mut().filter(|e| !e.is_dir) {
            if patterns.iter().any(|m| wildcard(m, &e.name)) {
                e.selected = select;
            }
        }
    }

    fn invert_selection(&mut self) {
        for e in self.active().items.iter_mut().filter(|e| !e.is_dir) {
            e.selected = !e.selected;
        }
    }

    /// Select files that are missing from the other panel or differ in size/time.
    fn compare(&mut self) {
        type Info = (Option<u64>, Option<chrono::DateTime<chrono::Local>>);
        fn files(p: &Panel) -> HashMap<String, Info> {
            p.items.iter().filter(|e| !e.is_dir).map(|e| (e.name.to_lowercase(), (e.size, e.modified))).collect()
        }
        let (a, b) = (self.active, self.passive_idx());
        if a == b {
            return;
        }
        let (fa, fb) = (files(&self.panels[a]), files(&self.panels[b]));
        let mut n = 0;
        for (i, other) in [(a, &fb), (b, &fa)] {
            for e in self.panels[i].items.iter_mut().filter(|e| !e.is_dir) {
                e.selected = other.get(&e.name.to_lowercase()) != Some(&(e.size, e.modified));
                if e.selected {
                    n += 1;
                }
            }
        }
        let text = if n == 0 { "The panels have the same files.".to_string() } else { format!("{n} files differ (selected).") };
        dialogs::message(self, "Compare directories", &text, false);
    }

    fn user_screen(&mut self) {
        self.screen.suspend();
        let _ = terminal::enable_raw_mode();
        loop {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind != KeyEventKind::Release {
                    break;
                }
            }
        }
        let _ = terminal::disable_raw_mode();
        self.screen.resume();
    }

    fn cmd_quit(&mut self) {
        if dialogs::choice(self, "Quit", &["Do you want to quit Orthodox Navigator?"], &["Yes", "No"], false) == Some(0) {
            self.quit = true;
        }
    }

    fn help(&mut self) {
        let lines = [
            "Tab / Shift+Tab  next / previous panel   Ctrl+U swap with other panel",
            "Ctrl+W hide a panel  Ctrl+N show it again  Ctrl+←/→ resize  Ctrl+E equal",
            "Enter open / run / browse archive        Bksp parent folder",
            "Insert select   + - * select / unselect / invert    Alt+a-z quick search",
            "F3 view (folder: size)  F4 edit  F5 copy (archive: extract)  F6 move",
            "Shift+F6 rename  F7 mkdir  F8/Del to Trash  Shift+F8 delete permanently",
            "Alt+F1/F2 drive (this / other panel)   Alt+F10 directory tree",
            "Ctrl+F2 calculator   Ctrl+F3..F6 sort name/ext/time/size",
            "Ctrl+B brief/full  Ctrl+H hidden  Ctrl+R re-read  Ctrl+T colors",
            "Ctrl+O user screen  Ctrl+Enter name to command line  F9/F10 menu  Alt+X quit",
            "",
            "Mouse: click selects, double-click opens, right-click marks, wheel scrolls,",
            "drag a border between panels to resize, click the top row for the menu.",
        ];
        dialogs::choice(self, "Help", &lines, &["OK"], false);
    }

    // -------------------------------------------------------- command line

    /// Remember a command so the wrapper can add it to the shell's history.
    fn add_history(&self, command: &str) {
        let Some(f) = &self.history_file else { return };
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(f) {
            let _ = writeln!(file, "{}", command.replace(['\r', '\n'], " "));
        }
    }

    fn execute(&mut self, text: &str) {
        let t = text.trim();
        self.add_history(t);
        let i = self.active;
        let lower = t.to_lowercase();

        if lower == "exit" {
            self.quit = true;
            return;
        }
        if t == "~" || lower == "cd ~" || (!cfg!(windows) && lower == "cd") {
            self.go(i, &platform::home(), None);
            return;
        }
        // "d:" switches drive (Windows)
        if cfg!(windows) && t.len() == 2 && t.ends_with(':') && t.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            let root = format!("{}:\\", t[..1].to_uppercase());
            let target = self.last_dir.get(&root[..2]).cloned().unwrap_or_else(|| PathBuf::from(&root));
            if !self.go(i, &target, None) {
                dialogs::message(self, "Error", &format!("Drive {root} is not ready."), true);
            }
            return;
        }
        // "cd x", "cd ..", "cd\x", "cd /d d:\x"
        if lower.starts_with("cd ") || lower.starts_with("cd\\") || lower.starts_with("cd..") || lower.starts_with("cd/") {
            let mut arg = t[2..].trim();
            if arg.to_lowercase().starts_with("/d ") {
                arg = arg[3..].trim();
            }
            let arg = arg.trim_matches(|c| c == '"' || c == '\'');
            if arg.is_empty() {
                return;
            }
            let arg = if let Some(rest) = arg.strip_prefix("~/").or_else(|| arg.strip_prefix("~\\")) {
                platform::home().join(rest)
            } else {
                PathBuf::from(arg)
            };
            let target = normalize(&self.active().dir.join(arg));
            if !self.go(i, &target, None) {
                dialogs::message(self, "Error", &format!("The folder does not exist:\n{}", target.display()), true);
            }
            return;
        }
        self.run_shell(t);
    }

    /// Run a command line.
    /// Started from the `on` wrapper: hand it to the real shell session (we exit; the wrapper runs it
    /// and starts us again). Started directly: run it in a child shell; output stays on the user screen.
    fn run_shell(&mut self, line: &str) {
        if self.action_file.is_some() {
            self.pending_run = Some(line.to_string());
            self.quit = true;
            return;
        }
        let prompt = self.prompt();
        let dir = self.active().dir.clone();
        self.screen.suspend();
        println!();
        println!("{prompt} {line}");
        let mut c = platform::shell_command(&self.shell, line);
        c.current_dir(&dir);
        if let Err(e) = c.status() {
            println!("{e}");
        }
        self.screen.resume();
        for p in self.panels.iter_mut() {
            p.load(None);
        }
    }
}
