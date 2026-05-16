//! # Driver Module
//!
//! Orchestrates the full compilation pipeline and integrates with Cargo.

mod build;
mod config;
mod cache;

pub use build::{BuildDriver, BuildOptions, BuildMode};
pub use config::{RuneConfig, TargetCrate};
pub use cache::CacheManager;

/// CLI commands supported by rune.
#[derive(Debug, Clone, Copy)]
pub enum Command {
    /// Development mode with hot reload
    Dev,
    /// Release build
    Build,
    /// Type check only
    Check,
    /// Transpile to stdout
    Transpile,
    /// Initialize new project
    Init,
}

impl Command {
    /// Parse from string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "dev" => Some(Self::Dev),
            "build" => Some(Self::Build),
            "check" => Some(Self::Check),
            "transpile" => Some(Self::Transpile),
            "init" => Some(Self::Init),
            _ => None,
        }
    }
}

/// Run the compiler with given options.
pub fn run(command: Command, options: BuildOptions) -> crate::Result<()> {
    let mut driver = BuildDriver::new(options)?;
    match command {
        Command::Dev => driver.dev(),
        Command::Build => driver.build(),
        Command::Check => driver.check(),
        Command::Transpile => driver.transpile(),
        Command::Init => driver.init(),
    }
}
