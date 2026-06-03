use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use crate::config::Config;
use crate::file_ref::FileRef;
use crate::output::OutputLine;
use crate::reader;
use crate::signal;

/// A file currently being tailed by a reader thread.
pub struct TrackedFile {
    /// The file's stable identity (device, inode).
    #[allow(dead_code)]
    pub file_ref: FileRef,
    /// Current filesystem path to the file.
    pub path: PathBuf,
    /// Set to `true` to signal the reader thread to stop.
    pub stop_flag: Arc<AtomicBool>,
    /// Join handle for the reader thread.
    pub handle: Option<JoinHandle<()>>,
}

/// Run the supervisor on the calling thread.
///
/// The supervisor maintains a map of currently-tracked files by `FileRef`.
/// It receives `DiscoveryEvent::Found` messages from the watcher thread via
/// `discovery_rx` and manages the lifecycle of per-file reader threads:
///
/// - **New files**: spawn a reader thread and add it to the tracked set.
/// - **Disappeared files** (present in the tracked set but missing from the
///   latest discovery): signal the reader to stop and remove from the set.
///
/// On shutdown (when `stop` is set), all remaining readers are signaled to
/// stop and joined. The function does NOT consume `output_tx` so callers can
/// drop it after this function returns to close the output thread.
pub fn run_supervisor(
    config: &Config,
    discovery_rx: Receiver<crate::watcher::DiscoveryEvent>,
    output_tx: Sender<OutputLine>,
    stop: Arc<AtomicBool>,
) {
    let idle_ts: Option<Arc<AtomicU64>> = config
        .idle_timeout
        .map(|_| Arc::new(AtomicU64::new(reader::now_millis())));
    if config.retry {
        run_supervisor_retry(config, discovery_rx, output_tx, stop, idle_ts);
    } else {
        run_supervisor_default(config, discovery_rx, output_tx, stop, idle_ts);
    }
}

