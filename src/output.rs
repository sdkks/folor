use std::io::{BufWriter, Write};
use std::path::Path;

/// Print lines to stdout, optionally prefixing each line with the filename.
///
/// Each line is written as raw bytes followed by a newline.
pub fn print_lines(
    writer: &mut BufWriter<impl Write>,
    display_path: &Path,
    lines: &[Vec<u8>],
    prefix: bool,
) -> std::io::Result<()> {
    for line in lines {
        if prefix {
            write!(writer, "{}: ", display_path.display())?;
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

    #[test]
    fn no_prefix_single_line() {
        let mut buf = Vec::new();
        {
            let mut writer = BufWriter::new(&mut buf);
            print_lines(
                &mut writer,
                &PathBuf::from("test.log"),
                &[b"hello".to_vec()],
                false,
            )
            .unwrap();
        }
        assert_eq!(buf, b"hello\n");
    }

    #[test]
    fn with_prefix_multiple_lines() {
        let mut buf = Vec::new();
        {
            let mut writer = BufWriter::new(&mut buf);
            print_lines(
                &mut writer,
                &PathBuf::from("/var/log/app.log"),
                &[b"line1".to_vec(), b"line2".to_vec()],
                true,
            )
            .unwrap();
        }
        assert_eq!(buf, b"/var/log/app.log: line1\n/var/log/app.log: line2\n");
    }

    #[test]
    fn empty_lines() {
        let mut buf = Vec::new();
        {
            let mut writer = BufWriter::new(&mut buf);
            print_lines(&mut writer, &PathBuf::from("test.log"), &[], false).unwrap();
        }
        assert!(buf.is_empty());
    }
}
