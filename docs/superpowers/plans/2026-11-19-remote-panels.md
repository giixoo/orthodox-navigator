# Remote Panels (SFTP, FTP) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A panel can browse and work on a folder on another machine over SFTP (system `sftp`) or FTP/FTPS (system `curl`), opened from saved connection profiles in the drive list, with saved passwords encrypted under a master password.

**Architecture:** A new module tree `rust/src/remote/` holds everything remote: listing parsers, the profile file, the password vault, the askpass channel, and two connection types behind one `Conn` trait (`SftpConn` drives one long-lived `sftp` process; `FtpConn` runs one `curl` per operation). A panel gets `remote: Option<RemoteView>`; `App` does the network calls (they need dialogs) by taking the view out of the panel, calling the connection, and putting it back. File operations in `fileops.rs` branch on remote panels the same way they branch on archives today.

**Tech Stack:** Rust 2021, crossterm 0.28, system OpenSSH `sftp`/`ssh` and `curl`; new crates `pbkdf2` and `chacha20poly1305` (pure Rust); `windows-sys` feature `Win32_Security_Cryptography` for DPAPI.

**Spec:** `docs/superpowers/specs/2026-11-19-remote-panels-design.md` (read it first; this plan argues from it).

## Global Constraints

- No new C dependencies; static builds (Linux musl, macOS, Windows) must keep building. New crates: `pbkdf2`, `chacha20poly1305` only.
- No password, passphrase or key in plain text on a command line, in an environment variable given to `curl`, in the session file, in a log or an error message, or in a file on disk (the askpass channel is a loopback socket, not files). The only exception is the unlock key file (DPAPI-wrapped on Windows, 0600 elsewhere).
- Every network wait can be cancelled with Esc; the screen never freezes.
- ssh/curl prompts never draw on ON's screen: `sftp` runs with stdin/stdout/stderr piped, `SSH_ASKPASS=<on.exe> --askpass`, `SSH_ASKPASS_REQUIRE=force`; with OpenSSH < 8.4, `-o BatchMode=yes` instead.
- `curl` secrets go only through `curl -K -` (config text on stdin): `user = "name:password"`.
- Protocols: `sftp`; `ftp`; `ftps` = `ftp://` + `--ssl-reqd` (port 21); `ftps-implicit` = `ftps://` (port 990).
- PBKDF2-HMAC-SHA256, 300 000 rounds, 16-byte salt; ChaCha20-Poly1305 with a random 12-byte nonce per value.
- FTP file names are UTF-8 only.
- Follow the project's style: user-facing text in plain short sentences; config files in `crate::usermenu::config_dir()`; dialogs through `dialogs::*`, `picklist::pick`, `app.window_input`; tests as `#[cfg(test)] mod tests` in the same file.
- Windows edits: files use LF; when patching with scripts keep backslash literals intact (use the Edit tool for `'\\'`).

## Review Focus

1. **Odd names**: spaces, `#`, `%`, `?`, quotes, non-ASCII and names starting with `-` in remote paths must list, download, upload, rename and delete correctly (curl URL percent-encoding; sftp double-quote escaping). Tests in Task 6 (`sftp_quote`) and Task 7 (`url_encoding`).
2. **A dropped connection** (sftp process exited, server restarted, FTP control connection refused) reconnects once on the next action and then reports the error; no hang. Test in Task 6 (`dead_process_is_reported`).
3. **A server that never answers** (packets dropped): connect gives up after 15 s with a message, and Esc cancels sooner. Test in Task 5 (`wait_cancels_and_times_out`).
4. **A big folder** (10 000 entries) lists without deadlock: stdout and stderr of child processes are read on their own threads. Test in Task 6 (`large_output_no_deadlock`).
5. **Wrong passwords**: a wrong typed password is asked again at most 3 times; a wrong master password 3 times falls back to asking the server password; no loop. Tests in Task 4 (`wrong_master_detected`) and Task 8 (manual check list item).

---

### Task 1: Test servers and protocol check (no product code)

Confirm the two risky mechanisms on this machine before building on them, and set up servers the later tasks test against.

**Files:**
- Modify: `docs/superpowers/specs/2026-11-19-remote-panels-design.md` (append a "Checked" section with findings)
- Create: `docs/superpowers/plans/remote-test-servers.md` (how to start / stop the test servers)

- [ ] **Step 1: Ask the user before installing anything.** Options to propose: (1) containers in the existing podman machine (`docker.io/atmoz/sftp` for SFTP with password and key users, `docker.io/delfer/alpine-ftp-server` or similar for FTP, plus a vsftpd with a self-signed cert for FTPS); (2) `openssh-server` and `vsftpd` inside WSL Ubuntu via apt; (3) the user points at servers they have. Use what the user picks.
- [ ] **Step 2: Start an SFTP server** with a password user and a key user on `127.0.0.1:2222`.
- [ ] **Step 3: Check sftp over pipes with askpass.** From PowerShell, with `SSH_ASKPASS` pointing to a `.cmd` that logs `%*` and prints the password, and `SSH_ASKPASS_REQUIRE=force`, run `sftp -P 2222 -o UserKnownHostsFile=<scratch>\kh user@127.0.0.1` with stdin/stdout/stderr redirected (a small Python `subprocess` script is fine). Record: (a) the exact host-key prompt text, (b) the password prompt text, (c) what sftp writes to stdout for `pwd`, `ls -la`, `cd /nope`, `get`, `put` when stdin is a pipe (does it echo `sftp> cmd` lines? where do errors go?), (d) whether output ends with a newline before the next command is read.
  Expected: askpass is called for both prompts; `pwd` prints `Remote working directory: /...`.
