# System monitor

**Alt+F5** in the panels. Refreshes every second.

- **CPU**: total load with a history line, model, cores, frequency, and a bar for each core
- **RAM and swap**: used, total, available, free
- **Network**: download and upload speed with history, per interface with totals
- **Disks**: used and free space
- **Processes**: the busiest ones

| Key | What it does |
|---|---|
| **↑ ↓ PgUp PgDn Home End** | move in the process list |
| **F6** | sort processes by CPU / memory |
| **F8** | kill the process under the cursor (asks first) |
| **Esc** | close |

A process's CPU % is of the whole machine, so they add up to the total load. Bars turn yellow at 70%
and red at 90%.