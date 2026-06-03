#![allow(dead_code)]

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

use clap::Parser;
use config::{Cli, Config};
use crossbeam_channel::bounded;
use is_terminal::IsTerminal;
use termcolor::{ColorChoice, StandardStream};

use crate::output::OutputLine;
use crate::watcher::DiscoveryEvent;

mod binary;
mod config;
mod discovery;
mod file_ref;
mod output;
mod reader;
mod signal;
mod supervisor;
mod watcher;

fn main() {
    let cli = Cli::parse();

    match Config::from_cli(cli) {
        Ok(config) => {
            if config.patterns.is_empty() {
                eprintln!(
                    "folor: no file patterns specified; reading from stdin (not yet implemented)"
                );
                std::process::exit(0);
            }
            if config.follow {
                run_follow(&config);
            } else {
                run_one_shot(&config);
            }
        }
        Err(e) => {
            eprintln!("folor: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_follow(config: &Config) {
    let is_tty = std::io::stdout().is_terminal();

    // For follow mode, we always show prefixes (at least one file, typically many).
    // Follow the same rules as one-shot: prefix when TTY, suppress when --no-filename,
    // force when --filename.
    let show_prefix = if config.no_filename {
        false
    } else if config.show_filename {
        true
    } else {
        is_tty
    };

    let use_color = is_tty && show_prefix;

    // Bounded channels with capacity 256.
    let (discovery_tx, discovery_rx) = bounded::<DiscoveryEvent>(256);
    let (output_tx, output_rx) = bounded::<OutputLine>(256);

    let stop = Arc::new(AtomicBool::new(false));

    // Placeholder signal handler: set up SIGINT/SIGTERM later (S7).
    // For now, Ctrl+C in the terminal will kill the process; the Drop impl
    // on threads does not run, but this is acceptable until signal handling
    // is implemented.
    signal::setup_signals();

    // Spawn output thread.
    let output_handle = thread::spawn(move || {
        output::run_output_thread(output_rx, show_prefix, use_color);
    });

    // Spawn watcher thread.
    let watcher_config = config.clone();
    let watcher_stop = Arc::clone(&stop);
    let watcher_handle = thread::spawn(move || {
        watcher::run_watcher(watcher_config, discovery_tx, watcher_stop);
    });

    // Main thread acts as supervisor.
    supervisor::run_supervisor(config, discovery_rx, output_tx.clone(), Arc::clone(&stop));

    // Signal watcher to stop, then drain threads in order.
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = watcher_handle.join();

    // Drop the last output_tx clone so the output thread sees disconnect.
    drop(output_tx);
    let _ = output_handle.join();
}

fn run_one_shot(config: &Config) {
    // FR-MODE-004: -n 0 in one-shot mode exits without output
    if config.lines == 0 {
        return;
    }

    let files = match discovery::discover(config) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("folor: {}", e);
            std::process::exit(1);
        }
    };

    if files.is_empty() {
        eprintln!("folor: no files matched the provided patterns");
        return;
    }

    let is_tty = std::io::stdout().is_terminal();

    // Determine whether to show filename prefixes:
    //   --no-filename  → never show
    //   --filename     → always show
    //   auto           → show only when TTY and multiple files are matched
    let show_prefix = if config.no_filename {
        false
    } else if config.show_filename {
        true
    } else {
        is_tty && files.len() > 1
    };

    // Colorize prefixes only when attached to a terminal. Even if
    // --filename forces prefixes in a pipe, colors stay off.
    let use_color = is_tty && show_prefix;

    let mut stdout = StandardStream::stdout(ColorChoice::Auto);

    for (path, _file_ref) in &files {
        if binary::is_binary_file(path) {
            // Warning already printed by is_binary_file
            continue;
        }

        match reader::read_last_lines(path, config.lines) {
            Ok(lines) => {
                if let Err(e) =
                    output::print_lines(&mut stdout, path, &lines, show_prefix, use_color)
                {
                    eprintln!("folor: {}", e);
                    std::process::exit(2);
                }
            }
            Err(e) => {
                eprintln!("folor: {}: {}", path.display(), e);
                std::process::exit(2);
            }
        }
    }
}