- [ ] **Step 4: Start an FTP and an FTPS (self-signed) server** and check with `curl`: `curl -s -S -K - ftp://127.0.0.1:2121/` (LIST), `curl -X MLSD ...`, `-Q "MKD x"`, `-T file`, and the exit code and message for a self-signed cert (expect 60) and a bad login (expect 67). On Windows also record what Schannel says for a self-signed cert.
- [ ] **Step 5: Write down the findings** in the spec's new "Checked" section and the start/stop commands in `remote-test-servers.md`. If askpass does not work through `sftp` on Windows, stop and report to the user.
- [ ] **Step 6: Commit**

```bash
git add docs/superpowers
git commit -m "docs: remote panels protocol check and test servers"
```

---

### Task 2: Listing parsers

**Files:**
- Create: `rust/src/remote/mod.rs` (module root: `pub mod listing;` plus later modules), `rust/src/remote/listing.rs`
- Modify: `rust/src/main.rs` (`mod remote;`)

**Interfaces:**
- Produces:
  - `pub struct RemoteEntry { pub name: String, pub is_dir: bool, pub is_link: bool, pub size: Option<u64>, pub modified: Option<chrono::DateTime<chrono::Local>>, pub hidden: bool, pub exec: bool }`
  - `pub fn parse_unix_ls(text: &str, now_year: i32) -> Vec<RemoteEntry>` — `ls -la` / FTP `LIST` Unix format; skips `.`/`..` and `total N`; `l` entries: name before ` -> `, `is_link = true`, `is_dir = false` (resolved later by trying to enter); "Mon DD HH:MM" means `now_year` (or the year before if that is in the future); `hidden` = name starts with `.`; `exec` = any `x` bit on a file.
  - `pub fn parse_dos_list(text: &str) -> Vec<RemoteEntry>` — `MM-DD-YY  HH:MMAM  <DIR>  name` / size format (IIS).
  - `pub fn parse_mlsd(text: &str) -> Vec<RemoteEntry>` — `type=file;size=N;modify=YYYYMMDDHHMMSS; name`; skips `type=cdir`/`pdir`; `modify` is UTC.
  - `pub fn parse_list(text: &str, now_year: i32) -> Vec<RemoteEntry>` — picks Unix or DOS by the first non-empty line.

- [ ] **Step 1: Write the failing tests** in `listing.rs`:

```rust
#[test]
fn unix_ls() {
    let t = "total 12\n\
drwxr-xr-x    2 user  grp   4096 Jan  5 10:30 my dir\n\
-rw-r--r--    1 user  grp  12345 Mar  3  2021 report 2021.pdf\n\
-rwxr-xr-x    1 user  grp     10 Nov 18 09:00 run.sh\n\
lrwxrwxrwx    1 user  grp      7 Nov 18 09:00 cur -> /var/x\n\
-rw-r--r--    1 user  grp      0 Nov 18 09:00 .hidden\n\
drwxr-xr-x    5 user  grp   4096 Nov 18 09:00 .\n";
    let e = parse_unix_ls(t, 2026);
    assert_eq!(e.len(), 5);
    assert!(e[0].is_dir && e[0].name == "my dir");
    assert_eq!(e[1].size, Some(12345));
    assert_eq!(e[1].modified.unwrap().format("%Y").to_string(), "2021");
    assert!(e[2].exec);
    assert!(e[3].is_link && e[3].name == "cur");
    assert!(e[4].hidden);
}

#[test]
fn dos_and_mlsd() {
    let d = parse_dos_list("11-18-26  09:00AM       <DIR>          Docs\n11-18-26  10:15PM              1234 a b.txt\n");
    assert!(d[0].is_dir && d[0].name == "Docs");
    assert_eq!((d[1].name.as_str(), d[1].size), ("a b.txt", Some(1234)));
    let m = parse_mlsd("type=cdir; .\ntype=dir;modify=20261118090000; src\ntype=file;size=5;modify=20261118090000; a;b.txt\n");
    assert_eq!(m.len(), 2);
    assert_eq!(m[1].name, "a;b.txt");
    assert_eq!(m[1].size, Some(5));
}
```

- [ ] **Step 2: Run them to see them fail**: `cargo test --release listing` → compile error (functions missing).
- [ ] **Step 3: Implement the four functions** in `listing.rs`. Names keep inner and trailing spaces exactly (split the fixed fields by whitespace, then take the rest of the line after the time/year field as the name).
- [ ] **Step 4: Run** `cargo test --release listing` → PASS.
- [ ] **Step 5: Commit** `git commit -m "remote: listing parsers"` (add `rust/src/remote`, `rust/src/main.rs`).

