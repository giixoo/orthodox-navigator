# Changelog

All notable changes to Orthodox Navigator. The format follows [Keep a Changelog](https://keepachangelog.com/),
and versions follow [Semantic Versioning](https://semver.org/). To release, add a section for the new version
and push a tag `vX.Y.Z` (see "Release downloads" in the README); the section becomes the release notes.

## [Unreleased]

### Added

- **User menu** (F9): the folder's own menu (`.on-menu` here or above), a global menu, and commands for the
  file under the cursor by its name (Options → Commands for file types); the one named Enter runs on Enter.
  Macros `!.!`, `!`, `!\`, `!&`, `!@`.
- **Folder history** (Alt+Backspace): folders visited, newest first, kept between sessions; type to filter.
- **Filter** (Ctrl+F): show only files matching masks, or hide some (`-*.log`); pick from the extensions in
  the folder.
- **Temp panel** (Alt+F1 / Alt+F2 → Temp panel): files gathered from many folders; F5 to it adds files, F8 in it
  takes them off the list.
- **Attributes** (Ctrl+A): read-only, hidden, system, archive on Windows; read / write / run for owner,
  group and others elsewhere; the time a file was changed. Works on several files at once.
- File colours by mask, editable (Options → File colours).
- Gray + starts with the extension of the file under the cursor.

### Changed

- Alt+F1 opens the drives for the left panel and Alt+F2 for the right one, as in DN (they were this /
  the other panel).

### Fixed

- Batch rename in search results used the relative path as the name.

## [0.5.0] - 2026-11-19

### Added

- One-command install, update and removal from the latest release, without security warnings:
  `install.ps1` (Windows) and `install.sh` (Linux, macOS). See "Install with one command" in the README.
- README: how to build on macOS.
- **Batch rename** (Alt+F6): new names for the selected files from a pattern: name, extension, letters of the
  name, counter, the file's date and time, folder name; find / replace and case. A live list shows every new
  name and what is wrong with it (duplicates, existing files, bad characters) before anything is renamed;
  names may swap or shift safely.
- Developer tools: the text tools start with the file under the cursor in Input (up to 4 MB).

## [0.4.0] - 2026-11-18

### Added

- Shift+↑ ↓ PgUp PgDn Home End (and ← → in the brief view) select the files the cursor passes over, like Insert;
  starting on a selected file unselects instead.
- macOS release: one universal binary (Apple silicon and Intel), alone or in a `.tar.gz` with the shell
  function. CI also tests on macOS.

### Changed

- Developer tools:
  - Settings are text boxes, number boxes and check boxes instead of one free-text field.
  - Each tool keeps its own settings, remembered between runs (JWT secrets only until ON closes).
  - The generators hide Input.
- About shows the program's real version.
- Preview folder summary and the panel's `git:` label: labels gray, values white (as on the free space line).
- VS Code themes highlight the panel's info line (the file under the cursor) like the DN themes.

## [0.3.0] - 2026-11-18

### Added

- **Editor** (F4, Shift+F4 for a new file):
  - Syntax colours; keeps the file's encoding (UTF-8, 1251, 866, UTF-16), line endings and indentation style.
  - Copy uses the system clipboard.
- **Viewer**:
  - Syntax colours for common languages.
  - JSON and XML formatting (F5), line wrap (F2).
  - UTF-16 files, detected with or without a byte order mark.
- **Preview** (Ctrl+L): the other panel shows the file under the cursor, or a summary of the folder. Tab gives it
  the keyboard to scroll and switch modes as in the viewer.
- **Compare**:
  - Two files side by side (Shift+F3) and directories (menu Disk).
  - **Git diff** of a file against HEAD (Alt+F3).
- **Git** in the panels: the current branch, and changes marked on files and folders.
- **Find files** (Alt+F7) by name masks and contained text (UTF-8 and UTF-16); the results become the panel's
  list.
- **Favourite folders** next to the drives (Alt+F1 / Alt+F2): Ctrl+D adds the current folder.
- **Developer tools** (F2):
  - Encoding and escaping: Base64, URL, JSON string escaping, HTML/XML escaping.
  - JWT decode / verify / sign, SHA-256, a regex tester.
  - GUID, Lorem ipsum and password generators.
- **System monitor** (Alt+F5): CPU (total, per core, history), memory and swap, network rates, disks, and processes
  sorted by CPU or memory, with kill.
- **Help** (F1): Far Manager-style help with linked pages for every part of the program, starting with the keys.
  The pages are Markdown files in `rust/help/`, built into the program.
- **Colour themes** (Ctrl+T): every VS Code theme installed on the computer, or "VS Code (current theme)".
- **Command line**:
  - History: Ctrl+E previous, Ctrl+Shift+E next. It includes the shell's own history (PowerShell, bash, zsh).
  - Ctrl+[ / Ctrl+] put the left / right panel's folder on the command line.
  - Ctrl+Ins copies the command line, or the file name when it is empty.
- **User screen** (Ctrl+O) is a working console: type and run commands there; Ctrl+O returns to the panels.
- **Key bar**: the F-keys that do something, then other shortcuts (`^L Preview`, `^P Panels`, ...) as many as fit;
  every item is clickable.
- Release builds:
  - Windows: installer, zip and exe.
  - Linux: `.deb`, `.rpm`, `.tar.gz` and static binaries for x64 and arm64.
  - Built by GitHub Actions; CI runs tests and clippy on Windows and Linux.

### Changed

- Ctrl+P hides / shows the other panel (instead of Ctrl+W / Ctrl+N); the hidden panel comes back as it was.
- The falling-blocks game is now the **Orthodox game**, on Ctrl+G.
- Equal panel widths is now only in the Panel menu; Ctrl+E is command history.
- Enter on `..` goes to the folder above; Backspace no longer does.
- Ctrl+Enter puts the file name on the command line as it is, without quotes.
- Dialogs and help open over the window they came from, which stays visible.
- Panel name columns are never narrower than 20 characters; the full view drops time, date and size columns first.

### Fixed

- Folder names in the directory tree were invisible.
- A stray letter in the calculator's "Copy as" row.
- UTF-16 text files opened as hex in the viewer.

## 0.1.0 - 2026-11-17

First version: two-panel file manager in the spirit of DOS Navigator.

- Panels:
  - One or two panels with mouse support.
  - Copy, move, rename and delete (to the Recycle Bin / Trash).
  - Archive browsing (zip, tar, tar.gz).
- Built-in text / hex viewer; directory tree; calculator; a falling-blocks game; DN Gray and Blue colours.
- Shell integration for PowerShell, bash and zsh: commands run in your own shell session.
