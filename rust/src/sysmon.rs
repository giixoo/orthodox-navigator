//! Alt+F5: system monitor. CPU load (total, history, per core), memory and swap, network rates,
//! disks, and the busiest processes. Refreshes every second.

use crate::app::{draw_key_bar, App, Input};
use crate::dialogs;
use crate::screen::{Screen, C};
use crate::theme;
use crate::util::{fit, human, pad_left};
use crate::viewer::{draw_close_mark, is_close_mark};
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use std::cell::Cell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use sysinfo::{Disks, Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const EVERY: Duration = Duration::from_secs(1);
const HISTORY: usize = 300;
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

struct Proc {
    pid: Pid,
    name: String,
    cpu: f32,
    mem: u64,
}

struct Iface {
    name: String,
    down: f64,
    up: f64,
    total_down: u64,
    total_up: u64,
}

struct Mon {
    sys: System,
    nets: Networks,
    disks: Disks,
    last: Instant,
    cpu_hist: VecDeque<f32>,
    down_hist: VecDeque<f64>,
    up_hist: VecDeque<f64>,
    ifaces: Vec<Iface>,
    procs: Vec<Proc>,
    by_mem: bool,
    cursor: usize,
    top: usize,
    host: String,
    os: String,
    cpu_name: String,
    cores: Option<usize>,
    /// process rows that fit on the screen, as last drawn
    rows: Cell<usize>,
}

fn push<T>(q: &mut VecDeque<T>, v: T) {
    if q.len() == HISTORY {
        q.pop_front();
    }
    q.push_back(v);
}

impl Mon {
    fn new() -> Option<Mon> {
        let mut sys = System::new().ok()?;
        sys.refresh_cpu_all();
        sys.refresh_memory();
        let cpu_name = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
        let mut m = Mon {
            sys,
            nets: Networks::new_with_refreshed_list().ok()?,
            disks: Disks::new_with_refreshed_list().ok()?,
            last: Instant::now(),
            cpu_hist: VecDeque::new(),
            down_hist: VecDeque::new(),
            up_hist: VecDeque::new(),
            ifaces: vec![],
            procs: vec![],
            by_mem: false,
            cursor: 0,
            top: 0,
            host: System::host_name().unwrap_or_default(),
            os: System::long_os_version().unwrap_or_default(),
            cpu_name,
            cores: System::physical_core_count().ok(),
            rows: Cell::new(10),
        };
        m.refresh_procs();
        Some(m)
    }

    fn refresh_procs(&mut self) {
        self.sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cpu().with_memory());
        let n = self.sys.cpus().len().max(1) as f32;
        let selected = self.procs.get(self.cursor).map(|p| p.pid);
        self.procs = self
            .sys
            .processes()
            .values()
            .map(|p| Proc { pid: p.pid(), name: p.name().to_string_lossy().into_owned(), cpu: p.cpu_usage() / n, mem: p.memory() })
            .collect();
        self.sort();
        // keep the cursor on the same process
        if let Some(pid) = selected {
            if let Some(i) = self.procs.iter().position(|p| p.pid == pid) {
                self.cursor = i;
            }
        }
        self.cursor = self.cursor.min(self.procs.len().saturating_sub(1));
    }

    fn sort(&mut self) {
        if self.by_mem {
            self.procs.sort_by(|a, b| b.mem.cmp(&a.mem).then(a.name.cmp(&b.name)));
        } else {
            self.procs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.mem.cmp(&a.mem)));
        }
    }

    fn refresh(&mut self) {
        let secs = self.last.elapsed().as_secs_f64().max(0.1);
        self.last = Instant::now();
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.nets.refresh(true);
        self.disks.refresh(true);
        self.refresh_procs();
        push(&mut self.cpu_hist, self.sys.global_cpu_usage());
        let mut ifaces: Vec<Iface> = self
            .nets
            .iter()
            .filter(|(_, d)| d.total_received() + d.total_transmitted() > 0)
            .map(|(n, d)| Iface {
                name: n.to_string_lossy().into_owned(),
                down: d.received() as f64 / secs,
                up: d.transmitted() as f64 / secs,
                total_down: d.total_received(),
                total_up: d.total_transmitted(),
            })
            .collect();
        ifaces.sort_by_key(|i| std::cmp::Reverse(i.total_down + i.total_up));
        push(&mut self.down_hist, ifaces.iter().map(|i| i.down).sum());
        push(&mut self.up_hist, ifaces.iter().map(|i| i.up).sum());
        self.ifaces = ifaces;
    }
}

