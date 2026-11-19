# Orthodox Navigator (prototype)

A two-panel console file manager in the spirit of DOS Navigator, for Windows, Linux and macOS.

## Install with one command

**Windows** (PowerShell):

    irm https://raw.githubusercontent.com/giixoo/orthodox-navigator/main/install.ps1 | iex

**Linux / macOS** (terminal):

    curl -fsSL https://raw.githubusercontent.com/giixoo/orthodox-navigator/main/install.sh | sh

Then open a new terminal and type `on`.

- **What it does:** downloads the latest release for your system and installs it for you only (no admin,
  no sudo). It adds the line that loads the `on` command to your shell's startup file: the PowerShell
  profiles on Windows, `~/.zshrc` or `~/.bashrc` elsewhere.
- **Where it goes:** `%LOCALAPPDATA%\Programs\Orthodox Navigator` on Windows,
  `~/.local/share/orthodox-navigator` elsewhere.
- **No security warnings:** files downloaded from the terminal don't get the "downloaded from the
  internet" mark, so SmartScreen and Gatekeeper don't stop the unsigned program.
- **Update:** run the same command again.
- **Remove:**

      & ([scriptblock]::Create((irm https://raw.githubusercontent.com/giixoo/orthodox-navigator/main/install.ps1))) -Uninstall
      curl -fsSL https://raw.githubusercontent.com/giixoo/orthodox-navigator/main/install.sh | sh -s -- --uninstall

- **A specific version:** set `ON_VERSION` first: `$env:ON_VERSION='0.5.0'` in PowerShell, or
  `... | ON_VERSION=0.5.0 sh`.

You can read the scripts before running them: [install.ps1](install.ps1), [install.sh](install.sh).

## Build from source

Written in Rust (`rust/`). Build it and copy the program to `bin/`, which is where the shell functions
start it from (so you can rebuild while it is running):

    cd rust
    cargo build --release
    copy target\release\on.exe ..\bin\on.exe       # Windows
    mkdir -p ../bin && cp target/release/on ../bin/on   # Linux / macOS

### Build on macOS

Instead of downloading the release (Apple silicon or Intel):

1. Install the command line tools (compiler and linker): `xcode-select --install`
2. Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`, then open a new terminal
3. Get the source and build:

        git clone https://github.com/giixoo/orthodox-navigator.git
        cd orthodox-navigator/rust
        cargo build --release
        mkdir -p ../bin && cp target/release/on ../bin/on

4. Add `source /path/to/orthodox-navigator/shell/on.sh` to `~/.zshrc`, open a new terminal and type `on`.

To update later: `git pull`, then repeat the `cargo build` and `cp` lines. A binary built on the Mac itself
needs no `xattr` or Gatekeeper approval.

Works best in Windows Terminal or any terminal with 24-bit colour. Colours are sent as exact VGA values;
set `ON_COLORS=16` to use the terminal's own 16-colour palette instead.

## Release downloads

Besides the one-command install, the [Releases](https://github.com/giixoo/orthodox-navigator/releases)
page has, for each version:

- **Windows x64**: `-setup.exe` (installer: per user, no admin needed; adds the program to PATH and the
  `on` command to your PowerShell profiles), `.zip` (program and shell function, unpack anywhere), or just
  the `.exe`.
- **Linux x64 / arm64**: `.deb`, `.rpm`, `.tar.gz`, or just the program. The Linux build is static, so it
  runs on any distribution. After installing a package, add `source /usr/share/orthodox-navigator/on.sh`
  to `~/.bashrc` or `~/.zshrc`.
- **macOS** (Apple silicon and Intel in one binary): `.tar.gz` (program and shell function) or just the
  program. It isn't signed, so after downloading run `xattr -d com.apple.quarantine <file>` (or allow it in
  System Settings → Privacy & Security). Unpack, then add `source <folder>/shell/on.sh` to `~/.zshrc`.

A browser marks downloaded files, so Windows (SmartScreen) and macOS (Gatekeeper) warn about the unsigned
program. Files downloaded from the terminal are not marked, so nothing warns:

    Invoke-WebRequest https://github.com/giixoo/orthodox-navigator/releases/download/v0.5.0/on-0.5.0-windows-x64.exe -OutFile on.exe
    curl -fLo on https://github.com/giixoo/orthodox-navigator/releases/download/v0.5.0/on-0.5.0-linux-x64 && chmod +x on

To clear the mark from a browser download, run `Unblock-File <file>` on Windows (unblock a zip before
unpacking it) or `xattr -d com.apple.quarantine <file>` on macOS. On Linux nothing is marked, but a browser
download loses the executable permission: `chmod +x <file>`.

To make a release, add the version's section to [CHANGELOG.md](CHANGELOG.md) (it becomes the release
notes) and push a tag: `git tag v0.2.0 && git push origin v0.2.0`. The workflow in
[.github/workflows/release.yml](.github/workflows/release.yml) sets that version, builds and tests on Windows,
Linux and macOS, and publishes the files. It can also be started by hand (Actions → Release → Run workflow); the
files are then on the run's page.

## Start it from your shell

Start it through a small shell function rather than directly. Then:

- commands you type in the file manager run in **your real shell session**: your aliases, functions,
  modules and variables work, and a `cd` in a command is followed. The file manager steps aside while the
  command runs and comes back exactly as it was; Ctrl+O shows the output;
- when you quit (Alt+X) your shell is in the folder of the active panel;
- every command you ran is in your shell history (Up arrow, Ctrl+R, `Get-History`).

**PowerShell** (Windows, or pwsh on Linux): add this to `$PROFILE` (`notepad $PROFILE`), open a new terminal,
then type `on`:

    . C:\work\my\projects\orthodox-navigator\shell\on.ps1

**bash / zsh** (Linux, macOS): add this to `~/.bashrc` or `~/.zshrc`, then type `on`:

    source /path/to/orthodox-navigator/shell/on.sh

How it works: a program can't change its parent shell's folder or run things in it, so the function runs the
file manager in a loop. When you enter a command, the file manager saves its state and exits; the function
runs the command in your session and starts the file manager again, which restores its panels.
Ctrl+C inside a command also stops the loop (you are left at the shell prompt).

## Features

- **Help** (F1): pages for each part, with links between them, like Far Manager's help. They are the
  Markdown files in [rust/help](rust/help/index.md), built into the program.
- One or two panels (Ctrl+P hides the other panel and brings it back as it was); resize with Ctrl+←/→ (equal widths:
  menu Panel), or drag the border with the mouse.
- **Preview** (Ctrl+L): the other panel shows the file under the cursor (with syntax colours) or a summary of
  the folder. Tab (or a click) gives it the keyboard: it then scrolls and has the viewer's modes (F2 wrap,
  F4 hex, F5 format, F6 syntax, F8 code page); Tab or Esc goes back to the files.
- **Developer tools** (F2): Base64 encode/decode, URL encode/decode, string escape/unescape (JSON), HTML/XML
  escape/unescape, JWT decode and verify (HS256/384/512) and sign (HS256), SHA-256, regex tester with match
  highlighting, GUID, Lorem ipsum and password generators (F6 makes new ones). The text tools start with the
  file under the cursor in Input; or type, paste, load another file (F3); F5 saves the result, F2 copies it.
- **Batch rename** (Alt+F6) of the selected files from a pattern: `[N]` name, `[E]` extension, `[C]` counter,
  `[DATE]` / `[TIME]` of the file, `[Y] [M] [D] [h] [m] [s]`, letters `[N2-5]`, the folder `[P]`; find / replace
  and case. The list shows every new name before anything is renamed; names may swap safely.
- **Compare** two files side by side (Shift+F3, with the file of the same name in the other panel, or the one
  under its cursor); **git diff** of a file against HEAD (Alt+F3). Panels show the current git branch (`git: main`)
  and mark changes: M modified,
  A added, ? untracked, U conflict, • changes inside a folder.
- **System monitor** (Alt+F5): CPU load (total with history, per core), memory and swap, network rates per
  interface, disks, and processes sorted by CPU or memory (F6), with kill (F8). Refreshes every second.
- **Find files** (Alt+F7) by name masks (`*.rs;*.toml`) and the text they contain; the results become the
  panel's list (Enter goes to a file, `..` returns to the folder).
- Built-in viewer (text in UTF-8 / 1251 / 866, or hex; JSON and XML formatted with F5) and editor, both with
  syntax colours for common languages (Rust, C/C++, C#, Java/Kotlin, JS/TS, Go, Python, shell, PowerShell,
  batch, SQL, JSON, TOML/INI, YAML, XML/HTML, CSS, Markdown, Lua, Ruby, Makefile, Dockerfile). The editor keeps
  the file's encoding, line endings and indentation style.
- **Colour themes** (Ctrl+T): DN Gray, DN Blue, and every VS Code theme installed on the computer (built-in and
  from extensions), or "VS Code (current theme)" to always match VS Code. Moving through the list previews it.
- Alt+F1 lists drives and your favourite folders (Work, Movies, Docs, ...): F7 adds the current folder,
  F4 renames it, F8 removes it, digits 1-9 jump to one. Ctrl+D adds the current folder from the panels.
- Archives: Enter browses inside `.zip` (and `.jar`, `.nupkg`, `.docx`, ...), `.tar`, `.tar.gz`/`.tgz`.
  F3 views a file, Enter opens it, F5 extracts. Archives are read-only for now.
- Directory tree (Alt+F10) with [+] / [-] on folders that open / close, calculator with DEC/HEX/BIN/OCT/EXP
  (Ctrl+F2), the Orthodox game, with falling blocks (Ctrl+G).
- Copy/move/delete with overwrite prompts, Trash.
- Mouse: click selects, double-click opens / runs / goes into a folder (`..` goes up), right-click marks files,
  wheel scrolls, the key bar and dialog buttons are clickable, a click on the top row opens the menu.

## Keys

Each action has one key. The key bar at the bottom shows the F-keys, then other shortcuts (`^L` is Ctrl+L) as
many as fit; click any of them. Hold Shift, Ctrl or Alt and it shows what the keys do with it (Windows;
other systems don't tell programs about a modifier pressed on its own). Esc closes any window;
[■] in its corner does the same with the mouse.

| Key | Action |
|---|---|
| Tab / Shift+Tab | next / previous panel |
| Ctrl+U | swap the two panels |
| Ctrl+P | hide / show the other panel |
| Ctrl+← / Ctrl+→ | narrower / wider |
| Ctrl+L | preview in the other panel |
| Enter | open, run or browse an archive (on `..`: the folder above) |
| Insert, Gray + − * | select file / select group / unselect group / invert |
| Shift+↑ ↓ PgUp PgDn Home End | select the files the cursor passes over (on a selected file: unselect) |
| Alt+letter | quick search |
| F2 | developer tools |
| F3 / Shift+F3 / Alt+F3 | view (on a folder: its size) / compare with the other panel / git diff with HEAD |
| F4 / Shift+F4 | edit / edit a new file |
| F5 / F6 / Shift+F6 | copy (from an archive: extract) / move / rename in place |
| Alt+F6 | batch rename the selected files by a pattern |
| F7 / Alt+F7 | make folder (nested paths work) / find files |
| F8, Del / Shift+F8, Shift+Del | to Recycle Bin or Trash / delete permanently |
| Alt+F1 / Alt+F2 | drives and favourites for this / the other panel |
| Ctrl+D | add the current folder to favourites |
| Alt+F10 | directory tree |
| Ctrl+F2 | calculator |
| Alt+F5 | system monitor |
| Ctrl+F3..F6 | sort by name / ext / time / size (press again to reverse) |
| Ctrl+B / Ctrl+H / Ctrl+R | brief↔full view / hidden files / re-read |
| Ctrl+T | colour theme |
| Ctrl+G | the Orthodox game |
| Ctrl+O | user screen: the console with a prompt; Ctrl+O again for the panels |
| Ctrl+E / Ctrl+Shift+E | previous / next command from the history (yours from the shell, too) |
| Ctrl+Enter | put the file name on the command line |
| Ctrl+[ / Ctrl+] | put the left / right panel's folder on the command line |
| Ctrl+Ins | copy the command line (or the file name) to the clipboard |
| F1 | help for where you are (Tab picks a link, Enter opens it, Backspace goes back) |
| F10 | menu |
| Alt+X | quit |

## Linux notes

- F8 moves files to the desktop Trash (`~/.local/share/Trash`); Shift+F8 deletes permanently.
- Alt+F1/F2 lists your home folder and mounted disks instead of drive letters.
- Hidden files are dot-files; files with the executable bit are shown as programs and run on Enter.
- Some desktop terminals keep Alt+F1/F2 or F10 for themselves; click the key bar or the top row instead.
- With the mouse on, select text in the terminal with Shift+drag.
## License

Apache License 2.0, see [LICENSE](LICENSE).
