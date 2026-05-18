//! # Artifact Management Module
//!
//! Handles dylib artifact copying and hot reload setup.

use crate::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::BuildMode;

/// Copy artifact to hot directory with atomic symlink update.
pub fn copy_artifact_to_hot_dir(
    hot_dir: &Path,
    artifact: &PathBuf,
    target_crate: &str,
) -> Result<()> {
    let hot_path = create_timestamped_copy(hot_dir, artifact, target_crate)?;
    update_atomic_symlink(hot_dir, &hot_path)?;
    cleanup_old_artifacts(hot_dir, &target_crate.replace('-', "_"), 5)?;
    let filename = hot_path.file_name().map_or_else(
        || "unknown".to_string(),
        |n| n.to_string_lossy().to_string(),
    );
    println!("Hot reload ready: {}", filename);
    Ok(())
}

/// Create a timestamped copy of the artifact.
pub fn create_timestamped_copy(
    hot_dir: &Path,
    artifact: &PathBuf,
    target_crate: &str,
) -> Result<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let safe_name = target_crate.replace('-', "_");
    let ext = std::env::consts::DLL_EXTENSION;
    let prefix = if cfg!(windows) { "" } else { "lib" };
    let hot_name = format!("{prefix}{safe_name}_{timestamp}.{ext}");
    let hot_path = hot_dir.join(&hot_name);
    fs::copy(artifact, &hot_path)?;
    Ok(hot_path)
}

/// Atomically update the .current symlink.
pub fn update_atomic_symlink(hot_dir: &Path, hot_path: &Path) -> Result<()> {
    let current = hot_dir.join(".current");
    let tmp_link = hot_dir.join(".current.tmp");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(hot_path, &tmp_link)?;
        fs::rename(&tmp_link, &current)?;
    }

    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(hot_path, &tmp_link)?;
        fs::rename(&tmp_link, &current)?;
    }

    Ok(())
}

/// Clean up old dylib artifacts.
pub fn cleanup_old_artifacts(hot_dir: &Path, prefix: &str, keep: usize) -> Result<()> {
    let mut artifacts: Vec<_> = fs::read_dir(hot_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            let ext = std::env::consts::DLL_EXTENSION;
            name_str.starts_with(prefix) && name_str.ends_with(ext)
        })
        .collect();

    if artifacts.len() <= keep {
        return Ok(());
    }

    artifacts.sort_by_key(|e| {
        e.path()
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.rsplit_once('_').map(|(_, tail)| tail).and_then(|t| t.parse::<u64>().ok()))
    });

    let to_remove = artifacts.len() - keep;
    for artifact in artifacts.into_iter().take(to_remove) {
        let _ = fs::remove_file(artifact.path());
    }

    Ok(())
}

/// Setup hot reload directory structure.
pub fn setup_hot_reload_directory(workspace: &Path) -> Result<PathBuf> {
    let hot_dir = workspace.join("target/hot");
    fs::create_dir_all(&hot_dir)?;
    Ok(hot_dir)
}

/// Get the artifact path for the current build mode.
#[allow(dead_code)]
pub fn get_artifact_path(workspace: &Path, target_crate: &str, mode: BuildMode) -> Option<PathBuf> {
    let profile = match mode {
        BuildMode::Dev => "debug",
        BuildMode::Release => "release",
    };
    let ext = std::env::consts::DLL_EXTENSION;
    let prefix = if cfg!(windows) { "" } else { "lib" };
    let artifact_name = format!("{prefix}{target_crate}.{ext}");
    let artifact = workspace.join("target").join(profile).join(&artifact_name);
    if artifact.exists() {
        Some(artifact)
    } else {
        None
    }
}
