# Compare files and git

**Shift+F3** compares the file under the cursor with the file of the same name in the other panel (or
with the file under the other panel's cursor). **Alt+F3** compares it with its last committed version
(git HEAD).

Left is the first file, right the second. **~** changed, **-** only on the left, **+** only on the right.

| Key | What it does |
|---|---|
| **F7 / Shift+F7** | next / previous change |
| **↑ ↓ PgUp PgDn Home End ← →** | scroll |
| **Esc** | close |

## Git in the panels

Files changed since the last commit are marked: **M** modified, **A** added, **?** untracked,
**U** conflict, **•** changes inside a folder. The current branch shows above the file info.