#![allow(dead_code)]

use clap::Parser;
use config::{Cli, Config};
use is_terminal::IsTerminal;
use termcolor::{ColorChoice, StandardStream};

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
