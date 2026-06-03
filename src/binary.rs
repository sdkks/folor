use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Read the first 8 KiB of a file and check if it contains binary content.
/// Returns `true` and prints a warning to stderr if the file is binary.
pub fn is_binary_file(path: &Path) -> bool {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = vec![0u8; 8192];
    let n = match file.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };
    buf.truncate(n);
    let binary = is_binary(&buf);
    if binary {
        eprintln!("folor: {}: skipping (binary file)", path.display());
    }
    binary
}

/// Inspect the first chunk of a file for binary content.
/// Returns `true` if the buffer contains a null byte (`0x00`) or if more than
/// 30% of the bytes are non-printable control characters (excluding `\t`, `\n`, `\r`).
pub fn is_binary(chunk: &[u8]) -> bool {
    if chunk.is_empty() {
        return false;
    }

    // Check for null byte (definitive binary signal)
    if chunk.contains(&0x00) {
        return true;
    }

    // Count control characters (excluding \t, \n, \r)
    let control_count = chunk
        .iter()
        .filter(|&&b| b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r')
        .count();

    // More than 30% control characters = binary
    (control_count as f64) / (chunk.len() as f64) > 0.30
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_not_binary() {
        assert!(!is_binary(&[]));
    }

    #[test]
    fn null_byte_is_binary() {
        let data = [0x48, 0x65, 0x00, 0x6c, 0x6f]; // "He\x00lo"
        assert!(is_binary(&data));
    }

    #[test]
    fn normal_text_is_not_binary() {
        let data = b"Hello, world!\nThis is a log file.\n";
        assert!(!is_binary(data));
    }

    #[test]
    fn high_control_chars_is_binary() {
        // 10 bytes, 4 of them are control chars (excluding tab/newline/cr) = 40% > 30%
        let data = [
            0x01, 0x02, 0x03, 0x04, // 4 control chars
            b'a', b'b', b'c', b'd', b'e', b'f', // 6 printable
        ];
        assert!(is_binary(&data));
    }

    #[test]
    fn exactly_30_percent_is_not_binary() {
        // 10 bytes, 3 control chars = 30% — not over threshold
        let data = [
            0x01, 0x02, 0x03, // 3 control chars
            b'a', b'b', b'c', b'd', b'e', b'f', b'g', // 7 printable
        ];
        assert!(!is_binary(&data));
    }

    #[test]
    fn tabs_newlines_carriage_returns_are_not_control() {
        let data = b"line1\tcol2\r\nline2\tcol2\r\n";
        assert!(!is_binary(data));
    }

    #[test]
    fn just_under_30_percent_is_not_binary() {
        // 100 bytes, 30 control = exactly 30%, false. 29 control = true.
        let mut data = vec![b'a'; 100];
        for i in 0..29 {
            data[i] = 0x01;
        }
        assert!(!is_binary(&data));
    }

    #[test]
    fn just_over_30_percent_is_binary() {
        let mut data = vec![b'a'; 100];
        for i in 0..31 {
            data[i] = 0x01;
        }
        assert!(is_binary(&data));
    }
}
