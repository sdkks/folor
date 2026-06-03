use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Global flag set by signal handlers to request shutdown.
static SHUTDOWN_FLAG: std::sync::LazyLock<Arc<AtomicBool>> =
    std::sync::LazyLock::new(|| Arc::new(AtomicBool::new(false)));

/// Global flag set by SIGHUP handler to request immediate rescan.
static RESCAN_FLAG: std::sync::LazyLock<Arc<AtomicBool>> =
    std::sync::LazyLock::new(|| Arc::new(AtomicBool::new(false)));

/// Register signal handlers. Must be called early in `main`.
/// - SIGINT/SIGTERM sets the shutdown flag.
/// - SIGHUP sets the rescan flag (only relevant in follow mode).
///
/// Only `AtomicBool` operations are used inside handlers (async-signal-safe).
pub fn setup_signals() {
    let shutdown = Arc::clone(&SHUTDOWN_FLAG);
    let rescan = Arc::clone(&RESCAN_FLAG);

    unsafe {
        libc::signal(libc::SIGINT, handler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, handler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, hup_handler as *const () as libc::sighandler_t);
    }

    // Store the arcs so they live for the process lifetime.
    std::mem::forget(shutdown);
    std::mem::forget(rescan);
}

extern "C" fn handler(_: libc::c_int) {
    SHUTDOWN_FLAG.store(true, Ordering::Relaxed);
}

extern "C" fn hup_handler(_: libc::c_int) {
    RESCAN_FLAG.store(true, Ordering::Relaxed);
}

/// Check whether shutdown has been requested (SIGINT/SIGTERM).
#[allow(dead_code)]
pub fn shutdown_requested() -> bool {
    SHUTDOWN_FLAG.load(Ordering::Relaxed)
}

/// Check and clear the rescan flag (SIGHUP). Returns `true` if a
/// rescan was requested since the last call.
#[allow(dead_code)]
pub fn take_rescan_request() -> bool {
    RESCAN_FLAG.swap(false, Ordering::Relaxed)
}
