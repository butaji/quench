//! Stage definitions for deterministic Node conformance runs.

use std::path::{Path, PathBuf};

use crate::reader::NodeFixture;

const STAGE_SPEC: &str = r#"
1. **test/parallel.**
1. **test/es-module.**
1. **test/common.**
1. **test/fixtures.**
"#;

/// One canonical stage entry from the embedded stage specification.
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
    Ok(list_stages()
        .into_iter()
        .map(|stage| ResolvedStage {
            id: stage.id,
            root: node_tests_root.join(&stage.path),
            path: stage.path,
        })
        .collect())
}

/// Discover all executable JavaScript fixtures under `root`, filtered by
/// stage. Node's upstream suite uses `.js`, `.mjs`, and `.cjs`; omitting the
/// latter two silently turns a purported full-suite run into a partial one.
pub fn discover_fixtures(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(read) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| matches!(ext.to_str(), Some("js" | "mjs" | "cjs")))
            {
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