fn rate(v: f64) -> String {
    format!("{}/s", human(v as u64))
}

fn uptime_text(s: u64) -> String {
    let (d, h, m) = (s / 86400, s / 3600 % 24, s / 60 % 60);
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else {
        format!("{h}h {m}m")
    }
}

/// A bar `w` wide, filled to `frac` (0..1); the colour turns yellow, then red, as it fills.
fn bar(s: &mut Screen, x: i32, y: i32, w: i32, frac: f64) {
    let t = theme::current();
    let frac = frac.clamp(0.0, 1.0);
    let fill = (frac * w as f64).round() as i32;
    let color = if frac >= 0.9 { C::LightRed } else if frac >= 0.7 { C::Yellow } else { C::LightGreen };
    for i in 0..w.max(0) {
        let (ch, fg) = if i < fill { ('█', color) } else { ('░', t.frame) };
        s.put(x + i, y, ch, fg, t.panel_bg);
    }
}

/// The last `w` values as a one-line chart scaled to `max`.
fn spark(s: &mut Screen, x: i32, y: i32, w: i32, vals: impl DoubleEndedIterator<Item = f64> + ExactSizeIterator, max: f64, fg: C) {
    let t = theme::current();
    let n = vals.len();
    let skip = n.saturating_sub(w.max(0) as usize);
    let start = x + w - (n - skip) as i32;
    for (i, v) in vals.skip(skip).enumerate() {
        let k = if max <= 0.0 { 0 } else { ((v / max) * 7.0).round().clamp(0.0, 7.0) as usize };
        s.put(start + i as i32, y, SPARK[k], fg, t.panel_bg);
    }
}

fn label(s: &mut Screen, x: i32, y: i32, text: &str) {
    let t = theme::current();
    s.text(x, y, text, t.header, t.panel_bg);
}

