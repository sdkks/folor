# folor

Recursive `tail -f` with glob pattern matching.

`folor` extends `tail -f` with recursive glob patterns, automatic discovery of new files, time-based filtering, and transparent handling of file rotation and symlinks.

## Install

```bash
cargo install folor
```

Or from source:

```bash
git clone https://github.com/sdkks/folor.git
cd folor
make install
```

## Usage

```bash
# Print last 50 lines of every .log file under the current directory
folor '*.log'

# Print last 20 lines from a specific directory
folor -n 20 -C /var/log '*.log'

# Follow mode: watch files and print new lines as they arrive
folor --tail '*.log'

# Match multiple patterns (union)
folor --tail '*.log' '*.jsonl'

# Pipe to jq — filenames and colors suppressed automatically
folor --tail '*.jsonl' | jq .

# Skip files older than 2 hours
folor --tail --older-than 2h '*.log'

# Stay on one filesystem (skip cross-device symlinks)
folor -x --tail '/var/log/*.log'

# Force filename prefixes even in a pipe
folor --tail --filename '*.log' | grep ERROR

# Read from stdin (when no patterns given)
echo -e "a\nb\nc" | folor -n 2
```

## How it works

- **Discovery**: `globset` + `walkdir` recursively finds files matching your patterns
- **Follow mode**: `notify` watcher + periodic rescan detects new files; per-file reader threads poll every 50ms
- **Rotation**: Tracks files by inode — when logrotate renames a file, `folor` follows the renamed file and starts tailing the new one
- **Output**: Dedicated output thread ensures lines are never interleaved; TTY-aware formatting suppresses filenames and colors when piping

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `-F, --tail` | off | Follow mode: watch files for new lines |
| `-n, --lines` | 50 | Number of lines to print from each file |
| `-C, --directory` | `.` | Working directory for pattern resolution |
| `--older-than` | off | Skip files older than this duration (e.g. `2h`, `30m`) |
| `-x, --one-file-system` | off | Skip files on other filesystems |
| `--no-truncation-reset` | off | Don't reset position on file truncation |
| `--filename` | auto | Always prefix with filename |
| `--no-filename` | auto | Never prefix with filename |
| `--scan-interval` | 2 | Discovery scan interval in seconds |
| `--deleted-grace` | 5 | Seconds before closing reader for deleted file |

## Supported platforms

- macOS
- Linux

## License

MIT
