//! Cargo.toml generation

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Generate Cargo.toml into the hidden build directory.
pub fn generate(_project_root: &Path, build_dir: &Path) -> Result<()> {
    use std::fs;

    let runts_lib_path = get_runts_lib_path()
        .context("Failed to locate runts-lib dependency")?;
    // Always use "runts-app" as binary name for consistency with run_plugin_build
    let cargo = build_cargo_toml("runts-app", &runts_lib_path);

    fs::create_dir_all(build_dir)?;
    fs::write(build_dir.join("Cargo.toml"), cargo)?;
    Ok(())
}

/// Find runts-lib path, validating it exists
fn get_runts_lib_path() -> anyhow::Result<PathBuf> {
    // Try relative to current exe first
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join("crates").join("runts-lib");
            if candidate.exists() {
                return Ok(candidate.canonicalize().unwrap_or(candidate));
            }
            // Also try parent directories (in case exe is in target/debug/ or target/release/)
            for ancestor in exe_dir.ancestors() {
                let candidate = ancestor.join("crates").join("runts-lib");
                if candidate.exists() {
                    return Ok(candidate.canonicalize().unwrap_or(candidate));
                }
            }
        }
    }

    // Try CARGO_MANIFEST_DIR at compile time via env!
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let manifest_path = PathBuf::from(manifest_dir);
    let runts_lib_from_manifest = manifest_path.parent()
        .map(|p| p.join("runts-lib"))
        .unwrap_or_else(|| manifest_path.join("runts-lib"));
    if runts_lib_from_manifest.exists() {
        return Ok(runts_lib_from_manifest.canonicalize()
            .unwrap_or(runts_lib_from_manifest));
    }

    anyhow::bail!(
        "runts-lib not found. Searched:\n\
         - crates/runts-lib relative to exe\n\
         - {}",
        runts_lib_from_manifest.display()
    )
}

fn build_cargo_toml(app_name: &str, runts_lib_path: &Path) -> String {
    format!(
        r#"[package]
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
tokio = {{ version = "1.0", features = ["rt", "rt-multi-thread", "sync", "macros", "io-util", "net"] }}
axum = "0.7"
tower = "0.4"
tower-http = {{ version = "0.5", features = ["fs", "cors", "trace"] }}
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}

[workspace]

[profile.release]
lto = true
codegen-units = 1
"#,
        runts_lib_path.display()
    )
}
