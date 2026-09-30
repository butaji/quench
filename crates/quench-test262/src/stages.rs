//! Stage definitions for deterministic conformance runs.

use std::path::{Path, PathBuf};

const STAGE_SPEC: &str = include_str!("../../../docs/STAGES.md");
const STAGE_PREFIX: &str = "- Stage ";

/// One conformance stage.
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

impl ResolvedStage {
    /// The most specific declared root owns each test, including tests directly
    /// in a parent directory that also contains separately ordered child stages.
    pub fn owns_file(&self, file: &Path, stages: &[Self]) -> bool {
        file.starts_with(&self.root)
            && !stages.iter().any(|nested| {
                nested.root != self.root
                    && nested.root.starts_with(&self.root)
                    && file.starts_with(&nested.root)
            })
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_stages_partition_parent_files_and_directory_boundaries() {
        let stages: Vec<_> = [
            "test/intl402",
            "test/intl402/DateTimeFormat",
            "test/built-ins",
        ]
        .into_iter()
        .enumerate()
        .map(|(id, path)| ResolvedStage {
            id: id as u32,
            path: path.into(),
            root: path.into(),
        })
        .collect();
        for (path, owner) in [
            ("test/intl402/default-locale.js", Some(0)),
            ("test/intl402/DateTimeFormat/prototype/format.js", Some(1)),
            ("test/intl402/DateTimeFormat-extra/test.js", Some(0)),
            ("test/built-ins/Array/test.js", Some(2)),
            ("test/language/test.js", None),
        ] {
            let owners: Vec<_> = stages
                .iter()
                .filter(|stage| stage.owns_file(Path::new(path), &stages))
                .map(|stage| stage.id)
                .collect();
            assert_eq!(owners, owner.into_iter().collect::<Vec<_>>(), "{path}");
        }
    }
}
