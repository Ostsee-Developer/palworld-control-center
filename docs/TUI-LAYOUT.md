# TUI layout

The default overview follows a 64/36 split:

- upper left: real-time server logs
- lower left: backup/restore job and progress
- upper right: CPU, RAM, disk, online players and Palworld version
- lower right: the most important game settings

All secondary functionality lives in focused tabs rather than an overloaded overview.

The visual language uses a near-black background, mint and cyan primary accents, amber warnings and red critical states. Circle indicators (`○ ◔ ◑ ◕ ●`) provide terminal-safe pie-style resource visualization alongside exact percentages and gauges.

The application must remain usable over an ordinary SSH terminal without mouse input, special fonts or true-color support. Color is supplementary; every state also has a textual label.

