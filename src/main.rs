use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::Ordering;
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

    let config = match Config::from_cli(cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("folor: {}", e);
            std::process::exit(1);
        }
    };

    if config.patterns.is_empty() {
        run_stdin(&config);
    } else if config.follow {
        run_follow(&config);
    } else {
        run_one_shot(&config);
    }
}

fn run_stdin(config: &Config) {
    let stdin = std::io::stdin();
    let reader = BufReader::new(stdin.lock());

    if config.follow {
        // Follow mode: read existing input, then follow.
        let mut lines: Vec<Vec<u8>> = Vec::new();
        for line in reader.lines() {
            match line {
                Ok(l) => lines.push(l.into_bytes()),
                Err(e) => {
                    eprintln!("folor: stdin: {}", e);
                    break;
                }
            }
        }
        // Print last N lines.
        let start = if lines.len() > config.lines {
            lines.len() - config.lines
        } else {
            0
        };
        let mut stdout = std::io::stdout().lock();
        for line in &lines[start..] {
            let _ = stdout.write_all(line);
            let _ = stdout.write_all(b"\n");
        }
        let _ = stdout.flush();
    } else {
        // One-shot mode: read all lines, print last N.
        let mut lines: Vec<Vec<u8>> = Vec::new();
        for line in reader.lines() {
            match line {
                Ok(l) => lines.push(l.into_bytes()),
                Err(e) => {
                    eprintln!("folor: stdin: {}", e);
                    std::process::exit(2);
                }
            }
        }
        if config.lines == 0 {
            return;
        }
        let start = if lines.len() > config.lines {
            lines.len() - config.lines
        } else {
            0
        };
        let mut stdout = std::io::stdout().lock();
        for line in &lines[start..] {
            let _ = stdout.write_all(line);
            let _ = stdout.write_all(b"\n");
        }
        let _ = stdout.flush();
    }
}

fn run_follow(config: &Config) {
    signal::setup_signals();

    let is_tty = std::io::stdout().is_terminal();

    let show_prefix = if config.no_filename {
        false
    } else if config.show_filename {
        true
    } else {
        is_tty
    };

    let use_color = is_tty && show_prefix;

    let (discovery_tx, discovery_rx) = bounded::<DiscoveryEvent>(256);
    let (output_tx, output_rx) = bounded::<OutputLine>(256);

    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Spawn output thread.
    let output_stop = Arc::clone(&stop);
    let output_handle = thread::spawn(move || {
        output::run_output_thread(output_rx, show_prefix, use_color, output_stop);
    });

    // Spawn watcher thread.
    let watcher_config = config.clone();
    let watcher_stop = Arc::clone(&stop);
    let watcher_handle = thread::spawn(move || {
        watcher::run_watcher(watcher_config, discovery_tx, watcher_stop);
    });

    // Main thread acts as supervisor.
    supervisor::run_supervisor(config, discovery_rx, output_tx.clone(), Arc::clone(&stop));

    // Signal watcher to stop, then drain threads.
    stop.store(true, Ordering::Relaxed);
    let _ = watcher_handle.join();
    drop(output_tx);
    let _ = output_handle.join();
}

fn run_one_shot(config: &Config) {
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
        // SC-05: no files matched in one-shot is success (exit 0).
        return;
    }

    let is_tty = std::io::stdout().is_terminal();

    let show_prefix = if config.no_filename {
        false
    } else if config.show_filename {
        true
    } else {
        is_tty && files.len() > 1
    };

    let use_color = is_tty && show_prefix;

    let mut stdout = StandardStream::stdout(ColorChoice::Auto);

    for (path, _file_ref) in &files {
        if binary::is_binary_file(path) {
            continue;
        }

        match reader::read_last_lines(path, config.lines) {
            Ok(lines) => {
                if let Err(e) =
                    output::print_lines(&mut stdout, path, &lines, show_prefix, use_color)
                {
                    if e.kind() == std::io::ErrorKind::BrokenPipe {
                        return;
                    }
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