fn run_supervisor_default(
    config: &Config,
    discovery_rx: Receiver<crate::watcher::DiscoveryEvent>,
    output_tx: Sender<OutputLine>,
    stop: Arc<AtomicBool>,
    idle_ts: Option<Arc<AtomicU64>>,
) {
    let mut tracked: HashMap<FileRef, TrackedFile> = HashMap::new();

    while !stop.load(Ordering::Relaxed) && !signal::shutdown_requested() {
        match discovery_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => {
                if signal::shutdown_requested() {
                    stop.store(true, Ordering::Relaxed);
                }

                let files = match event {
                    crate::watcher::DiscoveryEvent::Found { files } => files,
                };

                let mut current_set: HashMap<FileRef, PathBuf> = HashMap::new();
                for (path, file_ref) in &files {
                    current_set.insert(*file_ref, path.clone());
                }

                for (file_ref, path) in &current_set {
                    if signal::shutdown_requested() || stop.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Some(existing) = tracked.get_mut(file_ref) {
                        if existing.path != *path {
                            existing.path = path.clone();
                        }
                    } else {
                        let reader_stop = Arc::new(AtomicBool::new(false));
                        let reader_tx = output_tx.clone();
                        let reader_path = path.clone();
                        let reader_lines = config.lines;
                        let reader_stop_clone = Arc::clone(&reader_stop);
                        let allow_truncation_reset = !config.no_truncation_reset;
                        let reader_idle_ts = idle_ts.clone();

                        let handle = std::thread::spawn(move || {
                            reader::follow_file(
                                reader_path,
                                reader_lines,
                                reader_tx,
                                reader_stop_clone,
                                allow_truncation_reset,
                                false,
                                reader_idle_ts,
                            );
                        });

                        tracked.insert(
                            *file_ref,
                            TrackedFile {
                                file_ref: *file_ref,
                                path: path.clone(),
                                stop_flag: reader_stop,
                                handle: Some(handle),
                            },
                        );
                    }
                }

                let mut to_remove: Vec<FileRef> = Vec::new();
                for (file_ref, tracked_file) in &tracked {
                    if !current_set.contains_key(file_ref) {
                        tracked_file.stop_flag.store(true, Ordering::Relaxed);
                        to_remove.push(*file_ref);
                    }
                }
                for file_ref in &to_remove {
                    if let Some(mut tracked_file) = tracked.remove(file_ref) {
                        if let Some(handle) = tracked_file.handle.take() {
                            let _ = handle.join();
                        }
                    }
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if signal::shutdown_requested() {
                    stop.store(true, Ordering::Relaxed);
                }
                if let (Some(ref ts), Some(timeout)) = (&idle_ts, config.idle_timeout) {
                    let elapsed = reader::now_millis() - ts.load(Ordering::Relaxed);
                    if elapsed >= timeout * 1000 {
                        stop.store(true, Ordering::Relaxed);
                    }
                }
                if pid_exited(config) {
                    stop.store(true, Ordering::Relaxed);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    for tracked_file in tracked.values() {
        tracked_file.stop_flag.store(true, Ordering::Relaxed);
    }
    for (_, mut tracked_file) in tracked {
        if let Some(handle) = tracked_file.handle.take() {
            let _ = handle.join();
        }
    }
}

fn pid_exited(config: &Config) -> bool {
    #[cfg(unix)]
    if let Some(pid) = config.pid {
        if unsafe { libc::kill(pid as i32, 0) } != 0 {
            return true;
        }
    }
    false
}

fn run_supervisor_retry(
    config: &Config,
    discovery_rx: Receiver<crate::watcher::DiscoveryEvent>,
    output_tx: Sender<OutputLine>,
    stop: Arc<AtomicBool>,
    idle_ts: Option<Arc<AtomicU64>>,
) {
    let mut tracked: HashMap<PathBuf, TrackedFile> = HashMap::new();

    while !stop.load(Ordering::Relaxed) && !signal::shutdown_requested() {
        match discovery_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => {
                if signal::shutdown_requested() {
                    stop.store(true, Ordering::Relaxed);
                }

                let files = match event {
                    crate::watcher::DiscoveryEvent::Found { files } => files,
                };

                for (path, file_ref) in &files {
                    if signal::shutdown_requested() || stop.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Some(existing) = tracked.get_mut(path) {
                        existing.file_ref = *file_ref;
                    } else {
                        let reader_stop = Arc::new(AtomicBool::new(false));
                        let reader_tx = output_tx.clone();
                        let reader_path = path.clone();
                        let reader_lines = config.lines;
                        let reader_stop_clone = Arc::clone(&reader_stop);
                        let allow_truncation_reset = !config.no_truncation_reset;
                        let reader_idle_ts = idle_ts.clone();

                        let handle = std::thread::spawn(move || {
                            reader::follow_file(
                                reader_path,
                                reader_lines,
                                reader_tx,
                                reader_stop_clone,
                                allow_truncation_reset,
                                true,
                                reader_idle_ts,
                            );
                        });

                        tracked.insert(
                            path.clone(),
                            TrackedFile {
                                file_ref: *file_ref,
                                path: path.clone(),
                                stop_flag: reader_stop,
                                handle: Some(handle),
                            },
                        );
                    }
                }
                // Retry mode: never remove tracked files (FR1.3).
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if signal::shutdown_requested() {
                    stop.store(true, Ordering::Relaxed);
                }
                if let (Some(ref ts), Some(timeout)) = (&idle_ts, config.idle_timeout) {
                    let elapsed = reader::now_millis() - ts.load(Ordering::Relaxed);
                    if elapsed >= timeout * 1000 {
                        stop.store(true, Ordering::Relaxed);
                    }
                }
                if pid_exited(config) {
                    stop.store(true, Ordering::Relaxed);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    for tracked_file in tracked.values() {
        tracked_file.stop_flag.store(true, Ordering::Relaxed);
    }
    for (_, mut tracked_file) in tracked {
        if let Some(handle) = tracked_file.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_file_creation() {
        let stop = Arc::new(AtomicBool::new(false));
        let tf = TrackedFile {
            file_ref: FileRef {
                device: 1,
                inode: 42,
            },
            path: PathBuf::from("/tmp/test.log"),
            stop_flag: stop,
            handle: None,
        };
        assert_eq!(tf.file_ref.inode, 42);
        assert_eq!(tf.path, PathBuf::from("/tmp/test.log"));
        assert!(!tf.stop_flag.load(std::sync::atomic::Ordering::Relaxed));
    }
}
