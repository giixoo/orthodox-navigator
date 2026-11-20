//! Remote connection profiles: <config>/orthodox-navigator/remote.txt. Edited from the drive list
//! (Alt+F1 / Alt+F2), not by hand, because saved passwords are encrypted (see vault.rs).

use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Proto {
    Sftp,
    Ftp,
    /// FTP with TLS asked for after connecting (`ftp://` + `--ssl-reqd`), usually port 21
    Ftps,
    /// FTP with TLS from the start (`ftps://`), usually port 990
    FtpsImplicit,
}

impl Proto {
    pub const ALL: [Proto; 4] = [Proto::Sftp, Proto::Ftp, Proto::Ftps, Proto::FtpsImplicit];

    pub fn name(self) -> &'static str {
        match self {
            Proto::Sftp => "sftp",
            Proto::Ftp => "ftp",
            Proto::Ftps => "ftps",
            Proto::FtpsImplicit => "ftps-implicit",
        }
    }

    pub fn parse(s: &str) -> Option<Proto> {
        Proto::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(s.trim()))
    }

    /// The scheme shown in panel titles.
    pub fn scheme(self) -> &'static str {
        match self {
            Proto::Sftp => "sftp",
            Proto::Ftp => "ftp",
            Proto::Ftps | Proto::FtpsImplicit => "ftps",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: String,
    pub proto: Proto,
    /// for SFTP also a ~/.ssh/config alias
    pub host: String,
    /// None = the protocol's default (or ~/.ssh/config's for SFTP)
    pub port: Option<u16>,
    /// empty = ssh's default user / anonymous FTP
    pub user: String,
    /// where to start; empty = the server's home folder
    pub folder: String,
    pub save_password: bool,
    /// the saved password, encrypted (vault::seal), base64
    pub password: Option<String>,
    /// FTPS: don't check the server's certificate
    pub insecure: bool,
}

/// "sftp://user@host:port/path" for the panel title.
pub fn title(p: &Profile, path: &str) -> String {
    let user = if p.user.is_empty() { String::new() } else { format!("{}@", p.user) };
    let port = p.port.map(|n| format!(":{n}")).unwrap_or_default();
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    format!("{}://{user}{}{port}{path}", p.proto.scheme(), p.host)
}

#[derive(Default, Debug, PartialEq)]
pub struct Profiles {
    /// (salt, check value), both base64; None until a password is saved the first time
    pub vault: Option<(String, String)>,
    pub list: Vec<Profile>,
}

const HEADER: &str = "# Orthodox Navigator: remote connections (edit them from Alt+F1 / Alt+F2).\n";

fn yes(v: &str) -> bool {
    matches!(v.trim().to_lowercase().as_str(), "yes" | "1" | "true")
}

pub fn parse(text: &str) -> Profiles {
    let mut out = Profiles::default();
    // a profile being read: its name and key = value pairs
    let mut cur: Option<(String, Vec<(String, String)>)> = None;
    fn finish(cur: Option<(String, Vec<(String, String)>)>, out: &mut Profiles) {
        let Some((name, kv)) = cur else { return };
        let get = |k: &str| kv.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str()).unwrap_or("");
        let Some(proto) = Proto::parse(get("proto")) else { return };
        out.list.push(Profile {
            name,
            proto,
            host: get("host").to_string(),
            port: get("port").parse().ok(),
            user: get("user").to_string(),
            folder: get("folder").to_string(),
            save_password: yes(get("save")),
            password: Some(get("password").to_string()).filter(|s| !s.is_empty()),
            insecure: yes(get("insecure")),
        });
    }
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') && t.ends_with(']') {
            finish(cur.take(), &mut out);
            cur = Some((t[1..t.len() - 1].to_string(), vec![]));
            continue;
        }
        let Some((k, v)) = t.split_once('=') else { continue };
        let (k, v) = (k.trim().to_lowercase(), v.trim().to_string());
        match cur.as_mut() {
            Some((_, kv)) => kv.push((k, v)),
            None if k == "vault" => out.vault = v.split_once(':').map(|(a, b)| (a.to_string(), b.to_string())),
            None => {}
        }
    }
    finish(cur, &mut out);
    out
}

pub fn format(p: &Profiles) -> String {
    let mut s = HEADER.to_string();
    if let Some((salt, check)) = &p.vault {
        s += &format!("vault = {salt}:{check}\n");
    }
    for pr in &p.list {
        let yn = |b: bool| if b { "yes" } else { "no" };
        s += &format!(
            "\n[{}]\nproto = {}\nhost = {}\nport = {}\nuser = {}\nfolder = {}\nsave = {}\npassword = {}\ninsecure = {}\n",
            pr.name,
            pr.proto.name(),
            pr.host,
            pr.port.map(|n| n.to_string()).unwrap_or_default(),
            pr.user,
            pr.folder,
            yn(pr.save_password),
            pr.password.as_deref().unwrap_or(""),
            yn(pr.insecure),
        );
    }
    s
}

pub fn file() -> Option<PathBuf> {
    crate::usermenu::config_dir().map(|d| d.join("remote.txt"))
}

pub fn load() -> Profiles {
    file().and_then(|f| fs::read_to_string(f).ok()).map(|t| parse(&t)).unwrap_or_default()
}

pub fn save(p: &Profiles) -> std::io::Result<()> {
    let f = file().ok_or_else(|| std::io::Error::other("no config folder"))?;
    if let Some(d) = f.parent() {
        fs::create_dir_all(d)?;
    }
    write_private(&f, format(p).as_bytes())
}

/// Write a file only the user can read (0600 on Unix; on Windows the user's profile folder already is).
pub fn write_private(f: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        // a fresh file, so one made beforehand (by anyone) can't keep its own owner or mode
        let _ = fs::remove_file(f);
        let mut fh = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(f)?;
        fh.write_all(data)
    }
    #[cfg(not(unix))]
    {
        fs::write(f, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Profiles {
        Profiles {
            vault: Some(("c2FsdA==".into(), "Y2hlY2s=".into())),
            list: vec![
                Profile {
                    name: "Work server".into(),
                    proto: Proto::Sftp,
                    host: "example.org".into(),
                    port: None,
                    user: String::new(),
                    folder: "/var/www".into(),
                    save_password: false,
                    password: None,
                    insecure: false,
                },
                Profile {
                    name: "NAS [home] = 1".into(),
                    proto: Proto::FtpsImplicit,
                    host: "10.0.0.5".into(),
                    port: Some(990),
                    user: "me".into(),
                    folder: String::new(),
                    save_password: true,
                    password: Some("AAEC".into()),
                    insecure: true,
                },
            ],
        }
    }

    #[test]
    fn profiles_round_trip() {
        let p = sample();
        assert_eq!(parse(&format(&p)), p);
    }

    #[test]
    fn titles_and_protocols() {
        let mut p = sample().list[0].clone();
        p.user = "deploy".into();
        assert_eq!(title(&p, "/var/www"), "sftp://deploy@example.org/var/www");
        assert_eq!(title(&sample().list[1], "/"), "ftps://me@10.0.0.5:990/");
        assert_eq!(Proto::parse("ftps-implicit"), Some(Proto::FtpsImplicit));
    }

    #[test]
    fn missing_proto_skips_profile() {
        let p = parse("[a]\nhost = h\n\n[b]\nproto = ftp\nhost = h2\nunknown = 1\n");
        assert_eq!(p.list.len(), 1);
        assert_eq!(p.list[0].name, "b");
    }
}
