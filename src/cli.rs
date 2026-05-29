//! CLI argument definitions using clap

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "runts")]
#[command(version = "0.1.0")]
#[command(about = "Fresh/Preact to native Rust compiler", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(clap::Subcommand)]
pub enum Commands {
    /// Initialize a new runts project
    Init {
        /// Project name
        #[arg(default_value = "my-runts-app")]
        name: String,
    },

    /// Start development server with hot reload
    Dev {
        /// Project path
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Build for production (transpile + compile)
    Build {
        /// Project path
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Build in release mode (default: true)
        #[arg(short, long, default_value = "true")]
        release: bool,

        /// Skip Rust compilation (transpile only)
        #[arg(short, long, default_value = "false")]
        no_compile: bool,
    },

    /// Transpile TS/TSX to Rust without compiling
    Transpile {
        /// Project path
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Output directory for generated files
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Evaluate a TypeScript expression
    Eval {
        /// The expression to evaluate
        expr: String,
    },

    /// Add a new component or route
    Add {
        /// Type of component to add
        #[arg(value_enum)]
        component_type: ComponentType,

        /// Name of the component
        name: String,

        /// Project path
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum ComponentType {
    /// Add an island component
    Island,
    /// Add a shared component
    Component,
    /// Add a route
    Route,
    /// Add middleware
    Middleware,
}

impl ComponentType {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            ComponentType::Island => "island",
            ComponentType::Component => "component",
            ComponentType::Route => "route",
            ComponentType::Middleware => "middleware",
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum Backend {
    /// Compile to native Rust binary (default)
    Rust,
    /// Compile to Hono (JS/TS edge runtime)
    Hono,
}
