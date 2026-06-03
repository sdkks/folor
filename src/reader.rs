use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::Sender;

use crate::output::OutputLine;

/// Read the last `n` lines from a file, returning each line as raw bytes
/// without a trailing newline.
///
/// Uses a heuristic that assumes an average of 200 bytes per line. If the
/// heuristic does not capture enough lines, falls back to reading the entire
/// file.
pub fn read_last_lines(path: &Path, n: usize) -> std::io::Result<Vec<Vec<u8>>> {
    if n == 0 {
        return Ok(Vec::new());
    }

    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    let file_len = metadata.len();

    if file_len == 0 {
        return Ok(Vec::new());
    }

    // Heuristic: seek to max(0, file_len - (N * 200 + 8192))
    let estimate_bytes = (n as u64)
        .saturating_mul(200)
        .saturating_add(8192)
        .min(file_len);
    let seek_pos = file_len - estimate_bytes;

    file.seek(SeekFrom::Start(seek_pos))?;
    let mut buf = Vec::with_capacity(estimate_bytes as usize);
    file.read_to_end(&mut buf)?;

    let lines = split_lines(&buf);

    if lines.len() >= n {
        let start = lines.len() - n;
        return Ok(lines[start..].to_vec());
    }

    // Fallback: not enough lines from the heuristic chunk — read the full file
    if seek_pos > 0 {
        file.seek(SeekFrom::Start(0))?;
        buf.clear();
        file.read_to_end(&mut buf)?;
        let all_lines = split_lines(&buf);
        if all_lines.len() <= n {
            return Ok(all_lines);
        }
        let start = all_lines.len() - n;
        return Ok(all_lines[start..].to_vec());
    }

    Ok(lines)
}

/// Split a byte slice into lines at `\n` boundaries, stripping trailing `\r`
/// and not including the newline character in the result.
///
/// A file ending with a terminating newline does not produce an extra empty
/// line (matching traditional `tail` behavior).
fn split_lines(data: &[u8]) -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = Vec::new();
    let mut start = 0;

    for (i, &b) in data.iter().enumerate() {
        if b == b'\n' {
            let mut line = data[start..i].to_vec();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            lines.push(line);
            start = i + 1;
        }
    }

    // Handle the last segment if data doesn't end with a newline
    if start < data.len() {
        let mut line = data[start..].to_vec();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        lines.push(line);
    }

    lines
}

