//! # CLI Module
//!
//! Shared CLI parsing and execution logic.

use clap::{Parser, Subcommand, ValueHint};
use rune::driver::{BuildDriver, BuildMode, BuildOptions};
use rune::Result;
use std::path::PathBuf;

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
pub fn run_command(command: &Commands, options: &mut BuildOptions) -> Result<()> {
    let mut driver = BuildDriver::new(options.clone())?;

    match command {
        Commands::Dev { path } => {
            driver.options.workspace = path.clone().unwrap_or_else(|| PathBuf::from("."));
            driver.options.mode = BuildMode::Dev;
            driver.dev()
        }
        Commands::Build { path, release } => {
            driver.options.workspace = path.clone().unwrap_or_else(|| PathBuf::from("."));
            driver.options.mode = if *release {
                BuildMode::Release
            } else {
                BuildMode::Dev
            };
            driver.build()
        }
        Commands::Check { path } => {
            driver.options.workspace = path.clone().unwrap_or_else(|| PathBuf::from("."));
            driver.check()
        }
        Commands::Transpile { file } => {
            driver.options.transpile_file = Some(file.clone());
            driver.transpile()
        }
        Commands::Init { name } => {
            if let Some(_n) = name {
                println!("Initializing project: {}", _n);
            }
            driver.init()
        }
    }
}
