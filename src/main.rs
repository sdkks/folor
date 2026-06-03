#![allow(dead_code)]

use clap::Parser;
use config::{Cli, Config};
use std::io::BufWriter;

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
                eprintln!("folor: follow mode not yet implemented");
                std::process::exit(0);
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

    let prefix = files.len() > 1;
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());

    for (path, _file_ref) in &files {
        if binary::is_binary_file(path) {
            // Warning already printed by is_binary_file
            continue;
        }

        match reader::read_last_lines(path, config.lines) {
            Ok(lines) => {
                if let Err(e) = output::print_lines(&mut writer, path, &lines, prefix) {
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
