//! Stage definitions for deterministic conformance runs.

use std::path::{Path, PathBuf};

const STAGE_SPEC: &str = include_str!("../../../docs/STAGES.md");
const STAGE_PREFIX: &str = "- Stage ";

/// One canonical stage entry from [`docs/STAGES.md`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceStage {
    /// Human-readable stage index.
    pub id: u32,
    /// Relative path used by the stage definition.
    pub path: String,
}

/// A stage path resolved against a concrete `test262` checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStage {
    /// Human-readable stage index.
    pub id: u32,
    /// Relative path used by the stage definition.
    pub path: String,
    /// Absolute path that can be discovered as runnable test files.
    pub root: PathBuf,
}

/// Parse all stage entries from [`STAGE_SPEC`].
pub fn list_stages() -> Vec<ConformanceStage> {
    STAGE_SPEC.lines().filter_map(parse_stage_line).collect()
}

/// Convert all declared stages into concrete filesystem paths.
pub fn resolve_stages(test262_root: &Path) -> Result<Vec<ResolvedStage>, String> {
    let mut stages = Vec::new();
    for stage in list_stages() {
        let root = resolve_stage_root(test262_root, &stage.path)?;
        if !root.is_dir() {
            return Err(format!(
                "stage {} has missing or non-directory path {}",
                stage.id,
                root.display(),
            ));
        }
        stages.push(ResolvedStage {
            id: stage.id,
            path: stage.path,
            root,
        });
    }
    Ok(stages)
}

fn parse_stage_line(line: &str) -> Option<ConformanceStage> {
    let trimmed = line.trim();
    if !trimmed.starts_with(STAGE_PREFIX) {
        return None;
    }
    let (left, right) = trimmed.split_once(':')?;
    let id = left.trim_start_matches(STAGE_PREFIX).trim().parse().ok()?;
    let path = right.trim();
    if !path.starts_with('`') || !path.ends_with('`') {
        return None;
    }
    let path = path[1..path.len() - 1].to_string();
    Some(ConformanceStage { id, path })
}

fn resolve_stage_root(test262_root: &Path, path: &str) -> Result<PathBuf, String> {
    let path = if path.starts_with("test/") {
        Path::new(path).to_path_buf()
    } else {
        Path::new("test").join(path)
    };
    let resolved = test262_root.join(path);
    if !resolved.exists() {
        return Err(format!(
            "stage path missing in test262 checkout: {}",
            resolved.display()
        ));
    }
    Ok(resolved)
}
