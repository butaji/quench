//! # Project Templates - Strings
//!
//! String constants for project template generation.

/// Cargo.toml additions for runie (user adds these themselves).
pub const CARGO_ADDITIONS: &str = r#"
[build-dependencies]
runie = "0.1"
"#;

/// build.rs template.
pub const BUILD_RS: &str = r#"//! Build script - run runie compiler
fn main() {
    runie::build();
}
"#;

/// Main.r.tsx template.
pub const MAIN_RS: &str = r#"//! Main entry point

/// Update state.
export function update(): void {
    // Your app logic here
}
"#;
