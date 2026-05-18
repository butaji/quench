//! # CLI Module
//!
//! Command-line interface for the Rune compiler driver.

use clap::{Parser, Subcommand, ValueHint};
use runie::driver::{BuildDriver, BuildMode, BuildOptions};
use runie::Result;
use std::path::PathBuf;

/// Rune compiler driver.
#[derive(Parser)]
#[command(name = "runie")]
#[command(about = "TypeScript to Rust compiler driver", long_about = None)]
pub struct Cli {
    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Output JSON format for machine consumption
    #[arg(long)]
    pub json: bool,

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

        /// Watch for changes and re-transpile
        #[arg(short, long)]
        watch: bool,
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
#[allow(clippy::missing_errors_doc)]
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
        Commands::Transpile { file, watch } => {
            driver.options.transpile_file = Some(file.clone());
            driver.options.watch_transpile = *watch;
            driver.transpile()
        }
        Commands::Init { name } => {
            if let Some(n) = name {
                println!("Initializing project: {n}");
            }
            driver.init()
        }
    }
}
