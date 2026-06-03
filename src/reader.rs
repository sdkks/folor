use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

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
