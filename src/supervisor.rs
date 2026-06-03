use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};

use crate::config::Config;
use crate::file_ref::FileRef;
use crate::output::OutputLine;
use crate::reader;

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
    let mut tracked: HashMap<FileRef, TrackedFile> = HashMap::new();

    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        match discovery_rx.recv() {
            Ok(event) => {
                let files = match event {
                    crate::watcher::DiscoveryEvent::Found { files } => files,
                };

                let mut current_set: HashMap<FileRef, PathBuf> = HashMap::new();
                for (path, file_ref) in &files {
                    current_set.insert(*file_ref, path.clone());
                }

                // Spawn readers for newly discovered files; update paths on rename.
                for (file_ref, path) in &current_set {
                    if let Some(existing) = tracked.get_mut(file_ref) {
                        // File was renamed. Update the tracked path so the supervisor's
                        // bookkeeping is correct. Full rotation handling (notifying the
                        // reader thread of the new path, following both old and new
                        // inodes) is deferred to STORY-0003.
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

                        let handle = std::thread::spawn(move || {
                            reader::follow_file(
                                reader_path,
                                reader_lines,
                                reader_tx,
                                reader_stop_clone,
                                allow_truncation_reset,
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

                // Stop readers for files that disappeared.
                let mut to_remove: Vec<FileRef> = Vec::new();
                for (file_ref, tracked_file) in &tracked {
                    if !current_set.contains_key(file_ref) {
                        tracked_file
                            .stop_flag
                            .store(true, std::sync::atomic::Ordering::Relaxed);
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
            Err(crossbeam_channel::RecvError) => {
                // Channel disconnected — watcher exited.
                break;
            }
        }
    }

    // Shutdown: signal all remaining readers to stop.
    for tracked_file in tracked.values() {
        tracked_file
            .stop_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    // Join all reader threads.
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
