#![allow(dead_code)]

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
            rt.block_on(commands::run_dev_server(&config, 8000))?;
        }

        cli::Commands::Build { path, release, no_compile } => {
            info!("Building for production...");
            let config = config::Config::load_from_path(&path)?;
            let rt = tokio::runtime::Runtime::new()?;
            
            if no_compile {
                // Just transpile
                let result = rt.block_on(commands::run_build(&config, path))?;
                info!("Transpilation complete!");
                info!("  Generated {} files", result.generated_files.len());
                info!("  Routes: {}", result.routes.len());
                info!("  Islands: {}", result.islands.len());
                info!("  Components: {}", result.components.len());
                info!("Run `cargo build --release` to compile.");
            } else {
                let result = rt.block_on(commands::build::run_full_build(&config, path, release))?;
                
                info!("Build complete!");
                if let Some(binary) = &result.binary_path {
                    info!("  Binary: {:?}", binary);
                }
                if let Some(size) = result.binary_size {
                    info!("  Size: {:.2} KB", size as f64 / 1024.0);
                }
            }
        }

        cli::Commands::Transpile { path, output } => {
            // Just transpile without compiling
            info!("Transpiling TypeScript to Rust...");
            let config = config::Config::load_from_path(&path)?;
            let rt = tokio::runtime::Runtime::new()?;
            let result = rt.block_on(commands::run_build(&config, path.clone()))?;
            
            if let Some(out_dir) = &output {
                for file in &result.generated_files {
                    let target = out_dir.join(file.path.file_name().unwrap_or_default());
                    std::fs::write(&target, &file.content)?;
                    info!("  Wrote: {:?}", target);
                }
            }
            
            info!("Transpilation complete!");
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
