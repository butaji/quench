//! HTTP handlers - plugin handles dev loop now
//!
//! This module is kept for reference but the plugin controls
//! the dev lifecycle via dev_run_once hook.

use crate::config::Config;
use anyhow::Result;

/// Stub - plugin controls dev loop now
pub async fn run_server(_config: &Config, _port: u16) -> Result<()> {
    // Plugin handles server/TUI via dev_run_once
    // This should not be called directly
    Ok(())
}
