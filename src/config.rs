use clap::Parser;
use std::path::PathBuf;
use std::time::Duration;

/// Parse a human-readable duration string (e.g. "2h", "30m", "1d", "90s") into a
/// `std::time::Duration`. Used as a `value_parser` for `--older-than`.
fn parse_duration(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}

/// CLI configuration, parsed from clap derive.
#[derive(Parser)]
#[command(
    name = "folor",
    version,
    about = "Recursive tail -f with glob pattern matching"
)]
pub struct Cli {
    /// File paths or glob patterns to follow. If omitted, reads from stdin.
    #[arg(value_name = "PATTERN", num_args = 0..)]
    pub patterns: Vec<String>,

    /// Follow mode: watch files and print new lines as they appear
    #[arg(short = 'F', long = "tail", default_value_t = false)]
    pub follow: bool,

    /// Number of lines to print from each file
    #[arg(short = 'n', long = "lines", default_value = "50")]
    pub lines: usize,

    /// Working directory for resolving relative paths and patterns
    #[arg(short = 'C', long = "directory")]
    pub directory: Option<PathBuf>,

    /// Only tail files modified more recently than this (e.g. 2h, 30m, 1d)
    #[arg(long = "newer-than", value_parser = parse_duration)]
    pub newer_than: Option<Duration>,

    /// Only tail files whose mtime is older than this (e.g. 2h, 30m, 1d)
    #[arg(long = "older-than", value_parser = parse_duration)]
    pub older_than: Option<Duration>,

    /// Do not cross filesystem boundaries (skip cross-device symlinks)
    #[arg(short = 'x', long = "one-file-system", default_value_t = false)]
    pub one_file_system: bool,

    /// Disable truncation detection (truncated files treated as EOF)
    #[arg(long = "no-truncation-reset", default_value_t = false)]
    pub no_truncation_reset: bool,

    /// Always prefix output lines with the source filename
    #[arg(
        long = "filename",
        default_value_t = false,
        conflicts_with = "no_filename"
    )]
    pub filename: bool,

    /// Never prefix output lines with the source filename
    #[arg(
        long = "no-filename",
        default_value_t = false,
        conflicts_with = "filename"
    )]
    pub no_filename: bool,

    /// File discovery scan interval in seconds
    #[arg(long = "scan-interval", default_value = "2")]
    pub scan_interval: u64,

    /// Grace period in seconds before closing a reader for a deleted inode
    #[arg(long = "deleted-grace", default_value = "5")]
    pub deleted_grace: u64,
}

/// Validated configuration derived from CLI arguments.
#[derive(Debug, Clone)]
pub struct Config {
    /// File paths or glob patterns to follow. Empty means stdin fallback.
    pub patterns: Vec<String>,
    /// Whether to run in follow mode.
    pub follow: bool,
    /// Number of lines to print per file.
    pub lines: usize,
    /// Working directory for relative path resolution.
    pub directory: Option<PathBuf>,
    /// Only include files modified more recently than this.
    pub newer_than: Option<Duration>,
    /// Only include files modified earlier than this.
    pub older_than: Option<Duration>,
    /// Stay within a single filesystem.
    pub one_file_system: bool,
    /// Disable truncation detection.
    pub no_truncation_reset: bool,
    /// Force filename prefixes in output.
    pub show_filename: bool,
    /// Suppress filename prefixes in output.
    pub no_filename: bool,
    /// File discovery scan interval in seconds.
    pub scan_interval: u64,
    /// Grace period in seconds before closing a reader for a deleted inode.
    #[allow(dead_code)]
    pub deleted_grace: u64,
}

impl Config {
    /// Maximum allowed value for `--lines` to prevent accidental OOM from
    /// unbounded buffer allocation when reading huge files.
    pub const MAX_LINES: usize = 1_000_000;

