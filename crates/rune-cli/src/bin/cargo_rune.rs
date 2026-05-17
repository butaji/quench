//! # Cargo Rune CLI
//!
//! Cargo subcommand entry point for `cargo rune`.

#![cfg_attr(
    not(any(feature = "binary-rune", feature = "binary-cargo")),
    forbid(unsafe_code)
)]

use rune::driver::BuildOptions;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use rune_cli::cli::{build_cli, run_command};

/// Main entry point for cargo-rune.
fn main() -> rune::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Get command line args - cargo passes ["cargo", "rune", ...]
    let mut args: Vec<String> = std::env::args().collect();

    // Strip the "rune" arg since clap expects args starting with program name
    if args.len() > 1 && args[1] == "rune" {
        args.remove(1);
    }

    let cli = build_cli(&args);
    let mut options = create_options(&cli);

    let result = run_command(&cli.command, &mut options);

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    Ok(())
}

/// Create build options from CLI arguments.
fn create_options(cli: &rune_cli::cli::Cli) -> BuildOptions {
    let mut options = BuildOptions::new(PathBuf::from("."));
    options.verbose = cli.verbose;
    options
}
