# Developer tools

**F2** in the panels. Pick a tool on the left; type or paste into Input, and the Output follows as you
type. Tools with settings show them above Input as boxes and check boxes. Each tool keeps its own settings,
and they are remembered between runs (except JWT secrets, which are kept only until ON closes).

- Base64 encode / decode, URL encode / decode
- JSON string escape / unescape, HTML/XML escape / unescape
- JWT decode and verify (HS256/384/512: put the secret in the Secret box), JWT sign (HS256)
- SHA-256
- Regex tester: matches highlighted, groups listed; Ignore case and Multi-line check boxes
- GUID generator: Count, Upper case, Braces, No dashes
- Lorem ipsum: Count of paragraphs, or of words with "Words, not paragraphs"
- Password generator: Length, Count, Symbols, No look-alikes (no I l 1 O 0 o). Every password has a
  lower-case and an upper-case letter, a digit and (with Symbols) a symbol.

The generators don't read Input, so it is hidden for them and Output takes the whole height.

| Key | What it does |
|---|---|
| **↑ ↓** | pick a tool (in the list); change a number by one (in a number box) |
| **Tab / Shift+Tab** | list → each setting → Input → Output |
| **Space** | tick / untick a check box (a click does the same) |
| **Enter** | in the list: to the settings; in a box: to Input |
| **F2** | copy the output to the clipboard |
| **F3** | load a file into Input (binary files stay bytes) |
| **F4** | move the output into Input, to chain tools |
| **F5** | save the output to a file |
| **F6** | generate new values (GUIDs, text, passwords) |
| **Ctrl+Y** | clear Input (or the box you are in) |
| **Esc** | close |