    /// Validate and convert CLI arguments into a `Config`.
    ///
    /// # Errors
    ///
    /// Returns an error string if any values are invalid:
    /// - `lines` must be in [1, MAX_LINES].
    /// - `scan_interval` must be greater than 0.
    /// - `deleted_grace` must be greater than 0.
    pub fn from_cli(cli: Cli) -> Result<Self, String> {
        if cli.lines > Self::MAX_LINES {
            return Err(format!(
                "--lines must not exceed {} (got {})",
                Self::MAX_LINES,
                cli.lines
            ));
        }
        if cli.scan_interval == 0 {
            return Err("--scan-interval must be greater than 0".to_string());
        }
        if cli.deleted_grace == 0 {
            return Err("--deleted-grace must be greater than 0".to_string());
        }

        Ok(Config {
            patterns: cli.patterns,
            follow: cli.follow,
            lines: cli.lines,
            directory: cli.directory,
            newer_than: cli.newer_than,
            older_than: cli.older_than,
            one_file_system: cli.one_file_system,
            no_truncation_reset: cli.no_truncation_reset,
            show_filename: cli.filename,
            no_filename: cli.no_filename,
            scan_interval: cli.scan_interval,
            deleted_grace: cli.deleted_grace,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cli() -> Cli {
        Cli {
            patterns: vec![],
            follow: false,
            lines: 50,
            directory: None,
            newer_than: None,
            older_than: None,
            one_file_system: false,
            no_truncation_reset: false,
            filename: false,
            no_filename: false,
            scan_interval: 2,
            deleted_grace: 5,
        }
    }

    #[test]
    fn valid_defaults() {
        let cli = make_cli();
        let cfg = Config::from_cli(cli).expect("defaults should be valid");
        assert_eq!(cfg.lines, 50);
        assert_eq!(cfg.scan_interval, 2);
        assert_eq!(cfg.deleted_grace, 5);
    }

    #[test]
    fn lines_zero_accepted() {
        let mut cli = make_cli();
        cli.lines = 0;
        let cfg = Config::from_cli(cli).expect("lines=0 should be valid");
        assert_eq!(cfg.lines, 0);
    }

    #[test]
    fn lines_above_max_rejected() {
        let mut cli = make_cli();
        cli.lines = Config::MAX_LINES + 1;
        assert!(Config::from_cli(cli).is_err());
    }

    #[test]
    fn lines_at_max_accepted() {
        let mut cli = make_cli();
        cli.lines = Config::MAX_LINES;
        assert!(Config::from_cli(cli).is_ok());
    }

    #[test]
    fn scan_interval_zero_rejected() {
        let mut cli = make_cli();
        cli.scan_interval = 0;
        assert!(Config::from_cli(cli).is_err());
    }

    #[test]
    fn deleted_grace_zero_rejected() {
        let mut cli = make_cli();
        cli.deleted_grace = 0;
        assert!(Config::from_cli(cli).is_err());
    }

    #[test]
    fn parse_duration_valid() {
        assert!(parse_duration("30s").is_ok());
        assert!(parse_duration("5m").is_ok());
        assert!(parse_duration("2h").is_ok());
        assert!(parse_duration("1d").is_ok());
    }

    #[test]
    fn parse_duration_invalid() {
        assert!(parse_duration("xyz").is_err());
        assert!(parse_duration("").is_err());
    }

    #[test]
    fn directory_is_some() {
        let mut cli = make_cli();
        cli.directory = Some("/tmp".into());
        let cfg = Config::from_cli(cli).unwrap();
        assert_eq!(cfg.directory, Some("/tmp".into()));
    }

    #[test]
    fn older_than_is_some() {
        let mut cli = make_cli();
        cli.older_than = Some(std::time::Duration::from_secs(3600));
        let cfg = Config::from_cli(cli).unwrap();
        assert_eq!(cfg.older_than, Some(std::time::Duration::from_secs(3600)));
    }

    #[test]
    fn newer_than_is_some() {
        let mut cli = make_cli();
        cli.newer_than = Some(std::time::Duration::from_secs(3600));
        let cfg = Config::from_cli(cli).unwrap();
        assert_eq!(cfg.newer_than, Some(std::time::Duration::from_secs(3600)));
    }
}
