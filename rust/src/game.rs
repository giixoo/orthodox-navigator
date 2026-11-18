//! A falling-blocks game in a DN-style window.
//! ←/→ move, ↑ rotate, ↓ drop faster, Space drop, F3/P pause, F2 new game, Esc quit.

use crate::app::{draw_key_bar, App, Input};
use crate::dialogs;
use crate::screen::{Screen, C};
use crate::theme;
use crossterm::event::KeyCode;
use std::fs;
use std::time::{Duration, Instant};

const W: usize = 10;
const H: usize = 20;

/// Shapes as 4 cells in a 4x4 box, plus a colour.
const SHAPES: [([(i32, i32); 4], C); 7] = [
    ([(0, 1), (1, 1), (2, 1), (3, 1)], C::LightCyan),
    ([(0, 0), (1, 0), (0, 1), (1, 1)], C::Yellow),
    ([(1, 0), (0, 1), (1, 1), (2, 1)], C::LightMagenta),
    ([(1, 0), (2, 0), (0, 1), (1, 1)], C::LightGreen),
    ([(0, 0), (1, 0), (1, 1), (2, 1)], C::LightRed),
    ([(0, 0), (0, 1), (1, 1), (2, 1)], C::LightBlue),
    ([(2, 0), (0, 1), (1, 1), (2, 1)], C::Brown),
];

#[derive(Clone, Copy)]
struct Piece {
    kind: usize,
    cells: [(i32, i32); 4],
    x: i32,
    y: i32,
}

impl Piece {
    fn new(kind: usize) -> Self {
        Piece { kind, cells: SHAPES[kind].0, x: 3, y: 0 }
    }

    fn rotated(&self) -> Self {
        if self.kind == 1 {
            return *self; // the square doesn't rotate
        }
        let size = if self.kind == 0 { 4 } else { 3 };
        let mut p = *self;
        for c in p.cells.iter_mut() {
            *c = (size - 1 - c.1, c.0);
        }
        p
    }

    fn blocks(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.cells.iter().map(move |(cx, cy)| (self.x + cx, self.y + cy))
    }
}

/// Small xorshift random generator (no extra crates).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

struct Game {
    board: [[Option<C>; W]; H],
    cur: Piece,
    next: usize,
    bag: Vec<usize>,
    rng: Rng,
    score: u64,
    lines: u64,
    level: u64,
    over: bool,
    paused: bool,
}

impl Game {
    fn new(seed: u64) -> Self {
        let mut g = Game {
            board: [[None; W]; H],
            cur: Piece::new(0),
            next: 0,
            bag: vec![],
            rng: Rng(seed | 1),
            score: 0,
            lines: 0,
            level: 1,
            over: false,
            paused: false,
        };
        let first = g.draw_kind();
        g.cur = Piece::new(first);
        g.next = g.draw_kind();
        g
    }

    /// Pieces come from a shuffled bag of all seven, so droughts are short.
    fn draw_kind(&mut self) -> usize {
        if self.bag.is_empty() {
            self.bag = (0..7).collect();
            for i in (1..7).rev() {
                let j = (self.rng.next() % (i as u64 + 1)) as usize;
                self.bag.swap(i, j);
            }
        }
        self.bag.pop().unwrap_or(0)
    }

    fn fits(&self, p: &Piece) -> bool {
        p.blocks().all(|(x, y)| x >= 0 && x < W as i32 && y < H as i32 && (y < 0 || self.board[y as usize][x as usize].is_none()))
    }

    fn shift(&mut self, dx: i32, dy: i32) -> bool {
        let mut p = self.cur;
        p.x += dx;
        p.y += dy;
        if self.fits(&p) {
            self.cur = p;
            true
        } else {
            false
        }
    }

    fn rotate(&mut self) {
        let r = self.cur.rotated();
        // try in place, then nudge sideways off a wall
        for dx in [0, -1, 1, -2, 2] {
            let mut p = r;
            p.x += dx;
            if self.fits(&p) {
                self.cur = p;
                return;
            }
        }
    }

    fn speed(&self) -> Duration {
        Duration::from_millis(800u64.saturating_sub((self.level - 1) * 70).max(90))
    }

    /// Move down one row; when it can't, lock the piece and bring the next one.
    fn step(&mut self) {
        if self.shift(0, 1) {
            return;
        }
        let color = SHAPES[self.cur.kind].1;
        for (x, y) in self.cur.blocks() {
            if y < 0 {
                self.over = true;
                return;
            }
            self.board[y as usize][x as usize] = Some(color);
        }
        let mut cleared = 0;
        let mut row = H;
        while row > 0 {
            row -= 1;
            if self.board[row].iter().all(|c| c.is_some()) {
                for r in (1..=row).rev() {
                    self.board[r] = self.board[r - 1];
                }
                self.board[0] = [None; W];
                cleared += 1;
                row += 1;
            }
        }
        self.score += [0, 40, 100, 300, 1200][cleared] * self.level;
        self.lines += cleared as u64;
        self.level = 1 + self.lines / 10;
        let next = self.next;
        self.cur = Piece::new(next);
        self.next = self.draw_kind();
        if !self.fits(&self.cur) {
            self.over = true;
        }
    }

    fn hard_drop(&mut self) {
        while self.shift(0, 1) {
            self.score += 2;
        }
        self.step();
    }
}

fn best_file() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("orthodox-navigator").join("game.txt"))
}

fn load_best() -> u64 {
    best_file().and_then(|f| fs::read_to_string(f).ok()).and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

fn save_best(score: u64) {
    if let Some(f) = best_file() {
        let _ = fs::create_dir_all(f.parent().unwrap_or(std::path::Path::new(".")));
        let _ = fs::write(f, score.to_string());
    }
}

fn seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(42)
}

