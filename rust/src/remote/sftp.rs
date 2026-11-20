//! SFTP through the system `sftp`: one process per remote panel, commands written to its stdin.
//!
//! Windows' sftp holds its stdout back until 4 KB pile up, so every command is framed and flushed:
//! `#ON-<n>-A`, the command, `#ON-<n>-B`, then `help` three times. sftp echoes each line it reads as
//! `sftp> <line>`, so the answer lies between the echoes of the two markers. Errors come on stderr
//! (unbuffered) before the second marker. See the spec's "Checked" section.

use crate::remote::askpass::Server;
use crate::remote::listing::{parse_unix_ls, RemoteEntry};
use crate::remote::profiles::Profile;
use crate::remote::wait::{wait_until, Child, Ui, Waited};
use crate::remote::{Conn, R};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const TIMEOUT: Duration = Duration::from_secs(15);

/// Quote a path for an sftp command: '...' (Windows' sftp turns `\` into `/`, so double quotes can't
/// be escaped), "..." when the name holds a `'`.
pub fn sftp_quote(s: &str) -> R<String> {
    if s.contains(['\n', '\r']) {
        return Err("Names with line breaks can't be used over SFTP.".into());
    }
    match (s.contains('\''), s.contains('"')) {
        (false, _) => Ok(format!("'{s}'")),
        (true, false) => Ok(format!("\"{s}\"")),
        (true, true) => Err(format!("{s}: names with both ' and \" can't be used over SFTP.")),
    }
}

/// A local path for sftp: forward slashes (it accepts C:/...).
fn local_arg(p: &Path) -> R<String> {
    sftp_quote(&p.to_string_lossy().replace('\\', "/"))
}

pub fn sftp_args(p: &Profile, askpass: bool) -> Vec<String> {
    let mut a: Vec<String> = vec!["-q".into()];
    if let Some(port) = p.port {
        a.extend(["-P".into(), port.to_string()]);
    }
    for o in ["ConnectTimeout=15", "ServerAliveInterval=30", "NumberOfPasswordPrompts=3"] {
        a.extend(["-o".into(), o.into()]);
    }
    if !askpass {
        a.extend(["-o".into(), "BatchMode=yes".into()]);
    }
    a.push(if p.user.is_empty() { p.host.clone() } else { format!("{}@{}", p.user, p.host) });
    a
}

/// The answer to command `n` once both of its markers have arrived: the lines between them, without
/// the echo of the command. What came before is leftover `help` text; what comes after stays in
/// `lines` for the next command.
fn take_answer(lines: &mut Vec<String>, n: u64) -> Option<Vec<String>> {
    let (ma, mb) = (format!("sftp> #ON-{n}-A"), format!("sftp> #ON-{n}-B"));
    let a = lines.iter().position(|l| *l == ma)?;
    let b = a + lines[a..].iter().position(|l| *l == mb)?;
    let mut ans: Vec<String> = lines[a + 1..b].to_vec();
    if ans.first().is_some_and(|l| l.starts_with("sftp> ")) {
        ans.remove(0);
    }
    lines.drain(..=b);
    Some(ans)
}

/// Entries from `ls -la <dir>`: sftp prints names as `<dir>/<name>`.
fn parse_ls_answer(lines: &[String]) -> Vec<RemoteEntry> {
    use chrono::Datelike;
    let mut v = parse_unix_ls(&lines.join("\n"), chrono::Local::now().year());
    for e in v.iter_mut() {
        if let Some(i) = e.name.rfind('/') {
            e.name = e.name[i + 1..].to_string();
        }
        e.hidden = e.name.starts_with('.');
    }
    v.retain(|e| !e.name.is_empty() && e.name != "." && e.name != "..");
    v
}

enum Fail {
    /// the process ended (connection lost or refused)
    Died(String),
    Msg(String),
}

pub struct SftpConn {
    profile: Profile,
    server: Option<Server>,
    child: Option<Child>,
    /// lines read past the last answer (leftover help text)
    leftover: Vec<String>,
    n: u64,
    home: String,
    /// OpenSSH can ask through ON (8.4+); otherwise BatchMode (keys / agent only)
    askpass: bool,
    version: (u32, u32),
    /// tests: run this program instead of sftp
    custom: Option<(std::ffi::OsString, Vec<std::ffi::OsString>)>,
}

