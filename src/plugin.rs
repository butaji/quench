//! Plugin registry for runts compiler.

use runts_fresh::FreshPlugin;
use runts_plugin::Plugin;
use runts_ratatui::RatatuiPlugin;
// use runts_react::ReactPlugin; // DISABLED: pre-existing bugs blocking build

/// Get a plugin by name.
///
/// `ink` is an alias for `ratatui` so users can write
/// `runts dev --ink examples/foo.tsx` instead of
/// `runts dev --plugin ratatui examples/foo.tsx`.
/// The Ink examples are pure `.tsx` that compile to
/// the `runts-ink` runtime via the ratatui plugin's
/// JSX dispatch; the alias just makes the
/// user-facing CLI shorter for that common case.
pub fn get_plugin(name: &str) -> anyhow::Result<Box<dyn Plugin>> {
    match name {
        "fresh" => Ok(Box::new(FreshPlugin)),
        "ratatui" | "ink" => Ok(Box::new(RatatuiPlugin)),
        // "react" => Ok(Box::new(ReactPlugin)), // DISABLED: pre-existing bugs blocking build
        _ => Err(anyhow::anyhow!(
            "Unknown plugin '{}'. Use --plugin <name>. Available: fresh, ratatui, ink",
            name
        )),
    }
}

/// List available plugin names.
#[allow(dead_code)]
pub fn available_plugins() -> &'static [&'static str] {
    &["fresh", "ratatui", "ink"]
}