fn draw(s: &mut Screen, g: &Game, best: u64) {
    let t = theme::current();
    let (w, h) = (2 * W as i32 + 34, H as i32 + 4);
    let x = ((s.w - w) / 2).max(0);
    let y = ((s.h - h) / 2).max(0);
    let bg = t.dlg_bg;
    dialogs::frame_at(s, x, y, w, h, "Orthodox game");

    // the well
    let (fx, fy) = (x + 3, y + 2);
    s.boxed(fx - 1, fy - 1, 2 * W as i32 + 2, H as i32 + 2, t.dlg_frame, bg, false);
    for r in 0..H {
        for c in 0..W {
            let color = g.board[r][c];
            s.text(fx + 2 * c as i32, fy + r as i32, "  ", C::Black, color.unwrap_or(C::Black));
        }
    }
    if !g.over {
        let color = SHAPES[g.cur.kind].1;
        for (bx, by) in g.cur.blocks() {
            if by >= 0 {
                s.text(fx + 2 * bx, fy + by, "  ", C::Black, color);
            }
        }
    }

    // side boxes
    let sx = fx + 2 * W as i32 + 3;
    let sw = w - (sx - x) - 3;
    s.boxed(sx, fy - 1, sw, 5, t.dlg_frame, bg, false);
    s.text(sx + (sw - 6) / 2, fy - 1, " Info ", t.dlg_title, bg);
    s.text(sx + 2, fy, &format!("Score: {}", g.score), t.dlg_fg, bg);
    s.text(sx + 2, fy + 1, &format!("Lines: {}", g.lines), t.dlg_fg, bg);
    s.text(sx + 2, fy + 2, &format!("Level: {}", g.level), t.dlg_fg, bg);

    s.boxed(sx, fy + 4, sw, 6, t.dlg_frame, bg, false);
    s.text(sx + (sw - 6) / 2, fy + 4, " Next ", t.dlg_title, bg);
    s.fill(sx + 1, fy + 5, sw - 2, 4, ' ', C::Black, C::Black);
    let color = SHAPES[g.next].1;
    for (cx, cy) in SHAPES[g.next].0 {
        s.text(sx + (sw - 8) / 2 + 2 * cx, fy + 5 + cy + 1, "  ", C::Black, color);
    }

    s.boxed(sx, fy + 11, sw, 4, C::Yellow, bg, false);
    s.text(sx + (sw - 6) / 2, fy + 11, " Best ", t.dlg_title, bg);
    s.text(sx + 2, fy + 12, "Score: ", t.dlg_fg, bg);
    s.text(sx + 9, fy + 12, &best.max(g.score).to_string(), C::Yellow, bg);

    let status = if g.over { "GAME OVER - F2 new game" } else if g.paused { "PAUSED - F3 to go on" } else { "" };
    s.text(sx, fy + 16, status, C::Red, bg);
    s.text(sx, fy + 18, "←→ move  ↑ rotate", t.dlg_fg, bg);
    s.text(sx, fy + 19, "↓ faster  Space drop", t.dlg_fg, bg);
    s.cursor = None;
    draw_key_bar(s, &["Help", "New", "Pause", "", "", "", "", "", "", ""]);
}

pub fn show(app: &mut App) {
    let mut best = load_best();
    let mut g = Game::new(seed());
    let mut next_tick = Instant::now() + g.speed();
    loop {
        let wait = if g.over || g.paused { None } else { Some(next_tick.saturating_duration_since(Instant::now())) };
        let input = app.window_input(&mut |s| draw(s, &g, best), wait);
        match input {
            Input::Tick => {
                g.step();
                next_tick = Instant::now() + g.speed();
            }
            Input::Key(k) => match k.code {
                KeyCode::Esc => break,
                KeyCode::F(2) => {
                    g = Game::new(seed());
                    next_tick = Instant::now() + g.speed();
                }
                KeyCode::F(3) if !g.over => {
                    g.paused = !g.paused;
                    next_tick = Instant::now() + g.speed();
                }
                KeyCode::F(1) => crate::help::show(app, "game"),
                _ if g.over || g.paused => {}
                KeyCode::Left => {
                    g.shift(-1, 0);
                }
                KeyCode::Right => {
                    g.shift(1, 0);
                }
                KeyCode::Up => g.rotate(),
                KeyCode::Down => {
                    if g.shift(0, 1) {
                        g.score += 1;
                    }
                }
                KeyCode::Char(' ') => {
                    g.hard_drop();
                    next_tick = Instant::now() + g.speed();
                }
                _ => {}
            },
            _ => {}
        }
        if g.score > best {
            best = g.score;
            save_best(best);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_a_row_scores() {
        let mut g = Game::new(7);
        for c in 0..W - 1 {
            g.board[H - 1][c] = Some(C::Red);
        }
        // a vertical bar dropped into the gap on the right clears the bottom row
        g.cur = Piece { kind: 0, cells: [(0, 0), (0, 1), (0, 2), (0, 3)], x: W as i32 - 1, y: 0 };
        g.hard_drop();
        assert_eq!(g.lines, 1);
        assert!(g.board[H - 1][..W - 1].iter().all(|c| c.is_none()));
    }

    #[test]
    fn rotation_stays_inside() {
        let mut g = Game::new(3);
        g.cur = Piece::new(0);
        g.cur.x = W as i32 - 4;
        for _ in 0..4 {
            g.rotate();
            assert!(g.fits(&g.cur));
        }
    }
}
