# Changelog

All notable changes to Orthodox Navigator. The format follows [Keep a Changelog](https://keepachangelog.com/),
and versions follow [Semantic Versioning](https://semver.org/). To release, add a section for the new version
and push a tag `vX.Y.Z` (see "Install a release" in the README); the section becomes the release notes.

## [Unreleased]

### Added

- Shift+↑ ↓ PgUp PgDn Home End (and ← → in the brief view) select the files the cursor passes over, like Insert;
  starting on a selected file unselects instead.

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
