//! Main loop, key and mouse handling, panel layout, command line and shell integration.

use crate::archive;
use crate::calc;
use crate::dialogs::{self, LineEdit};
use crate::diff;
use crate::editor;
use crate::favorites;
use crate::fileops;
use crate::find;
use crate::game;
use crate::help;
use crate::sysmon;
use crate::git;
use crate::menu;
use crate::panel::{self, Panel, SortMode, ViewMode};
use crate::platform;
use crate::preview::Preview;
use crate::screen::{Screen, Snapshot};
use crate::theme;
use crate::tools;
use crate::tree;
use crate::util::{fit, fit_left, normalize, same_path, wildcard};
use crate::viewer;
use crate::vscode;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crossterm::terminal;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
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
    DriveLeft,
    DriveRight,
    Tree,
    View,
    Edit,
    NewFile,
    AddFavorite,
    Copy,
    Move,
    Rename,
    BatchRename,
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
    SysMon,
    TogglePanel,
    Wider,
    Narrower,
    EqualWidths,
    Preview,
    Tools,
    CompareFiles,
    GitDiff,
    Find,
    UserMenu,
    FolderHistory,
    TempPanel,
    AddToTemp,
    Filter,
    Attributes,
    EditColors,
    EditExtensions,
    EditGlobalMenu,
    RemoteConnect,
    MasterPassword,
    ForgetPasswords,
}

/// One thing that happened while a screen was waiting.
pub enum Input {
    Key(KeyEvent),
    Mouse(MouseEvent),
    /// Text pasted into the terminal (bracketed paste; not on Windows, where it arrives as keys).
    Paste(String),
    /// The timeout given to `modal_input` ran out.
    Tick,
}

/// How a panel should start: its folder, and optionally an archive and the item to focus.
struct PanelInit {
    arc: Option<(PathBuf, String)>,
    focus: Option<String>,
    /// it was showing the Temp panel
    temp: bool,
    /// it was showing a server: (profile name, folder there)
    remote: Option<(String, String)>,
}

pub struct App {
    pub screen: Screen,
    pub panels: Vec<Panel>,
    fracs: Vec<f64>,
    active: usize,
    prev: usize,
    cmd: LineEdit,
    /// The panel Ctrl+P hid: (where it was, the panel, its width share)
    hidden_panel: Option<(usize, Panel, f64)>,
    hist: crate::history::History,
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
    /// Ctrl+L: the other panel shows a preview of the item under the cursor.
    preview: Option<Preview>,
    /// screens that dialogs are drawn over (the window that opened them), innermost last
    backdrops: Vec<Option<Snapshot>>,
    /// the last frame drawn at each dialog depth (0 = no dialog open)
    /// (None: the panels or a menu, which dialogs redraw live)
    frames: Vec<Option<Snapshot>>,
    /// set by window_input: the next frame is a window that dialogs should be drawn over
    window_frame: bool,
    /// a command was typed on the user screen (Ctrl+O): come back to it after the shell runs it
    resume_user: bool,
    last_find: find::Query,
    /// folders visited, newest first (Alt+Backspace)
    dir_hist: Vec<PathBuf>,
    /// the Temp panel's list
    temp_list: Vec<PathBuf>,
    /// remote connections (remote.txt)
    pub remote_profiles: crate::remote::profiles::Profiles,
    /// the key of the saved passwords, once the master password was given
    pub vault_key: Option<crate::remote::vault::Key>,
    /// passwords typed this session, by profile name (never written anywhere)
    pub typed_passwords: HashMap<String, String>,
    /// password questions so far in the current remote operation, by profile name
    pub prompt_count: HashMap<String, u32>,
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

/// Modifier keys held right now, as last seen by `modal_input` (Windows only; elsewhere always none).
static MODS: AtomicU8 = AtomicU8::new(0);

pub fn held_modifiers() -> KeyModifiers {
    KeyModifiers::from_bits_truncate(MODS.load(Ordering::Relaxed))
}

/// The panels' shortcuts besides the F-keys, most useful first (the key bar shows as many as fit).
/// Without a modifier it lists Ctrl shortcuts too, so they can be found (and clicked).
const PANEL_EXTRAS: [Extra; 28] = {
    const N: KeyModifiers = KeyModifiers::NONE;
    const C: KeyModifiers = KeyModifiers::CONTROL;
    const A: KeyModifiers = KeyModifiers::ALT;
    [
        Extra { mods: N, key: "^L", label: "Preview", code: KeyCode::Char('l') },
        Extra { mods: N, key: "^P", label: "Panels", code: KeyCode::Char('p') },
        Extra { mods: N, key: "^O", label: "Screen", code: KeyCode::Char('o') },
        Extra { mods: N, key: "^E", label: "History", code: KeyCode::Char('e') },
        Extra { mods: N, key: "^B", label: "View", code: KeyCode::Char('b') },
        Extra { mods: N, key: "^R", label: "Reread", code: KeyCode::Char('r') },
        Extra { mods: N, key: "^T", label: "Theme", code: KeyCode::Char('t') },
        Extra { mods: N, key: "^G", label: "Game", code: KeyCode::Char('g') },
        Extra { mods: C, key: "L", label: "Preview", code: KeyCode::Char('l') },
        Extra { mods: C, key: "P", label: "Panels", code: KeyCode::Char('p') },
        Extra { mods: C, key: "O", label: "Screen", code: KeyCode::Char('o') },
        Extra { mods: C, key: "E", label: "History", code: KeyCode::Char('e') },
        Extra { mods: C, key: "B", label: "View", code: KeyCode::Char('b') },
        Extra { mods: C, key: "R", label: "Reread", code: KeyCode::Char('r') },
        Extra { mods: C, key: "H", label: "Hidden", code: KeyCode::Char('h') },
        Extra { mods: C, key: "U", label: "Swap", code: KeyCode::Char('u') },
        Extra { mods: C, key: "T", label: "Theme", code: KeyCode::Char('t') },
        Extra { mods: C, key: "D", label: "Fav", code: KeyCode::Char('d') },
        Extra { mods: C, key: "F", label: "Filter", code: KeyCode::Char('f') },
        Extra { mods: C, key: "A", label: "Attr", code: KeyCode::Char('a') },
        Extra { mods: C, key: "G", label: "Game", code: KeyCode::Char('g') },
        Extra { mods: C, key: "Ins", label: "Copy", code: KeyCode::Insert },
        Extra { mods: C, key: "Enter", label: "Name", code: KeyCode::Enter },
        Extra { mods: C, key: "[", label: "LPath", code: KeyCode::Char('[') },
        Extra { mods: C, key: "]", label: "RPath", code: KeyCode::Char(']') },
        Extra { mods: C, key: "\\", label: "Root", code: KeyCode::Char('\\') },
        Extra { mods: A, key: "BkSp", label: "Folders", code: KeyCode::Backspace },
        Extra { mods: A, key: "X", label: "Quit", code: KeyCode::Char('x') },
    ]
};

/// Key bar labels for no modifier, Shift, Ctrl and Alt.
pub type KeyBar<'a> = [(KeyModifiers, [&'a str; 10])];

