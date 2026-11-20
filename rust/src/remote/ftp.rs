//! FTP and FTPS through the system `curl`: one curl run per operation. Secrets go only through
//! `curl -K -` (config text on stdin), never on the command line.
//!
//! Paths are relative to the folder the server logs in to, shown as "/..." (curl's ftp://host/x is
//! x in the login folder; most servers lock users in it anyway).

use crate::remote::listing::{parse_list, parse_mlsd, RemoteEntry};
use crate::remote::profiles::{Profile, Proto};
use crate::remote::wait::{wait_until, Child, Ui, Waited};
use crate::remote::{Conn, R};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);

/// The start of the error when the server's TLS certificate can't be verified (the panel then offers
/// "Connect anyway").
pub const CERT_ERR: &str = "This server's certificate can't be verified.";

/// Percent-encode everything but unreserved characters and `/`.
fn encode_path(path: &str) -> String {
    let mut s = String::new();
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// The URL of a path (relative to the login folder); a trailing `/` for folders.
pub fn url(p: &Profile, path: &str, dir: bool) -> String {
    let scheme = if p.proto == Proto::FtpsImplicit { "ftps" } else { "ftp" };
    let port = p.port.map(|n| format!(":{n}")).unwrap_or_default();
    let mut rel = path.trim_start_matches('/').to_string();
    if dir && !rel.is_empty() && !rel.ends_with('/') {
        rel.push('/');
    }
    format!("{scheme}://{}{port}/{}", p.host, encode_path(&rel))
}

/// A path for a raw FTP command (-Q): relative to the login folder, not encoded.
pub fn quote_path(path: &str) -> String {
    path.trim_start_matches('/').to_string()
}

pub fn base_args(p: &Profile) -> Vec<String> {
    let mut a: Vec<String> = ["-s", "-S", "--connect-timeout", "15", "-K", "-"].iter().map(|s| s.to_string()).collect();
    if p.proto == Proto::Ftps {
        a.push("--ssl-reqd".into());
    }
    if p.insecure {
        a.push("-k".into());
    }
    a
}

/// curl config text with the login (empty user = anonymous: no line).
pub fn config_text(user: &str, password: Option<&str>) -> String {
    if user.is_empty() {
        return String::new();
    }
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("user = \"{}:{}\"\n", esc(user), esc(password.unwrap_or("")))
}

#[derive(Debug, PartialEq)]
pub enum CurlFail {
    Login,
    Cert,
    NotFound,
    Other(String),
}

pub fn classify(code: i32, stderr: &str) -> CurlFail {
    let low = stderr.to_lowercase();
    let text = stderr.lines().map(|l| l.trim_start_matches("curl: ").trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
    let text = text.split_once(") ").filter(|(c, _)| c.starts_with('(')).map(|(_, t)| t.to_string()).unwrap_or(text);
    match code {
        67 => CurlFail::Login,
        60 => CurlFail::Cert,
        35 if low.contains("certificate") || low.contains("sec_e_untrusted_root") || low.contains("revocation") => CurlFail::Cert,
        9 | 78 => CurlFail::NotFound,
        18 if low.contains("522") => CurlFail::Other(
            "The server requires TLS session reuse (vsftpd's require_ssl_reuse), which this curl can't do. The server's admin can set require_ssl_reuse=NO.".into(),
        ),
        _ if low.contains("failed with 550") || low.contains("response: 550") => CurlFail::Other("The server refused: 550".into()),
        _ if code == 7 || code == 28 => CurlFail::Other(format!("Can't connect: {text}")),
        _ => CurlFail::Other(if text.is_empty() { format!("curl failed ({code}).") } else { text }),
    }
}

fn curl_program() -> PathBuf {
    // Windows' own curl rather than another one on PATH
    if cfg!(windows) {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let p = PathBuf::from(root).join("System32").join("curl.exe");
            if p.exists() {
                return p;
            }
        }
    }
    PathBuf::from("curl")
}

pub struct FtpConn {
    profile: Profile,
    /// the password for this session (asked, or the saved one)
    password: Option<String>,
    /// does the server know MLSD? (learned on the first listing)
    mlsd: Option<bool>,
    /// curl's error text of the last failed run
    last_err: String,
}

impl FtpConn {
    pub fn open(ui: &mut dyn Ui, p: &Profile) -> R<FtpConn> {
        let mut c = FtpConn { profile: p.clone(), password: None, mlsd: None, last_err: String::new() };
        if !p.user.is_empty() {
            c.password = Some(c.ask_password(ui)?);
        }
        c.list(ui, "/")?;
        Ok(c)
    }

    fn ask_password(&mut self, ui: &mut dyn Ui) -> R<String> {
        let prompt = format!("{}@{}'s password: ", self.profile.user, self.profile.host);
        ui.ask(&self.profile, &prompt).ok_or_else(|| "Cancelled.".to_string())
    }

    /// Run curl with these arguments; a wrong password is asked again (3 times at most).
    fn run(&mut self, ui: &mut dyn Ui, title: &str, args: &[String], timeout: Option<Duration>) -> R<Vec<String>> {
        let mut tries = 0;
        loop {
            match self.run_once(ui, title, args, timeout)? {
                Ok(out) => return Ok(out),
                Err(CurlFail::Login) if !self.profile.user.is_empty() && tries < 2 => {
                    tries += 1;
                    self.password = Some(self.ask_password(ui)?);
                }
                Err(CurlFail::Login) => return Err("The server didn't accept the user name or password.".into()),
                Err(CurlFail::Cert) => return Err(format!("{CERT_ERR}\n{}", self.last_err)),
                Err(CurlFail::NotFound) => return Err("Not found.".into()),
                Err(CurlFail::Other(m)) => return Err(m),
            }
        }
    }

    fn run_once(&mut self, ui: &mut dyn Ui, title: &str, args: &[String], timeout: Option<Duration>) -> R<Result<Vec<String>, CurlFail>> {
        let mut cmd = Command::new(curl_program());
        cmd.args(base_args(&self.profile)).args(args);
        let mut child = Child::spawn(cmd).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "curl was not found. It comes with Windows 10 and later and macOS; on Linux install the curl package.".to_string()
            } else {
                format!("curl: {e}")
            }
        })?;
        child.write_all_and_close(&config_text(&self.profile.user, self.password.as_deref())).map_err(|e| e.to_string())?;
        let (mut out, mut err) = (vec![], vec![]);
        let res = wait_until(ui, title, timeout, None, || {
            out.extend(child.take_out());
            err.extend(child.take_err());
            child.exit_code().map(Ok)
        });
        let code = match res {
            Waited::Done(code) => code,
            Waited::Cancelled => return Err("Cancelled.".into()),
            Waited::TimedOut => return Err(format!("{} didn't answer in time.", self.profile.host)),
        };
        // the reader threads may still hold the last lines
        std::thread::sleep(Duration::from_millis(20));
        out.extend(child.take_out());
        err.extend(child.take_err());
        if code == 0 {
            return Ok(Ok(out));
        }
        self.last_err = err.join("\n");
        Ok(Err(classify(code, &self.last_err)))
    }

    fn quote(&mut self, ui: &mut dyn Ui, title: &str, cmds: &[String]) -> R<()> {
        let mut args = vec![];
        for c in cmds {
            args.extend(["-Q".to_string(), c.clone()]);
        }
        args.extend(["-o".to_string(), if cfg!(windows) { "NUL".into() } else { "/dev/null".into() }, url(&self.profile, "/", true)]);
        self.run(ui, title, &args, Some(TIMEOUT)).map(|_| ())
    }

    fn name_of(path: &str) -> &str {
        path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path)
    }
}

