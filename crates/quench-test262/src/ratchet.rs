//! Immutable pass expectations shared by full and focused runner checks.
use std::{collections::BTreeSet, fs, path::Path};

pub const RATCHET_SCHEMA_VERSION: u64 = 1;
pub const RATCHET_ENGINE: &str = "next";

pub const DEFAULT_RATCHET: &str = "target/test262-ratchet.json";

/// Stable test identity used by reports and both ratchet scopes.
pub fn relative_test_path(path: &Path, test_root: &Path) -> String {
    path.strip_prefix(test_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The frozen pass set is an expectation, not a report of the current run.
pub struct PassSet {
    passes: BTreeSet<String>,
}

impl PassSet {
    pub fn read(path: &Path) -> Result<Self, String> {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("read Test262 ratchet {}: {error}", path.display()))?;
        let baseline = serde_json::from_str(&contents)
            .map_err(|error| format!("parse Test262 ratchet {}: {error}", path.display()))?;
        Self::from_json(&baseline)
    }

    pub fn from_json(baseline: &serde_json::Value) -> Result<Self, String> {
        if baseline["schema"] != RATCHET_SCHEMA_VERSION || baseline["engine"] != RATCHET_ENGINE {
            return Err("Test262 ratchet has an unsupported schema or engine".into());
        }
        let passes = baseline["passes"]
            .as_array()
            .ok_or_else(|| "Test262 ratchet has no pass set".to_string())?;
        let passes = passes
            .iter()
            .map(|path| {
                path.as_str().map(str::to_owned).ok_or_else(|| {
                    "Test262 ratchet pass set contains a non-string path".to_string()
                })
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if passes.is_empty() {
            return Err("Test262 ratchet has an empty pass set".into());
        }
        Ok(Self { passes })
    }

    /// Scope selects expectations, including paths deleted since freezing.
    /// A focused run never treats expectations outside its scope as failures.
    pub fn regressions(
        &self,
        current_passes: &std::collections::HashSet<String>,
        in_scope: impl Fn(&str) -> bool,
    ) -> Vec<String> {
        self.passes
            .iter()
            .filter(|path| in_scope(path) && !current_passes.contains(*path))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn focused_scope_detects_failed_and_deleted_expected_paths() {
        let baseline = PassSet::from_json(&serde_json::json!({
            "schema":1,"engine":"next","passes":["Array/keep.js","Array/deleted.js","Array/failed.js","Promise/other.js"]
        })).unwrap();
        let current = HashSet::from(["Array/keep.js".into(), "Array/new.js".into()]);
        assert_eq!(
            baseline.regressions(&current, |path| path.starts_with("Array/")),
            ["Array/deleted.js", "Array/failed.js"]
        );
        assert_eq!(
            baseline.regressions(&current, |_| true),
            ["Array/deleted.js", "Array/failed.js", "Promise/other.js"]
        );
    }

    #[test]
    fn invalid_expectations_are_rejected_even_outside_a_selected_scope() {
        for baseline in [
            serde_json::json!({"schema":1,"engine":"legacy","passes":[]}),
            serde_json::json!({"schema":1,"engine":"next"}),
            serde_json::json!({"schema":1,"engine":"next","passes":[]}),
            serde_json::json!({"schema":1,"engine":"next","passes":["keep.js",42]}),
        ] {
            assert!(PassSet::from_json(&baseline).is_err());
        }
    }
}