---

### Task 3: Profiles file

**Files:**
- Create: `rust/src/remote/profiles.rs`

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, PartialEq, Debug)] pub enum Proto { Sftp, Ftp, Ftps, FtpsImplicit }` with `pub fn name(self) -> &'static str` (`"sftp"`, `"ftp"`, `"ftps"`, `"ftps-implicit"`), `pub fn parse(s: &str) -> Option<Proto>`, `pub fn default_port(self) -> u16` (22, 21, 21, 990), `pub fn scheme(self) -> &'static str` (`"sftp"`, `"ftp"`, `"ftps"`, `"ftps"` — for titles).
  - `#[derive(Clone, Debug, PartialEq)] pub struct Profile { pub name: String, pub proto: Proto, pub host: String, pub port: Option<u16>, pub user: String, pub folder: String, pub save_password: bool, pub password: Option<String> /* encrypted, base64 */, pub insecure: bool }`
  - `pub fn title(p: &Profile, path: &str) -> String` → `"sftp://user@host/path"` (no `user@` when user is empty; `:port` only when set).
  - `#[derive(Default, Debug, PartialEq)] pub struct Profiles { pub vault: Option<(String, String)> /* salt b64, check b64 */, pub list: Vec<Profile> }`
  - `pub fn parse(text: &str) -> Profiles`, `pub fn format(p: &Profiles) -> String`, `pub fn file() -> Option<PathBuf>` (`remote.txt`), `pub fn load() -> Profiles`, `pub fn save(p: &Profiles) -> std::io::Result<()>` (0600 on Unix).
- File format (fixed):

```
# Orthodox Navigator: remote connections (edit them from Alt+F1 / Alt+F2).
vault = <salt b64>:<check b64>

[Work server]
proto = sftp
host = example.org
port =
user = deploy
folder = /var/www
save = yes
password = <b64>
insecure = no
```

- [ ] **Step 1: Write the failing test** `profiles_round_trip`: build `Profiles` with a vault and two profiles (one sftp with empty port/user, one `ftps-implicit` with port 990, `insecure = true`, a name containing `]` and `=`), `assert_eq!(parse(&format(&p)), p)`; also `assert_eq!(title(&sftp, "/var/www"), "sftp://deploy@example.org/var/www")` and a missing `proto` line makes `parse` skip that profile.
- [ ] **Step 2: Run** `cargo test --release profiles` → fails to compile.
- [ ] **Step 3: Implement.** Section header is the rest of the line between the first `[` and the last `]`. Unknown keys are ignored (forward compatible).
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `git commit -m "remote: connection profiles file"`.

---

### Task 4: Password vault and unlock file

**Files:**
- Create: `rust/src/remote/vault.rs`
- Modify: `rust/Cargo.toml` (add `pbkdf2 = { version = "0.12", default-features = false, features = ["hmac"] }`, `chacha20poly1305 = "0.10"`; add `"Win32_Security_Cryptography"` to the `windows-sys` features)

**Interfaces:**
- Consumes: `Profiles.vault` (Task 3); `crate::util::base64` (encode) and `crate::tools::base64_decode` (decode).
- Produces:
  - `pub type Key = [u8; 32];`
  - `pub fn new_vault(master: &str) -> (Key, (String, String))` — random salt, key, check value (= `seal(key, "orthodox-navigator")`).
  - `pub fn unlock(master: &str, vault: &(String, String)) -> Option<Key>` — `None` when the check value doesn't open.
  - `pub fn seal(key: &Key, plain: &str) -> String` — base64(nonce ‖ ciphertext).
  - `pub fn open(key: &Key, sealed: &str) -> Option<String>`.
  - `pub fn rekey(old: &Key, new: &Key, profiles: &mut Profiles)` — re-encrypts every saved password (used by "Change master password").
  - `pub fn unlock_file(session_file: &Path) -> PathBuf` — `<session_file>.unlock`.
  - `pub fn store_key(file: &Path, key: &Key) -> std::io::Result<()>` — Windows: `CryptProtectData` (current user) then write; Unix: create with mode 0o600.
  - `pub fn load_key(file: &Path) -> Option<Key>` — reverse; `None` on any failure.
  - `pub fn clean_leftovers(dir: &Path)` — deletes `*.unlock` files whose session file no longer exists or that are older than 24 h.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test]
fn seal_and_open() {
    let (key, vault) = new_vault("correct horse");
    let s = seal(&key, "p@ss wörd");
    assert_ne!(s, seal(&key, "p@ss wörd")); // fresh nonce each time
    assert_eq!(open(&key, &s).as_deref(), Some("p@ss wörd"));
    assert_eq!(unlock("correct horse", &vault), Some(key));
}

#[test]
fn wrong_master_detected() {
    let (_, vault) = new_vault("right");
    assert_eq!(unlock("wrong", &vault), None);
    assert_eq!(open(&[7u8; 32], &seal(&[1u8; 32], "x")), None);
}

