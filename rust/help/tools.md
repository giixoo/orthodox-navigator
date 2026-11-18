# Developer tools

**F2** in the panels. Pick a tool on the left; type or paste into Input, and the Output follows as you
type.

- Base64 encode / decode, URL encode / decode
- JSON string escape / unescape, HTML/XML escape / unescape
- JWT decode and verify (HS256/384/512: put the secret in the field), JWT sign (HS256)
- SHA-256
- Regex tester: matches highlighted, groups listed
- GUID generator: the field takes a count and `upper`, `braces`, `nodash` (e.g. `10 upper`)
- Lorem ipsum: the field takes paragraphs (`3`) or words (`50w`)
- Password generator: the field takes length, count, `nosym` (letters and digits only), `noamb` (no I l 1 O 0 o),
  e.g. `16 10 noamb`. Every password has a lower-case and an upper-case letter, a digit and a symbol.

| Key | What it does |
|---|---|
| **↑ ↓** | pick a tool (in the list) |
| **Tab / Shift+Tab** | list → secret / pattern → Input → Output |
| **F2** | copy the output to the clipboard |
| **F3** | load a file into Input (binary files stay bytes) |
| **F4** | move the output into Input, to chain tools |
| **F5** | save the output to a file |
| **F6** | generate new values (GUIDs, text, passwords) |
| **Ctrl+Y** | clear Input |
| **Esc** | close |