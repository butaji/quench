//! runts — Fresh/Preact to Native Rust Compiler
//!
//! A CLI tool that compiles Fresh/Preact TypeScript/TSX to native Rust binaries.
//! Zero external JS runtimes - pure Rust compilation pipeline.

mod cli;
mod config;
mod commands;
mod transpile;
mod runtime;

use anyhow::Result;
use clap::Parser;
use tracing::info;

use cli::Cli;

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .compact()
        .init();

    // Parse CLI arguments
    let cli = Cli::parse();

    // Execute command
    match cli.command {
        cli::Commands::Init { name } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(commands::run_init(name, None))?;
        }

        cli::Commands::Dev { path } => {
            info!("Starting development server...");
            let config = config::Config::load_from_path(&path)?;
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(commands::start_dev_server(&config, path))?;
        }

        cli::Commands::Build { path } => {
            info!("Building for production...");
            let config = config::Config::load_from_path(&path)?;
            let rt = tokio::runtime::Runtime::new()?;
            let result = rt.block_on(commands::run_build(&config, path))?;
            info!("Build complete!");
            info!("  Generated {} files", result.generated_files.len());
            info!("  Routes: {}", result.routes.len());
            info!("  Islands: {}", result.islands.len());
            info!("  Components: {}", result.components.len());
        }

        cli::Commands::Add { component_type, name, path } => {
            // Convert CLI component type to commands component type
            let cmd_type = match component_type {
                cli::ComponentType::Island => commands::add::ComponentType::Island,
                cli::ComponentType::Component => commands::add::ComponentType::Component,
                cli::ComponentType::Route => commands::add::ComponentType::Route,
                cli::ComponentType::Middleware => commands::add::ComponentType::Middleware,
            };
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(commands::run_add(cmd_type, name, Some(path)))?;
        }
    }

    Ok(())
}