fn draw(s: &mut Screen, m: &Mon) {
    let t = theme::current();
    let (w, h) = (s.w, s.h);
    let (bg, fg) = (t.panel_bg, t.file);
    s.cursor = None;
    s.fill(0, 0, w, h, ' ', fg, bg);

    // title line
    s.fill(0, 0, w, 1, ' ', t.bar_fg, t.bar_bg);
    draw_close_mark(s);
    let title = format!("System monitor   {}   {}   up {}", m.host, m.os, uptime_text(System::uptime().unwrap_or(0)));
    s.text(4, 0, &fit(&title, (w - 5).max(0) as usize), t.bar_fg, t.bar_bg);

    let x0 = 1;
    let iw = w - 2;
    let mut y = 2;

    // ---- CPU
    let cpu = m.sys.global_cpu_usage();
    let cpus = m.sys.cpus();
    label(s, x0, y, "CPU");
    s.text(x0 + 5, y, &format!("{cpu:5.1}%"), fg, bg);
    let bw = (iw / 3).max(10);
    bar(s, x0 + 13, y, bw, cpu as f64 / 100.0);
    let freq = cpus.first().map(|c| c.frequency()).unwrap_or(0);
    let mut info = m.cpu_name.clone();
    match m.cores {
        Some(c) => info += &format!("  {c} cores, {} threads", cpus.len()),
        None => info += &format!("  {} threads", cpus.len()),
    }
    if freq > 0 {
        info += &format!("  {:.2} GHz", freq as f64 / 1000.0);
    }
    if let Ok(l) = System::load_average().ok().filter(|l| l.one > 0.0).ok_or(()) {
        info += &format!("  load {:.2} {:.2} {:.2}", l.one, l.five, l.fifteen);
    }
    let ix = x0 + 14 + bw;
    s.text(ix, y, &fit(&info, (w - 1 - ix).max(0) as usize), t.info, bg);
    y += 1;
    s.text(x0, y, "100%", t.frame, bg);
    spark(s, x0 + 5, y, iw - 5, m.cpu_hist.iter().map(|&v| v as f64), 100.0, C::LightGreen);
    y += 1;
    // per core, as many columns of 18 as fit
    let per_col = 18;
    let ncol = (iw / per_col).max(1) as usize;
    for (i, c) in cpus.iter().enumerate() {
        let (cx, cy) = (x0 + (i % ncol) as i32 * per_col, y + (i / ncol) as i32);
        s.text(cx, cy, &format!("{:>2}", i), t.frame, bg);
        bar(s, cx + 3, cy, 8, c.usage() as f64 / 100.0);
        s.text(cx + 12, cy, &format!("{:>3.0}%", c.usage()), fg, bg);
    }
    y += cpus.len().div_ceil(ncol) as i32 + 1;

    // ---- memory
    let (total, used, avail, free) = (m.sys.total_memory(), m.sys.used_memory(), m.sys.available_memory(), m.sys.free_memory());
    label(s, x0, y, "RAM");
    let frac = if total > 0 { used as f64 / total as f64 } else { 0.0 };
    s.text(x0 + 5, y, &format!("{:5.1}%", frac * 100.0), fg, bg);
    bar(s, x0 + 13, y, bw, frac);
    let text = format!("used {} of {}   available {}   free {}", human(used), human(total), human(avail), human(free));
    s.text(ix, y, &fit(&text, (w - 1 - ix).max(0) as usize), t.info, bg);
    y += 1;
    let (st, su) = (m.sys.total_swap(), m.sys.used_swap());
    if st > 0 {
        label(s, x0, y, "Swap");
        let frac = su as f64 / st as f64;
        s.text(x0 + 5, y, &format!("{:5.1}%", frac * 100.0), fg, bg);
        bar(s, x0 + 13, y, bw, frac);
        s.text(ix, y, &fit(&format!("used {} of {}", human(su), human(st)), (w - 1 - ix).max(0) as usize), t.info, bg);
        y += 1;
    }
    y += 1;

    // ---- network
    let down: f64 = m.ifaces.iter().map(|i| i.down).sum();
    let up: f64 = m.ifaces.iter().map(|i| i.up).sum();
    label(s, x0, y, "Net");
    s.text(x0 + 5, y, &format!("↓ {:>10}  ↑ {:>10}", rate(down), rate(up)), fg, bg);
    let peak = m.down_hist.iter().chain(m.up_hist.iter()).cloned().fold(1.0, f64::max);
    let sx = x0 + 32;
    let sw = ((w - 1 - sx) / 2 - 1).max(0);
    if sw > 4 {
        spark(s, sx, y, sw, m.down_hist.iter().copied(), peak, C::LightCyan);
        spark(s, sx + sw + 1, y, sw, m.up_hist.iter().copied(), peak, C::LightMagenta);
    }
    y += 1;
    for i in m.ifaces.iter().take(4) {
        let line = format!(
            "  {}  ↓ {:>10}  ↑ {:>10}   total ↓ {}  ↑ {}",
            fit(&i.name, 24),
            rate(i.down),
            rate(i.up),
            human(i.total_down),
            human(i.total_up)
        );
        s.text(x0, y, &fit(&line, iw as usize), fg, bg);
        y += 1;
    }
    y += 1;

    // ---- disks
    label(s, x0, y, "Disks");
    y += 1;
    for d in m.disks.iter().take(6) {
        let (tot, av) = (d.total_space(), d.available_space());
        if tot == 0 {
            continue;
        }
        let used = tot - av.min(tot);
        let mp = d.mount_point().to_string_lossy();
        s.text(x0 + 2, y, &fit(&mp, 10), fg, bg);
        bar(s, x0 + 13, y, bw, used as f64 / tot as f64);
        let text = format!("{:>3.0}%  used {} of {}   free {}", used as f64 * 100.0 / tot as f64, human(used), human(tot), human(av));
        s.text(ix, y, &fit(&text, (w - 1 - ix).max(0) as usize), t.info, bg);
        y += 1;
    }
    y += 1;

    // ---- processes (the rest of the screen)
    let head = format!("{:>8}  {:>6}  {:>9}  Name", "PID", "CPU", "Memory");
    label(s, x0, y, &fit(&format!("Processes ({})  sorted by {}", m.procs.len(), if m.by_mem { "memory" } else { "CPU" }), iw as usize));
    y += 1;
    s.text(x0, y, &fit(&head, iw as usize), t.frame, bg);
    y += 1;
    let rows = (h - 1 - y).max(0) as usize;
    m.rows.set(rows.max(1));
    for (r, p) in m.procs.iter().enumerate().skip(m.top).take(rows) {
        let cur = r == m.cursor;
        let (pf, pb) = if cur { (t.cursor_fg, t.cursor_bg) } else { (fg, bg) };
        let line = format!("{:>8}  {:>5.1}%  {:>9}  {}", p.pid.as_u32(), p.cpu, pad_left(&human(p.mem), 9), p.name);
        s.text(x0, y, &fit(&line, iw as usize), pf, pb);
        y += 1;
    }

    draw_key_bar(s, &["Help", "", "", "", "", if m.by_mem { "ByCPU" } else { "ByMem" }, "", "Kill", "", ""]);
}

