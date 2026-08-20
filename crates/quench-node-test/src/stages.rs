//! Stage definitions for deterministic Node conformance runs.

use std::path::{Path, PathBuf};

use crate::reader::NodeFixture;

const STAGE_SPEC: &str = include_str!("../../../docs/NODE-STAGES.md");

/// One canonical stage entry from `docs/NODE-STAGES.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeStage {
    pub id: u32,
    pub path: String,
}

/// A stage path resolved against a concrete `node-tests` checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStage {
    pub id: u32,
    pub path: String,
    pub root: PathBuf,
}

/// Parse all stage entries from the stage spec.
pub fn list_stages() -> Vec<NodeStage> {
    let mut stages = Vec::new();
    for line in STAGE_SPEC.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("1. **") else {
            continue;
        };
        let Some((name, _)) = rest.split_once(".**") else {
            continue;
        };
        let id = stages.len() as u32 + 1;
        stages.push(NodeStage {
            id,
            path: name.to_string(),
        });
    }
    stages
}

/// Convert all declared stages into concrete filesystem paths.
pub fn resolve_stages(node_tests_root: &Path) -> Result<Vec<ResolvedStage>, String> {
    let mut stages = Vec::new();
    for (i, stage) in list_stages().into_iter().enumerate() {
        let root = node_tests_root.join(&stage.path);
        if !root.exists() {
            // Stage is optional; an empty stage is a non-fatal skip.
            stages.push(ResolvedStage {
                id: i as u32 + 1,
                path: stage.path,
                root,
            });
            continue;
        }
        stages.push(ResolvedStage {
            id: i as u32 + 1,
            path: stage.path,
            root,
        });
    }
    Ok(stages)
}

/// Discover all `*.js` fixtures under `root`, filtered by stage.
pub fn discover_fixtures(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(read) = std::fs::read_dir(root) {
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "js") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Trampoline adapter so `NodeTestRunner::run_fixture` can work
/// on `NodeFixture` directly.
pub fn adapter(fixture: &NodeFixture) -> &Path {
    fixture.path()
}
