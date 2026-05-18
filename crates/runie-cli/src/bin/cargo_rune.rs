//! # Cargo Rune CLI
//!
//! Cargo subcommand entry point for `cargo runie`.

#![cfg_attr(
    not(any(feature = "binary-runie", feature = "binary-cargo")),
    forbid(unsafe_code)
)]

use runie::driver::BuildOptions;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use runie_cli::cli::{build_cli, run_command};

/// Main entry point for cargo-runie.
fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Get command line args - cargo passes ["cargo", "runie", ...]
    let mut args: Vec<String> = std::env::args().collect();

    // Strip the "runie" arg since clap expects args starting with program name
    if args.len() > 1 && args[1] == "runie" {
        args.remove(1);
    }

    let cli = build_cli(&args);
    let mut options = create_options(&cli);

    if let Err(e) = run_command(&cli.command, &mut options) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

/// Create build options from CLI arguments.
fn create_options(cli: &runie_cli::cli::Cli) -> BuildOptions {
    let mut options = BuildOptions::new(PathBuf::from("."));
    options.verbose = cli.verbose;
    options.json = cli.json;
    options
}
