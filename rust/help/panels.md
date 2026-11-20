# Panels

The key bar at the bottom shows the F-keys, then other shortcuts as many as fit (`^L` is Ctrl+L). Click
one to press it. Hold Shift, Ctrl or Alt (Windows) to see what the keys do with it.

## Moving around

| Key | What it does |
|---|---|
| **↑ ↓ PgUp PgDn Home End** | move the cursor |
| **Enter** | open a folder, run a program, open a file with its program, browse an archive |
| **Enter** on `..` | the folder above (double click does the same) |
| **Ctrl+\\** | the root of the drive |
| **Tab / Shift+Tab** | the other panel (or the [preview](preview.md) when it is on) |
| **Alt+letter** | quick search: jump to names starting with what you type |
| **Alt+F1 / Alt+F2** | [drives and favourites](favorites.md) for the left / right panel, and [servers](remote.md) |
| **Alt+F10** | [directory tree](tree.md) |
| **Alt+F7** | [find files](find.md) |
| **Alt+Backspace** | folder history: the folders you were in, newest first (type to filter, Del removes one) |

## Files

| Key | What it does |
|---|---|
| **F3** | [view](viewer.md) the file; on a folder: count its size |
| **Shift+F3** | [compare](diff.md) with the same file in the other panel |
| **Alt+F3** | [git diff](diff.md) of the file with the last commit |
| **F4** | [edit](editor.md) the file |
| **Shift+F4** | edit a new file |
| **F5** | copy to the other panel (in an archive: extract) |
| **F6** | move to the other panel |
| **Shift+F6** | rename in place |
| **Alt+F6** | [batch rename](rename.md): new names from a pattern (date, time, counter, ...) |
| **Ctrl+A** | attributes: read-only, hidden, system, archive (Windows) or read / write / run (others), and the time changed |
| **F7** | make a folder |
| **F8, Del** | delete to the Trash |
| **Shift+F8** | delete permanently |

F5, F6, Alt+F6, Ctrl+A and F8 work on the selected files, or on the one under the cursor when nothing is
selected. Enter on a file runs its **Enter** command if the [file commands](usermenu.md) have one.

## Selecting

| Key | What it does |
|---|---|
| **Insert** | select / unselect the file under the cursor |
| **Shift+↑ ↓ PgUp PgDn Home End** | select the files the cursor passes over; starting on a selected file unselects them |
| **Gray +  /  Gray -** | select / unselect files matching a mask (it starts as the extension of the file under the cursor) |
| **Gray \*** | invert the selection |
| **Right click** | select / unselect |

## The panels themselves

| Key | What it does |
|---|---|
| **Ctrl+B** | brief / full view |
| **Ctrl+H** | show / hide hidden files |
| **Ctrl+F** | filter: show only files matching masks (see below) |
| **Ctrl+R** | re-read the folder |
| **Ctrl+F3 ... Ctrl+F6** | sort by name, extension, time, size; again to reverse |
| **Ctrl+U** | swap the panels |
| **Ctrl+P** | hide / show the other panel |
| **Ctrl+G** | [Orthodox game](game.md) |
| **Ctrl+← / Ctrl+→** | narrower / wider (or drag the border with the mouse) |
| **Ctrl+E / Ctrl+Shift+E** | previous / next command from the [history](cmdline.md) |
| **Ctrl+L** | [preview](preview.md) in the other panel |
| **Ctrl+D** | add this folder to [favourites](favorites.md) |
| **Ctrl+T** | [colour theme](themes.md) |

## Tools and the rest

| Key | What it does |
|---|---|
| **F2** | [developer tools](tools.md) |
| **F9** | [user menu](usermenu.md): your commands for this folder, for this kind of file, and everywhere |
| **Ctrl+F2** | [calculator](calc.md) |
| **Alt+F5** | [system monitor](sysmon.md) |
| **F10** | menu (or click the top row) |
| **Alt+X** | quit, staying in the active panel's folder |

## Filter

**Ctrl+F** shows only the files matching masks, `;` between them: `*.rs;*.toml`. A mask with `-` in front
hides: `-*.log`. The list under the masks has the extensions in the folder with how many files have each;
Space ticks one. `*.` means files without an extension. Folders are always shown. The filter shows on the
line above the file info; Del (or an empty filter) shows everything again.

## Temp panel

A list of files and folders from anywhere, to work on them together: copy them somewhere (F5), view, edit,
pack, rename. Open it from **Alt+F1** / **Alt+F2** (drives, for the left / right panel) or the Disk menu. With the Temp panel in the other panel, **F5** adds
the selected files to it instead of copying them (also File → Add to Temp panel). In the Temp panel **F8**
only takes files off the list; the files stay where they are. The list is kept until you remove them.

## Colours

Files get colours by name: programs, archives, documents, images, junk... Options → **File colours** opens
the list (`colour = masks`) in the editor; the first line that matches a name gives its colour. A colour
is a part of the theme (`exec`, `archive`, `doc`, `image`, `junk`, `type`, `string`, ...), one of the 16
colours (`light green`) or `#rrggbb`.

## Command line

Typing goes to the [command line](cmdline.md) under the panels.

## What the marks mean

Next to a name, git marks show changes: **M** modified, **A** added, **?** untracked, **U** conflict,
**•** changes inside a folder. The line above the file info shows the current git branch. A letter in
the top left corner (`[t]`) shows the sort order when it isn't by name; a capital letter means reversed.

## Mouse

Click selects the file, double click opens it (runs a program, enters a folder, `..` goes up), right
click marks, the wheel scrolls. Drag the border between the panels to resize them.