impl SftpConn {
    pub fn open(ui: &mut dyn Ui, p: &Profile) -> R<SftpConn> {
        let version = crate::remote::openssh()?;
        let askpass = crate::remote::askpass_supported(version);
        let server = if askpass { Some(Server::start().map_err(|e| e.to_string())?) } else { None };
        let mut c = SftpConn { profile: p.clone(), server, child: None, leftover: vec![], n: 0, home: "/".into(), askpass, version, custom: None };
        match c.connect(ui) {
            Ok(()) => Ok(c),
            Err(e) if !askpass && e != "Cancelled." => Err(format!(
                "{e}\n\nThis OpenSSH ({}.{}) can't ask for passwords inside ON. Use an SSH key or ssh-agent, connect once with \"ssh {}\" in a terminal to trust the server, or update OpenSSH (8.4 or newer).",
                version.0, version.1, p.host
            )),
            Err(e) => Err(e),
        }
    }

    #[cfg(test)]
    fn with_command(p: Profile, cmd: Command) -> SftpConn {
        let custom = Some((cmd.get_program().to_owned(), cmd.get_args().map(|a| a.to_owned()).collect()));
        SftpConn { profile: p, server: None, child: Some(Child::spawn(cmd).expect("spawn")), leftover: vec![], n: 0, home: "/".into(), askpass: false, version: (0, 0), custom }
    }

    fn command(&self) -> Command {
        if let Some((prog, args)) = &self.custom {
            let mut c = Command::new(prog);
            c.args(args);
            return c;
        }
        let mut c = Command::new("sftp");
        let mut args = sftp_args(&self.profile, self.askpass);
        #[cfg(test)]
        if let Ok(kh) = std::env::var("ON_TEST_KNOWN_HOSTS") {
            args.splice(1..1, ["-o".to_string(), format!("UserKnownHostsFile={kh}")]);
        }
        c.args(&args);
        let exe = std::env::current_exe();
        // tests run inside the test binary; the askpass helper must be a built on.exe
        #[cfg(test)]
        let exe = std::env::var("ON_TEST_EXE").map(std::path::PathBuf::from).or(exe);
        if let (Some(srv), Ok(exe)) = (&self.server, exe) {
            let (k, v) = srv.env();
            c.env("SSH_ASKPASS", exe).env("SSH_ASKPASS_REQUIRE", "force").env(k, v);
        }
        c
    }

    /// Start sftp and wait until it answers (passwords and host keys get asked meanwhile).
    fn connect(&mut self, ui: &mut dyn Ui) -> R<()> {
        self.child = Some(Child::spawn(self.command()).map_err(|e| format!("sftp: {e}"))?);
        self.leftover.clear();
        let title = format!("Connecting to {}...", self.profile.host);
        let ans = match self.exchange(ui, &title, "pwd", Some(CONNECT_TIMEOUT), false) {
            Ok(a) => a,
            Err(Fail::Died(m) | Fail::Msg(m)) => {
                self.child = None;
                return Err(m);
            }
        };
        if let Some(h) = ans.iter().find_map(|l| l.strip_prefix("Remote working directory: ")) {
            self.home = h.trim().to_string();
        }
        Ok(())
    }

    /// Run one command; a connection that was lost since the last command is made again once.
    fn run(&mut self, ui: &mut dyn Ui, title: &str, cmd: &str, timeout: Option<Duration>) -> R<Vec<String>> {
        let mut fresh = false;
        loop {
            if self.child.as_mut().is_none_or(|c| !c.is_alive()) {
                if fresh || self.custom.is_some() && self.n > 0 {
                    return Err("The connection was lost.".into());
                }
                if self.custom.is_some() {
                    let c = self.command();
                    self.child = Some(Child::spawn(c).map_err(|e| e.to_string())?);
                } else {
                    self.connect(ui)?;
                }
                fresh = true;
            }
            match self.exchange(ui, title, cmd, timeout, true) {
                Ok(v) => return Ok(v),
                Err(Fail::Died(_)) if !fresh => self.child = None,
                Err(Fail::Died(m) | Fail::Msg(m)) => return Err(m),
            }
        }
    }

