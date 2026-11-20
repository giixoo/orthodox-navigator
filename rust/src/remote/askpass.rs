//! The askpass channel. `sftp` runs with SSH_ASKPASS pointing at ON itself (`on --askpass <prompt>`);
//! that helper passes ssh's question to the running ON over a loopback socket (with a random token),
//! ON shows its own dialog, and the answer goes back the same way. Nothing touches the disk.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::Duration;

pub const ENV: &str = "ON_ASKPASS";

/// ON's end: a loopback listener that ssh's askpass helper connects to.
pub struct Server {
    listener: TcpListener,
    token: String,
}

/// One question from ssh, waiting for its answer.
pub struct Request {
    pub prompt: String,
    stream: TcpStream,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromptKind {
    HostKey,
    Password,
    Passphrase,
    Other,
}

impl Server {
    pub fn start() -> std::io::Result<Server> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let mut b = [0u8; 16];
        getrandom::fill(&mut b).map_err(|e| std::io::Error::other(e.to_string()))?;
        let token = b.iter().map(|x| format!("{x:02x}")).collect();
        Ok(Server { listener, token })
    }

    /// The environment variable for the processes that may ask: (name, "port:token").
    pub fn env(&self) -> (String, String) {
        let port = self.listener.local_addr().map(|a| a.port()).unwrap_or(0);
        (ENV.to_string(), format!("{port}:{}", self.token))
    }

    /// A question that has arrived, if any (never blocks for long).
    pub fn poll(&mut self) -> Option<Request> {
        let (mut stream, _) = self.listener.accept().ok()?;
        stream.set_nonblocking(false).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        let mut text = String::new();
        stream.read_to_string(&mut text).ok()?;
        let (token, prompt) = text.split_once('\t')?;
        (token == self.token).then(|| Request { prompt: prompt.to_string(), stream })
    }
}

impl Request {
    /// Send the answer; None cancels (ssh then gives up on this question).
    pub fn answer(mut self, text: Option<&str>) {
        if let Some(t) = text {
            let _ = self.stream.write_all(format!("1{t}").as_bytes());
        }
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

pub fn kind(prompt: &str) -> PromptKind {
    let p = prompt.to_lowercase();
    if p.contains("continue connecting") || p.contains("fingerprint") {
        PromptKind::HostKey
    } else if p.contains("passphrase") {
        PromptKind::Passphrase
    } else if p.contains("password") {
        PromptKind::Password
    } else {
        PromptKind::Other
    }
}

/// The helper's side: send the question to ON (env value "port:token") and wait for the answer.
fn ask_with(env_value: &str, prompt: &str) -> Option<String> {
    let (port, token) = env_value.split_once(':')?;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port.parse().ok()?));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).ok()?;
    s.write_all(format!("{token}\t{prompt}").as_bytes()).ok()?;
    s.shutdown(Shutdown::Write).ok()?;
    let mut ans = String::new();
    s.read_to_string(&mut ans).ok()?;
    ans.strip_prefix('1').map(str::to_string)
}

/// `on --askpass <prompt>`: run by ssh; prints the answer for ssh to read.
pub fn helper_main(prompt: &str) -> i32 {
    let Ok(v) = std::env::var(ENV) else { return 1 };
    match ask_with(&v, prompt) {
        Some(a) => {
            println!("{a}");
            0
        }
        None => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_request(srv: &mut Server) -> Request {
        for _ in 0..500 {
            if let Some(r) = srv.poll() {
                return r;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("no request");
    }

    #[test]
    fn round_trip() {
        let mut srv = Server::start().unwrap();
        let (name, val) = srv.env();
        assert_eq!(name, "ON_ASKPASS");
        let h = std::thread::spawn(move || ask_with(&val, "The authenticity of host 'h' can't be established.\nAre you sure you want to continue connecting (yes/no/[fingerprint])? "));
        let req = wait_request(&mut srv);
        assert_eq!(kind(&req.prompt), PromptKind::HostKey);
        assert!(req.prompt.contains('\n'));
        req.answer(Some("yes"));
        assert_eq!(h.join().unwrap().as_deref(), Some("yes"));
    }

    #[test]
    fn cancel_and_empty_answer() {
        let mut srv = Server::start().unwrap();
        let (_, val) = srv.env();
        let v2 = val.clone();
        let h = std::thread::spawn(move || ask_with(&val, "u@h's password: "));
        wait_request(&mut srv).answer(None);
        assert_eq!(h.join().unwrap(), None);
        let h = std::thread::spawn(move || ask_with(&v2, "u@h's password: "));
        wait_request(&mut srv).answer(Some(""));
        assert_eq!(h.join().unwrap().as_deref(), Some(""));
    }

    #[test]
    fn wrong_token_ignored() {
        let mut srv = Server::start().unwrap();
        let (_, val) = srv.env();
        let port = val.split(':').next().unwrap().to_string();
        let h = std::thread::spawn(move || ask_with(&format!("{port}:00"), "password: "));
        for _ in 0..50 {
            assert!(srv.poll().is_none());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(h.join().unwrap(), None);
    }

    /// Live: real sftp asks through `on --askpass` (see docs/superpowers/plans/remote-test-servers.md).
    /// Needs ON_TEST_EXE (a built on.exe), ON_TEST_SFTP (host:port), ON_TEST_USER, ON_TEST_PASSWORD.
    #[test]
    #[ignore]
    fn sftp_askpass_live() {
        let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"));
        let (exe, hp, user, pw) = (env("ON_TEST_EXE"), env("ON_TEST_SFTP"), env("ON_TEST_USER"), env("ON_TEST_PASSWORD"));
        let (host, port) = hp.split_once(':').unwrap();
        let kh = std::env::temp_dir().join(format!("on-kh-{}", std::process::id()));
        let mut srv = Server::start().unwrap();
        let (k, v) = srv.env();
        let mut cmd = std::process::Command::new("sftp");
        cmd.args(["-q", "-P", port, "-o", &format!("UserKnownHostsFile={}", kh.display()), &format!("{user}@{host}")])
            .env("SSH_ASKPASS", &exe)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env(k, v);
        let mut c = crate::remote::wait::Child::spawn(cmd).unwrap();
        c.write_line("pwd").unwrap();
        for _ in 0..3 {
            c.write_line("help").unwrap();
        }
        let (mut asked, mut got) = (vec![], false);
        let t0 = std::time::Instant::now();
        while !got && t0.elapsed() < Duration::from_secs(30) {
            if let Some(r) = srv.poll() {
                let a = if kind(&r.prompt) == PromptKind::HostKey { "yes".to_string() } else { pw.clone() };
                asked.push(kind(&r.prompt));
                r.answer(Some(&a));
            }
            got = c.take_out().iter().any(|l| l.starts_with("Remote working directory"));
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_file(&kh);
        assert!(got, "no answer; asked {asked:?}; stderr {:?}", c.take_err());
        assert_eq!(asked, vec![PromptKind::HostKey, PromptKind::Password]);
    }

    #[test]
    fn kinds() {
        assert_eq!(kind("Please type 'yes', 'no' or the fingerprint: "), PromptKind::HostKey);
        assert_eq!(kind("tester@127.0.0.1's password: "), PromptKind::Password);
        assert_eq!(kind("Enter passphrase for key 'C:\\Users\\u/.ssh/id_ed25519': "), PromptKind::Passphrase);
        assert_eq!(kind("Verification code: "), PromptKind::Other);
    }
}
