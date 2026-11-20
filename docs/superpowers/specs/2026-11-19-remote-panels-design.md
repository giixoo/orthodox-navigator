# Remote panels: SFTP and FTP

Date: 2026-11-19. Branch: `feature/remote-panels`.

## Intent

A panel can show a folder on another machine, reached over SFTP or FTP/FTPS, and the usual panel keys
work on it: browse, view, edit, copy and move both ways, make folders, delete, rename. Connections are
kept as named profiles and opened from the drive list (Alt+F1 / Alt+F2), like a drive. This follows
DOS Navigator's idea of "any source is a panel" (ON already does it for archives and the Temp panel).

## Goals

- **SFTP** through the system `sftp` program (OpenSSH; in Windows 10+, macOS, Linux). One `sftp` process
  per remote panel, kept running while the panel shows that server; commands go to its stdin, answers
  are read from its stdout. The user's `~/.ssh/config` (host aliases, keys, ports, jump hosts), ssh-agent
  and `known_hosts` work as in a terminal.
- **FTP and FTPS** through the system `curl` (in Windows 10+, macOS, nearly all Linux). One `curl` run
  per operation. Passive mode (curl's default).
- **Profiles**: name, protocol, host, port (empty = default), user, start folder, "save password"
  (yes / no), the saved password (encrypted), and for FTPS "don't check the certificate" (yes / no).
  Protocols: `sftp`; `ftp`; `ftps` = explicit TLS (`ftp://` + `--ssl-reqd`, port 21); `ftps-implicit`
  (`ftps://`, port 990). For SFTP the host may be a
  `~/.ssh/config` alias, and user / port may be empty (ssh's own defaults then apply).
- **Passwords**:
  - Asked when a server needs one. Kept in memory for the session.
  - With "save password" on, kept in the profile file, encrypted with a key made from the **master
    password**.
  - The master password is asked the first time a saved password is needed. The key made from it is then
    remembered until the user leaves ON for good (see Constraints), so ON's restarts around shell
    commands don't ask again.
  - The master password is set the first time a password is saved (typed twice). It can be changed
    (Options menu), which re-encrypts the saved passwords. A forgotten master password: "Forget saved
    passwords" clears them; they are typed again on the next connection.
- **FTPS certificates**: when curl can't verify the server's certificate (self-signed, unknown issuer,
  revocation check failing), ON asks "This server's certificate can't be verified" with Connect anyway /
  Cancel. Connect anyway sets the profile's "don't check the certificate" (curl `--insecure`) and saves
  it; the form shows it as a checkbox that can be cleared. (User's choice.)
- **Panel operations on a remote panel**: list (with size, time, folder / link / hidden marks), Enter into
  folders and `..` up (at the server's root `..` goes back to the local folder the panel showed before),
  F3 view, F4 edit (download to a temp file, upload on save), F5 / F6 to and from a local panel, F7 make
  folder, F8 delete (folders recursively, after the usual confirmation), Shift+F6 rename, Ctrl+R re-read.
  Between two remote panels F5 / F6 go through a local temp folder.
- **Profiles UI**: the drive list gets a "Remote" part after the drives (one row per profile, plus "New
  connection..."). Enter connects in that panel. Ins adds, F4 edits, F8 deletes a profile, in a form
  dialog (the password field shows `*`).
- The panel title shows `sftp://user@host/path` (or `ftp://` / `ftps://`).
- Errors (wrong password, host down, permission denied, a server message) are shown in ON's message box
  with the server's own text. A dropped connection reconnects once on the next action.
- Help page, README, CHANGELOG.

## Non-goals

- SCP. Plain FTP through `ftp.exe` (no passive mode). A built-in SSH or FTP implementation.
- Running commands on the server; the command line and the user menu (F9) are not available in a remote
  panel (a message says so).
- FTP file names in encodings other than UTF-8 (cp1251, cp866...).
- Search (Alt+F7), folder sizes, attributes (Ctrl+A), filter (Ctrl+F), git marks, archives browsed on the
  server, the Temp panel with remote files, batch rename (Alt+F6) on remote files.
- Sync / compare folders. Resuming broken transfers.

## Anti-goals (would be a failure even if it works)

- A password, passphrase or key in plain text anywhere (except the unlock key file, protected as
  described under Constraints): on a command line (visible in the process list),
  in an environment variable passed to `curl`, in the session file, in a log, in an error message.
- An ssh or curl prompt drawn over ON's screen, or ON hanging while a hidden program waits for an answer.
- Accepting an unknown or changed host key without the user seeing its fingerprint.
- The UI freezing with no way out: every wait on the network can be cancelled with Esc.

## Constraints

- **No new C dependencies.** Releases are static single binaries (Linux musl, macOS, Windows). Encryption
  uses pure-Rust crates: `pbkdf2` (with the existing `sha2` / `hmac`) to make the key, `chacha20poly1305`
  to encrypt. `getrandom` (already a dependency) for salt and nonces.
- **ON restarts around shell commands.** With the `on` shell wrapper, running a command (command line,
  user menu) quits ON, the shell runs the command, and ON starts again with `--session-file`. So:
  - Remote panels are saved in the session (profile name + remote folder) and reconnect when ON comes
    back. Passwords typed (not saved) are lost on such a restart and are asked again.
  - The master-password key is kept between those restarts in a file **per ON window**: next to that
    window's `--session-file` (or named after it), so several ON windows don't share or delete each
    other's key. On Unix the file is 0600; on Windows the key is wrapped with DPAPI (`CryptProtectData`,
    current user, through `windows-sys`), so a copied file is useless to another account or machine. It
    is deleted when that ON quits for good (Alt+X, closing the window, quitting without a pending
    command). Leftovers (a crash) are deleted on start when their session file is gone or they are older
    than a day.
  - On a fresh start (state.txt, not a session) a panel that was remote opens its last local folder
    instead; nothing connects until the user picks a profile.
- **OpenSSH 8.4 or newer for prompts** (`SSH_ASKPASS_REQUIRE` appeared in 8.4; Windows 10 ships 8.1,
  Ubuntu 20.04 8.2, RHEL 8 8.0). ON checks `sftp -V` / `ssh -V` once. With an older OpenSSH it connects
  with `-o BatchMode=yes` (keys and ssh-agent only); when that fails for lack of a password or an unknown
  host key, the message says that this OpenSSH can't ask through ON and what to do (key / agent, add the
  host with `ssh host` once in a terminal, or update OpenSSH). (User's choice.)
- **Prompts go through ON** (OpenSSH 8.4+). `sftp` is started with `SSH_ASKPASS` pointing at ON itself (`on --askpass`)
  and `SSH_ASKPASS_REQUIRE=force`, and with no console input of its own, so every question ssh would ask
  (password, key passphrase, "trust this host?") comes to the askpass helper. The helper passes the
  question to the running ON (a private channel: files in a per-session folder in the user's temp folder,
  or a named pipe) and returns the answer. ON shows it as its own dialog: a password field, or the host
  key fingerprint with Yes / No. Saved passwords answer the password question without a dialog.
  - Host keys: an unknown key is shown with its fingerprint; Yes adds it to `known_hosts` (ssh does that).
    A changed key fails the connection; the message tells which line of `known_hosts` to check.
- **curl gets secrets on stdin**, never in arguments: `curl -K -` with `user = "name:password"` in the
  config text written to its stdin.
- **Programs not found**: if `sftp` or `curl` is missing, the message says which and how to get it.

## Approaches

1. **System programs (chosen).** `sftp` and `curl` do the network work; ON drives them and parses their
   output. No new network or crypto code beyond the password file, keeps the static builds as they are,
   and SSH uses the user's own config, keys and agent. Costs: parsing text output, slower listings than a
   library, depends on what is installed, and ON has to keep prompts off the screen (askpass).
2. **Pure-Rust libraries** (`russh` + `russh-sftp`, `suppaftp`). Full control and speed, but async
   (tokio) in a synchronous app, a large dependency set, and re-implementing ssh config, agent and
   known_hosts handling.
3. **`ssh2` (libssh2) + an FTP crate.** Mature, but C code (libssh2, OpenSSL) in the static musl and macOS
   builds, and again no ssh config / agent integration for free.

The user picked 1 ("go through cheaper route"), SFTP and FTP only (no SCP).

## Decided by the user

- SFTP and FTP / FTPS; no SCP.
- OpenSSH older than 8.4: keys / agent only, with an explaining message.
- FTPS certificates that can't be verified: ask once, remember "don't check" in the profile.
- System programs, not libraries.
- Passwords: asked per session; per-profile "save password"; saved ones encrypted with a master password.
- The master password is remembered until the user leaves ON for good (option 2), surviving ON's restarts
  around commands.

## My calls (the user can change them)

- Profiles in `<config>/orthodox-navigator/remote.txt`, edited through a form (not by hand, because of the
  encrypted passwords). A text format like the other config files; the encrypted password as base64.
- The remembered key's file is `<config>/orthodox-navigator/unlock.key` (or the user's temp folder if
  that is safer on the platform).
- A wrong master password three times stops asking for this connection; the server's password is asked
  instead.
- The unlock key file is per ON window; DPAPI on Windows, 0600 elsewhere.
- FTPS has two protocol values (explicit, implicit). FTP names are UTF-8 only.
- Fresh starts don't reconnect (above), so starting ON never asks for a password.
- Command line and F9 are blocked in remote panels rather than running locally, to avoid running a
  command in a folder the user didn't mean.

## Left to the builder

- How to tell where one `sftp` answer ends (e.g. a marker command after each one, such as `pwd`, and
  reading until its answer), and how to parse `ls -la` output (Unix servers; tolerate others). For FTP,
  `MLSD` where the server has it (`curl -X MLSD`), `LIST` parsing (Unix and DOS formats) otherwise.
- Progress and cancel during transfers (Esc kills the process; SFTP then reconnects).
- How remote entries are represented in `Panel` (an enum next to `arc` / `temp`, or a new field), and how
  `fileops`, the viewer and the editor branch on it. The archive panel (`ArcView`, `extract_temp`) is the
  closest existing pattern.
- Key derivation parameters (PBKDF2-SHA256, at least 200 000 rounds), the file format of the encrypted
  values, and a check value to tell a wrong master password.
- Tests: unit tests for the listing parsers, the profile file, encryption round trip and wrong-password
  detection. Manual checks against a local OpenSSH server (Windows optional feature, or Linux) and a local
  FTP server (e.g. `python -m pyftpdlib`).
- The askpass route was checked on Windows OpenSSH 9.5 (2026-11-19): with `SSH_ASKPASS` set to a `.cmd`
  and `SSH_ASKPASS_REQUIRE=force`, `ssh-keygen -y` on a passphrase key with stdin from a file called the
  helper with the prompt as its argument and took its stdout as the answer. Still check it with `sftp`
  against a real server (password and host-key prompts) before building the rest on it.

## Checked (2026-11-20, Windows 11, OpenSSH_for_Windows 9.5p2, curl 8.21.0 Schannel)

Against local containers (see `docs/superpowers/plans/remote-test-servers.md`).

**sftp over pipes**
- Askpass works for both prompts with stdin/stdout/stderr piped. Prompt texts:
  host key: a multi-line text ending `Are you sure you want to continue connecting (yes/no/[fingerprint])?`
  (first line `The authenticity of host '[127.0.0.1]:2222 ([127.0.0.1]:2222)' can't be established.`);
  a wrong answer is asked again forever as `Please type 'yes', 'no' or the fingerprint: `, so that text
  is a host-key prompt too and ON must answer it or cancel; password: `tester@127.0.0.1's password: `.
- **stdout is fully buffered on Windows** (the C runtime has no line buffering): answers arrive only when
  4 KB have piled up or at exit. stderr is unbuffered. A long-lived `sftp` therefore needs a flush after
  every command. Working protocol: send `#ON-<n>-A`, the command, `#ON-<n>-B`, then `help` three times
  (each about 2.2 KB). sftp echoes every line it reads as `sftp> <line>` (comment lines too), so the
  answer is what lies between the echoes of the two markers (minus the echo of the command), and
  everything before `sftp> #ON-<n>-A` is leftover help text to skip. Answers arrived in 0.1-0.3 s.
  Error text comes on stderr before the B marker (`Can't ls: "/nope" not found`,
  `remote delete /upload/nope: No such file or directory`, `stat remote: No such file or directory`).
- One `sftp` run per operation instead took 1.5-2 s each on localhost: too slow for browsing.
- Windows sftp turns `\` into `/` in arguments, so `"` inside a double-quoted name can't be escaped.
  Single quotes work (`'/upload/x y/q"uote.txt'`). Quote with `'...'`; use `"..."` when the name holds
  `'`; a name with both can't be passed (error message).
- `ls -la <dir>` prints names as `<dir>/<name>` (with `/.` and `/..`); the owner column is `?` on this
  server. Strip the `<dir>/` prefix.

**curl**
- LIST works (Unix format); `-X MLSD` fails on vsftpd (`curl: (19) RETR response: 500`), so the LIST
  fallback is needed. `MKD` with Cyrillic, `-T` with `--ftp-create-dirs` and percent-encoded names work.
- Exit codes: 67 `Access denied: 530` (wrong password); 9 `Server denied you to change to the given
  directory` (missing folder); 21 `QUOT command failed with 550` (failed DELE/MKD/...); 60
  `schannel: SEC_E_UNTRUSTED_ROOT (0x80090325) - The certificate chain was issued by an authority that is
  not trusted.` (self-signed); 18 `server did not report OK, got 522` = vsftpd's `require_ssl_reuse=YES`
  (its default), which Windows' Schannel curl can't satisfy. ON should explain 522: "The server requires
  TLS session reuse, which Windows' curl can't do; the server's admin can set require_ssl_reuse=NO."
- Implicit FTPS (port 990) was not tested (no server for it).
