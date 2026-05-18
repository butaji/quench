//! # Manifest Generator
//!
//! Generates Cargo.toml and lib.rs for simple runie projects.

/// Generate Cargo.toml for cdylib build.
#[must_use]
pub fn generate_cargo_toml() -> String {
    r#"[package]
name = "runie-app"
version = "0.1.0"
edition = "2021"

[lib]
name = "app"
crate-type = ["cdylib", "rlib"]

[dependencies]
ratatui = "0.26"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#.to_string()
}

/// Generate lib.rs that exports runie_render.
#[must_use]
pub fn generate_lib_content() -> String {
    r#"//! Generated runie UI

pub mod generated;

/// Render the UI (called by host).
#[no_mangle]
pub extern "C" fn runie_render(frame: &mut ratatui::Frame) {
    generated::main::render(frame);
}
"#.to_string()
}
