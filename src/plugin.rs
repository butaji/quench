//! Plugin registry for runts compiler.

use runts_fresh::FreshPlugin;
use runts_plugin::Plugin;
use runts_ratatui::RatatuiPlugin;
// use runts_react::ReactPlugin; // DISABLED: pre-existing bugs blocking build

/// Get a plugin by name.
///
/// # Errors
/// Returns an error if the plugin name is not recognized.
pub fn get_plugin(name: &str) -> anyhow::Result<Box<dyn Plugin>> {
    match name {
        "fresh" => Ok(Box::new(FreshPlugin)),
        "ratatui" => Ok(Box::new(RatatuiPlugin)),
        // "react" => Ok(Box::new(ReactPlugin)), // DISABLED: pre-existing bugs blocking build
        _ => Err(anyhow::anyhow!(
            "Unknown plugin '{}'. Use --plugin <name>. Available: fresh, ratatui",
            name
        )),
    }
}

/// List available plugin names.
#[allow(dead_code)]
pub fn available_plugins() -> &'static [&'static str] {
    &["fresh", "ratatui"]
}
