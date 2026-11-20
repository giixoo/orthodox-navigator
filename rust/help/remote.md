# Remote panels: SFTP and FTP

A panel can show a folder on another machine and work on it like a local one: browse, view, edit, copy
both ways, make folders, rename, delete. ON uses the programs that come with your system: `sftp`
(OpenSSH) for SFTP and `curl` for FTP and FTPS.

## Connecting

Open the drive list (**Alt+F1** for the left panel, **Alt+F2** for the right one, or Disk → Remote
connection). Your connections are in the **Remote** part at the end; Enter connects in that panel.

| Key in the Remote part | What it does |
|---|---|
| **Enter**, double click | connect (on "New connection..." make one first) |
| **Ins** | new connection |
| **F4** | edit the connection |
| **F8, Del** | delete the connection |

A connection has a name, a protocol, the host, a port (empty for the usual one), a user and a folder to
start in (empty for the server's home folder).

- **SFTP** uses your ssh setup: the host can be a name from `~/.ssh/config`, and user and port may stay
  empty. Your keys and ssh-agent work as in a terminal.
- **FTP** has no encryption: use it only on your own network.
- **FTPS** is FTP with encryption asked for after connecting (port 21, the usual kind).
- **FTPS implicit** is encrypted from the start (port 990).

FTP folders are shown from the folder the server logs you in to, as `/`.

## Passwords

When a server asks for a password, ON asks you. The password is kept until you leave ON. Tick **Save
password** to keep it in the connection: it is then encrypted with your **master password**, which you
set the first time you save one. ON asks for the master password once, the first time it needs a saved
password, and remembers it until you quit ON (Alt+X), also while commands run.

Options → **Change master password** sets a new one. If you forget it, Options → **Forget saved
passwords** removes them all; they are asked again the next time.

## New servers and certificates

The first time you connect to an SFTP server, ON shows its key fingerprint and asks whether to trust it;
ssh remembers the answer in `known_hosts`. If a server's key changes later, the connection fails: check
with the server's admin before removing the old line from `known_hosts`.

If an FTPS server's certificate can't be verified (often a self-signed one on a NAS or a router), ON asks
whether to connect anyway and remembers that for this connection. The edit form has a check box to undo it.

## In a remote panel

| Key | What it does |
|---|---|
| **Enter** | open a folder; a file opens with its program (a copy is downloaded first) |
| **..** at the top folder | back to the local folder the panel showed before |
| **F3** | view a file |
| **F4** | edit a file; saving uploads it again |
| **F5 / F6** | copy / move to or from the other panel (also between two servers) |
| **F7** | make a folder |
| **Shift+F6** | rename |
| **F8** | delete (on a server there is no trash) |
| **Ctrl+R** | read the folder again |
| **Esc** | stop a connection or a copy that takes too long |

Not in a remote panel: the command line, the user menu (F9), find files, filter, attributes, folder
sizes, batch rename, git and comparing folders.

When you run a command in the other panel, ON closes for it and connects again afterwards; when you
start ON anew, a remote panel shows its last local folder instead.

## What it needs

- `sftp` and `ssh` (OpenSSH): built into Windows 10 and later, macOS, and most Linux (on Linux the
  `openssh-client` package).
- To answer passwords and "trust this server?" inside ON, OpenSSH **8.4 or newer** (Windows 11 has it).
  With an older one, SFTP works with keys or ssh-agent only; connect once with `ssh host` in a terminal to
  trust a new server.
- `curl` for FTP: built into Windows 10 and later and macOS; on Linux the `curl` package.
