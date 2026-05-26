//! # runts — Fresh/Preact to Native Rust Compiler
//!
//! This crate provides the core transpilation pipeline for converting
//! Fresh/Preact TypeScript/TSX to native Rust code.
//!
//! ## Architecture
//!
//! 1. **Parse**: TS/TSX → AST
//! 2. **Analyze**: Semantic analysis (types, islands, routes)
//! 3. **Transform**: High-level IR (Hir)
//! 4. **Generate**: Rust source code
//! 5. **Runtime**: Native Rust runtime for Preact patterns

pub mod config;
pub mod transpile;
pub mod commands;

// Runtime is in src/runtime/ directory as a module
pub mod runtime;

// Route generation
pub mod routegen;
pub use routegen::{RouteHandler, RouteInfo, RouteMethod, parse_route_path, generate_route_handlers};

// Middleware generation
pub mod middlewaregen;
pub use middlewaregen::{MiddlewareInfo, extract_middleware, generate_middleware};

pub use config::Config;
pub use transpile::{Transpiler, TranspileResult};

/// Version of the runts compiler
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
