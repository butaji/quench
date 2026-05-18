//! # Manifest Generator
//!
//! Generates Cargo.toml and lib.rs for simple runie projects.

/// Generate Cargo.toml additions for user project.
#[must_use]
pub fn generate_cargo_additions() -> String {
    r#"[build-dependencies]
runie = "0.1"
"#.to_string()
}

/// Generate lib.rs content with generated modules.
#[must_use]
pub fn generate_lib_content() -> String {
    String::from(
        r#"//! Generated runie modules

pub mod generated;
"#,
    )
}