pub fn show(app: &mut App) {
    let Some(mut m) = Mon::new() else {
        return dialogs::message(app, "System monitor", "Can't read system information on this computer.", true);
    };
    // CPU usage needs two samples a little apart; take the first one soon
    let mut next = Instant::now() + Duration::from_millis(300);
    loop {
        let wait = next.saturating_duration_since(Instant::now());
        let mr = &m;
        let inp = app.window_input(&mut |s| draw(s, mr), Some(wait));
        let rows = m.rows.get();
        match inp {
            Input::Tick => {
                m.refresh();
                next = Instant::now() + EVERY;
            }
            Input::Key(k) => match k.code {
                KeyCode::Esc => return,
                KeyCode::Up => m.cursor = m.cursor.saturating_sub(1),
                KeyCode::Down => m.cursor += 1,
                KeyCode::PageUp => m.cursor = m.cursor.saturating_sub(rows),
                KeyCode::PageDown => m.cursor += rows,
                KeyCode::Home => m.cursor = 0,
                KeyCode::End => m.cursor = m.procs.len(),
                KeyCode::F(6) => {
                    m.by_mem = !m.by_mem;
                    m.sort();
                    m.cursor = 0;
                }
                KeyCode::F(8) => {
                    if let Some(p) = m.procs.get(m.cursor) {
                        let (pid, name) = (p.pid, p.name.clone());
                        let q = format!("Kill process {name} (PID {})?", pid.as_u32());
                        if dialogs::choice(app, "Kill", &[&q, "Unsaved work in it will be lost."], &["Kill", "Cancel"], true) == Some(0) {
                            let ok = m.sys.process(pid).is_some_and(|p| p.kill());
                            if !ok {
                                dialogs::message(app, "Kill", &format!("Couldn't stop {name}: it may need administrator rights."), true);
                            }
                            m.refresh();
                            next = Instant::now() + EVERY;
                        }
                    }
                }
                KeyCode::F(1) => crate::help::show(app, "sysmon"),
                _ => {}
            },
            Input::Mouse(mo) => match mo.kind {
                MouseEventKind::Down(MouseButton::Left) if is_close_mark(mo.column, mo.row) => return,
                MouseEventKind::ScrollUp => m.cursor = m.cursor.saturating_sub(3),
                MouseEventKind::ScrollDown => m.cursor += 3,
                _ => {}
            },
            Input::Paste(_) => {}
        }
        m.cursor = m.cursor.min(m.procs.len().saturating_sub(1));
        if m.cursor < m.top {
            m.top = m.cursor;
        } else if m.cursor >= m.top + rows {
            m.top = m.cursor + 1 - rows;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_this_computer() {
        let mut m = Mon::new().expect("sysinfo works here");
        std::thread::sleep(Duration::from_millis(300));
        m.refresh();
        assert!(m.sys.total_memory() > 0);
        assert!(!m.sys.cpus().is_empty());
        assert!(!m.procs.is_empty());
        assert_eq!(m.cpu_hist.len(), 1);
        println!(
            "cpu {:.1}% ({}), ram {} of {}, {} procs, {} ifaces, {} disks",
            m.sys.global_cpu_usage(),
            m.cpu_name,
            human(m.sys.used_memory()),
            human(m.sys.total_memory()),
            m.procs.len(),
            m.ifaces.len(),
            m.disks.iter().count()
        );
    }
}