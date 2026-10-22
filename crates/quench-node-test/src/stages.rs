//! Stage definitions for deterministic Node conformance runs.

use std::path::{Path, PathBuf};

const STAGE_SPEC: &str = include_str!("../../../STAGES.md");

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
        let Some(rest) = trimmed.strip_prefix("### ") else {
            continue;
        };
        let Some((number, name)) = rest.split_once(". ") else {
            continue;
        };
        let Ok(id) = number.parse::<u32>() else {
            continue;
        };
        stages.push(NodeStage {
            id,
            path: name.trim().to_string(),
        });
    }
    stages
}

/// Convert all declared stages into concrete filesystem paths.
pub fn resolve_stages(node_tests_root: &Path) -> Result<Vec<ResolvedStage>, String> {
    let mut stages = Vec::new();
    for stage in list_stages() {
        let root = node_tests_root.join(&stage.path);
        if !root.exists() {
            // Stage is optional; an empty stage is a non-fatal skip.
            stages.push(ResolvedStage {
                id: stage.id,
                path: stage.path,
                root,
            });
            continue;
        }
        stages.push(ResolvedStage {
            id: stage.id,
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
pub fn discover_fixtures(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    enum DirectoryStep {
        Enter(PathBuf),
        Leave(PathBuf),
    }
    let mut out = Vec::new();
    let mut pending = vec![DirectoryStep::Enter(root.to_path_buf())];
    let mut ancestors = std::collections::HashSet::new();
    while let Some(step) = pending.pop() {
        let directory = match step {
            DirectoryStep::Leave(canonical) => {
                ancestors.remove(&canonical);
                continue;
            }
            DirectoryStep::Enter(directory) => directory,
        };
        let canonical = directory.canonicalize()?;
        if !ancestors.insert(canonical.clone()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("cyclic fixture directory: {}", directory.display()),
            ));
        }
        pending.push(DirectoryStep::Leave(canonical));
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if std::fs::metadata(&path)?.is_dir() {
                pending.push(DirectoryStep::Enter(path));
            } else if is_fixture(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn is_fixture(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "js" | "mjs" | "cjs"))
}

#[cfg(test)]
mod tests {
    use super::{discover_fixtures, list_stages};
    use std::fs;

    #[test]
    fn discovers_nested_node_fixture_extensions() {
        let root =
            std::env::temp_dir().join(format!("quench-node-test-discovery-{}", std::process::id()));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("test-a.js"), "").unwrap();
        fs::write(nested.join("test-b.mjs"), "").unwrap();
        fs::write(nested.join("test-c.cjs"), "").unwrap();
        fs::write(nested.join("README.md"), "").unwrap();

        let fixtures = discover_fixtures(&root).unwrap();
        assert_eq!(fixtures.len(), 3);
        assert!(fixtures.iter().any(|path| path.ends_with("test-b.mjs")));
        assert!(fixtures.iter().any(|path| path.ends_with("test-c.cjs")));
        assert!(discover_fixtures(&root.join("missing")).is_err());
        assert!(discover_fixtures(&root.join("test-a.js")).is_err());
        #[cfg(unix)]
        {
            let cycle = nested.join("cycle");
            std::os::unix::fs::symlink(&root, &cycle).unwrap();
            assert!(discover_fixtures(&root).is_err());
            fs::remove_file(cycle).unwrap();
            let alias = root.join("alias");
            std::os::unix::fs::symlink(&nested, &alias).unwrap();
            assert_eq!(discover_fixtures(&root).unwrap().len(), 5);
            fs::remove_file(alias).unwrap();
            std::os::unix::fs::symlink(root.join("missing.js"), nested.join("broken.js")).unwrap();
            assert!(discover_fixtures(&root).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_numbered_stage_headings() {
        let stages = list_stages();
        assert!(stages.len() >= 12);
        assert_eq!(stages[0].id, 0);
        assert_eq!(stages[0].path, "Measurement and runner truth");
        assert_eq!(stages[11].id, 11);
    }
}
