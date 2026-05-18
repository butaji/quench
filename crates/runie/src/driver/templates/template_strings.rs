//! # Project Templates - Strings
//!
//! String constants for project template generation.

/// Workspace Cargo.toml template.
pub const WORKSPACE_CARGO: &str = r#"[workspace]
resolver = "2"
members = [
    "crates/protocol",
    "crates/host",
    "crates/{target_crate}",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["{authors}"]
license = "MIT"
rust-version = "1.75"

[workspace.dependencies]
ratatui = "0.26"
crossterm = "0.27"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#;

/// Rune config template.
pub const RUNE_CONFIG: &str = r#"[project]
name = "{name}"

[build]
target_crate = "{target_crate}"
host_crate = "host"

[dev]
hot_reload = true
debounce = 100

[release]
static_binary = true
lto = true
"#;

/// Protocol crate Cargo.toml template.
pub const PROTOCOL_CARGO: &str = r#"[package]
name = "protocol"
version = "0.1.0"
edition = "2021"

[dependencies]
ratatui = { workspace = true }
crossterm = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
"#;

/// Host crate Cargo.toml template.
pub const HOST_CARGO: &str = r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{name}"
path = "src/main.rs"

[dependencies]
protocol = {{ path = "../protocol" }}
ratatui = {{ workspace = true }}
crossterm = {{ workspace = true }}
serde = {{ workspace = true }}
serde_json = {{ workspace = true }}
libloading = "0.8"
"#;

/// App crate Cargo.toml template.
pub const APP_CARGO: &str = r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lib]
name = "{name}"
crate-type = ["cdylib", "rlib"]
path = "src/lib.rs"

[dependencies]
protocol = {{ path = "../protocol" }}
ratatui = {{ workspace = true }}
crossterm = {{ workspace = true }}
serde = {{ workspace = true }}
serde_json = {{ workspace = true }}

[build-dependencies]
rune = {{ path = "../../.." }}
"#;
