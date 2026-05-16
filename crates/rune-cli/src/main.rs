//! # Rune CLI
//!
//! Main CLI entry point for the `rune` binary.

use anyhow::Result;
use clap::{Parser, Subcommand, ValueHint};
use std::path::PathBuf;
use rune::driver::{BuildOptions, BuildMode, Command};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

        /// Build release
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
        #[arg(value_hint = ValueHint::FilePath)]
        file: PathBuf,
    },

    /// Initialize a new project
    Init {
        /// Project name
        name: Option<String>,
    },
}

/// Main entry point.
fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("rune=info".parse()?))
        .init();

    let cli = Cli::parse();

    // Determine workspace path
    let workspace = match &cli.command {
        Commands::Dev { path } | Commands::Build { path, .. } | Commands::Check { path } => {
            path.clone()
                .or_else(|| std::env::var("CARGO_MANIFEST_DIR").ok().map(PathBuf::from))
                .or_else(|| std::env::var("PWD").ok().map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("."))
        }
        Commands::Init { .. } => {
            std::env::var("PWD")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."))
        }
        Commands::Transpile { .. } => {
            std::env::var("CARGO_MANIFEST_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."))
        }
    };

    // Build options
    let mut options = BuildOptions::new(workspace);
    options.verbose = cli.verbose;

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
            options.mode = if *release { BuildMode::Release } else { BuildMode::Dev };
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
            if name.is_some() {
                // TODO: use name
            }
            rune::driver::run(Command::Init, options)
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
