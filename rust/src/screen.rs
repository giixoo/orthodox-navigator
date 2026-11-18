//! Off-screen cell buffer, flushed to the terminal with ANSI sequences.
//! Only changed cells are written, so there is no flicker.

use crossterm::{cursor, execute, terminal};
use std::fmt::Write as _;
use std::io::{self, Write};

/// The 16 classic DOS text-mode colours, or any 24-bit colour (VS Code themes).
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum C {
    Black, Blue, Green, Cyan, Red, Magenta, Brown, LightGray,
    DarkGray, LightBlue, LightGreen, LightCyan, LightRed, LightMagenta, Yellow, White,
    Rgb(u8, u8, u8),
}

const NAMED: [C; 16] = [
    C::Black, C::Blue, C::Green, C::Cyan, C::Red, C::Magenta, C::Brown, C::LightGray,
    C::DarkGray, C::LightBlue, C::LightGreen, C::LightCyan, C::LightRed, C::LightMagenta, C::Yellow, C::White,
];

impl C {
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            C::Rgb(r, g, b) => (r, g, b),
            named => VGA[NAMED.iter().position(|&n| n == named).unwrap_or(7)],
        }
    }

    /// The nearest of the 16 colours (for terminals without 24-bit colour).
    fn index16(self) -> usize {
        if let Some(i) = NAMED.iter().position(|&n| n == self) {
            return i;
        }
        let (r, g, b) = self.rgb();
        let dist = |&(vr, vg, vb): &(u8, u8, u8)| {
            let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
            d(r, vr) + d(g, vg) + d(b, vb)
        };
        (0..16).min_by_key(|&i| dist(&VGA[i])).unwrap_or(7)
    }

    /// The same colour made darker (for shadows over 24-bit colours).
    pub fn darker(self) -> C {
        let (r, g, b) = self.rgb();
        C::Rgb(r / 3, g / 3, b / 3)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: C,
    pub bg: C,
}

const BLANK: Cell = Cell { ch: ' ', fg: C::LightGray, bg: C::Black };

/// Exact VGA palette, sent as 24-bit colour so the terminal's own colour scheme
/// can't change the look. Set ON_COLORS=16 to use the terminal's 16 colours instead.
const VGA: [(u8, u8, u8); 16] = [
    (0, 0, 0), (0, 0, 170), (0, 170, 0), (0, 170, 170), (170, 0, 0), (170, 0, 170), (170, 85, 0), (170, 170, 170),
    (85, 85, 85), (85, 85, 255), (85, 255, 85), (85, 255, 255), (255, 85, 85), (255, 85, 255), (255, 255, 85), (255, 255, 255),
];
/// DOS colour index -> ANSI colour index.
const ANSI: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

/// A copy of a frame, to draw dialogs over (see App::over).
#[derive(Clone)]
pub struct Snapshot {
    w: i32,
    h: i32,
    cells: Vec<Cell>,
}

pub struct Screen {
    pub w: i32,
    pub h: i32,
    pub cursor: Option<(i32, i32)>,
    buf: Vec<Cell>,
    prev: Vec<Cell>,
    full: bool,
    truecolor: bool,
}