impl Conn for FtpConn {
    fn home(&mut self, _ui: &mut dyn Ui) -> R<String> {
        Ok("/".into())
    }

    fn list(&mut self, ui: &mut dyn Ui, dir: &str) -> R<Vec<RemoteEntry>> {
        use chrono::Datelike;
        let title = format!("Reading {dir}...");
        let u = url(&self.profile, dir, true);
        if self.mlsd != Some(false) {
            match self.run(ui, &title, &["-X".into(), "MLSD".into(), u.clone()], Some(TIMEOUT)) {
                Ok(lines) => {
                    self.mlsd = Some(true);
                    return Ok(parse_mlsd(&lines.join("\n")));
                }
                // the server doesn't know MLSD: LIST from now on
                Err(e) if self.mlsd.is_none() && (e.contains("500") || e.contains("502") || e.contains("refused")) => self.mlsd = Some(false),
                Err(e) => return Err(if e == "Not found." { format!("{dir}: no such folder.") } else { e }),
            }
        }
        let lines = self.run(ui, &title, &[u], Some(TIMEOUT)).map_err(|e| if e == "Not found." { format!("{dir}: no such folder.") } else { e })?;
        Ok(parse_list(&lines.join("\n"), chrono::Local::now().year()))
    }

    fn get(&mut self, ui: &mut dyn Ui, remote: &str, local: &Path, is_dir: bool) -> R<()> {
        if is_dir {
            std::fs::create_dir_all(local).map_err(|e| format!("{}: {e}", local.display()))?;
            for e in self.list(ui, remote)? {
                self.get(ui, &crate::remote::join(remote, &e.name), &local.join(&e.name), e.is_dir)?;
            }
            return Ok(());
        }
        let title = format!("Copying {}...", Self::name_of(remote));
        let args = ["-o".to_string(), local.to_string_lossy().to_string(), url(&self.profile, remote, false)];
        self.run(ui, &title, &args, None).map(|_| ())
    }