    /// Send a framed command and read its answer. `errors`: stderr text means the command failed.
    fn exchange(&mut self, ui: &mut dyn Ui, title: &str, cmd: &str, timeout: Option<Duration>, errors: bool) -> Result<Vec<String>, Fail> {
        self.n += 1;
        let n = self.n;
        let child = self.child.as_mut().ok_or_else(|| Fail::Died("Not connected.".into()))?;
        child.take_err();
        let sent = [format!("#ON-{n}-A"), cmd.to_string(), format!("#ON-{n}-B"), "help".into(), "help".into(), "help".into()].iter().try_for_each(|l| child.write_line(l));
        if sent.is_err() {
            return Err(Fail::Died("The connection was lost.".into()));
        }
        let mut acc = std::mem::take(&mut self.leftover);
        let mut errs: Vec<String> = vec![];
        let profile = &self.profile;
        let res = wait_until(ui, title, timeout, self.server.as_mut().map(|s| (s, profile)), || {
            let new = child.take_out();
            errs.extend(child.take_err());
            let progressed = !new.is_empty();
            acc.extend(new);
            if let Some(ans) = take_answer(&mut acc, n) {
                return Some(Ok(Some(ans)));
            }
            if !child.is_alive() && !child.has_output() {
                return Some(Ok(None));
            }
            progressed.then_some(Err(()))
        });
        match res {
            Waited::Done(Some(ans)) => {
                // errors come on stderr just before the second marker; let late ones arrive
                std::thread::sleep(Duration::from_millis(30));
                errs.extend(child.take_err());
                self.leftover = acc;
                if errors && !errs.is_empty() {
                    return Err(Fail::Msg(errs.join("\n")));
                }
                Ok(ans)
            }
            Waited::Done(None) => {
                errs.extend(child.take_err());
                let msg = errs.iter().filter(|l| !l.trim().is_empty()).cloned().collect::<Vec<_>>().join("\n");
                self.child = None;
                Err(Fail::Died(if msg.is_empty() { "The connection was closed.".into() } else { msg }))
            }
            Waited::Cancelled => {
                self.child = None;
                Err(Fail::Msg("Cancelled.".into()))
            }
            Waited::TimedOut => {
                self.child = None;
                Err(Fail::Msg(format!("{} didn't answer in time.", self.profile.host)))
            }
        }
    }

    fn name_of(path: &str) -> &str {
        path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path)
    }
}

impl Conn for SftpConn {
    fn home(&mut self, _ui: &mut dyn Ui) -> R<String> {
        Ok(self.home.clone())
    }

    fn list(&mut self, ui: &mut dyn Ui, dir: &str) -> R<Vec<RemoteEntry>> {
        let cmd = format!("ls -la {}", sftp_quote(dir)?);
        Ok(parse_ls_answer(&self.run(ui, &format!("Reading {dir}..."), &cmd, Some(TIMEOUT))?))
    }

    fn get(&mut self, ui: &mut dyn Ui, remote: &str, local: &Path, is_dir: bool) -> R<()> {
        let cmd = format!("get {}{} {}", if is_dir { "-r " } else { "" }, sftp_quote(remote)?, local_arg(local)?);
        self.run(ui, &format!("Copying {}...", Self::name_of(remote)), &cmd, None).map(|_| ())
    }

    fn put(&mut self, ui: &mut dyn Ui, local: &Path, remote: &str, is_dir: bool) -> R<()> {
        let cmd = format!("put {}{} {}", if is_dir { "-r " } else { "" }, local_arg(local)?, sftp_quote(remote)?);
        self.run(ui, &format!("Copying {}...", Self::name_of(remote)), &cmd, None).map(|_| ())
    }

    fn mkdir(&mut self, ui: &mut dyn Ui, path: &str) -> R<()> {
        self.run(ui, "Making folder...", &format!("mkdir {}", sftp_quote(path)?), Some(TIMEOUT)).map(|_| ())
    }

