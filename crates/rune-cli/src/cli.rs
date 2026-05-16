//! # CLI Module
//!
//! Shared CLI parsing and execution logic.

use clap::{Parser, Subcommand, ValueHint};
use std::path::PathBuf;
use rune::driver::{BuildOptions, BuildMode, Command};
use rune::Result;

/// Rune compiler driver.
#[derive(Parser)]
#[command(name = "rune")]
#[command(about = "TypeScript to Rust compiler driver", long_about = None)]
pub struct Cli {
    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
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

/// Build CLI from arguments.
#[must_use]
pub fn build_cli(args: &[String]) -> Cli {
    Cli::parse_from(args)
}

/// Run command based on CLI arguments.
pub fn run_command(
    command: &Commands,
    options: &mut BuildOptions,
) -> Result<()> {
    match command {
        Commands::Dev { path } => {
            options.workspace = path.clone().unwrap_or_else(|| PathBuf::from("."));
            options.mode = BuildMode::Dev;
            rune::driver::run(Command::Dev, options.clone())
        }
        Commands::Build { path, release } => {
            options.workspace = path.clone().unwrap_or_else(|| PathBuf::from("."));
            options.mode = if *release {
                BuildMode::Release
            } else {
                BuildMode::Dev
            };
            rune::driver::run(Command::Build, options.clone())
        }
        Commands::Check { path } => {
            options.workspace = path.clone().unwrap_or_else(|| PathBuf::from("."));
            rune::driver::run(Command::Check, options.clone())
        }
        Commands::Transpile { file } => {
            options.transpile_file = Some(file.clone());
            rune::driver::run(Command::Transpile, options.clone())
        }
        Commands::Init { name } => {
            if let Some(n) = name {
                println!("Initializing project: {n}");
            }
            rune::driver::run(Command::Init, options.clone())
        }
    }
}