    fn put(&mut self, ui: &mut dyn Ui, local: &Path, remote: &str, is_dir: bool) -> R<()> {
        if is_dir {
            // an existing folder is fine
            let _ = self.mkdir(ui, remote);
            let rd = std::fs::read_dir(local).map_err(|e| format!("{}: {e}", local.display()))?;
            for de in rd.flatten() {
                let name = de.file_name().to_string_lossy().to_string();
                let sub = de.path();
                self.put(ui, &sub, &crate::remote::join(remote, &name), sub.is_dir())?;
            }
            return Ok(());
        }
        let title = format!("Copying {}...", Self::name_of(remote));
        let args = ["--ftp-create-dirs".to_string(), "-T".into(), local.to_string_lossy().to_string(), url(&self.profile, remote, false)];
        self.run(ui, &title, &args, None).map(|_| ())
    }

    fn mkdir(&mut self, ui: &mut dyn Ui, path: &str) -> R<()> {
        self.quote(ui, "Making folder...", &[format!("MKD {}", quote_path(path))])
    }

    fn remove(&mut self, ui: &mut dyn Ui, path: &str, is_dir: bool) -> R<()> {
        let title = format!("Deleting {}...", Self::name_of(path));
        if is_dir {
            for e in self.list(ui, path)? {
                self.remove(ui, &crate::remote::join(path, &e.name), e.is_dir && !e.is_link)?;
            }
            return self.quote(ui, &title, &[format!("RMD {}", quote_path(path))]);
        }
        self.quote(ui, &title, &[format!("DELE {}", quote_path(path))])
    }

    fn rename(&mut self, ui: &mut dyn Ui, from: &str, to: &str) -> R<()> {
        self.quote(ui, "Renaming...", &[format!("RNFR {}", quote_path(from)), format!("RNTO {}", quote_path(to))])
    }

