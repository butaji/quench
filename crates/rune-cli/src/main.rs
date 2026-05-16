//! # Rune CLI
//!
//! Main CLI entry point for the `rune` binary and `cargo rune` subcommand.

use anyhow::Result;
use clap::{Parser, Subcommand, ValueHint};
use std::path::PathBuf;
use rune::driver::{BuildOptions, BuildMode, Command};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Detect if running as cargo subcommand (binary named cargo-rune).
fn is_cargo_subcommand() -> bool {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(name) = exe.file_name() {
            if let Some(name_str) = std::ffi::OsStr::to_str(name) {
                return name_str.starts_with("cargo-");
            }
        }
    }
    false
}

/// Rune compiler driver.
#[derive(Parser)]
#[command(name = "rune")]
#[command(about = "TypeScript to Rust compiler driver", long_about = None)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Development mode with hot reload
    Dev {
        /// Working directory
        #[arg(value_hint = ValueHint::DirPath)]
        path: Option<PathBuf>,
    },

    /// Release build
    Build {
        /// Working directory
        #[arg(value_hint = ValueHint::DirPath)]
        path: Option<PathBuf>,

        /// Build release mode
        #[arg(short, long)]
        release: bool,
    },

    /// Type check only
    Check {
        /// Working directory
        #[arg(value_hint = ValueHint::DirPath)]
        path: Option<PathBuf>,
    },

    /// Transpile a file to stdout
    Transpile {
        /// File to transpile
        file: PathBuf,
    },

    /// Initialize a new project
    Init {
        /// Project name
        name: Option<String>,
    },
}

fn main() -> Result<()> {
    // Setup logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Check if running as cargo subcommand
    let is_cargo = is_cargo_subcommand();

    // Get command line args
    let mut args: Vec<String> = std::env::args().collect();

    // When cargo invokes cargo-rune, it passes ["cargo", "rune", ...]
    // We need to strip the "rune" arg for clap to work
    if is_cargo && args.len() > 1 && args[1] == "rune" {
        args.remove(1);
    }

    // Parse arguments using clap's from_iter which doesn't consume args
    let cli = Cli::parse_from(&args);

    let mut options: BuildOptions;

    // Execute command
    let result = match &cli.command {
        Commands::Dev { path } => {
            options = BuildOptions::new(path.clone().unwrap_or_else(|| PathBuf::from(".")));
            options.mode = BuildMode::Dev;
            options.verbose = cli.verbose;
            rune::driver::run(Command::Dev, options)
        }
        Commands::Build { path, release } => {
            let path = path.clone().unwrap_or_else(|| PathBuf::from("."));
            options = BuildOptions::new(path);
            options.mode = if *release {
                BuildMode::Release
            } else {
                BuildMode::Dev
            };
            options.verbose = cli.verbose;
            rune::driver::run(Command::Build, options)
        }
        Commands::Check { path } => {
            options = BuildOptions::new(path.clone().unwrap_or_else(|| PathBuf::from(".")));
            options.verbose = cli.verbose;
            rune::driver::run(Command::Check, options)
        }
        Commands::Transpile { file } => {
            options = BuildOptions::new(PathBuf::from("."));
            options.transpile_file = Some(file.clone());
            options.verbose = cli.verbose;
            rune::driver::run(Command::Transpile, options)
        }
        Commands::Init { name } => {
            options = BuildOptions::new(PathBuf::from("."));
            if let Some(n) = name {
                // Use name for project
                println!("Initializing project: {n}");
            }
            rune::driver::run(Command::Init, options)
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    Ok(())
}
