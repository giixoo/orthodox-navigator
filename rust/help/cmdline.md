# Command line and user screen

Whatever you type in the panels goes to the command line at the bottom; **Enter** runs it. Started with
`on` (the shell function), commands run in your own shell session: aliases, functions and variables work,
`cd` is followed, and the commands go to the shell's history.

`cd`, `cd ..`, `~` and drive letters (`d:`) change the active panel's folder; `exit` quits.

| Key | What it does |
|---|---|
| **Enter** | run the command (empty line: open the file under the cursor) |
| **Esc** | clear the command line |
| **Ctrl+E** | previous command from the history (the shell's own history and the commands run from ON) |
| **Ctrl+Shift+E** | next command (after the newest: what you had typed) |
| **Ctrl+Enter** | put the file name on the command line |
| **Ctrl+[  /  Ctrl+]** | put the left / right panel's folder on the command line |
| **Ctrl+Ins** | copy the command line to the clipboard (if it's empty: the file name) |
| **Ctrl+O** | user screen |

## User screen

**Ctrl+O** hides the panels and shows the console with the output of earlier commands. The prompt stays
at the bottom: type and run commands as in a plain shell. **Ctrl+O** again brings the panels back.