//! Remote panels: folders on other machines over SFTP (the system `sftp`) and FTP / FTPS (the system
//! `curl`). See docs/superpowers/specs/2026-11-19-remote-panels-design.md.


pub mod askpass;
pub mod ftp;
pub mod listing;
pub mod profiles;
pub mod vault;
pub mod sftp;
pub mod ui;
pub mod wait;

use crate::app::App;
use listing::RemoteEntry;
use profiles::{Profile, Proto};
use std::path::Path;
use wait::Ui;

/// Err holds the server's own text (or "Cancelled.").
pub type R<T> = Result<T, String>;

/// A connection to a server; paths are POSIX paths on the server.
pub trait Conn {
    /// The folder the server starts in.
    fn home(&mut self, ui: &mut dyn Ui) -> R<String>;
    fn list(&mut self, ui: &mut dyn Ui, dir: &str) -> R<Vec<RemoteEntry>>;
    /// Download a file or (is_dir) a whole folder to `local`.
    fn get(&mut self, ui: &mut dyn Ui, remote: &str, local: &Path, is_dir: bool) -> R<()>;
    /// Upload a file or (is_dir) a whole folder to `remote`.
    fn put(&mut self, ui: &mut dyn Ui, local: &Path, remote: &str, is_dir: bool) -> R<()>;
    fn mkdir(&mut self, ui: &mut dyn Ui, path: &str) -> R<()>;
    /// Delete a file, or a folder with everything in it.
    fn remove(&mut self, ui: &mut dyn Ui, path: &str, is_dir: bool) -> R<()>;
    fn rename(&mut self, ui: &mut dyn Ui, from: &str, to: &str) -> R<()>;
    fn close(&mut self);
}

/// A panel showing a folder on a server.
pub struct RemoteView {
    /// the profile's name
    pub profile: String,
    /// the folder shown, a path on the server
    pub cwd: String,
    pub conn: Box<dyn Conn>,
    /// "sftp://user@host:port" (the folder is added for the title)
    pub base: String,
}

impl RemoteView {
    pub fn title(&self) -> String {
        format!("{}{}", self.base, if self.cwd.starts_with('/') { self.cwd.clone() } else { format!("/{}", self.cwd) })
    }
}

/// Connect with a profile.
pub fn connect(ui: &mut dyn Ui, p: &Profile) -> R<Box<dyn Conn>> {
    match p.proto {
        Proto::Sftp => Ok(Box::new(sftp::SftpConn::open(ui, p)?)),
        Proto::Ftp | Proto::Ftps | Proto::FtpsImplicit => Ok(Box::new(ftp::FtpConn::open(ui, p)?)),
    }
}

/// `dir` + `name` with one `/` between.
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// (parent folder, name); None for the root.
pub fn parent(path: &str) -> Option<(String, String)> {
    let p = path.trim_end_matches('/');
    let i = p.rfind('/')?;
    let up = if i == 0 { "/".to_string() } else { p[..i].to_string() };
    Some((up, p[i + 1..].to_string()))
}

/// "OpenSSH_8.2p1 ..." / "OpenSSH_for_Windows_9.5p2, ..." -> (8, 2) / (9, 5).
pub fn parse_ssh_version(text: &str) -> Option<(u32, u32)> {
    let i = text.find("OpenSSH_")? + "OpenSSH_".len();
    let rest = text[i..].trim_start_matches("for_Windows_");
    let mut it = rest.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty());
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// ssh can send its questions to ON (SSH_ASKPASS_REQUIRE) from 8.4.
pub fn askpass_supported(v: (u32, u32)) -> bool {
    v >= (8, 4)
}

/// The installed OpenSSH version.
pub fn openssh() -> R<(u32, u32)> {
    let mut cmd = std::process::Command::new("ssh");
    cmd.arg("-V");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|_| "OpenSSH (ssh, sftp) was not found. It comes with Windows 10 and later, macOS and most Linux; on Linux install the openssh-client package.".to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
    parse_ssh_version(&text).ok_or_else(|| format!("Unknown ssh version: {}", text.trim()))
}

/// Answer a question from ssh (password, passphrase, "trust this host?") for a profile.
pub fn answer_prompt(app: &mut App, profile: &Profile, prompt: &str) -> Option<String> {
    ui::answer(app, profile, prompt)
}

/// Where remote files are downloaded to view or edit them (removed when ON quits for good).
pub fn temp_dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("on-remote-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_floor() {
        assert!(super::askpass_supported((8, 4)));
        assert!(super::askpass_supported((9, 5)));
        assert!(!super::askpass_supported((8, 2)));
    }
}
