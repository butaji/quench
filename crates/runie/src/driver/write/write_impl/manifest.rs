//! # Manifest Generator
//!
//! Generates Cargo.toml manifests for generated crates.

/// Generate Cargo.toml manifest for the cache directory.
#[must_use]
pub fn generate_manifest(target_crate: &str, cache_to_crates: &str) -> String {
    format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[lib]
name = "{}"
crate-type = ["cdylib", "rlib"]
path = "src/lib.rs"

[dependencies]
protocol = {{ path = "{}/protocol" }}
ratatui = "0.26"
crossterm = "0.27"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"

[workspace]
"#,
        target_crate, target_crate, cache_to_crates
    )
}

/// Generate lib.rs content for the cache directory.
#[must_use]
pub fn generate_lib_content() -> String {
    String::from(
        r#"//! Generated Rune modules

mod native;

pub mod generated;

use protocol::App;
pub use protocol::{AppState, Filter, Task};

#[no_mangle]
pub extern "C" fn create_app() -> *mut dyn App {
    Box::into_raw(Box::new(AppImpl::default()))
}

#[derive(Default)]
struct AppImpl;

impl App for AppImpl {
    fn update(&mut self, state: &mut AppState) {
        generated::main::update(state);
    }

    fn render(&self, frame: &mut ratatui::Frame, state: &AppState) {
        let _ = frame;
        let _ = state;
    }

    fn handle_key(&mut self, _key: crossterm::event::KeyEvent, _state: &mut AppState) {}
}
"#,
    )
}
