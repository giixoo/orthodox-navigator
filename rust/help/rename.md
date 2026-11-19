# Batch rename

**Alt+F6** in the panels renames the selected files (or the one under the cursor) at once. Type a pattern
for the new names; the list below shows each old name and its new one as you type. Nothing is renamed
until you press **Enter**.

## The pattern

**Name** and **Extension** are text with placeholders in square brackets. The new name is the Name, then
a dot and the Extension (no dot when the Extension comes out empty).

| Placeholder | What it becomes |
|---|---|
| **[N]** | the old name, without the extension |
| **[N2-5]** | letters 2 to 5 of the old name; **[N3]** just letter 3, **[N4-]** from letter 4 to the end |
| **[E]** | the old extension (without the dot) |
| **[C]** | a counter: from **[C] from**, going up by **step**, with at least **digits** digits (3: 001, 002, ...) |
| **[P]** | the name of the folder the file is in |
| **[DATE]** | the file's date: 2026-11-19 |
| **[TIME]** | the file's time: 08-05-09 (a colon can't be in a file name) |
| **[Y] [M] [D]** | year, month, day of the file's date |
| **[h] [m] [s]** | hours, minutes, seconds of its time |

The date and time are when the file was last changed. Anything else in brackets stays as you typed it.

Then, on the whole new name:

- **Find** / **Replace with**: replaces every occurrence of the text (exact case).
- **lower case** / **UPPER CASE**: changes the case.

Examples, for `IMG_4410.JPG` changed on 19 November 2026 at 08:05:

| Name | Extension | Other | New name |
|---|---|---|---|
| `[DATE] [N]` | `[E]` | | `2026-11-19 IMG_4410.JPG` |
| `Holiday [C]` | `[E]` | digits 3 | `Holiday 001.JPG` |
| `[Y][M][D]_[h][m][s]` | `jpg` | | `20261119_080509.jpg` |
| `[N5-]` | `[E]` | lower case | `4410.jpg` |

## Problems

A new name in red can't be used, and Enter won't rename until you fix the pattern:

- **same name twice**: two files would get the same name (add [C] to tell them apart);
- **exists**: a file that isn't being renamed already has that name;
- **bad character** or **bad name**: a character a file name can't have (`/`, and on Windows `\ : * ? " < > |`),
  or a name Windows doesn't allow (ending in a dot or space).

Files may swap or shift names (`1.txt` → `2.txt` while `2.txt` → `3.txt`): ON renames them through
temporary names, so nothing gets overwritten.

## Keys

| Key | What it does |
|---|---|
| **Tab / Shift+Tab** | next / previous box, then the list |
| **↑ ↓** | in a number box: change it by one; in a text box: the row above / below; in the list: scroll |
| **Space** | tick / untick a check box (a click does the same) |
| **Enter, F2** | rename |
| **Esc** | close without renaming |

The pattern is kept for the next batch rename until ON closes.