pub fn restore_terminal() {
    let mut out = io::stdout();
    let _ = write!(out, "\x1b[0m");
    #[cfg(not(windows))]
    let _ = execute!(out, crossterm::event::DisableBracketedPaste);
    let _ = execute!(out, crossterm::event::DisableMouseCapture, cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}

fn push_color(out: &mut String, truecolor: bool, fg: C, bg: C) {
    if truecolor {
        let (fr, fgc, fb) = fg.rgb();
        let (br, bgc, bb) = bg.rgb();
        let _ = write!(out, "\x1b[38;2;{fr};{fgc};{fb};48;2;{br};{bgc};{bb}m");
    } else {
        let (fg, bg) = (fg.index16(), bg.index16());
        let f = if fg < 8 { 30 + ANSI[fg] } else { 90 + ANSI[fg - 8] };
        let b = if bg < 8 { 40 + ANSI[bg] } else { 100 + ANSI[bg - 8] };
        let _ = write!(out, "\x1b[{f};{b}m");
    }
}

impl Screen {
    pub fn new() -> Self {
        let mut s = Screen {
            w: 0,
            h: 0,
            cursor: None,
            buf: vec![],
            prev: vec![],
            full: true,
            truecolor: std::env::var("ON_COLORS").map(|v| v != "16").unwrap_or(true),
        };
        s.resize();
        s
    }

    pub fn init(&mut self) -> io::Result<()> {
        crate::platform::enable_vt();
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), terminal::EnterAlternateScreen, cursor::Hide, crossterm::event::EnableMouseCapture)?;
        #[cfg(not(windows))]
        execute!(io::stdout(), crossterm::event::EnableBracketedPaste)?;
        self.full = true;
        Ok(())
    }

    pub fn shutdown(&mut self) {
        restore_terminal();
    }

    /// Leave the alternate screen so the user sees normal console output.
    pub fn suspend(&mut self) {
        restore_terminal();
    }

    pub fn resume(&mut self) {
        let _ = self.init();
    }

    /// The frame drawn so far.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { w: self.w, h: self.h, cells: self.buf.clone() }
    }

    /// Start a frame from a snapshot; false if the screen size has changed since.
    pub fn restore(&mut self, s: &Snapshot) -> bool {
        if s.w != self.w || s.h != self.h || s.cells.len() != self.buf.len() {
            return false;
        }
        self.buf.copy_from_slice(&s.cells);
        true
    }

    /// Re-read the terminal size; returns true if it changed.
    pub fn resize(&mut self) -> bool {
        let (w, h) = terminal::size().unwrap_or((80, 25));
        let (w, h) = ((w as i32).max(40), (h as i32).max(12));
        if w == self.w && h == self.h {
            return false;
        }
        self.w = w;
        self.h = h;
        self.buf = vec![BLANK; (w * h) as usize];
        self.prev = vec![BLANK; (w * h) as usize];
        self.full = true;
        true
    }

    pub fn put(&mut self, x: i32, y: i32, ch: char, fg: C, bg: C) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        self.buf[(y * self.w + x) as usize] = Cell { ch, fg, bg };
    }

    pub fn text(&mut self, x: i32, y: i32, s: &str, fg: C, bg: C) {
        for (i, ch) in s.chars().enumerate() {
            self.put(x + i as i32, y, ch, fg, bg);
        }
    }

    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, ch: char, fg: C, bg: C) {
        for j in 0..h {
            for i in 0..w {
                self.put(x + i, y + j, ch, fg, bg);
            }
        }
    }

    pub fn hline(&mut self, x: i32, y: i32, w: i32, ch: char, fg: C, bg: C) {
        for i in 0..w {
            self.put(x + i, y, ch, fg, bg);
        }
    }

    pub fn vline(&mut self, x: i32, y: i32, h: i32, ch: char, fg: C, bg: C) {
        for j in 0..h {
            self.put(x, y + j, ch, fg, bg);
        }
    }

    pub fn boxed(&mut self, x: i32, y: i32, w: i32, h: i32, fg: C, bg: C, dbl: bool) {
        let c: [char; 6] = if dbl { ['╔', '╗', '╚', '╝', '═', '║'] } else { ['┌', '┐', '└', '┘', '─', '│'] };
        self.hline(x + 1, y, w - 2, c[4], fg, bg);
        self.hline(x + 1, y + h - 1, w - 2, c[4], fg, bg);
        self.vline(x, y + 1, h - 2, c[5], fg, bg);
        self.vline(x + w - 1, y + 1, h - 2, c[5], fg, bg);
        self.put(x, y, c[0], fg, bg);
        self.put(x + w - 1, y, c[1], fg, bg);
        self.put(x, y + h - 1, c[2], fg, bg);
        self.put(x + w - 1, y + h - 1, c[3], fg, bg);
    }

    /// DN-style shadow: two columns on the right, one row below.
    pub fn shadow(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let mut dim = |cx: i32, cy: i32| {
            if cx < 0 || cy < 0 || cx >= self.w || cy >= self.h {
                return;
            }
            let c = &mut self.buf[(cy * self.w + cx) as usize];
            if let C::Rgb(..) = c.bg {
                c.fg = c.fg.darker();
                c.bg = c.bg.darker();
            } else {
                c.fg = C::DarkGray;
                c.bg = C::Black;
            }
        };
        for j in 1..=h {
            dim(x + w, y + j);
            dim(x + w + 1, y + j);
        }
        for i in 2..w + 2 {
            dim(x + i, y + h);
        }
    }

    pub fn flush(&mut self) {
        let mut out = String::with_capacity((self.w * self.h * 2) as usize);
        let mut cur: Option<(C, C)> = None;
        let (mut cx, mut cy) = (-1, -1);
        for y in 0..self.h {
            for x in 0..self.w {
                let i = (y * self.w + x) as usize;
                let c = self.buf[i];
                if !self.full && c == self.prev[i] {
                    continue;
                }
                if cx != x || cy != y {
                    let _ = write!(out, "\x1b[{};{}H", y + 1, x + 1);
                }
                if cur != Some((c.fg, c.bg)) {
                    push_color(&mut out, self.truecolor, c.fg, c.bg);
                    cur = Some((c.fg, c.bg));
                }
                out.push(if c.ch == '\0' { ' ' } else { c.ch });
                cx = x + 1;
                cy = y;
                self.prev[i] = c;
            }
        }
        self.full = false;
        match self.cursor {
            Some((x, y)) => {
                let _ = write!(out, "\x1b[{};{}H\x1b[?25h", y + 1, x + 1);
            }
            None => out.push_str("\x1b[?25l"),
        }
        let mut so = io::stdout().lock();
        let _ = so.write_all(out.as_bytes());
        let _ = so.flush();
    }
}