    fn remove(&mut self, ui: &mut dyn Ui, path: &str, is_dir: bool) -> R<()> {
        if is_dir {
            for e in self.list(ui, path)? {
                let p = crate::remote::join(path, &e.name);
                self.remove(ui, &p, e.is_dir && !e.is_link)?;
            }
            return self.run(ui, &format!("Deleting {}...", Self::name_of(path)), &format!("rmdir {}", sftp_quote(path)?), Some(TIMEOUT)).map(|_| ());
        }
        self.run(ui, &format!("Deleting {}...", Self::name_of(path)), &format!("rm {}", sftp_quote(path)?), Some(TIMEOUT)).map(|_| ())
    }

    fn rename(&mut self, ui: &mut dyn Ui, from: &str, to: &str) -> R<()> {
        self.run(ui, "Renaming...", &format!("rename {} {}", sftp_quote(from)?, sftp_quote(to)?), Some(TIMEOUT)).map(|_| ())
    }

    fn close(&mut self) {
        if let Some(c) = self.child.as_mut() {
            let _ = c.write_line("bye");
        }
        self.child = None;
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::remote::profiles::Proto;
    use crate::remote::{join, parent};

    pub struct TestUi {
        pub password: String,
    }

    impl Ui for TestUi {
        fn ask(&mut self, _p: &Profile, prompt: &str) -> Option<String> {
            Some(if crate::remote::askpass::kind(prompt) == crate::remote::askpass::PromptKind::HostKey { "yes".into() } else { self.password.clone() })
        }
    }

    fn prof(host: &str, port: Option<u16>, user: &str) -> Profile {
        Profile { name: "t".into(), proto: Proto::Sftp, host: host.into(), port, user: user.into(), folder: String::new(), save_password: false, password: None, insecure: false }
    }

    #[test]
    fn quoting_and_paths() {
        assert_eq!(sftp_quote("a b").unwrap(), "'a b'");
        assert_eq!(sftp_quote(r#"q"uote"#).unwrap(), r#"'q"uote'"#);
        assert_eq!(sftp_quote("it's").unwrap(), "\"it's\"");
        assert_eq!(sftp_quote("-rf").unwrap(), "'-rf'");
        assert!(sftp_quote("a'\"b").is_err());
        assert_eq!(join("/", "x"), "/x");
        assert_eq!(join("/a/", "b c"), "/a/b c");
        assert_eq!(join("/a", "b"), "/a/b");
        assert_eq!(parent("/a/b"), Some(("/a".into(), "b".into())));
        assert_eq!(parent("/a"), Some(("/".into(), "a".into())));
        assert_eq!(parent("/"), None);
    }

    #[test]
    fn args_and_version() {
        let a = sftp_args(&prof("h", Some(2222), "u"), false);
        assert!(a.windows(2).any(|w| w == ["-P", "2222"]));
        assert!(a.contains(&"BatchMode=yes".to_string()));
        assert_eq!(a.last().unwrap(), "u@h");
        let b = sftp_args(&prof("alias", None, ""), true);
        assert!(!b.contains(&"-P".to_string()) && !b.contains(&"BatchMode=yes".to_string()));
        assert_eq!(b.last().unwrap(), "alias");
        assert_eq!(crate::remote::parse_ssh_version("OpenSSH_for_Windows_9.5p2, LibreSSL 3.8.2"), Some((9, 5)));
        assert_eq!(crate::remote::parse_ssh_version("OpenSSH_8.2p1 Ubuntu-4ubuntu0.5, OpenSSL 1.1.1f"), Some((8, 2)));
        assert_eq!(crate::remote::parse_ssh_version("nonsense"), None);
    }

    #[test]
    fn framing() {
        let mut lines: Vec<String> = ["Available commands:", "bye   Quit sftp", "sftp> #ON-7-A", "sftp> ls -la '/upload'", "drwx r 1 u g 1 Nov 20 09:56 /upload/x y", "sftp> #ON-7-B", "Available commands:"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(take_answer(&mut lines, 8), None); // waits for its own markers
        let ans = take_answer(&mut lines, 7).unwrap();
        assert_eq!(ans, vec!["drwx r 1 u g 1 Nov 20 09:56 /upload/x y".to_string()]);
        assert_eq!(lines, vec!["Available commands:".to_string()]); // leftovers stay for the next skip
        let mut partial: Vec<String> = vec!["sftp> #ON-9-A".into(), "sftp> pwd".into()];
        assert_eq!(take_answer(&mut partial, 9), None);
    }

    #[test]
    fn ls_answer_names() {
        let lines: Vec<String> = ["drwx******    ? 1001     100          4096 Nov 20 09:56 /upload/.", "drwx******    ? 0        0            4096 Nov 20 09:52 /upload/..", "drwx******    ? 1001     1001         4096 Nov 20 09:57 /upload/x y", "-rw-******    ? 1001     1001            6 Nov 20 09:57 /upload/q\"uote.txt"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let e = parse_ls_answer(&lines);
        assert_eq!(e.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["x y", "q\"uote.txt"]);
    }

    #[test]
    fn big_folder_parses_fast() {
        let lines: Vec<String> = (0..10_000).map(|i| format!("-rw-r--r--    1 1001     1001         {i} Nov 20 09:57 /big/file {i}.txt")).collect();
        let t0 = std::time::Instant::now();
        let e = parse_ls_answer(&lines);
        assert_eq!(e.len(), 10_000);
        assert_eq!(e[9_999].name, "file 9999.txt");
        assert!(t0.elapsed() < Duration::from_millis(200), "{:?}", t0.elapsed());
    }

    #[test]
    fn dead_process_is_reported() {
        let mut cmd = Command::new(if cfg!(windows) { "cmd" } else { "sh" });
        cmd.args(if cfg!(windows) { vec!["/c", "exit 1"] } else { vec!["-c", "exit 1"] });
        let mut c = SftpConn::with_command(prof("h", None, ""), cmd);
        let t0 = std::time::Instant::now();
        let r = c.list(&mut TestUi { password: String::new() }, "/");
        assert!(r.is_err());
        assert!(t0.elapsed() < Duration::from_secs(5), "took {:?}", t0.elapsed());
    }

    /// Live: every operation against the test server (docs/superpowers/plans/remote-test-servers.md).
    #[test]
    #[ignore]
    fn sftp_live() {
        let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"));
        let (hp, user, pw) = (env("ON_TEST_SFTP"), env("ON_TEST_USER"), env("ON_TEST_PASSWORD"));
        std::env::set_var("ON_TEST_KNOWN_HOSTS", std::env::temp_dir().join(format!("on-kh-live-{}", std::process::id())));
        let (host, port) = hp.split_once(':').unwrap();
        let mut ui = TestUi { password: pw };
        let mut c = SftpConn::open(&mut ui, &prof(host, port.parse().ok(), &user)).unwrap();
        let home = c.home(&mut ui).unwrap();
        assert!(home.starts_with('/'));
        let base = "/upload/on live #%";
        let _ = c.remove(&mut ui, base, true);
        c.mkdir(&mut ui, base).unwrap();
        let tmp = std::env::temp_dir().join(format!("on-live-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("tree/sub")).unwrap();
        std::fs::write(tmp.join("tree/sub/Документ #1.txt"), "hello").unwrap();
        std::fs::write(tmp.join("a b.txt"), "x").unwrap();
        c.put(&mut ui, &tmp.join("a b.txt"), &join(base, "a b.txt"), false).unwrap();
        c.put(&mut ui, &tmp.join("tree"), &join(base, "tree"), true).unwrap();
        let names: Vec<String> = c.list(&mut ui, base).unwrap().into_iter().map(|e| e.name).collect();
        assert!(names.contains(&"a b.txt".to_string()) && names.contains(&"tree".to_string()), "{names:?}");
        c.rename(&mut ui, &join(base, "a b.txt"), &join(base, "c \"d\".txt")).unwrap();
        assert!(c.list(&mut ui, base).unwrap().iter().any(|e| e.name == "c \"d\".txt"));
        c.get(&mut ui, &join(base, "tree"), &tmp.join("back"), true).unwrap();
        assert_eq!(std::fs::read_to_string(tmp.join("back/sub/Документ #1.txt")).unwrap(), "hello");
        assert!(c.list(&mut ui, "/nope").is_err());
        c.remove(&mut ui, base, true).unwrap();
        assert!(!c.list(&mut ui, "/upload").unwrap().iter().any(|e| e.name == "on live #%"));
        c.close();
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
