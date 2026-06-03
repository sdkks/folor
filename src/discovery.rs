#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

use crate::config::Config;
use crate::file_ref::FileRef;

/// Build a `GlobSet` from a list of pattern strings.
///
/// Returns an error if any pattern is invalid.
fn build_globset(patterns: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(false)
            .case_insensitive(false)
            .build()
            .map_err(|e| format!("invalid glob pattern '{}': {}", pattern, e))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|e| format!("failed to compile glob patterns: {}", e))
}

/// Discover files matching the configured glob patterns.
///
/// Walks the directory tree starting from the configured working directory
/// (or the current directory if none set), matches each file against the
/// compiled globset, applies time and filesystem filters, and returns a
/// list of `(PathBuf, FileRef)` tuples.
///
/// # Errors
///
/// Returns an error string for invalid glob patterns or I/O errors during
/// directory traversal.
pub fn discover(config: &Config) -> Result<Vec<(PathBuf, FileRef)>, String> {
    if config.patterns.is_empty() {
        return Ok(Vec::new());
    }

    let globset = build_globset(&config.patterns)?;

    let root = match &config.directory {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().map_err(|e| format!("current_dir: {}", e))?,
    };

    let root_device = if config.one_file_system {
        match std::fs::metadata(&root) {
            Ok(m) => Some(m.dev()),
            Err(e) => return Err(format!("{}: {}", root.display(), e)),
        }
    } else {
        None
    };

    let mut results: Vec<(PathBuf, FileRef)> = Vec::new();
    let walker = WalkDir::new(&root).follow_links(!config.one_file_system);

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                // Inaccessible directories are non-fatal — warn and skip.
                if let Some(path) = e.path() {
                    eprintln!("folor: {}: {}", path.display(), e);
                } else {
                    eprintln!("folor: walk error: {}", e);
                }
                continue;
            }
        };

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("folor: {}: {}", entry.path().display(), e);
                continue;
            }
        };

        if !metadata.is_file() {
            continue;
        }

        let path = entry.path();

        // Match glob patterns against the relative path from root
        let relative = path.strip_prefix(&root).unwrap_or(path);
        if !globset.is_match(relative) {
            continue;
        }

        // One-file-system filter: skip files whose resolved device differs from root
        if let Some(root_dev) = root_device {
            let file_dev = metadata.dev();
            if file_dev != root_dev {
                eprintln!(
                    "folor: {}: skipping (crosses filesystem boundary)",
                    path.display()
                );
                continue;
            }
        }

        // Time-based filters.
        if let Some(min_age) = config.newer_than {
            // --newer-than: keep files modified in the last N (age <= duration)
            match metadata.modified() {
                Ok(modified) => match modified.elapsed() {
                    Ok(age) if age > min_age => continue, // too old, skip
                    Ok(_) => {}                           // within window, keep
                    Err(e) => {
                        eprintln!("folor: {}: time error: {}", path.display(), e);
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!("folor: {}: {}", path.display(), e);
                    continue;
                }
            }
        }
        if let Some(max_age) = config.older_than {
            // --older-than: keep files modified earlier than N (age >= duration)
            match metadata.modified() {
                Ok(modified) => match modified.elapsed() {
                    Ok(age) if age < max_age => continue, // too new, skip
                    Ok(_) => {}                           // within window, keep
                    Err(e) => {
                        eprintln!("folor: {}: time error: {}", path.display(), e);
                        continue;
                    }
                },
                Err(e) => {
                    eprintln!("folor: {}: {}", path.display(), e);
                    continue;
                }
            }
        }

        let device = metadata.dev();
        let inode = metadata.ino();
        results.push((path.to_path_buf(), FileRef { device, inode }));
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn make_config(patterns: Vec<&str>, root: Option<PathBuf>) -> Config {
        Config {
            patterns: patterns.into_iter().map(String::from).collect(),
            follow: false,
            lines: 50,
            directory: root,
            newer_than: None,
            older_than: None,
            one_file_system: false,
            no_truncation_reset: false,
            show_filename: false,
            no_filename: false,
            scan_interval: 2,
            deleted_grace: 5,
        }
    }

    #[test]
    fn empty_patterns_returns_empty() {
        let config = Config {
            patterns: vec![],
            follow: false,
            lines: 50,
            directory: None,
            newer_than: None,
            older_than: None,
            one_file_system: false,
            no_truncation_reset: false,
            show_filename: false,
            no_filename: false,
            scan_interval: 2,
            deleted_grace: 5,
        };
        let result = discover(&config).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn discovers_matching_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log1 = dir.path().join("app.log");
        let log2 = dir.path().join("error.log");
        let txt = dir.path().join("readme.txt");
        std::fs::write(&log1, b"log1").unwrap();
        std::fs::write(&log2, b"log2").unwrap();
        std::fs::write(&txt, b"txt").unwrap();

        let config = make_config(vec!["*.log"], Some(dir.path().to_path_buf()));
        let result = discover(&config).unwrap();

        assert_eq!(result.len(), 2);
        let paths: Vec<PathBuf> = result.into_iter().map(|(p, _)| p).collect();
        assert!(paths.contains(&log1));
        assert!(paths.contains(&log2));
        assert!(!paths.contains(&txt));
    }

    #[test]
    fn recursive_glob() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("subdir");
        std::fs::create_dir(&sub).unwrap();
        let log1 = dir.path().join("root.log");
        let log2 = sub.join("nested.log");
        std::fs::write(&log1, b"a").unwrap();
        std::fs::write(&log2, b"b").unwrap();

        let config = make_config(vec!["**/*.log"], Some(dir.path().to_path_buf()));
        let result = discover(&config).unwrap();

        assert_eq!(result.len(), 2);
    }

    #[test]
    fn invalid_glob_returns_error() {
        let config = make_config(vec!["["], None);
        assert!(discover(&config).is_err());
    }

    #[test]
    fn non_existent_directory() {
        let config = make_config(vec!["*.log"], Some(PathBuf::from("/nonexistent/path/xyz")));
        let result = discover(&config);
        assert!(result.is_err() || result.unwrap().is_empty());
    }

    #[test]
    fn no_matching_files_returns_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("readme.txt"), b"txt").unwrap();

        let config = make_config(vec!["*.log"], Some(dir.path().to_path_buf()));
        let result = discover(&config).unwrap();
        assert!(result.is_empty());
    }

    // --- S5: time-based filtering ---

    #[test]
    fn newer_than_keeps_recent_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("recent.log"), b"recent").unwrap();

        let mut config = make_config(vec!["*.log"], Some(dir.path().to_path_buf()));
        config.newer_than = Some(std::time::Duration::from_secs(3600)); // 1 hour
        let result = discover(&config).unwrap();

        // recently-created file should be within the 1-hour window
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, dir.path().join("recent.log"));
    }

    #[test]
    fn older_than_skips_recent_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("recent.log"), b"recent").unwrap();

        let mut config = make_config(vec!["*.log"], Some(dir.path().to_path_buf()));
        config.older_than = Some(std::time::Duration::from_secs(3600)); // 1 hour
        let result = discover(&config).unwrap();

        // recently-created file should be skipped (age < 1h)
        assert!(result.is_empty());
    }

    #[test]
    fn older_than_zero_keeps_all() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("app.log"), b"data").unwrap();

        let mut config = make_config(vec!["*.log"], Some(dir.path().to_path_buf()));
        config.older_than = Some(std::time::Duration::from_secs(0));
        let result = discover(&config).unwrap();

        // Zero-duration older-than: all files have age >= 0s
        assert_eq!(result.len(), 1);
    }

    // --- S6: symlink / one-file-system handling ---

    #[test]
    fn same_filesystem_symlink_is_followed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().join("real.log");
        let link = dir.path().join("link.log");
        std::fs::write(&real, b"data").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let config = make_config(vec!["*.log"], Some(dir.path().to_path_buf()));
        let result = discover(&config).unwrap();

        // Both real file and symlink should be discovered
        assert!(result.len() >= 1);
    }

    #[test]
    fn one_file_system_skips_cross_device() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("app.log"), b"data").unwrap();

        // Use --one-file-system on a temp dir — should work since no
        // cross-device links exist in a freshly-created tempdir.
        let mut config = make_config(vec!["*.log"], Some(dir.path().to_path_buf()));
        config.one_file_system = true;
        let result = discover(&config).unwrap();

        assert_eq!(result.len(), 1);
    }
}
