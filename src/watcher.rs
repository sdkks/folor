use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use notify::RecursiveMode;
use notify::Watcher;

use crate::config::Config;
use crate::discovery;
use crate::file_ref::FileRef;

/// Message sent from the watcher thread to the supervisor.
///
/// Each message carries a fresh list of files matching the configured patterns.
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    /// Discovery produced a fresh list of matching files.
    Found { files: Vec<(PathBuf, FileRef)> },
}

/// Run the directory watcher in a dedicated thread.
///
/// Combines `notify` filesystem events with a periodic scan timer.
/// FS events are debounced within a 100ms window to avoid redundant
/// scans when many events arrive in a burst. The periodic timer ensures
/// discovery re-runs even without FS activity (catching files created
/// while the watcher was temporarily disconnected, for example).
///
/// The function blocks until `stop` is set to `true`, then returns.
pub fn run_watcher(config: Config, tx: Sender<DiscoveryEvent>, stop: Arc<AtomicBool>) {
    let root = match &config.directory {
        Some(dir) => dir.clone(),
        None => match std::env::current_dir() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("folor: watcher: current_dir: {}", e);
                return;
            }
        },
    };

    // Internal mpsc channel from notify callback to our poll loop.
    let (event_tx, event_rx) = mpsc::channel();

    let mut watcher =
        match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            match res {
                Ok(event) => {
                    // Only trigger on content-relevant events
                    match event.kind {
                        notify::EventKind::Create(_)
                        | notify::EventKind::Modify(_)
                        | notify::EventKind::Remove(_) => {
                            let _ = event_tx.send(());
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    eprintln!("folor: watch error: {}", e);
                }
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("folor: failed to create filesystem watcher: {}", e);
                return;
            }
        };

    if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
        eprintln!("folor: failed to watch {}: {}", root.display(), e);
        return;
    }

    let scan_interval = Duration::from_secs(config.scan_interval);

    // Run an immediate scan on start so the supervisor gets an initial file list.
    run_scan(&config, &tx);
    let mut last_scan = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        // Calculate how long until the next periodic scan.
        let elapsed = last_scan.elapsed();
        let timeout = if elapsed >= scan_interval {
            Duration::ZERO
        } else {
            scan_interval - elapsed
        };

        // Wait for either an FS event or the periodic timeout.
        match event_rx.recv_timeout(timeout) {
            Ok(()) => {
                // Drain any additional events within the debounce window.
                let drain_deadline = Instant::now() + Duration::from_millis(100);
                while let Some(remaining) = drain_deadline.checked_duration_since(Instant::now()) {
                    match event_rx.recv_timeout(remaining) {
                        Ok(()) => {} // keep draining
                        Err(_) => break,
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Periodic scan timer fired.
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }

        if stop.load(Ordering::Relaxed) {
            break;
        }

        run_scan(&config, &tx);
        last_scan = Instant::now();
    }
}

/// Run a single discovery scan and send the results.
fn run_scan(config: &Config, tx: &Sender<DiscoveryEvent>) {
    match discovery::discover(config) {
        Ok(files) => {
            let _ = tx.send(DiscoveryEvent::Found { files });
        }
        Err(e) => {
            eprintln!("folor: discovery error: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_event_debug() {
        let event = DiscoveryEvent::Found {
            files: vec![(
                PathBuf::from("/tmp/test.log"),
                FileRef {
                    device: 1,
                    inode: 42,
                },
            )],
        };
        let debug = format!("{:?}", event);
        assert!(debug.contains("Found"));
        assert!(debug.contains("/tmp/test.log"));
    }

    #[test]
    fn discovery_event_clone() {
        let event = DiscoveryEvent::Found {
            files: vec![(
                PathBuf::from("/tmp/a.log"),
                FileRef {
                    device: 0,
                    inode: 1,
                },
            )],
        };
        let cloned = event.clone();
        match cloned {
            DiscoveryEvent::Found { files } => {
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].0, PathBuf::from("/tmp/a.log"));
                assert_eq!(files[0].1.inode, 1);
            }
        }
    }

    #[test]
    fn discovery_event_empty_files() {
        let event = DiscoveryEvent::Found { files: vec![] };
        match event {
            DiscoveryEvent::Found { files } => {
                assert!(files.is_empty());
            }
        }
    }
}