/// One shortcut on the key bar besides the F-keys: shown as `key label`, clicking it presses `code` + `mods`.
pub struct Extra {
    pub mods: KeyModifiers,
    pub key: &'static str,
    pub label: &'static str,
    pub code: KeyCode,
}

/// Where each key bar item was last drawn: (from x, to x, the key it presses).
static KEY_BAR_HITS: std::sync::Mutex<Vec<(i32, i32, KeyEvent)>> = std::sync::Mutex::new(Vec::new());

/// The key a click at column x of the key bar presses, if any.
pub fn key_bar_hit(x: i32) -> Option<KeyEvent> {
    KEY_BAR_HITS.lock().ok()?.iter().find(|(a, b, _)| x >= *a && x < *b).map(|h| h.2)
}

/// DN-style key bar with one label set; shows nothing while a modifier is held.
pub fn draw_key_bar(s: &mut Screen, labels: &[&str; 10]) {
    draw_key_bars(s, &[(KeyModifiers::NONE, *labels)]);
}

/// Key bar that follows the held modifiers: hold Shift / Ctrl / Alt to see what F-keys do with it.
/// Clicking an item presses that key (with the held modifiers).
pub fn draw_key_bars(s: &mut Screen, sets: &KeyBar) {
    draw_key_bars_with(s, sets, &[]);
}

