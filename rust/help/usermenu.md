# User menu and file commands

**F9** in the panels opens the user menu: your own commands, one key away. It has three parts:

- **For the file under the cursor**: commands for this kind of file, from the *file commands*
  (Options → Commands for file types). The one named **Enter** also runs when you press Enter on such a file,
  instead of opening it with its program.
- **The folder's menu**: the file `.on-menu` in this folder, or in the nearest folder above it. Put one in
  each project: build, test, deploy...
- **The global menu**: the same everywhere (Options → Global user menu).

Enter (or a double click) runs the command in the panel's folder, like a command typed on the command line.
**F4** edits the file the command comes from; on an "Add..." or "Make..." line it creates the file from a
template and opens it. A letter marked with `&` in a name runs that command at once.

## Menu files

One command per line: `Name = command`. For several commands, leave the right side empty and indent them
below; they run one after another. A line with only `-` is a separator, `#` starts a comment.

```
&Build = cargo build --release
&Test and lint =
    cargo test
    cargo clippy
-
&Open in VS Code = code .
```

## File commands

The same, under a line of masks (`;` between them), indented:

```
*.py;*.pyw
    Enter = python "!.!"
    &Lint = ruff check "!.!"
*.md
    &Preview = glow "!.!"
```

## In commands

| Write | It becomes |
|---|---|
| **!.!** | the file name, `main.py` |
| **!** | the name without its extension, `main` |
| **!\\** (or **!/**) | the panel's folder, with the separator at the end |
| **!&** | the selected files (or the one under the cursor), quoted for the shell |
| **!@** | the name of a file listing them, one per line |
| **!!** | a single `!` |

Names with spaces need quotes: `"!.!"`.

## Keys

| Key | What it does |
|---|---|
| **Enter**, double click | run the command |
| **letter** | run the command with that `&` letter |
| **F4** | edit the menu (or file commands) this line comes from |
| **Esc** | close |
