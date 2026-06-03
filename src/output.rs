use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use termcolor::{Color, ColorSpec, WriteColor};

/// Hash the path into a bucket index in `[0, 12)`.
fn hash_bucket(path: &Path) -> u8 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    (hasher.finish() % 12) as u8
}

/// Compute a deterministic `ColorSpec` for a file path.
///
/// Bucket 0-5 are ANSI colors 1-6 (normal); bucket 6-11 are the same colors
/// in bright (intense) variant. Returns `None` when color output is disabled.
fn color_for_path(path: &Path, use_color: bool) -> Option<ColorSpec> {
    if !use_color {
        return None;
    }
    let idx = hash_bucket(path);
    let base_color = match idx % 6 {
        0 => Color::Red,
        1 => Color::Green,
        2 => Color::Yellow,
        3 => Color::Blue,
        4 => Color::Magenta,
        _ => Color::Cyan,
    };
    let mut spec = ColorSpec::new();
    spec.set_fg(Some(base_color)).set_intense(idx >= 6);
    Some(spec)
}

/// Print lines to a color-capable writer, optionally prefixing each line with
/// the filename.
///
/// - `show_prefix`: whether to emit `display_path: ` before each line.
/// - `use_color`: whether to colorize the prefix (only applies when
///   `show_prefix` is also true).
///
/// Each line is written as raw bytes followed by a newline.
pub fn print_lines<W: WriteColor>(
    writer: &mut W,
    display_path: &Path,
    lines: &[Vec<u8>],
    show_prefix: bool,
    use_color: bool,
) -> std::io::Result<()> {
    let prefix_color = color_for_path(display_path, use_color && show_prefix);

    for line in lines {
        if show_prefix {
            if let Some(ref spec) = prefix_color {
                writer.set_color(spec)?;
            }
            write!(writer, "{}: ", display_path.display())?;
            if prefix_color.is_some() {
                writer.reset()?;
            }
        }
        writer.write_all(line)?;
        writer.write_all(b"\n")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use termcolor::{Ansi, NoColor};

    /// Helper: collect output of `print_lines` with color disabled.
    fn collect_no_color(path: &Path, lines: &[Vec<u8>], show_prefix: bool) -> Vec<u8> {
        let mut buf = NoColor::new(vec![]);
        print_lines(&mut buf, path, lines, show_prefix, false).unwrap();
        buf.into_inner()
    }

    /// Helper: collect output of `print_lines` with ANSI color enabled.
    fn collect_ansi(path: &Path, lines: &[Vec<u8>], show_prefix: bool, use_color: bool) -> Vec<u8> {
        let mut buf = Ansi::new(vec![]);
        print_lines(&mut buf, path, lines, show_prefix, use_color).unwrap();
        buf.into_inner()
    }

    // --- Prefix tests (no color) ---

    #[test]
    fn no_prefix_single_line() {
        let buf = collect_no_color(&PathBuf::from("test.log"), &[b"hello".to_vec()], false);
        assert_eq!(buf, b"hello\n");
    }

    #[test]
    fn with_prefix_multiple_lines() {
        let buf = collect_no_color(
            &PathBuf::from("/var/log/app.log"),
            &[b"line1".to_vec(), b"line2".to_vec()],
            true,
        );
        assert_eq!(buf, b"/var/log/app.log: line1\n/var/log/app.log: line2\n");
    }

    #[test]
    fn empty_lines() {
        let buf = collect_no_color(&PathBuf::from("test.log"), &[], false);
        assert!(buf.is_empty());
    }

    // --- Color cycling tests ---

    #[test]
    fn color_is_deterministic() {
        let path = PathBuf::from("/var/log/app.log");
        let spec1 = color_for_path(&path, true);
        let spec2 = color_for_path(&path, true);
        // Same path always produces the same spec — verify by emitting
        // ANSI output and checking they match.
        let mut buf1 = Ansi::new(vec![]);
        let mut buf2 = Ansi::new(vec![]);
        buf1.set_color(spec1.as_ref().unwrap()).unwrap();
        buf2.set_color(spec2.as_ref().unwrap()).unwrap();
        buf1.reset().unwrap();
        buf2.reset().unwrap();
        assert_eq!(buf1.into_inner(), buf2.into_inner());
    }

    #[test]
    fn different_paths_produce_different_buckets() {
        // With 12 buckets and DefaultHasher, two distinct paths should
        // resolve to different buckets almost always.
        let p1 = PathBuf::from("/var/log/app.log");
        let p2 = PathBuf::from("/var/log/db.log");
        assert_ne!(hash_bucket(&p1), hash_bucket(&p2));
    }

    #[test]
    fn color_disabled_returns_none() {
        let spec = color_for_path(&PathBuf::from("/x.log"), false);
        assert!(spec.is_none());
    }

    // --- Colorized output tests ---

    #[test]
    fn colorized_prefix_contains_ansi_escapes() {
        let buf = collect_ansi(
            &PathBuf::from("test.log"),
            &[b"hello".to_vec()],
            true, // show_prefix
            true, // use_color
        );
        let output = String::from_utf8(buf).unwrap();
        // Should contain ANSI escape sequences
        assert!(output.contains("\x1b["), "expected ANSI escape: {}", output);
        // ANSI escapes are embedded between the prefix and the line content,
        // so strip them before checking the raw text.
        let stripped = strip_ansi(&output);
        assert_eq!(stripped, "test.log: hello\n");
    }

    /// Strip ANSI escape sequences from a string for test assertions.
    fn strip_ansi(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' && chars.peek() == Some(&'[') {
                // Consume until the final byte of the CSI sequence.
                // CSI sequences end with a byte in 0x40-0x7E.
                chars.next(); // skip '['
                loop {
                    match chars.next() {
                        Some(c) if (0x40..=0x7E).contains(&(c as u32)) => break,
                        Some(_) => continue,
                        None => break,
                    }
                }
            } else {
                result.push(ch);
            }
        }
        result
    }

    #[test]
    fn no_color_when_use_color_false() {
        let buf = collect_ansi(
            &PathBuf::from("test.log"),
            &[b"hello".to_vec()],
            true,  // show_prefix
            false, // use_color
        );
        let output = String::from_utf8(buf).unwrap();
        assert!(
            !output.contains("\x1b["),
            "expected no ANSI escapes: {}",
            output
        );
        assert_eq!(output, "test.log: hello\n");
    }

    // --- Filename suppression tests ---

    #[test]
    fn no_prefix_when_single_file() {
        let buf = collect_no_color(&PathBuf::from("only.log"), &[b"data".to_vec()], false);
        assert_eq!(buf, b"data\n");
    }

    #[test]
    fn show_filename_flag_overrides() {
        let buf = collect_no_color(&PathBuf::from("forced.log"), &[b"x".to_vec()], true);
        assert_eq!(buf, b"forced.log: x\n");
    }

    #[test]
    fn no_filename_flag_overrides() {
        let buf = collect_no_color(&PathBuf::from("hidden.log"), &[b"x".to_vec()], false);
        assert_eq!(buf, b"x\n");
    }

    #[test]
    fn use_color_without_prefix_no_ansi() {
        let buf = collect_ansi(
            &PathBuf::from("x.log"),
            &[b"data".to_vec()],
            false, // show_prefix
            true,  // use_color
        );
        assert_eq!(buf, b"data\n");
    }

    #[test]
    fn hash_bucket_spreads_across_range() {
        // With 50 distinct paths we should hit at least 5 different buckets.
        let buckets: std::collections::HashSet<u8> = (0..50)
            .map(|i| hash_bucket(&PathBuf::from(format!("/var/log/file{}.log", i))))
            .collect();
        assert!(
            buckets.len() >= 5,
            "expected at least 5 distinct buckets out of 12, got {}",
            buckets.len()
        );
        for b in &buckets {
            assert!(*b < 12, "bucket {} out of range", b);
        }
    }

    #[test]
    fn hash_bucket_is_stable() {
        // Regression: same path should always produce the same bucket.
        let b1 = hash_bucket(&PathBuf::from("/stable/path.log"));
        let b2 = hash_bucket(&PathBuf::from("/stable/path.log"));
        assert_eq!(b1, b2);
    }
}
