# Viewer

**F3** in the panels. Shows text with syntax colours, or any file in hex.

| Key | What it does |
|---|---|
| **↑ ↓ PgUp PgDn Home End** | scroll |
| **← →** | scroll sideways |
| **F2** | wrap long lines on / off |
| **F4** | hex / text |
| **F5** | JSON and XML: formatted / as it is |
| **F6** | syntax colours on / off |
| **F8** | code page: UTF-8, Windows-1251, DOS-866, UTF-16 |
| **Esc** | close |

The code page is guessed when the file opens: UTF-16 files (what PowerShell's `>` writes) are recognised.
Files over 32 MB show their first 32 MB.

The same keys work in the [preview panel](preview.md).