# Find files

**Alt+F7** in the panels. Searches the current folder and every folder below it.

- **File names**: masks, with `;` between them: `*.rs;*.toml`
- **Containing text**: optional; any case. UTF-16 files are searched too, binary files skipped.

**Tab** switches between the two fields, **Enter** starts, **Esc** stops a running search (keeping what
it found so far).

The results become the panel's list. **Enter** on a file goes to its folder and puts the cursor on it;
`..` goes back to the folder. F3, F4, F5 and the other file keys work on the results.