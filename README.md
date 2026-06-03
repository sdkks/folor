# folor

Recursive `tail -f` with glob pattern matching.

`folor` extends `tail -f` with recursive glob patterns, automatic discovery of new files, time-based filtering, and transparent handling of file rotation and symlinks.

## Install

```bash
# Cargo
cargo install folor

# Homebrew
brew tap sdkks/tap && brew install sdkks/tap/folor
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
# Use -R to read raw strings, fromjson? to skip invalid lines gracefully
folor --tail '*.jsonl' | jq -R 'fromjson?'

# Only tail files modified in the last 2 hours
folor --tail --newer-than 2h '*.log'

# Only tail files modified more than 2 hours ago
folor --tail --older-than 2h '*.log'

# Stay on one filesystem (skip cross-device symlinks)
folor -x --tail '/var/log/*.log'

# Force filename prefixes even in a pipe
folor --tail --filename '*.log' | grep ERROR

# Read from stdin (when no patterns given)
echo -e "a\nb\nc" | folor -n 2
```

## Why folor?

`tail -f` can't handle recursive globs or pick up newly created files. The standard workaround is fragile:

```bash
# 😵 before — find the most recent jsonl file and tail it, hope no new ones appear
tail -f $(find ~/.claude/projects -type f -name '*.jsonl' \
  -exec stat -f '%m|%N' {} \; | sort -rnk1 -t '|' | head -1 | \
  awk -F '|' '{print $2}') | jq .
```

```bash
# 🎯 with folor — tail only recent jsonl files, one line each, pipe to jq
folor --tail -n 1 --newer-than 2h -C ~/.claude/projects '*.jsonl' | jq -R 'fromjson?'

# Or convert to YAML on the way out with another one of my tools
folor --tail -n 1 --newer-than 2h -C ~/.claude/projects '*.jsonl' | nesdit --format jsonl --output-format yaml
```

More on `nesdit` [here.](https://github.com/sdkks/nesdit)

## How it works

- **Discovery**: `globset` + `walkdir` recursively finds files matching your patterns
- **Follow mode**: `notify` watcher + periodic rescan detects new files; per-file reader threads poll every 50ms
- **Rotation**: Tracks files by inode — when logrotate renames a file, `folor` follows the renamed file and starts tailing the new one
- **Output**: Dedicated output thread ensures lines are never interleaved; TTY-aware formatting suppresses filenames and colors when piping

## Flags

| Flag                    | Default | Description                                                                |
| ----------------------- | ------- | -------------------------------------------------------------------------- |
| `-F, --tail`            | off     | Follow mode: watch files for new lines                                     |
| `-n, --lines`           | 50      | Number of lines to print from each file                                    |
| `-C, --directory`       | `.`     | Working directory for pattern resolution                                   |
| `--newer-than`          | off     | Only files modified in the last N (e.g. `2h`, `30m`)                       |
| `--older-than`          | off     | Only files modified earlier than N                                         |
| `-x, --one-file-system` | off     | Skip files on other filesystems                                            |
| `--no-truncation-reset` | off     | Don't reset position on file truncation                                    |
| `--filename`            | auto    | Always prefix with filename                                                |
| `--no-filename`         | auto    | Never prefix with filename                                                 |
| `--retry`               | off     | Track by filename — follow recreated files across renames (like `tail -F`) |
| `--idle-timeout`        | off     | Exit after N seconds with no output from any file                          |
| `--pid`                 | off     | Exit when process with PID N terminates (Unix only)                        |
| `--scan-interval`       | 2       | Discovery scan interval in seconds                                         |
| `--deleted-grace`       | 5       | Seconds before closing reader for deleted file                             |

## Supported platforms

- macOS
- Linux

## License

[MIT](./LICENSE)