/// Key bar: the F-keys that do something with the held modifiers first, then the other shortcuts
/// for those modifiers, as many as fit. Spare width is shared out between the items.
pub fn draw_key_bars_with(s: &mut Screen, sets: &KeyBar, extras: &[Extra]) {
    let mods = held_modifiers();
    let mut items: Vec<(String, &str, KeyEvent)> = Vec::new();
    if let Some((_, labels)) = sets.iter().find(|(m, _)| *m == mods) {
        for (i, l) in labels.iter().enumerate().filter(|(_, l)| !l.is_empty()) {
            items.push((format!("F{}", i + 1), l, KeyEvent::new(KeyCode::F(i as u8 + 1), mods)));
        }
    }
    let fkeys = items.len();
    for e in extras.iter().filter(|e| e.mods == mods) {
        items.push((e.key.to_string(), e.label, KeyEvent::new(e.code, e.mods)));
    }
    let width = |it: &(String, &str, KeyEvent)| (it.0.chars().count() + 1 + it.1.chars().count()) as i32;
    // drop shortcuts from the end until everything fits (the F-keys always stay)
    while items.len() > fkeys && items.iter().map(|i| width(i) + 1).sum::<i32>() > s.w {
        items.pop();
    }
    let t = theme::current();
    let y = s.h - 1;
    s.fill(0, y, s.w, 1, ' ', t.key_label, t.key_bg);
    let total: i32 = items.iter().map(width).sum();
    let n = items.len() as i32;
    let spare = (s.w - total).max(0);
    let mut hits = Vec::new();
    let mut x = 0;
    for (i, it) in items.iter().enumerate() {
        // spread the spare columns: item i gets its share (the first ones get the remainder)
        let pad = if n > 0 { spare / n + i32::from((i as i32) < spare % n) } else { 0 };
        let w = width(it) + pad;
        s.text(x, y, &it.0, t.key_num, t.key_bg);
        let kw = it.0.chars().count() as i32;
        s.text(x + kw, y, &fit(&format!(" {}", it.1), (w - kw).clamp(0, (s.w - x - kw).max(0)) as usize), t.key_label, t.key_bg);
        hits.push((x, x + w, it.2));
        x += w;
    }
    if let Ok(mut h) = KEY_BAR_HITS.lock() {
        *h = hits;
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
            if !theme::set(t) {
                vscode::apply(t);
            }
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
            if host_relaunch {
                p.filter = src.get(&format!("p{i}.filter")).filter(|f| !f.is_empty()).cloned();
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
            let temp = host_relaunch && src.get(&format!("p{i}.temp")).is_some_and(|v| v == "1");
            // a server panel comes back only after a command, never on a fresh start
            let remote = if host_relaunch { src.get(&format!("p{i}.remote")).map(|r| (r.clone(), src.get(&format!("p{i}.rpath")).cloned().unwrap_or_else(|| "/".into()))) } else { None };
            panels.push(p);
            init.push(PanelInit { arc, focus, temp, remote });
        }
        let fracs = parse_fracs(src.get("fracs"), n);
        let active = if host_relaunch { src.get("active").and_then(|v| v.parse().ok()).unwrap_or(0).min(n - 1) } else { 0 };
        let prev = if host_relaunch { src.get("prev").and_then(|v| v.parse().ok()).unwrap_or(0).min(n - 1) } else { 1.min(n - 1) };
        // the remembered master-password key belongs to this window's session; a fresh start drops it
        if let Some(sf) = &session_file {
            if !host_relaunch {
                let _ = fs::remove_file(crate::remote::vault::unlock_file(sf));
            }
            if let Some(d) = sf.parent() {
                crate::remote::vault::clean_leftovers(d);
            }
        }

        App {
            screen: Screen::new(),
            panels,
            fracs,
            active,
            prev,
            cmd: LineEdit::new(""),
            hidden_panel: None,
            hist: crate::history::History::new(),
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
            preview: None,
            backdrops: Vec::new(),
            frames: Vec::new(),
            window_frame: false,
            resume_user: host_relaunch && session.get("user").is_some_and(|v| v == "1"),
            last_find: find::Query { mask: "*".into(), text: String::new() },
            dir_hist: crate::dirhist::load(),
            temp_list: crate::temp::load(),
            remote_profiles: crate::remote::profiles::load(),
            vault_key: None,
            typed_passwords: HashMap::new(),
            prompt_count: HashMap::new(),
        }
    }

    /// The file that keeps the master-password key between restarts (only inside the shell wrapper).
    pub fn unlock_file(&self) -> Option<PathBuf> {
        self.session_file.as_deref().map(crate::remote::vault::unlock_file)
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
            self.preview = None; // the preview panel becomes a normal panel again
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

    /// Ctrl+P: hide the other panel (the active one takes the width), or show it again as it was.
    fn toggle_panel(&mut self) {
        if self.panels.len() > 1 {
            let i = self.passive_idx();
            let p = self.panels.remove(i);
            let f = self.fracs.remove(i);
            let nb = if i < self.panels.len() { i } else { i - 1 };
            self.fracs[nb] += f;
            if self.active > i {
                self.active -= 1;
            }
            self.prev = self.active;
            self.quick = None;
            self.hidden_panel = Some((i, p, f));
        } else if let Some((i, mut p, f)) = self.hidden_panel.take() {
            let i = i.min(self.panels.len());
            let f = f.clamp(0.1, 0.9);
            for x in self.fracs.iter_mut() {
                *x *= 1.0 - f;
            }
            self.fracs.insert(i, f);
            p.load(None);
            self.panels.insert(i, p);
            if self.active >= i {
                self.active += 1;
            }
            self.prev = i;
        } else {
            let a = self.active;
            self.new_panel();
            self.set_active(a);
        }
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
            if let Some((name, path)) = pi.remote {
                self.restore_remote(i, &name, &path, pi.focus.as_deref());
            }
            if pi.temp {
                self.show_temp(i);
                if let Some(f) = &pi.focus {
                    if let Some(c) = self.panels[i].items.iter().position(|e| &e.name == f) {
                        self.panels[i].move_to(c);
                    }
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

        if std::mem::take(&mut self.resume_user) {
            self.user_screen();
        }
        while !self.quit {
            match self.modal_input(&mut |_| {}, None) {
                Input::Key(k) => self.handle(k),
                Input::Mouse(m) => self.handle_mouse(m),
                Input::Paste(t) => self.cmd.insert(&t.replace(['\r', '\n'], " ")),
                Input::Tick => {}
            }
        }

        self.save_state();
        if self.action_file.is_some() {
            self.save_session();
        }
        // leaving ON for good (not just for a command): forget the master password and close servers
        if self.pending_run.is_none() {
            if let Some(f) = self.unlock_file() {
                let _ = fs::remove_file(f);
            }
            for p in self.panels.iter_mut() {
                if let Some(mut r) = p.remote.take() {
                    r.conn.close();
                }
            }
            let _ = fs::remove_dir_all(crate::remote::temp_dir());
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
        crate::dirhist::save(&self.dir_hist);
    }

    /// Everything needed to come back exactly as we were after a command runs in the shell.
    fn save_session(&self) {
        let Some(f) = &self.session_file else { return };
        let mut out = format!("panels={}\nactive={}\nprev={}\nuser={}\n", self.panels.len(), self.active, self.prev, self.resume_user as u8);
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
            if let Some(f) = &p.filter {
                out += &format!("p{i}.filter={f}\n");
            }
            if p.temp {
                out += &format!("p{i}.temp=1\n");
            }
            out += &remote_session_lines(i, p);
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
        crate::dirhist::visit(&mut self.dir_hist, &d);
        self.last_dir.insert(platform::volume_of(&d), d);
        true
    }

    /// Redraw (base screen + overlay) and wait for a key or mouse event.
    /// Redraws every second for the clock and at once on resize. With a timeout, returns Tick when it runs out.
    pub fn modal_input(&mut self, overlay: &mut dyn FnMut(&mut Screen), timeout: Option<Duration>) -> Input {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            self.screen.resize();
            self.draw_backdrop();
            overlay(&mut self.screen);
            // remember this frame for dialogs opened from here
            let depth = self.backdrops.len();
            self.frames.truncate(depth + 1);
            // a window's frame, or a dialog's (a dialog opened from a dialog is drawn over both)
            let keep = std::mem::take(&mut self.window_frame) || depth > 0;
            let snap = keep.then(|| self.screen.snapshot());
            if self.frames.len() > depth {
                self.frames[depth] = snap;
            } else if self.frames.len() == depth {
                self.frames.push(snap);
            }
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
                // short slices, so holding Shift / Ctrl / Alt updates the key bar quickly
                let slice = (until - now).min(Duration::from_millis(80));
                let mods = platform::modifiers();
                if mods.bits() != MODS.load(Ordering::Relaxed) {
                    MODS.store(mods.bits(), Ordering::Relaxed);
                    break;
                }
                match event::poll(slice) {
                    Ok(true) => match event::read() {
                        Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => return Input::Key(k),
                        // a click on the key bar presses that F-key, in every screen that has one
                        Ok(Event::Mouse(m)) if m.kind == MouseEventKind::Down(MouseButton::Left) && m.row as i32 == self.screen.h - 1 => {
                            if let Some(k) = key_bar_hit(m.column as i32) {
                                return Input::Key(k);
                            }
                        }
                        Ok(Event::Mouse(m)) if m.kind != MouseEventKind::Moved => return Input::Mouse(m),
                        Ok(Event::Paste(t)) => return Input::Paste(t),
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

    /// modal_input for a window (viewer, editor, tools...): dialogs it opens are drawn over it.
    pub fn window_input(&mut self, overlay: &mut dyn FnMut(&mut Screen), timeout: Option<Duration>) -> Input {
        self.window_frame = true;
        self.modal_input(overlay, timeout)
    }

    /// Run  (a dialog) over what is on the screen now, e.g. the viewer, instead of over the panels.
    pub fn over<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> R {
        let depth = self.backdrops.len();
        let snap = self.frames.get(depth).cloned().flatten();
        self.backdrops.push(snap);
        let r = f(self);
        self.backdrops.pop();
        r
    }

    /// The screen under a dialog: the window that opened it, or the panels.
    pub fn draw_backdrop(&mut self) {
        let restored = match self.backdrops.last() {
            Some(Some(b)) => self.screen.restore(b),
            _ => false,
        };
        if !restored {
            self.draw_base();
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
        let cur = self.panels[self.active].current().cloned();
        let in_arc = self.panels[self.active].arc.is_some();
        let cur_dir = self.panels[self.active].dir.clone();
        for (i, (x, pw)) in lay.into_iter().enumerate() {
            let active = i == self.active;
            match self.preview.as_mut() {
                Some(p) if !active => p.draw(&mut self.screen, x, 0, pw, h - 2, cur.as_ref(), in_arc, &cur_dir),
                _ => self.panels[i].draw(&mut self.screen, x, 0, pw, h - 2, active, if active { q } else { None }),
            }
        }
        draw_clock(&mut self.screen);

        let plen = prompt.chars().count() as i32;
        self.screen.text(0, h - 2, &prompt, t.cmd_fg, t.cmd_bg);
        self.cmd.draw(&mut self.screen, plen, h - 2, w - plen - 1, t.cmd_fg, t.cmd_bg);
        if self.quick.is_some() {
            self.screen.cursor = Some(self.panels[self.active].quick_pos);
        }
        if let Some(p) = self.preview.as_ref().filter(|p| p.focused) {
            let labels = p.key_labels();
            draw_key_bars(&mut self.screen, &[(KeyModifiers::NONE, labels)]);
            return;
        }
        // in the Temp panel F8 only takes files off the list
        let temp = self.panels[self.active].temp;
        let (f8, sf8) = if temp { ("Remove", "Remove") } else { ("Delete", "Wipe") };
        draw_key_bars_with(
            &mut self.screen,
            &[
                (KeyModifiers::NONE, ["Help", "Tools", "View", "Edit", "Copy", "RenMov", "MkDir", f8, "User", "Menu"]),
                (KeyModifiers::SHIFT, ["", "", "Diff", "New", "", "Rename", "", sf8, "", ""]),
                (KeyModifiers::CONTROL, ["", "Calc", "Name", "Ext", "Time", "Size", "", "", "", ""]),
                (KeyModifiers::ALT, ["Left", "Right", "GitDif", "", "SysMon", "Batch", "Find", "", "", "Tree"]),
            ],
            &PANEL_EXTRAS,
        );
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
                    if let Some(k) = key_bar_hit(mx) {
                        self.handle(k);
                    }
                    return;
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
                if let Some(p) = self.preview.as_mut() {
                    // the preview stays: a click on it gives it the keyboard, on the file panel takes it back
                    p.focused = i != self.active;
                    if p.focused {
                        return;
                    }
                }
                self.set_active(i);
                if let Some(idx) = self.panels[i].hit(mx, my) {
                    let double = self.last_click.is_some_and(|(at, pi, pidx)| pi == i && pidx == idx && at.elapsed().as_millis() < platform::double_click_ms());
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
                let d = if m.kind == MouseEventKind::ScrollUp { -3 } else { 3 };
                if i != self.active {
                    if let Some(p) = self.preview.as_mut() {
                        return p.scroll(d);
                    }
                }
                self.set_active(i);
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

        // the preview has the keyboard (Tab gave it): it scrolls and switches view modes like F3
        if let Some(p) = self.preview.as_mut().filter(|p| p.focused) {
            match k.code {
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc => {
                    p.focused = false;
                    return;
                }
                KeyCode::F(1) => return help::show(self, "preview"),
                _ if !ctrl && !alt => match p.key(k.code) {
                    viewer::DocKey::Done => return,
                    viewer::DocKey::Error(e) => return dialogs::message(self, "Format", &e, true),
                    viewer::DocKey::No => {}
                },
                _ => {}
            }
        }

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
                KeyCode::F(1) => return self.run_cmd(Cmd::DriveLeft),
                KeyCode::F(2) => return self.run_cmd(Cmd::DriveRight),
                KeyCode::F(10) => return self.run_cmd(Cmd::Tree),
                KeyCode::F(3) => return self.run_cmd(Cmd::GitDiff),
                KeyCode::F(7) => return self.run_cmd(Cmd::Find),
                KeyCode::F(5) => return self.run_cmd(Cmd::SysMon),
                KeyCode::F(6) => return self.run_cmd(Cmd::BatchRename),
                KeyCode::Backspace => return self.run_cmd(Cmd::FolderHistory),
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
                KeyCode::Char('p') => return self.run_cmd(Cmd::TogglePanel),
                KeyCode::Char('g') => return self.run_cmd(Cmd::Game),
                // Ctrl+E / Ctrl+Shift+E: previous / next command from the history
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    let back = !shift && k.code == KeyCode::Char('e');
                    let line = if back { self.hist.prev(&self.shell, &self.cmd.value()) } else { self.hist.next() };
                    if let Some(l) = line {
                        self.cmd.set(&l);
                    }
                    return;
                }
                KeyCode::Char('d') => return self.run_cmd(Cmd::AddFavorite),
                KeyCode::Char('f') => return self.run_cmd(Cmd::Filter),
                KeyCode::Char('a') => return self.run_cmd(Cmd::Attributes),
                KeyCode::Char('l') => return self.run_cmd(Cmd::Preview),
                KeyCode::Up | KeyCode::Down if self.preview.is_some() => {
                    if let Some(p) = self.preview.as_mut() {
                        p.scroll(if k.code == KeyCode::Up { -1 } else { 1 });
                    }
                    return;
                }
                KeyCode::Right => return self.run_cmd(Cmd::Wider),
                KeyCode::Left => return self.run_cmd(Cmd::Narrower),
                KeyCode::Char('\\') => return self.go_root(),
                // the file name, as it is, to the command line
                KeyCode::Enter | KeyCode::Char('j') => {
                    if let Some(e) = self.active().current().filter(|e| !e.is_up).cloned() {
                        self.cmd.insert(&format!("{} ", e.name));
                    }
                    return;
                }
                // Ctrl+[ / Ctrl+]: the left / right panel's folder to the command line.
                // Consoles may report Ctrl+[ as Esc and Ctrl+] as Ctrl+5.
                KeyCode::Char('[') | KeyCode::Esc => return self.insert_panel_path(0),
                KeyCode::Char(']') | KeyCode::Char('5') => return self.insert_panel_path(1),
                // Ctrl+Ins: copy the command line, or the file name when it is empty
                KeyCode::Insert => {
                    let text = if cmd_empty { self.active().current().filter(|e| !e.is_up).map(|e| e.name.clone()).unwrap_or_default() } else { self.cmd.value() };
                    if !text.is_empty() {
                        crate::editor::copy(&text);
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
                match self.preview.as_mut() {
                    Some(pv) => pv.focused = true,
                    None => self.set_active((self.active + 1) % n),
                }
                true
            }
            KeyCode::BackTab => {
                match self.preview.as_mut() {
                    Some(pv) => pv.focused = true,
                    None => self.set_active((self.active + n - 1) % n),
                }
                true
            }
            // Shift+movement selects the files passed over (like Insert, in that direction)
            KeyCode::Up if shift => {
                p.select_by(-1, false);
                true
            }
            KeyCode::Down if shift => {
                p.select_by(1, false);
                true
            }
            KeyCode::PageUp if shift => {
                p.select_by(-page, false);
                true
            }
            KeyCode::PageDown if shift => {
                p.select_by(page, false);
                true
            }
            KeyCode::Home if shift && cmd_empty => {
                p.select_by(-(p.cursor as isize), true);
                true
            }
            KeyCode::End if shift && cmd_empty => {
                p.select_by(p.items.len() as isize, true);
                true
            }
            KeyCode::Left if shift && cmd_empty => {
                p.select_by(-step, false);
                true
            }
            KeyCode::Right if shift && cmd_empty => {
                p.select_by(step, false);
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
                help::show(self, "panels");
                true
            }
            KeyCode::F(2) => {
                self.run_cmd(Cmd::Tools);
                true
            }
            KeyCode::F(9) => {
                self.run_cmd(Cmd::UserMenu);
                true
            }
            KeyCode::F(10) => {
                if let Some(c) = menu::run(self, 0) {
                    self.run_cmd(c);
                }
                true
            }
            KeyCode::F(3) => {
                if shift { self.compare_files() } else { self.cmd_view() }
                true
            }
            KeyCode::F(4) => {
                if shift { self.cmd_new_file() } else { self.cmd_edit() }
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
                self.delete(shift);
                true
            }
            KeyCode::Delete if cmd_empty => {
                self.delete(shift);
                true
            }
            _ => false,
        };
        if !handled && self.cmd.handle(&k) {
            self.hist.reset();
        }
    }

    pub fn run_cmd(&mut self, c: Cmd) {
        if self.panels[self.active].remote.is_some() && blocked_on_remote(&c) {
            return dialogs::message(self, "Remote", "Not available in a remote panel.", false);
        }
        match c {
            Cmd::ToggleView => self.active().toggle_view(),
            Cmd::Sort(m) => self.active().set_sort(m),
            Cmd::Hidden => {
                let p = self.active();
                p.show_hidden = !p.show_hidden;
                p.load(None);
                self.remote_reread(self.active);
            }
            Cmd::Reread => {
                self.active().load(None);
                self.remote_reread(self.active);
                if let Some(p) = self.preview.as_mut() {
                    p.forget();
                }
            }
            // Alt+F1 the left panel, Alt+F2 the right one (the rightmost when there are more)
            Cmd::DriveLeft => self.change_drive(0),
            Cmd::DriveRight => self.change_drive(self.panels.len() - 1),
            Cmd::Tree => self.cmd_tree(),
            Cmd::View => self.cmd_view(),
            Cmd::Edit => self.cmd_edit(),
            Cmd::NewFile => self.cmd_new_file(),
            Cmd::AddFavorite => {
                let dir = self.panels[self.active].dir.clone();
                favorites::add(self, &dir);
            }
            Cmd::Copy => fileops::copy_or_move(self, false),
            Cmd::Move => fileops::copy_or_move(self, true),
            Cmd::Rename => self.cmd_rename(),
            Cmd::BatchRename => crate::rename::show(self),
            Cmd::Mkdir => self.cmd_mkdir(),
            Cmd::Delete => self.delete(false),
            Cmd::Wipe => self.delete(true),
            Cmd::SelectGroup => self.select_group(true),
            Cmd::UnselectGroup => self.select_group(false),
            Cmd::Invert => self.invert_selection(),
            Cmd::Compare => self.compare(),
            Cmd::FolderSizes => fileops::folder_sizes(self),
            Cmd::Swap => self.swap_panels(),
            Cmd::UserScreen => self.user_screen(),
            Cmd::Quit => self.cmd_quit(),
            Cmd::Help => help::show(self, "panels"),
            Cmd::About => dialogs::message(self, "About", &format!("Orthodox Navigator {}\n\nIn memory of DOS Navigator", env!("CARGO_PKG_VERSION")), false),
            Cmd::NextTheme => vscode::pick(self),
            Cmd::Calculator => {
                if let Some(text) = calc::show(self) {
                    self.cmd.insert(&text);
                }
            }
            Cmd::Game => game::show(self),
            Cmd::SysMon => sysmon::show(self),
            Cmd::TogglePanel => self.toggle_panel(),
            Cmd::Wider => self.resize_active(2),
            Cmd::Narrower => self.resize_active(-2),
            Cmd::EqualWidths => self.equal_widths(),
            Cmd::CompareFiles => self.compare_files(),
            Cmd::GitDiff => self.git_diff(),
            Cmd::Find => {
                let last = std::mem::replace(&mut self.last_find, find::Query { mask: String::new(), text: String::new() });
                let q = find::ask(self, &last);
                self.last_find = last;
                let Some(q) = q else { return };
                let root = self.panels[self.active].dir.clone();
                let results = find::search(self, &root, &q);
                if results.is_empty() {
                    dialogs::message(self, "Find files", "Nothing found.", false);
                } else {
                    let a = self.active;
                    self.panels[a].set_results(format!("{} ({})", q.describe(), results.len()), results);
                }
                self.last_find = q;
            }
            Cmd::UserMenu => crate::usermenu::show(self),
            Cmd::FolderHistory => {
                let current = self.panels[self.active].dir.clone();
                let mut list = std::mem::take(&mut self.dir_hist);
                let pick = crate::dirhist::pick(self, &mut list, &current);
                self.dir_hist = list;
                if let Some(d) = pick {
                    let i = self.active;
                    if !self.go(i, &d, None) {
                        dialogs::message(self, "Folder history", &format!("{} is not there any more.", d.display()), true);
                        self.dir_hist.retain(|p| !same_path(p, &d));
                    }
                }
            }
            Cmd::TempPanel => {
                let a = self.active;
                if self.panels[a].temp {
                    self.panels[a].close_results();
                } else {
                    self.show_temp(a);
                }
            }
            Cmd::AddToTemp => self.add_to_temp(),
            Cmd::Filter => {
                let p = &self.panels[self.active];
                if p.found.is_some() {
                    return dialogs::message(self, "Filter", "The filter works on folders and archives,\nnot on search results or the Temp panel.", false);
                }
                let (dir, cur) = (p.dir.clone(), p.filter.clone());
                if let Some(f) = crate::filter::ask(self, &dir, cur.as_deref()) {
                    let p = self.active();
                    p.filter = f;
                    p.load(None);
                }
            }
            Cmd::Attributes => crate::attrs::show(self),
            Cmd::EditColors => {
                if let Some(f) = crate::colors::file() {
                    if !f.exists() {
                        let _ = fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
                        let _ = fs::write(&f, crate::colors::DEFAULT);
                    }
                    editor::edit(self, &f);
                    crate::colors::reload();
                }
            }
            Cmd::EditExtensions => crate::usermenu::edit_extensions(self),
            Cmd::EditGlobalMenu => crate::usermenu::edit_global(self),
            Cmd::RemoteConnect => self.change_drive(self.active),
            Cmd::MasterPassword => crate::remote::ui::change_master(self),
            Cmd::ForgetPasswords => crate::remote::ui::forget_passwords(self),
            Cmd::Tools => {
                let file = self.panels[self.active].current().filter(|e| !e.is_dir && !e.is_up).map(|e| e.path.clone());
                let dir = self.panels[self.active].dir.clone();
                tools::show(self, file, &dir);
            }
            Cmd::Preview => {
                if self.preview.is_some() {
                    self.preview = None;
                } else {
                    if self.panels.len() < 2 {
                        self.new_panel();
                        self.set_active(self.prev);
                    }
                    self.preview = Some(Preview::default());
                }
            }
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
        if self.panels[i].remote.is_some() {
            return self.remote_open(i, e);
        }
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
        if self.panels[i].found.is_some() {
            // in search results: go to the item (or back to the folder for ..)
            if e.is_up {
                return self.panels[i].close_results();
            }
            if !e.is_dir {
                let focus = e.path.file_name().map(|n| n.to_string_lossy().to_string());
                if let Some(parent) = e.path.parent() {
                    self.go(i, parent, focus.as_deref());
                }
                return;
            }
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
        // a command for this kind of file (F9 shows them; the one named Enter runs here)
        if let Some(line) = crate::usermenu::enter_command(self, &e) {
            return self.run_shell(&line);
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
        if self.panels[i].remote.is_some() {
            if let Some(up) = self.panels[i].items.first().filter(|e| e.is_up).cloned() {
                return self.remote_open(i, up);
            }
            return;
        }
        if self.panels[i].arc.is_some() {
            return self.panels[i].archive_up();
        }
        if self.panels[i].found.is_some() {
            return self.panels[i].close_results();
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
        let title = match panel {
            _ if self.panels.len() < 2 => "Drives and favourites",
            0 => "Drives and favourites: left panel",
            _ => "Drives and favourites: right panel",
        };
        let current = self.panels[panel].dir.clone();
        let Some(pick) = favorites::pick(self, title, &current) else { return };
        if let favorites::Pick::Remote(k) = pick {
            return self.open_remote(panel, k);
        }
        if matches!(pick, favorites::Pick::Temp) {
            self.show_temp(panel);
            return self.set_active(panel);
        }
        let (target, shown) = match pick {
            favorites::Pick::Temp | favorites::Pick::Remote(_) => return,
            favorites::Pick::Volume(root) if root != platform::home() => {
                let t = self.last_dir.get(&platform::volume_of(&root)).filter(|p| p.is_dir()).cloned().unwrap_or(root.clone());
                (t, root)
            }
            favorites::Pick::Volume(p) | favorites::Pick::Folder(p) => (p.clone(), p),
        };
        if !self.go(panel, &target, None) {
            dialogs::message(self, "Error", &format!("{} is not available.", shown.display()), true);
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

    /// Shift+F3: compare the file under the cursor with the other panel's file
    /// (the one with the same name there, else the one under its cursor).
    fn compare_files(&mut self) {
        let Some(a) = self.panels[self.active].current().filter(|e| !e.is_dir).cloned() else {
            return dialogs::message(self, "Compare", "Put the cursor on a file.", false);
        };
        if self.panels.len() < 2 || self.panels[self.active].arc.is_some() {
            return dialogs::message(self, "Compare", "Compare needs two panels, each showing a file.", false);
        }
        let other = &self.panels[self.passive_idx()];
        let same_name = other.dir.join(&a.name);
        let b = if same_name.is_file() && !same_path(&same_name, &a.path) {
            same_name
        } else {
            match other.current().filter(|e| !e.is_dir) {
                Some(e) => e.path.clone(),
                None => return dialogs::message(self, "Compare", "Put the other panel's cursor on a file to compare with.", false),
            }
        };
        let read = |p: &Path| fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned());
        match (read(&a.path), read(&b)) {
            (Ok(ta), Ok(tb)) => diff::show(self, &a.path.display().to_string(), &ta, &b.display().to_string(), &tb),
            (Err(e), _) | (_, Err(e)) => dialogs::message(self, "Compare", &e.to_string(), true),
        }
    }

    /// Alt+F3: what changed in this file since the last commit.
    fn git_diff(&mut self) {
        let Some(e) = self.panels[self.active].current().filter(|e| !e.is_dir).cloned() else {
            return dialogs::message(self, "Git diff", "Put the cursor on a file.", false);
        };
        if git::repo_root(&self.panels[self.active].dir).is_none() {
            return dialogs::message(self, "Git diff", "This folder is not in a git repository.", false);
        }
        let Some(head) = git::head_text(&e.path) else {
            return dialogs::message(self, "Git diff", &format!("{} is not committed yet (or git isn't installed).", e.name), false);
        };
        let now = fs::read(&e.path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        diff::show(self, &format!("{} (HEAD)", e.name), &head, &format!("{} (working copy)", e.name), &now);
    }

    /// F4: the built-in editor.
    fn cmd_edit(&mut self) {
        if self.active().arc.is_some() {
            return fileops::read_only(self);
        }
        let Some(e) = self.active().current().filter(|e| !e.is_dir).cloned() else { return };
        if editor::edit(self, &e.path) {
            self.reload_after_edit(&e.path);
        }
    }

    /// Shift+F4: edit a new (or any) file by name.
    fn cmd_new_file(&mut self) {
        if self.active().arc.is_some() {
            return fileops::read_only(self);
        }
        let Some(name) = dialogs::input(self, "Edit file", "File to edit (it is created when you save):", "") else { return };
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let path = self.active().dir.join(name);
        if let Some(parent) = path.parent().filter(|p| !p.exists()) {
            if let Err(err) = fs::create_dir_all(parent) {
                return dialogs::message(self, "Error", &err.to_string(), true);
            }
        }
        if editor::edit(self, &path) {
            self.reload_after_edit(&path);
        }
    }

    fn reload_after_edit(&mut self, path: &Path) {
        let a = self.active;
        if path.parent().is_some_and(|p| same_path(p, &self.panels[a].dir)) {
            let focus = path.file_name().map(|n| n.to_string_lossy().to_string());
            self.panels[a].load(focus.as_deref());
        }
        let p = self.passive_idx();
        if p != a {
            self.panels[p].load(None);
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
        // the extension of the file under the cursor, so Gray+ Enter selects all files like it
        let def = match self.active().current().filter(|e| !e.is_dir).map(|e| e.ext()) {
            Some(ext) if !ext.is_empty() => format!("*{ext}"),
            _ => "*.*".to_string(),
        };
        let Some(mask) = dialogs::input(self, title, "Files matching:", &def) else { return };
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

    /// Ctrl+O: hide the panels and show the console. The command line stays at the bottom: type and run
    /// commands as in a plain shell; Ctrl+O brings the panels back.
    fn user_screen(&mut self) {
        use crossterm::{cursor, queue, style::Print};
        self.screen.suspend();
        let _ = terminal::enable_raw_mode();
        let mut out = std::io::stdout();
        if cursor::position().is_ok_and(|(x, _)| x > 0) {
            let _ = write!(out, "\r\n");
        }
        loop {
            // the prompt line, scrolled so the cursor stays visible
            let prompt = self.prompt();
            let pad = if prompt.ends_with(' ') { "" } else { " " };
            let plen = prompt.chars().count() + pad.len();
            let width = terminal::size().map_or(80, |(w, _)| w as usize);
            let avail = width.saturating_sub(plen + 1).max(1);
            let chars: Vec<char> = self.cmd.value().chars().collect();
            let pos = self.cmd.pos();
            let start = pos.saturating_sub(avail);
            let shown: String = chars[start..chars.len().min(start + avail)].iter().collect();
            let _ = queue!(
                out,
                cursor::MoveToColumn(0),
                terminal::Clear(terminal::ClearType::UntilNewLine),
                Print(format!("{prompt}{pad}{shown}")),
                cursor::MoveToColumn((plen + pos - start) as u16)
            );
            let _ = out.flush();

            let k = match event::read() {
                Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => k,
                Ok(Event::Paste(t)) => {
                    self.cmd.insert(&t.replace(['\r', '\n'], " "));
                    continue;
                }
                _ => continue,
            };
            let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
            match k.code {
                KeyCode::Char('o') | KeyCode::Char('O') if ctrl => break,
                KeyCode::Esc => self.cmd.set(""),
                KeyCode::Enter => {
                    let text = self.cmd.value();
                    let t = text.trim();
                    let wrapper = self.action_file.is_some() && !t.is_empty() && self.builtin_name(t).is_none();
                    if wrapper {
                        // the wrapper prints the prompt and command itself
                        let _ = queue!(out, cursor::MoveToColumn(0), terminal::Clear(terminal::ClearType::CurrentLine));
                        let _ = out.flush();
                    } else {
                        let _ = write!(out, "\r\n");
                    }
                    if t.is_empty() {
                        continue;
                    }
                    self.cmd.set("");
                    self.add_history(t);
                    match self.builtin(t) {
                        Some(Err(e)) => {
                            let _ = write!(out, "{}\r\n", e.replace('\n', "\r\n"));
                        }
                        Some(Ok(())) if self.quit => {
                            let _ = terminal::disable_raw_mode();
                            return;
                        }
                        Some(Ok(())) => {}
                        // the `on` wrapper runs it in the real shell, then starts us again on this screen
                        None if self.action_file.is_some() => {
                            self.pending_run = Some(t.to_string());
                            self.resume_user = true;
                            self.quit = true;
                            let _ = terminal::disable_raw_mode();
                            return;
                        }
                        None => {
                            let _ = terminal::disable_raw_mode();
                            let mut c = platform::shell_command(&self.shell, t);
                            c.current_dir(&self.panels[self.active].dir);
                            if let Err(e) = c.status() {
                                println!("{e}");
                            }
                            let _ = terminal::enable_raw_mode();
                            if cursor::position().is_ok_and(|(x, _)| x > 0) {
                                let _ = write!(out, "\r\n");
                            }
                            for p in self.panels.iter_mut() {
                                p.load(None);
                            }
                        }
                    }
                }
                _ => {
                    self.cmd.handle(&k);
                }
            }
        }
        let _ = write!(out, "\r\n");
        let _ = terminal::disable_raw_mode();
        self.screen.resume();
    }
    /// Put panel `i`'s folder (0 = left, 1 = right) on the command line.
    fn insert_panel_path(&mut self, i: usize) {
        let i = i.min(self.panels.len() - 1);
        let mut path = self.panels[i].dir.to_string_lossy().to_string();
        if !path.ends_with(std::path::MAIN_SEPARATOR) {
            path.push(std::path::MAIN_SEPARATOR);
        }
        self.cmd.insert(&path);
    }

    /// The shell commands run in (for quoting).
    pub fn shell_name(&self) -> String {
        self.shell.clone()
    }

    /// Run a command line in the shell, as if typed (without adding it to the history).
    pub fn run_line(&mut self, line: &str) {
        self.run_shell(line);
    }

    /// Re-read both panels (after something changed files).
    pub fn reload_panels(&mut self) {
        for p in self.panels.iter_mut() {
            p.load(None);
        }
    }

    /// Show the Temp panel in panel i (keeping the cursor where it was if it already shows it).
    fn show_temp(&mut self, i: usize) {
        self.panels[i].remote = None;
        let was = self.panels[i].temp.then_some(self.panels[i].cursor);
        let entries = crate::temp::entries(&self.temp_list);
        let p = &mut self.panels[i];
        p.set_results(crate::temp::title(entries.len()), entries);
        p.temp = true;
        if let Some(c) = was {
            p.move_to(c);
        }
    }

    fn refresh_temp(&mut self) {
        crate::temp::save(&self.temp_list);
        for i in 0..self.panels.len() {
            if self.panels[i].temp {
                self.show_temp(i);
            }
        }
    }

    /// Put the selected files (or the one under the cursor) on the Temp panel's list.
    pub fn add_to_temp(&mut self) {
        let p = &self.panels[self.active];
        if p.arc.is_some() || p.temp {
            return;
        }
        let paths: Vec<PathBuf> = p.targets().into_iter().filter(|e| !e.is_up).map(|e| e.path).collect();
        if paths.is_empty() {
            return;
        }
        let n = crate::temp::add(&mut self.temp_list, &paths);
        for e in self.active().items.iter_mut() {
            e.selected = false;
        }
        self.refresh_temp();
        // with the Temp panel on screen the change shows there; otherwise say where the files went
        if !self.panels.iter().any(|p| p.temp) {
            let text = format!("{n} added; the Temp panel has {}.\nOpen it from Alt+F1 / Alt+F2 or the Disk menu.", self.temp_list.len());
            dialogs::message(self, "Temp panel", &text, false);
        }
    }

    /// F8: delete, or in the Temp panel take the files off the list.
    fn delete(&mut self, permanent: bool) {
        if self.panels[self.active].temp {
            let paths: Vec<PathBuf> = self.panels[self.active].targets().into_iter().map(|e| e.path).collect();
            crate::temp::remove(&mut self.temp_list, &paths);
            return self.refresh_temp();
        }
        fileops::delete(self, permanent);
    }

    /// The other panel shows the Temp panel (F5 there adds files to it instead of copying).
    pub fn passive_is_temp(&self) -> bool {
        self.panels[self.passive_idx()].temp && self.passive_idx() != self.active
    }

    fn cmd_quit(&mut self) {
        if dialogs::choice(self, "Quit", &["Do you want to quit Orthodox Navigator?"], &["Yes", "No"], false) == Some(0) {
            self.quit = true;
        }
    }

    // -------------------------------------------------------- command line

    /// Remember a command so the wrapper can add it to the shell's history.
    fn add_history(&mut self, command: &str) {
        self.hist.add(command);
        let Some(f) = &self.history_file else { return };
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(f) {
            let _ = writeln!(file, "{}", command.replace(['\r', '\n'], " "));
        }
    }

    fn execute(&mut self, text: &str) {
        let t = text.trim();
        if self.panels[self.active].remote.is_some() {
            self.cmd.set(text);
            return dialogs::message(self, "Remote", "Commands don't run in a remote panel.\nSwitch to a local panel (Tab) to run them.", false);
        }
        self.add_history(t);
        match self.builtin(t) {
            Some(Err(e)) => dialogs::message(self, "Error", &e, true),
            Some(Ok(())) => {}
            None => self.run_shell(t),
        }
    }

    /// Is `t` one of ON's own commands (see builtin)? Doesn't run it.
    fn builtin_name(&self, t: &str) -> Option<()> {
        let lower = t.to_lowercase();
        let drive = cfg!(windows) && t.len() == 2 && t.ends_with(':') && t.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
        let cd = lower.starts_with("cd ") || lower.starts_with("cd\\") || lower.starts_with("cd..") || lower.starts_with("cd/");
        (lower == "exit" || t == "~" || lower == "cd ~" || (!cfg!(windows) && lower == "cd") || drive || cd).then_some(())
    }

    /// Commands ON does itself: exit, cd, ~, a drive letter. None if `t` is for the shell.
    fn builtin(&mut self, t: &str) -> Option<Result<(), String>> {
        let i = self.active;
        let lower = t.to_lowercase();
        if lower == "exit" {
            self.quit = true;
            return Some(Ok(()));
        }
        if t == "~" || lower == "cd ~" || (!cfg!(windows) && lower == "cd") {
            self.go(i, &platform::home(), None);
            return Some(Ok(()));
        }
        // "d:" switches drive (Windows)
        if cfg!(windows) && t.len() == 2 && t.ends_with(':') && t.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            let root = format!("{}:\\", t[..1].to_uppercase());
            let target = self.last_dir.get(&root[..2]).cloned().unwrap_or_else(|| PathBuf::from(&root));
            if !self.go(i, &target, None) {
                return Some(Err(format!("Drive {root} is not ready.")));
            }
            return Some(Ok(()));
        }
        // "cd x", "cd ..", "cd\x", "cd /d d:\x"
        if lower.starts_with("cd ") || lower.starts_with("cd\\") || lower.starts_with("cd..") || lower.starts_with("cd/") {
            let mut arg = t[2..].trim();
            if arg.to_lowercase().starts_with("/d ") {
                arg = arg[3..].trim();
            }
            let arg = arg.trim_matches(|c| c == '"' || c == '\'');
            if arg.is_empty() {
                return Some(Ok(()));
            }
            let arg = if let Some(rest) = arg.strip_prefix("~/").or_else(|| arg.strip_prefix("~\\")) {
                platform::home().join(rest)
            } else {
                PathBuf::from(arg)
            };
            let target = normalize(&self.active().dir.join(arg));
            if !self.go(i, &target, None) {
                return Some(Err(format!("The folder does not exist:\n{}", target.display())));
            }
            return Some(Ok(()));
        }
        None
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

/// Commands that need local files: not in a remote panel.
fn blocked_on_remote(c: &Cmd) -> bool {
    matches!(
        c,
        Cmd::UserMenu
            | Cmd::Find
            | Cmd::Attributes
            | Cmd::Filter
            | Cmd::FolderSizes
            | Cmd::AddToTemp
            | Cmd::BatchRename
            | Cmd::GitDiff
            | Cmd::Tools
            | Cmd::Compare
            | Cmd::CompareFiles
            | Cmd::NewFile
            | Cmd::AddFavorite
    )
}

/// The session lines for a panel showing a server.
fn remote_session_lines(i: usize, p: &Panel) -> String {
    match &p.remote {
        Some(r) => format!("p{i}.remote={}\np{i}.rpath={}\n", r.profile, r.cwd),
        None => String::new(),
    }
}

// ------------------------------------------------------------------ remote panels

impl App {
    /// Show folder `path` of the server in panel i. False (after a message) when it can't be read.
    pub fn remote_cd(&mut self, i: usize, path: &str, focus: Option<&str>) -> bool {
        let Some(mut view) = self.panels[i].remote.take() else { return false };
        self.prompt_count.remove(&view.profile);
        let r = view.conn.list(self, path);
        let ok = match r {
            Ok(list) => {
                view.cwd = path.to_string();
                self.panels[i].remote = Some(view);
                self.panels[i].set_remote_items(path, list, focus);
                return true;
            }
            Err(e) => e,
        };
        self.panels[i].remote = Some(view);
        if ok != "Cancelled." {
            dialogs::message(self, "Remote", &ok, true);
        }
        false
    }

    fn remote_reread(&mut self, i: usize) {
        let Some(cwd) = self.panels[i].remote.as_ref().map(|r| r.cwd.clone()) else { return };
        let focus = self.panels[i].current().map(|e| e.name.clone());
        self.remote_cd(i, &cwd, focus.as_deref());
    }

    /// Back to the local folder the panel showed before.
    fn leave_remote(&mut self, i: usize) {
        if let Some(mut r) = self.panels[i].remote.take() {
            r.conn.close();
        }
        self.panels[i].items.clear();
        self.panels[i].load(None);
    }

    /// Connect panel i to profile k (from the drive list).
    fn open_remote(&mut self, i: usize, k: usize) {
        match crate::remote::ui::connect(self, k) {
            Ok(view) => {
                let cwd = view.cwd.clone();
                let p = &mut self.panels[i];
                p.arc = None;
                p.found = None;
                p.temp = false;
                p.filter = None;
                p.items.clear();
                p.remote = Some(view);
                self.set_active(i);
                if !self.remote_cd(i, &cwd, None) && !(cwd != "/" && self.remote_cd(i, "/", None)) {
                    self.leave_remote(i);
                }
            }
            Err(e) if e == "Cancelled." => {}
            Err(e) => dialogs::message(self, "Remote", &e, true),
        }
    }

    /// After a command: connect again and show the folder it showed.
    fn restore_remote(&mut self, i: usize, name: &str, path: &str, focus: Option<&str>) {
        let Some(k) = self.remote_profiles.list.iter().position(|p| p.name == name) else { return };
        match crate::remote::ui::connect(self, k) {
            Ok(view) => {
                self.panels[i].remote = Some(view);
                if !self.remote_cd(i, path, focus) {
                    self.leave_remote(i);
                }
            }
            Err(e) if e == "Cancelled." => {}
            Err(e) => dialogs::message(self, "Remote", &e, true),
        }
    }

    /// Enter / double click in a remote panel.
    fn remote_open(&mut self, i: usize, e: panel::Entry) {
        let path = e.path.to_string_lossy().replace('\\', "/");
        if e.is_up {
            let cwd = self.panels[i].remote.as_ref().map(|r| r.cwd.clone()).unwrap_or_default();
            match crate::remote::parent(&cwd) {
                Some((up, name)) => {
                    self.remote_cd(i, &up, Some(&name));
                }
                None => self.leave_remote(i),
            }
            return;
        }
        // a link may lead to a folder: try to enter it first
        if (e.is_dir || e.is_link) && (self.remote_cd(i, &path, None) || e.is_dir) {
            return;
        }
        dialogs::message(self, "Remote", "Opening files on a server comes in the next step.", false);
    }
}

#[cfg(test)]
mod remote_tests {
    use super::*;
    use crate::remote::listing::RemoteEntry;
    use crate::remote::wait::Ui;
    use crate::remote::{Conn, RemoteView, R};

    struct NoConn;
    impl Conn for NoConn {
        fn home(&mut self, _: &mut dyn Ui) -> R<String> { Ok("/".into()) }
        fn list(&mut self, _: &mut dyn Ui, _: &str) -> R<Vec<RemoteEntry>> { Ok(vec![]) }
        fn get(&mut self, _: &mut dyn Ui, _: &str, _: &Path, _: bool) -> R<()> { Ok(()) }
        fn put(&mut self, _: &mut dyn Ui, _: &Path, _: &str, _: bool) -> R<()> { Ok(()) }
        fn mkdir(&mut self, _: &mut dyn Ui, _: &str) -> R<()> { Ok(()) }
        fn remove(&mut self, _: &mut dyn Ui, _: &str, _: bool) -> R<()> { Ok(()) }
        fn rename(&mut self, _: &mut dyn Ui, _: &str, _: &str) -> R<()> { Ok(()) }
        fn close(&mut self) {}
    }

    #[test]
    fn session_keeps_remote_panels() {
        let mut p = Panel::new(std::env::temp_dir());
        assert_eq!(remote_session_lines(0, &p), "");
        p.remote = Some(RemoteView { profile: "Work".into(), cwd: "/var/www".into(), conn: Box::new(NoConn), base: "sftp://h".into() });
        assert_eq!(remote_session_lines(1, &p), "p1.remote=Work\np1.rpath=/var/www\n");
        assert_eq!(p.title_path(), "sftp://h/var/www");
    }
}