    fn close(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prof(proto: Proto, port: Option<u16>) -> Profile {
        Profile { name: "t".into(), proto, host: "h".into(), port, user: "u".into(), folder: String::new(), save_password: false, password: None, insecure: false }
    }

    #[test]
    fn url_encoding() {
        let p = prof(Proto::Ftp, None);
        assert_eq!(url(&p, "/a b/#1%?.txt", false), "ftp://h/a%20b/%231%25%3F.txt");
        assert_eq!(url(&p, "/Документы", true), "ftp://h/%D0%94%D0%BE%D0%BA%D1%83%D0%BC%D0%B5%D0%BD%D1%82%D1%8B/");
        assert_eq!(url(&prof(Proto::FtpsImplicit, Some(990)), "/", true), "ftps://h:990/");
        assert_eq!(url(&p, "/", true), "ftp://h/");
        assert_eq!(quote_path("/sub/a b.txt"), "sub/a b.txt");
    }

    #[test]
    fn secrets_stay_off_the_command_line() {
        let mut p = prof(Proto::Ftps, None);
        let args = base_args(&p).join(" ");
        assert!(args.contains("--ssl-reqd") && args.contains("-K -"));
        assert!(!args.contains("-k ") && !args.ends_with("-k"));
        p.insecure = true;
        assert!(base_args(&p).contains(&"-k".to_string()));
        assert!(!base_args(&prof(Proto::Ftp, None)).contains(&"--ssl-reqd".to_string()));
        assert_eq!(config_text("u", Some("p\"w\\")), "user = \"u:p\\\"w\\\\\"\n");
        assert_eq!(config_text("u", None), "user = \"u:\"\n");
        assert_eq!(config_text("", None), "");
    }

    #[test]
    fn errors() {
        assert_eq!(classify(67, "curl: (67) Access denied: 530"), CurlFail::Login);
        assert_eq!(classify(60, "curl: (60) schannel: SEC_E_UNTRUSTED_ROOT (0x80090325) - The certificate chain was issued by an authority that is not trusted."), CurlFail::Cert);
        assert_eq!(classify(35, "curl: (35) SSL certificate problem: self-signed certificate"), CurlFail::Cert);
        assert_eq!(classify(9, "curl: (9) Server denied you to change to the given directory"), CurlFail::NotFound);
        assert!(matches!(classify(18, "curl: (18) server did not report OK, got 522"), CurlFail::Other(m) if m.contains("require_ssl_reuse")));
        assert_eq!(classify(21, "curl: (21) QUOT command failed with 550"), CurlFail::Other("The server refused: 550".into()));
    }

    /// Live: FTP and FTPS test servers (docs/superpowers/plans/remote-test-servers.md).
    #[test]
    #[ignore]
    fn ftp_live() {
        let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"));
        for (var, proto) in [("ON_TEST_FTP", Proto::Ftp), ("ON_TEST_FTPS", Proto::Ftps)] {
            let hp = env(var);
            let (host, port) = hp.split_once(':').unwrap();
            let mut p = Profile { name: "t".into(), proto, host: host.into(), port: port.parse().ok(), user: env("ON_TEST_USER"), folder: String::new(), save_password: false, password: None, insecure: false };
            let mut ui = crate::remote::sftp::tests::TestUi { password: env("ON_TEST_PASSWORD") };
            if proto == Proto::Ftps {
                let e = FtpConn::open(&mut ui, &p).err().expect("self-signed must fail");
                assert!(e.starts_with(CERT_ERR), "{e}");
                p.insecure = true;
            }
            let mut c = FtpConn::open(&mut ui, &p).unwrap();
            let base = "/on live #%";
            let _ = c.remove(&mut ui, base, true);
            c.mkdir(&mut ui, base).unwrap();
            let tmp = std::env::temp_dir().join(format!("on-ftp-live-{}", std::process::id()));
            std::fs::create_dir_all(tmp.join("tree/sub")).unwrap();
            std::fs::write(tmp.join("tree/sub/Документ #1.txt"), "hello").unwrap();
            std::fs::write(tmp.join("a b.txt"), "x").unwrap();
            c.put(&mut ui, &tmp.join("a b.txt"), &crate::remote::join(base, "a b.txt"), false).unwrap();
            c.put(&mut ui, &tmp.join("tree"), &crate::remote::join(base, "tree"), true).unwrap();
            let names: Vec<String> = c.list(&mut ui, base).unwrap().into_iter().map(|e| e.name).collect();
            assert!(names.contains(&"a b.txt".to_string()) && names.contains(&"tree".to_string()), "{names:?}");
            c.rename(&mut ui, &crate::remote::join(base, "a b.txt"), &crate::remote::join(base, "c d?.txt")).unwrap();
            c.get(&mut ui, &crate::remote::join(base, "tree"), &tmp.join("back"), true).unwrap();
            assert_eq!(std::fs::read_to_string(tmp.join("back/sub/Документ #1.txt")).unwrap(), "hello");
            assert!(c.list(&mut ui, "/nope").is_err());
            c.remove(&mut ui, base, true).unwrap();
            assert!(!c.list(&mut ui, "/").unwrap().iter().any(|e| e.name == "on live #%"));
            let _ = std::fs::remove_dir_all(&tmp);
        }
    }
}
