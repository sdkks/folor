use std::sync::atomic::{AtomicBool, Ordering};

/// Set to `true` by SIGINT/SIGTERM handler to request shutdown.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Set to `true` by SIGHUP handler to request immediate rescan.
static RESCAN: AtomicBool = AtomicBool::new(false);

extern "C" fn sig_handler(_: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

extern "C" fn hup_handler(_: libc::c_int) {
    RESCAN.store(true, Ordering::SeqCst);
}

/// Register signal handlers using sigaction (POSIX) rather than the
/// deprecated signal(). Must be called early in `main`.
pub fn setup_signals() {
    unsafe {
        let mut shutdown_action: libc::sigaction = std::mem::zeroed();
        let mut hup_action: libc::sigaction = std::mem::zeroed();

        #[cfg(target_os = "linux")]
        {
            shutdown_action.sa_sigaction =
                sig_handler as extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void);
            shutdown_action.sa_flags = libc::SA_SIGINFO;
            hup_action.sa_sigaction =
                hup_handler as extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void);
            hup_action.sa_flags = libc::SA_SIGINFO;
        }
        #[cfg(not(target_os = "linux"))]
        {
            shutdown_action.sa_sigaction = sig_handler as *const () as usize;
            hup_action.sa_sigaction = hup_handler as *const () as usize;
        }

        if libc::sigaction(libc::SIGINT, &shutdown_action, std::ptr::null_mut()) != 0 {
            eprintln!("folor: failed to install SIGINT handler");
        }
        if libc::sigaction(libc::SIGTERM, &shutdown_action, std::ptr::null_mut()) != 0 {
            eprintln!("folor: failed to install SIGTERM handler");
        }
        if libc::sigaction(libc::SIGHUP, &hup_action, std::ptr::null_mut()) != 0 {
            eprintln!("folor: failed to install SIGHUP handler");
        }
    }
}

/// Check whether shutdown has been requested (SIGINT/SIGTERM).
pub fn shutdown_requested() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}

/// Check and clear the rescan flag (SIGHUP). Returns `true` if a
/// rescan was requested since the last call.
#[allow(dead_code)]
pub fn take_rescan_request() -> bool {
    RESCAN.swap(false, Ordering::SeqCst)
}
