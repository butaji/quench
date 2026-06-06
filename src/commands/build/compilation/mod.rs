//! Compilation utilities

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use anyhow::{Context, Result};
use tracing::info;

/// Compile the project using cargo
pub fn compile_project(build_dir: &Path, release: bool) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if release {
        cmd.arg("--release");
    }
    cmd.arg("--manifest-path").arg(build_dir.join("Cargo.toml"));
    cmd.current_dir(build_dir);
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    info!("Running cargo build...");
    let status = cmd.status().context("cargo build failed")?;
    if !status.success() {
        anyhow::bail!("cargo build failed with exit code: {}", status);
    }
    Ok(())
}

/// Wait for child process
pub fn wait_for_child(child: &mut std::process::Child) -> Result<()> {
    let status = child.wait().context("failed to wait for child")?;
    if !status.success() {
        anyhow::bail!("process exited with code: {}", status);
    }
    Ok(())
}

/// Find the compiled binary
pub fn find_binary(_project_root: &Path, build_dir: &Path, release: bool) -> Option<PathBuf> {
    let profile = if release { "release" } else { "debug" };
    let base = build_dir.join("target").join(profile);
    
    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "runts".to_string());

    let binary = base.join(&exe_name);
    if binary.exists() {
        Some(binary)
    } else {
        base.join("deps").read_dir().ok().and_then(|entries| {
            entries.filter_map(|e| e.ok()).find(|e| {
                e.path().file_stem().map(|s| s.to_string_lossy().starts_with(&exe_name)).unwrap_or(false)
            }).map(|e| e.path())
        })
    }
}
