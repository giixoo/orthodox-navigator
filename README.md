# Orthodox Navigator (prototype)

A two-panel console file manager in the spirit of DOS Navigator, for Windows, Linux and macOS.

Written in Rust (`rust/`). Build it and copy the program to `bin/`, which is where the shell functions
start it from (so you can rebuild while it is running):

    cd rust
    cargo build --release
    copy target\release\on.exe ..\bin\on.exe       # Windows
    mkdir -p ../bin && cp target/release/on ../bin/on   # Linux / macOS

Works best in Windows Terminal or any terminal with 24-bit colour. Colours are sent as exact VGA values;
set `ON_COLORS=16` to use the terminal's own 16-colour palette instead.

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

- One or two panels (Ctrl+W hides the active one, Ctrl+N brings the second back); resize with Ctrl+←/→,
  Ctrl+E for equal widths, or drag the border with the mouse.
- Mouse: click selects, double-click opens, right-click marks files, wheel scrolls, the key bar and dialog
  buttons are clickable, a click on the top row opens the menu.
- Archives: Enter browses inside `.zip` (and `.jar`, `.nupkg`, `.docx`, ...), `.tar`, `.tar.gz`/`.tgz`.
  F3 views a file, Enter opens it, F5 extracts. Archives are read-only for now.
- Directory tree (Alt+F10), calculator with DEC/HEX/BIN/OCT/EXP (Ctrl+F2), a falling-blocks game (menu Utilities).
- Built-in viewer (text in UTF-8 / 1251 / 866, or hex), copy/move/delete with overwrite prompts, Trash.
- DN Gray and Blue colour schemes (Ctrl+T).

## Keys

| Key | Action |
|---|---|
| Tab / Shift+Tab | next / previous panel |
| Ctrl+U | swap the two panels |
| Ctrl+W / Ctrl+N | hide the active panel / show two panels again |
| Ctrl+← / Ctrl+→ / Ctrl+E | narrower / wider / equal widths |
| Enter, Backspace | open, run or browse an archive / parent folder |
| Insert, Gray + − * | select file / select group / unselect group / invert |
| Alt+letter | quick search |
| F3 | view (text/hex, UTF-8/1251/866); on a folder: calculate its size |
| F4 | edit (`$EDITOR`; otherwise notepad on Windows, vi elsewhere) |
| F5 / F6 / Shift+F6 | copy (from an archive: extract) / move / rename in place |
| F7 | make folder (nested paths work) |
| F8, Del / Shift+F8, Shift+Del | to Recycle Bin or Trash / delete permanently |
| Alt+F1 / Alt+F2 | drive menu for this / the other panel |
| Alt+F10 | directory tree |
| Ctrl+F2 | calculator |
| Ctrl+F3..F6 | sort by name / ext / time / size (press again to reverse) |
| Ctrl+B / Ctrl+H / Ctrl+R | brief↔full view / hidden files / re-read |
| Ctrl+T | switch colour scheme (DN Gray / Blue), remembered between runs |
| Ctrl+O | user screen (output of the last command) |
| Ctrl+Enter | put the file name on the command line |
| F9, F10 | menu |
| Alt+X | quit |

## Linux notes

- F8 moves files to the desktop Trash (`~/.local/share/Trash`); Shift+F8 deletes permanently.
- Alt+F1/F2 lists your home folder and mounted disks instead of drive letters.
- Hidden files are dot-files; files with the executable bit are shown as programs and run on Enter.
- Some desktop terminals keep Alt+F1/F2 or F10 for themselves; use F9 for the menu.
- With the mouse on, select text in the terminal with Shift+drag.

## License

Apache License 2.0, see [LICENSE](LICENSE).
