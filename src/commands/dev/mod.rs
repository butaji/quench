//! Development server with instant hot-reload
//!
//! In development mode, runts:
//! - Parses TS/TSX to HIR (NO Rust compilation)
//! - Executes HIR directly with the interpreter
//! - Provides instant hot-reload (<100ms)
//! - Full parity with production rendering

pub mod routes;
pub mod handlers;

use anyhow::Result;
use parking_lot::RwLock;
use std::{
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::broadcast;

use crate::config::Config;
use crate::runtime::interpreter::Interpreter;

/// Application state shared across requests
#[derive(Clone)]
pub struct AppState {
    /// Project root
    pub root: PathBuf,
    /// Route table
    pub route_table: Arc<RwLock<routes::RouteTable>>,
    /// Interpreter (HIR executor)
    pub interpreter: Arc<RwLock<Interpreter>>,
    /// Broadcast channel for hot reload events
    pub reload_tx: broadcast::Sender<ReloadEvent>,
    /// File watcher (kept alive to prevent drop)
    #[allow(dead_code)]
    pub watcher: Arc<std::sync::Mutex<notify::RecommendedWatcher>>,
}

/// Reload event types
#[derive(Debug, Clone)]
pub enum ReloadEvent {
    RouteChanged(String),
    ModuleChanged(String),
    Error(String),
}

/// Run dev server
pub async fn run_dev_server(config: &Config, port: u16) -> Result<()> {
    handlers::run_server(config, port).await
}
