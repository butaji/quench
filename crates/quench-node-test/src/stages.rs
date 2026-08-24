//! Stage definitions for deterministic Node conformance runs.

use std::path::{Path, PathBuf};

use crate::reader::NodeFixture;

const STAGE_SPEC: &str = "";

/// One canonical stage entry.
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

/// Discover executable Node fixtures recursively under `root`.
///
/// Node's parallel suite contains nested `.js`, `.mjs`, and `.cjs` files;
/// top-level `.js` enumeration silently understates the compatibility gate.
pub fn discover_fixtures(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(read) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if is_fixture(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn is_fixture(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "js" | "mjs" | "cjs"))
}

/// Trampoline adapter so `NodeTestRunner::run_fixture` can work
/// on `NodeFixture` directly.
pub fn adapter(fixture: &NodeFixture) -> &Path {
    fixture.path()
}

#[cfg(test)]
mod tests {
    use super::discover_fixtures;
    use std::fs;

    #[test]
    fn discovers_nested_node_fixture_extensions() {
        let root = std::env::temp_dir().join(format!(
            "quench-node-test-discovery-{}",
            std::process::id()
        ));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("test-a.js"), "").unwrap();
        fs::write(nested.join("test-b.mjs"), "").unwrap();
        fs::write(nested.join("test-c.cjs"), "").unwrap();
        fs::write(nested.join("README.md"), "").unwrap();

        let fixtures = discover_fixtures(&root);
        assert_eq!(fixtures.len(), 3);
        assert!(fixtures.iter().any(|path| path.ends_with("test-b.mjs")));
        assert!(fixtures.iter().any(|path| path.ends_with("test-c.cjs")));
        fs::remove_dir_all(root).unwrap();
    }
}