/// Follow a file in tail mode.
///
/// First, reads and sends the last `lines` lines (same as one-shot mode).
/// Then enters a poll loop: every 50ms, stat() the file. If the size exceeds
/// the current read position, new bytes are read, split into lines, and sent
/// as `OutputLine` messages via the channel.
///
/// If the file shrinks (truncation detected, i.e., size < current position)
/// and `allow_truncation_reset` is true, the position resets to 0 and reading
/// continues from the start of the file.
///
/// The loop exits when `stop` is set to `true` or when the file becomes
/// permanently inaccessible (e.g., deleted and not recreated).
pub fn follow_file(
    path: PathBuf,
    lines: usize,
    tx: Sender<OutputLine>,
    stop: Arc<AtomicBool>,
    allow_truncation_reset: bool,
) {
    // Phase 1: print last N lines (like one-shot)
    if lines > 0 {
        match read_last_lines(&path, lines) {
            Ok(initial_lines) => {
                let display_path = path.clone();
                for line in initial_lines {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let _ = tx.send(OutputLine {
                        display_path: display_path.clone(),
                        line,
                    });
                }
            }
            Err(e) => {
                // File may not exist yet or is unreadable — not fatal in follow mode.
                // We'll pick it up in the poll loop if it becomes available.
                eprintln!("folor: {}: {}", path.display(), e);
            }
        }
    }

    // Phase 2: poll loop
    let poll_interval = Duration::from_millis(50);

    // Set initial position to current file size via a single open+seek+query
    // to avoid the race between read_last_lines and a separate metadata() call.
    let mut position: u64 = File::open(&path)
        .and_then(|mut f| {
            f.seek(SeekFrom::End(0))?;
            f.stream_position()
        })
        .unwrap_or(0);

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                // File disappeared — keep polling in case it reappears.
                thread::sleep(poll_interval);
                continue;
            }
        };

        let current_size = meta.len();

        if current_size < position {
            // Truncation detected.
            if allow_truncation_reset {
                position = 0;
            } else {
                // Treat as EOF — don't read anything new.
                position = current_size;
            }
        }

        if current_size > position {
            let mut file = match File::open(&path) {
                Ok(f) => f,
                Err(_) => {
                    thread::sleep(poll_interval);
                    continue;
                }
            };

            if file.seek(SeekFrom::Start(position)).is_err() {
                thread::sleep(poll_interval);
                continue;
            }

            let to_read = (current_size - position) as usize;
            let mut buf = vec![0u8; to_read];
            match file.read_exact(&mut buf) {
                Ok(()) => {
                    let new_lines = split_lines(&buf);
                    for line in new_lines {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let _ = tx.send(OutputLine {
                            display_path: path.clone(),
                            line,
                        });
                    }
                    position = current_size;
                }
                Err(e) => {
                    eprintln!("folor: {}: read error: {}", path.display(), e);
                }
            }
        }

        if stop.load(Ordering::Relaxed) {
            break;
        }

        thread::sleep(poll_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp_file(content: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.log");
        std::fs::write(&path, content).expect("write");
        (dir, path)
    }

    #[test]
    fn empty_file_returns_empty() {
        let (_dir, path) = write_temp_file(b"");
        let lines = read_last_lines(&path, 10).unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn n_zero_returns_empty() {
        let (_dir, path) = write_temp_file(b"line1\nline2\n");
        let lines = read_last_lines(&path, 0).unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn fewer_lines_than_requested() {
        let (_dir, path) = write_temp_file(b"one\ntwo\n");
        let lines = read_last_lines(&path, 10).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], b"one");
        assert_eq!(lines[1], b"two");
    }

    #[test]
    fn exact_line_count() {
        let content = (1..=20)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let (_dir, path) = write_temp_file(content.as_bytes());
        let lines = read_last_lines(&path, 10).unwrap();
        assert_eq!(lines.len(), 10);
        assert_eq!(lines[0], b"line11");
        assert_eq!(lines[9], b"line20");
    }

    #[test]
    fn file_with_trailing_newline() {
        let (_dir, path) = write_temp_file(b"hello\nworld\n");
        let lines = read_last_lines(&path, 5).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], b"hello");
        assert_eq!(lines[1], b"world");
    }

    #[test]
    fn file_without_trailing_newline() {
        let (_dir, path) = write_temp_file(b"hello\nworld");
        let lines = read_last_lines(&path, 5).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], b"hello");
        assert_eq!(lines[1], b"world");
    }

    #[test]
    fn long_lines_trigger_fallback() {
        // Create a file with very long lines so heuristic chunk is too small
        let long_line = "a".repeat(10_000);
        let mut content = Vec::new();
        for i in 1..=50 {
            content.extend_from_slice(format!("line{}\n", i).as_bytes());
        }
        content.extend_from_slice(long_line.as_bytes());
        content.push(b'\n');
        let (_dir, path) = write_temp_file(&content);
        let lines = read_last_lines(&path, 5).unwrap();
        assert_eq!(lines.len(), 5);
        // The last line should be the long line
        assert_eq!(lines[4].len(), 10_000);
    }

    #[test]
    fn carriage_return_stripped() {
        let (_dir, path) = write_temp_file(b"line1\r\nline2\r\n");
        let lines = read_last_lines(&path, 5).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], b"line1");
        assert_eq!(lines[1], b"line2");
    }

    #[test]
    fn split_lines_basic() {
        let lines = split_lines(b"a\nb\nc");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], b"a");
        assert_eq!(lines[1], b"b");
        assert_eq!(lines[2], b"c");
    }

    #[test]
    fn split_lines_trailing_newline() {
        let lines = split_lines(b"a\nb\n");
        assert_eq!(lines.len(), 2);
    }
}