#[test]
fn key_file_round_trip() {
    let dir = std::env::temp_dir().join(format!("on-vault-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = unlock_file(&dir.join("session.txt"));
    store_key(&f, &[9u8; 32]).unwrap();
    assert_ne!(std::fs::read(&f).unwrap(), vec![9u8; 32]); // on Windows wrapped; on Unix check mode instead
    assert_eq!(load_key(&f), Some([9u8; 32]));
    clean_leftovers(&dir); // session.txt doesn't exist -> removed
    assert!(!f.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
```

(On Unix, replace the `assert_ne!` with a check that the mode is `0o600`, under `#[cfg(unix)]`.)
- [ ] **Step 2: Run** `cargo test --release vault` → fails to compile.
- [ ] **Step 3: Implement** with the constants from Global Constraints. Zero the master string's derived buffers you own where cheap; don't add a zeroize crate.
- [ ] **Step 4: Run** `cargo test --release vault` → PASS (note: 300 000 PBKDF2 rounds in release take well under a second; tests must use `--release`).
- [ ] **Step 5: Check the Linux target still builds**: `cargo check --release --target x86_64-unknown-linux-gnu --tests` → no errors.
- [ ] **Step 6: Commit** `git commit -m "remote: password vault and unlock key file"`.

---

### Task 5: Askpass channel and the child-process waiter

**Files:**
- Create: `rust/src/remote/askpass.rs`, `rust/src/remote/wait.rs`
- Modify: `rust/src/main.rs` (handle `--askpass` first, before the terminal check)

**Interfaces:**
- Produces (askpass.rs):
  - `pub struct Server` — `pub fn start() -> std::io::Result<Server>` (TCP listener on `127.0.0.1:0`, non-blocking, random 128-bit token from `getrandom`); `pub fn env(&self) -> (String, String)` → (`"ON_ASKPASS"`, `"<port>:<token hex>"`); `pub fn poll(&mut self) -> Option<Request>` (accepts one pending connection, reads one line `"<token>\t<prompt>"`, drops it if the token is wrong).
  - `pub struct Request { pub prompt: String, stream: TcpStream }` with `pub fn answer(self, text: Option<&str>)` (writes the answer line, or closes without one to cancel).
  - `pub enum PromptKind { HostKey, Password, Passphrase, Other }` and `pub fn kind(prompt: &str) -> PromptKind` (host key: contains `continue connecting`; passphrase: `passphrase`; password: `password`; case-insensitive).
  - `fn ask_with(env_value: &str, prompt: &str) -> Option<String>` — the helper's core: connect, send token and prompt, read the answer line (`None` when the server closes without one).
  - `pub fn helper_main(prompt: &str) -> i32` — the `on --askpass <prompt>` side: `ask_with(ON_ASKPASS value, prompt)`, print the answer to stdout and exit 0; exit 1 when cancelled or anything fails.
- Produces (wait.rs):
  - `pub struct Child` wrapping `std::process::Child` with reader threads for stdout and stderr into `mpsc` channels: `pub fn spawn(cmd: std::process::Command) -> std::io::Result<Child>`, `pub fn write_line(&mut self, s: &str) -> std::io::Result<()>`, `pub fn kill(&mut self)`, `pub fn is_alive(&mut self) -> bool`.
  - `pub enum Waited<T> { Done(T), Cancelled, TimedOut }`
  - `pub fn wait_until<T>(app: &mut App, title: &str, timeout: Duration, askpass: Option<&mut Server>, poll: impl FnMut() -> Option<T>) -> Waited<T>` — loops every ~30 ms: calls `poll`; serves askpass requests through `App::answer_prompt` (Task 8; until then a stub returning `None`); checks for Esc with `crossterm::event::poll(Duration::ZERO)`; after 300 ms shows `dialogs::progress(app, title, ...)` with "Esc to cancel"; returns `TimedOut` when `timeout` passes with no output (the timeout restarts whenever `poll` reports progress — make `poll` return `Some` only when done, and let the caller extend the deadline by calling `wait_until` again for long transfers).

- [ ] **Step 1: Write the failing tests**:

```rust
// askpass.rs
#[test]
fn round_trip() {
    let mut srv = Server::start().unwrap();
    let (_, val) = srv.env();
    let h = std::thread::spawn(move || ask_with(&val, "user@h's password: ")); // ask_with: helper core taking the env value, returns Option<String>
    let req = loop { if let Some(r) = srv.poll() { break r } std::thread::sleep(std::time::Duration::from_millis(10)) };
    assert_eq!(kind(&req.prompt), PromptKind::Password);
    req.answer(Some("s3cret"));
    assert_eq!(h.join().unwrap().as_deref(), Some("s3cret"));
}

#[test]
fn wrong_token_ignored() { /* connect with "<port>:00" -> srv.poll() returns None, helper gets None */ }
```

and in `wait.rs` a test `wait_cancels_and_times_out` for a pure helper `fn deadline_passed(start: Instant, timeout: Duration, now: Instant) -> bool` plus a `Child` test: spawn a command that prints 200 000 lines to stdout and 200 000 to stderr (`cmd /c` loop on Windows, `sh -c` on Unix, or the test binary itself re-invoked with an env var), read them all, assert counts (no deadlock).
- [ ] **Step 2: Run** `cargo test --release askpass wait` → fail.
- [ ] **Step 3: Implement.** `main.rs`: if `args[1] == "--askpass"`, `std::process::exit(remote::askpass::helper_main(args.get(2).map(String::as_str).unwrap_or("")))` before anything touches the terminal.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Manual check with real sftp** (test server from Task 1): a tiny `#[ignore]` test `sftp_askpass_live` that starts `Server`, runs `sftp` with `SSH_ASKPASS=<current_exe of a built on.exe> --askpass` — use `env!("CARGO_BIN_EXE_on")` only if available, else take the path from `ON_TEST_EXE` — answers the password from `ON_TEST_SFTP_PASSWORD`, and sees `Remote working directory` after `pwd`. Run `cargo test --release -- --ignored sftp_askpass_live` with the env set → PASS.
- [ ] **Step 6: Commit** `git commit -m "remote: askpass channel and process waiter"`.

---

### Task 6: Conn trait and SFTP connection

**Files:**
- Modify: `rust/src/remote/mod.rs`
- Create: `rust/src/remote/sftp.rs`

**Interfaces:**
- Consumes: `wait::{Child, wait_until, Waited}`, `askpass::Server`, `listing::{RemoteEntry, parse_unix_ls}`, `profiles::Profile`.
- Produces (mod.rs):
  - `pub type R<T> = Result<T, String>;` (`Err` holds the server's own text; `"Cancelled."` on Esc).
  - `pub trait Conn { fn home(&mut self, app: &mut App) -> R<String>; fn list(&mut self, app: &mut App, dir: &str) -> R<Vec<RemoteEntry>>; fn get(&mut self, app: &mut App, remote: &str, local: &Path, is_dir: bool) -> R<()>; fn put(&mut self, app: &mut App, local: &Path, remote: &str, is_dir: bool) -> R<()>; fn mkdir(&mut self, app: &mut App, path: &str) -> R<()>; fn remove(&mut self, app: &mut App, path: &str, is_dir: bool) -> R<()>; fn rename(&mut self, app: &mut App, from: &str, to: &str) -> R<()>; fn close(&mut self); }` — `remove` on a folder is recursive.
  - `pub fn join(dir: &str, name: &str) -> String` and `pub fn parent(path: &str) -> Option<(String, String)>` (POSIX remote paths; `/` root has no parent).
  - `pub fn connect(app: &mut App, p: &Profile) -> R<Box<dyn Conn>>` — dispatches on `proto`.
  - `pub struct OpenSsh { pub version: (u32, u32), pub askpass: bool }` and `pub fn openssh() -> R<OpenSsh>` (runs `ssh -V`, parses `OpenSSH_8.2p1` / `OpenSSH_for_Windows_9.5p2`; `askpass = version >= (8, 4)`; `Err` with "sftp was not found. Install OpenSSH ..." when missing).
- Produces (sftp.rs): `pub struct SftpConn` implementing `Conn`; `pub fn sftp_quote(s: &str) -> String`; `pub fn sftp_args(p: &Profile, askpass: bool) -> Vec<String>` (`-q`, `-P port` when set, `-o ConnectTimeout=15`, `-o ServerAliveInterval=30`, `-o BatchMode=yes` when `!askpass`, `-o NumberOfPasswordPrompts=3`, then `user@host` or `host`).
- Protocol (from Task 1 findings; adjust to what Task 1 recorded): send one command line, then `pwd`; read stdout until the `Remote working directory:` line that answers the marker; whatever came on stderr meanwhile (drain stderr for 50 ms after the marker) is the error text. Commands: `ls -la "<dir>"`, `get [-r] "<remote>" "<local>"`, `put [-r] "<local>" "<remote>"`, `mkdir`, `rm`, `rmdir` (recursive remove: list and walk, deepest first), `rename`. If the process is dead before a command: reconnect once (new process, same profile and password source), then run the command; if it dies again, `Err`.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test]
fn quoting_and_paths() {
    assert_eq!(sftp_quote(r#"a "b"\c"#), r#""a \"b\"\\c""#);
    assert_eq!(sftp_quote("-rf"), "\"-rf\"");
    assert_eq!(join("/", "x"), "/x");
    assert_eq!(join("/a/", "b c"), "/a/b c");
    assert_eq!(parent("/a/b"), Some(("/a".into(), "b".into())));
    assert_eq!(parent("/"), None);
}

#[test]
fn args_and_version() {
    let p = Profile { name: "w".into(), proto: Proto::Sftp, host: "h".into(), port: Some(2222), user: "u".into(), folder: String::new(), save_password: false, password: None, insecure: false };
    let a = sftp_args(&p, false);
    assert!(a.windows(2).any(|w| w == ["-P", "2222"]));
    assert!(a.contains(&"BatchMode=yes".to_string()));
    assert_eq!(a.last().unwrap(), "u@h");
    assert_eq!(parse_ssh_version("OpenSSH_for_Windows_9.5p2, LibreSSL 3.8.2"), Some((9, 5)));
    assert_eq!(parse_ssh_version("OpenSSH_8.2p1 Ubuntu-4ubuntu0.5, OpenSSL 1.1.1f"), Some((8, 2)));
}

#[test]
fn dead_process_is_reported() { /* SftpConn over a command that exits at once (e.g. `cmd /c exit 1` / `sh -c "exit 1"`) via a test constructor `SftpConn::with_command`; list() returns Err after one retry, within 2 s */ }

#[test]
fn large_output_no_deadlock() { /* covered by wait.rs Child test; here: parse 10 000 synthetic ls lines in under 200 ms */ }
```

- [ ] **Step 2: Run** `cargo test --release sftp` → fail.
- [ ] **Step 3: Implement** `SftpConn::open(app, profile, askpass_server) -> R<SftpConn>` (spawn, then a first `pwd` as the connect check with the 15 s timeout) and the trait methods. `App` is passed so `wait_until` can show progress and prompts.
- [ ] **Step 4: Run** `cargo test --release sftp` → PASS; and `cargo test --release -- --ignored sftp_live` (an ignored test doing list / put / get / rename / mkdir / recursive remove on the Task 1 server, names with spaces, `#`, `%`, quotes and Cyrillic) → PASS.
- [ ] **Step 5: Commit** `git commit -m "remote: SFTP connection through the system sftp"`.

---

### Task 7: FTP connection through curl

**Files:**
- Create: `rust/src/remote/ftp.rs`

**Interfaces:**
- Consumes: `Conn`, `R`, `wait::*`, `listing::{parse_mlsd, parse_list}`, `Profile`.
- Produces:
  - `pub struct FtpConn` implementing `Conn` (holds the profile, the password for this session, `mlsd: Option<bool>` learned on first list).
  - `pub fn url(p: &Profile, path: &str, dir: bool) -> String` — `ftp://host:port/<percent-encoded path>` (`ftps://` for implicit); each path segment percent-encoded (everything except unreserved `A-Za-z0-9-._~`), `/` kept; trailing `/` when `dir`.
  - `pub fn base_args(p: &Profile) -> Vec<String>` — `-s -S --connect-timeout 15 -K -`, plus `--ssl-reqd` for `Ftps`, `-k` when `insecure`; never a user or password.
  - `pub fn config_text(user: &str, password: Option<&str>) -> String` — `user = "u:p"` with `\` and `"` escaped; empty user → anonymous (no line).
  - `pub enum CurlFail { Login, Cert, NotFound, Other(String) }` and `pub fn classify(code: i32, stderr: &str) -> CurlFail` (67 → Login; 60, 35 with "certificate"/"SEC_E_UNTRUSTED_ROOT"/"revocation" → Cert; 78 or 550 in text → NotFound).
- Operations: list via `-X MLSD <dir url>` (fallback to plain `<dir url>` LIST when MLSD fails with 500/502, remember in `mlsd`); get file `-o <local> <url>`; get folder: list + recurse; put file `-T <local> <dir url>` with `--ftp-create-dirs`; put folder: recurse; mkdir `-Q "MKD <path>" <root url>`; remove file `-Q "DELE <path>"`, folder: recurse then `RMD`; rename `-Q "RNFR <from>" -Q "RNTO <to>"`. `-Q` paths are raw (not percent-encoded).
- `curl` missing (spawn fails with NotFound) → `Err("curl was not found. It comes with Windows 10+ and macOS; on Linux install the curl package.")`.
- On `Login` the connection asks the password (via `App::answer_prompt`, Task 8 — up to 3 times) and retries. On `Cert` it returns `Err("certificate")`-tagged error that Task 8 turns into the "can't be verified" dialog.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test]
fn url_encoding() {
    let p = prof(Proto::Ftp, None);
    assert_eq!(url(&p, "/a b/#1%?.txt", false), "ftp://h/a%20b/%231%25%3F.txt");
    assert_eq!(url(&p, "/Документы", true), "ftp://h/%D0%94%D0%BE%D0%BA%D1%83%D0%BC%D0%B5%D0%BD%D1%82%D1%8B/");
    assert_eq!(url(&prof(Proto::FtpsImplicit, Some(990)), "/", true), "ftps://h:990/");
}

#[test]
fn secrets_stay_off_the_command_line() {
    let p = prof(Proto::Ftps, None);
    let args = base_args(&p).join(" ");
    assert!(args.contains("--ssl-reqd") && args.contains("-K -"));
    assert!(!args.contains("pw"));
    assert_eq!(config_text("u", Some("p\"w\\")), "user = \"u:p\\\"w\\\\\"\n");
}

#[test]
fn errors() {
    assert!(matches!(classify(67, "curl: (67) Access denied: 530"), CurlFail::Login));
    assert!(matches!(classify(60, "SSL certificate problem: self-signed certificate"), CurlFail::Cert));
    assert!(matches!(classify(35, "schannel: ... SEC_E_UNTRUSTED_ROOT"), CurlFail::Cert));
}
```

- [ ] **Step 2: Run** `cargo test --release ftp` → fail.
- [ ] **Step 3: Implement.** Each curl run goes through `wait::Child` + `wait_until` (15 s with no progress → `TimedOut`; Esc → kill, `Err("Cancelled.")`). Write `config_text` to curl's stdin and close it.
- [ ] **Step 4: Run** → PASS; then the ignored `ftp_live` test against the Task 1 FTP and FTPS servers (same operations and names as `sftp_live`) → PASS.
- [ ] **Step 5: Commit** `git commit -m "remote: FTP and FTPS through the system curl"`.

---

### Task 8: Remote panels in the UI: profiles, connecting, browsing, passwords, session

**Files:**
- Create: `rust/src/remote/ui.rs` (profile form, password and master-password dialogs, connect flow)
- Modify: `rust/src/panel.rs` (field `pub remote: Option<RemoteView>`; reset in `change_dir`; `load()` keeps items for remote panels; `title_path()`; `pub fn set_remote_items(&mut self, list: Vec<RemoteEntry>, focus: Option<&str>)` builds `Entry`s with `path = PathBuf::from(remote path)`, a `..` entry, applies sort and the hidden setting)
- Modify: `rust/src/favorites.rs` (`Pick::Remote(usize)` and `Pick::NewRemote`; a "Remote" header and one row per profile after the Temp panel row, then "New connection..."; on those rows Ins adds, F4 edits, F8/Del deletes via `remote::ui`)
- Modify: `rust/src/app.rs` (fields `remote_profiles: Profiles`, `vault_key: Option<Key>`, `typed_passwords: HashMap<String, String>`, `askpass: Option<askpass::Server>`; `change_drive` handles the new picks; `open`/`go_parent` for remote panels; Ctrl+R; session save/restore `p{i}.remote` = profile name, `p{i}.rpath`; `save_state` writes the local `dir` only; fresh start never connects; command line, F9 and the blocked features (spec Non-goals) show "Not available in a remote panel."; `pub fn answer_prompt(&mut self, profile: &str, prompt: &str) -> Option<String>`; quitting for good deletes the unlock file and closes connections; start-up calls `vault::clean_leftovers` next to the session file)
- Modify: `rust/src/menu.rs` (Options: "Change master password", "Forget saved passwords"; Disk: "Remote connection..." → the drive list for the active panel)

**Interfaces:**
- Consumes: everything from Tasks 2–7.
- Produces:
  - `pub struct RemoteView { pub profile: String, pub cwd: String, pub conn: Box<dyn Conn>, pub title: String }` (in `remote/mod.rs`).
  - `App::remote_cd(&mut self, i: usize, path: &str, focus: Option<&str>) -> bool` — takes `panels[i].remote`, lists, puts it back, shows errors.
  - `remote::ui::edit_profile(app: &mut App, p: Option<&Profile>) -> Option<Profile>`; `remote::ui::connect(app: &mut App, idx: usize) -> R<RemoteView>`.
- `answer_prompt` rules:
  - `HostKey` → `dialogs::choice` showing the prompt text (it holds the fingerprint), "Trust this server?" Yes / No → `"yes"` / `None`.
  - `Password` → typed password for this profile if any (first ask only); else the saved one (unlock the vault first: `vault_key`, else `load_key(unlock_file)`, else ask the master password up to 3 times, then store with `store_key` when a session file exists); else a password dialog with a "Save password" checkbox (when ticked: create the vault on first use by asking the new master password twice, then `seal` and save the profile). A second password prompt in the same connect means the first was wrong: always ask, max 3.
  - `Passphrase`/`Other` → an input dialog with the prompt text (masked).
- FTPS `Cert` error from `connect` → `dialogs::choice(app, "Certificate", &["This server's certificate can't be verified.", "<curl message>"], &["Connect anyway", "Cancel"], true)`; Connect anyway sets `insecure = true`, saves profiles, retries once.
- OpenSSH < 8.4 and the connect fails with "Permission denied" / "Host key verification failed" → message: "This OpenSSH (<ver>) can't ask for passwords inside ON. Use an SSH key or ssh-agent, connect once with `ssh <host>` in a terminal to trust the server, or update OpenSSH (8.4 or newer)."
- Password dialog: a `LineEdit` drawn as `*` (add `pub fn draw_masked` to `dialogs::LineEdit` or a `masked: bool` field).

- [ ] **Step 1: Write the failing unit tests** where logic is pure: `panel::tests::remote_items` (`set_remote_items` gives `..` first, hides `.dot` names when `show_hidden == false`, keeps the cursor on `focus`); `app` session round trip for `p0.remote`/`p0.rpath` if the session code has a testable helper (otherwise extract `fn session_remote_lines(...) -> String` and test that, and that `save_state` output has no `remote` keys).
- [ ] **Step 2: Run** `cargo test --release remote_items` → fail.
- [ ] **Step 3: Implement** the files above.
- [ ] **Step 4: Run** `cargo test --release` → all PASS; `cargo clippy --release` → only the existing `calc.rs` warning.
- [ ] **Step 5: Manual check in a real terminal** (build, copy to `bin\on.exe`), against the Task 1 servers:
  1. Alt+F2 → New connection → SFTP profile with password → connect: host key dialog shows the fingerprint; password dialog with "Save password" ticked → master password asked twice → listing shows; title `sftp://user@127.0.0.1:2222/...`.
  2. Enter into folders, `..` up, `..` at `/` returns to the local folder; Ctrl+R re-reads.
  3. Type `dir` on the command line in the other (local) panel and run it: ON restarts, the remote panel reconnects without asking anything.
  4. Alt+X, start ON again: the panel shows its local folder; connect again → master password asked once (wrong one 3 times → asked for the server password instead).
  5. FTP profile anonymous; FTPS profile against the self-signed server → "can't be verified" → Connect anyway → connects; next time no question.
  6. Pull the network (stop the container) → next action shows an error after a reconnect attempt; Esc during a stuck connect cancels.
  7. F9, command line, Alt+F7, Ctrl+A, Ctrl+F in a remote panel → "Not available in a remote panel."
- [ ] **Step 6: Commit** `git commit -m "remote: connect from the drive list, browse, passwords, session"`.

---

### Task 9: File operations on remote panels

**Files:**
- Modify: `rust/src/fileops.rs` (`copy_or_move`, `delete` branch on `remote`), `rust/src/app.rs` (F3 `cmd_view`, F4 `cmd_edit`, Enter on a file in `open`, F7 make folder, Shift+F6 rename in `rename.rs` single-item path)
- Modify: `rust/src/rename.rs` if the single rename lives there

**Interfaces:**
- Consumes: `Conn` methods, `App::remote_cd`, `RemoteView`.
- Produces: `fn remote_temp_dir() -> PathBuf` (`<temp>/on-remote-<pid>/`, removed on quit) in `remote/mod.rs`.
- Behaviour:
  - Remote → local (F5): the usual "Copy N items to:" dialog with the local panel's folder; for each target `conn.get(remote, local_dst, is_dir)`; overwrite prompts use the existing `ask_overwrite` for local files. F6 = copy, then `conn.remove` of the sources that copied without error.
  - Local → remote: the destination dialog shows the remote title (`sftp://...`); `conn.put`; existence check against the passive panel's current listing names for the overwrite prompt. F6 removes local sources after success.
  - Remote → remote: get into `remote_temp_dir()`, then put; temp copy removed after.
  - Remote → Temp panel: "The Temp panel holds local files only."
  - F3 / Enter: download into `remote_temp_dir()` and view (`viewer::show`) / open (`platform::open_default`).
  - F4: download, `editor::edit`; when it returns `true` (saved), upload back; on upload error offer Retry / Cancel and keep the temp file path in the message.
  - F7: input dialog, `conn.mkdir(join(cwd, name))`, re-list with focus on it. F8: the usual confirmation (always "Permanently delete" wording — there is no trash), `conn.remove`. Shift+F6: input, `conn.rename`.
  - After each operation re-list the remote panel(s) with `remote_cd(i, cwd, focus)`.

- [ ] **Step 1: Write the failing test** for the one pure helper: `fileops::tests::remote_dest` — `fn remote_dest(cwd: &str, input: &str, name: &str, many: bool) -> String` resolving the destination typed in the dialog (`"/srv/"` + `"a.txt"`; relative input joined to `cwd`; a single file to a name not ending in `/` renames).
- [ ] **Step 2: Run** `cargo test --release remote_dest` → fail.
- [ ] **Step 3: Implement** the behaviour above.
- [ ] **Step 4: Run** `cargo test --release` → PASS; clippy clean as before.
- [ ] **Step 5: Manual check** (SFTP and FTP each): copy a folder tree both ways, move a file both ways, remote→remote between an SFTP and an FTP panel, F3 a remote text file, F4 edit and save (re-open shows the change), F7, Shift+F6, F8 on a non-empty folder; names with spaces, `#`, `%` and Cyrillic; Esc during a large transfer stops it and the panel still works.
- [ ] **Step 6: Commit** `git commit -m "remote: copy, move, view, edit, make folder, rename, delete"`.

---

### Task 10: Help, docs, final checks

**Files:**
- Create: `rust/help/remote.md`
- Modify: `rust/src/help.rs` (`("remote", include_str!("../help/remote.md"))`), `rust/help/index.md`, `rust/help/favorites.md` (Remote rows and keys), `rust/help/panels.md` (one line), `README.md` (feature bullet; Linux/macOS/Windows: needs `sftp` and `curl`; OpenSSH 8.4+ for passwords in ON), `CHANGELOG.md` (`[Unreleased]` → Added)

- [ ] **Step 1: Write `remote.md`**: what it is; making a profile (fields, the four protocols, `~/.ssh/config` aliases); passwords, "Save password", the master password (asked once until you quit ON; change / forget in Options); host keys and FTPS certificates; what works in a remote panel and what doesn't; needs `sftp` / `curl`; older OpenSSH = keys only; keys table.
- [ ] **Step 2: Run** `cargo test --release` (the help test that checks links, if any) → PASS.
- [ ] **Step 3: Full checks**: `cargo clippy --release` (only the old calc warning), `cargo check --release --target x86_64-unknown-linux-gnu --tests`, `cargo check --release --target aarch64-apple-darwin --tests` → no errors.
- [ ] **Step 4: Build and install** `cargo build --release` and copy `target\release\on.exe` to `..\bin\on.exe`.
- [ ] **Step 5: Commit** `git commit -m "remote: help and docs"`.
