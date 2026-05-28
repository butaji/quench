//! Cargo.toml generation

use anyhow::Result;
use std::path::{Path, PathBuf};

/// Generate Cargo.toml into the hidden build directory.
pub fn generate(project_root: &Path, build_dir: &Path) -> Result<()> {
    use std::fs;

    let app_name = get_app_name(project_root);
    let runts_lib_path = get_runts_lib_path();
    let cargo = build_cargo_toml(&app_name, &runts_lib_path);

    fs::create_dir_all(build_dir)?;
    fs::write(build_dir.join("Cargo.toml"), cargo)?;
    Ok(())
}

fn get_app_name(project_root: &Path) -> String {
    project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf())
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("app")
        .replace('-', "_")
}

fn get_runts_lib_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .map(|p| p.join("crates").join("runts-lib"))
        .unwrap_or_else(|| PathBuf::from(".."))
}

fn build_cargo_toml(app_name: &str, runts_lib_path: &Path) -> String {
    format!(r#"[package]
name = "{app_name}"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[[bin]]
name = "{app_name}"
path = "src/main.rs"

[dependencies]
runts-lib = {{ path = "{}" }}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
tokio = {{ version = "1.0", features = ["full"] }}
axum = "0.7"
tower = "0.4"
tower-http = {{ version = "0.5", features = ["fs", "cors", "trace"] }}
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}

[workspace]

[profile.release]
lto = true
codegen-units = 1
"#, runts_lib_path.display())
}
