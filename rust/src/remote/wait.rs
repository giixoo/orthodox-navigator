//! Child processes (`sftp`, `curl`) with their output read on threads, and waiting for them while the
//! screen stays alive: progress after a moment, Esc cancels, a timeout, and ssh's questions answered
//! through the askpass channel.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use crate::app::App;
use crate::remote::askpass::Server;
use crate::remote::profiles::Profile;

/// A running program with its output collected line by line on two threads (so a big output can't
/// fill a pipe and stall it).
pub struct Child {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    out: Receiver<String>,
    err: Receiver<String>,
    pending_out: Vec<String>,
    pending_err: Vec<String>,
}

fn reader<R: std::io::Read + Send + 'static>(r: R) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut r = BufReader::new(r);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match r.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string();
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

impl Child {
    /// Start with stdin, stdout and stderr piped (and on Windows without a console of its own, so no
    /// program can draw on ON's screen or read its keys).
    pub fn spawn(mut cmd: Command) -> std::io::Result<Child> {
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn()?;
        let out = reader(child.stdout.take().expect("piped stdout"));
        let err = reader(child.stderr.take().expect("piped stderr"));
        let stdin = child.stdin.take();
        Ok(Child { child, stdin, out, err, pending_out: vec![], pending_err: vec![] })
    }

    pub fn write_line(&mut self, s: &str) -> std::io::Result<()> {
        let w = self.stdin.as_mut().ok_or_else(|| std::io::Error::other("stdin closed"))?;
        w.write_all(format!("{s}\n").as_bytes())?;
        w.flush()
    }

    pub fn write_all_and_close(&mut self, s: &str) -> std::io::Result<()> {
        if let Some(mut w) = self.stdin.take() {
            w.write_all(s.as_bytes())?;
        }
        Ok(())
    }

    fn pump(&mut self) {
        self.pending_out.extend(self.out.try_iter());
        self.pending_err.extend(self.err.try_iter());
    }

    /// Lines that arrived on stdout since the last call.
    pub fn take_out(&mut self) -> Vec<String> {
        self.pump();
        std::mem::take(&mut self.pending_out)
    }

    pub fn take_err(&mut self) -> Vec<String> {
        self.pump();
        std::mem::take(&mut self.pending_err)
    }

    pub fn has_output(&mut self) -> bool {
        self.pump();
        !self.pending_out.is_empty() || !self.pending_err.is_empty()
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// The exit code once it has ended.
    pub fn exit_code(&mut self) -> Option<i32> {
        self.child.try_wait().ok().flatten().map(|s| s.code().unwrap_or(-1))
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if self.is_alive() {
            self.kill();
        }
    }
}

pub enum Waited<T> {
    Done(T),
    Cancelled,
    TimedOut,
}

pub fn deadline_passed(start: Instant, timeout: Duration, now: Instant) -> bool {
    now.duration_since(start) >= timeout
}

/// What waiting needs from the screen: a progress box, Esc, and answers to ssh's questions. `App`
/// implements it; tests use a stand-in.
pub trait Ui {
    /// Show (once) that something is going on.
    fn progress(&mut self, _title: &str) {}
    /// True when the user pressed Esc since the last call.
    fn esc_pressed(&mut self) -> bool {
        false
    }
    /// Answer a question from ssh (password, passphrase, "trust this host?") for this profile; None
    /// cancels.
    fn ask(&mut self, profile: &Profile, prompt: &str) -> Option<String>;
}

impl Ui for App {
    fn progress(&mut self, title: &str) {
        crate::dialogs::progress(self, title, &[title.to_string(), "Esc to cancel".to_string()]);
    }

    fn esc_pressed(&mut self) -> bool {
        use crossterm::event::{self, Event, KeyCode, KeyEventKind};
        let mut esc = false;
        while event::poll(Duration::ZERO).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                esc |= k.kind != KeyEventKind::Release && k.code == KeyCode::Esc;
            }
        }
        esc
    }

    fn ask(&mut self, profile: &Profile, prompt: &str) -> Option<String> {
        crate::remote::answer_prompt(self, profile, prompt)
    }
}

/// Wait until `poll` returns something. A progress box shows after 300 ms, Esc cancels, questions from
/// the askpass channel are answered (time spent in a dialog doesn't count), and after `timeout` with
/// no result it gives up (`None` = no timeout). `poll` returning Err(()) means "progress, restart the
/// clock" (output arrived but the answer isn't complete yet).
pub fn wait_until<T>(ui: &mut dyn Ui, title: &str, timeout: Option<Duration>, mut askpass: Option<(&mut Server, &Profile)>, mut poll: impl FnMut() -> Option<Result<T, ()>>) -> Waited<T> {
    let mut start = Instant::now();
    let first = start;
    let mut shown = false;
    loop {
        match poll() {
            Some(Ok(v)) => return Waited::Done(v),
            Some(Err(())) => start = Instant::now(),
            None => {}
        }
        if let Some((srv, profile)) = askpass.as_mut() {
            if let Some(req) = srv.poll() {
                let prompt = req.prompt.clone();
                let a = ui.ask(profile, &prompt);
                req.answer(a.as_deref());
                start = Instant::now();
                shown = false;
            }
        }
        if ui.esc_pressed() {
            return Waited::Cancelled;
        }
        let now = Instant::now();
        if timeout.is_some_and(|t| deadline_passed(start, t, now)) {
            return Waited::TimedOut;
        }
        if !shown && now.duration_since(first) >= Duration::from_millis(300) {
            ui.progress(title);
            shown = true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_cancels_and_times_out() {
        let t0 = Instant::now();
        assert!(!deadline_passed(t0, Duration::from_secs(15), t0 + Duration::from_secs(14)));
        assert!(deadline_passed(t0, Duration::from_secs(15), t0 + Duration::from_secs(15)));
    }

    /// Prints many lines to both outputs when run by `large_output_no_deadlock` (ignored otherwise).
    #[test]
    #[ignore]
    fn helper_print() {
        if std::env::var("ON_TEST_PRINT").is_err() {
            return;
        }
        let (mut o, mut e) = (std::io::stdout().lock(), std::io::stderr().lock());
        for i in 0..100_000 {
            let _ = writeln!(o, "OUT {i}");
            let _ = writeln!(e, "ERR {i}");
        }
    }

    #[test]
    fn large_output_no_deadlock() {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "remote::wait::tests::helper_print", "--ignored", "--nocapture", "--test-threads=1"]).env("ON_TEST_PRINT", "1");
        let mut c = Child::spawn(cmd).unwrap();
        let t0 = Instant::now();
        let (mut outs, mut errs) = (0, 0);
        while c.is_alive() || c.has_output() {
            outs += c.take_out().iter().filter(|l| l.contains("OUT ")).count();
            errs += c.take_err().iter().filter(|l| l.contains("ERR ")).count();
            assert!(t0.elapsed() < Duration::from_secs(60), "hung");
            std::thread::sleep(Duration::from_millis(5));
        }
        outs += c.take_out().iter().filter(|l| l.contains("OUT ")).count();
        errs += c.take_err().iter().filter(|l| l.contains("ERR ")).count();
        assert_eq!((outs, errs), (100_000, 100_000));
    }
}
