//! Collect every `.js` test under a stage directory (no silent filtering).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Recursively collect test files. Fixtures (`*_FIXTURE.js`) are excluded;
/// crash/feature skips are applied at run time, not here.
pub fn collect_tests(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_into(dir, &mut out);
    out.sort();
    out
}

/// Keep only tests whose path string appears in a prior digest JSON.
pub fn filter_by_failed_json(tests: Vec<PathBuf>, json_path: &str) -> Vec<PathBuf> {
    let Ok(text) = fs::read_to_string(json_path) else {
        return tests;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return tests;
    };
    let Some(groups) = value.get("groups").and_then(|g| g.as_array()) else {
        return tests;
    };
    let mut keep = HashSet::new();
    for group in groups {
        if let Some(paths) = group.get("paths").and_then(|p| p.as_array()) {
            for p in paths {
                if let Some(s) = p.as_str() {
                    keep.insert(s.to_string());
                }
            }
        }
        if let Some(samples) = group.get("samples").and_then(|p| p.as_array()) {
            for p in samples {
                if let Some(s) = p.as_str() {
                    keep.insert(s.to_string());
                }
            }
        }
    }
    if keep.is_empty() {
        return tests;
    }
    tests
        .into_iter()
        .filter(|p| keep.contains(&p.display().to_string()))
        .collect()
}

fn collect_into(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.is_file() {
        if is_test_file(dir) {
            out.push(dir.to_path_buf());
        }
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_into(&p, out);
        } else if is_test_file(&p) {
            out.push(p);
        }
    }
}

fn is_test_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "js")
        && !path
            .file_name()
            .map(|n| n.to_string_lossy().ends_with("_FIXTURE.js"))
            .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn collects_js_and_skips_fixtures() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.js"), "").unwrap();
        fs::write(dir.path().join("b_FIXTURE.js"), "").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/c.js"), "").unwrap();
        let got = collect_tests(dir.path());
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|p| !p.to_string_lossy().contains("FIXTURE")));
    }

    #[test]
    fn includes_formerly_skipped_dirs() {
        let dir = tempdir().unwrap();
        for name in ["elements", "method", "dstr"] {
            let sub = dir.path().join(name);
            fs::create_dir(&sub).unwrap();
            fs::write(sub.join("t.js"), "").unwrap();
        }
        assert_eq!(collect_tests(dir.path()).len(), 3);
    }

    #[test]
    fn filter_by_failed_json_keeps_listed_paths() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.js");
        let b = dir.path().join("b.js");
        fs::write(&a, "").unwrap();
        fs::write(&b, "").unwrap();
        let json = dir.path().join("failures.json");
        let body = format!(r#"{{"groups":[{{"paths":["{}"]}}]}}"#, a.display());
        fs::write(&json, body).unwrap();
        let all = collect_tests(dir.path());
        let filtered = filter_by_failed_json(all, json.to_str().unwrap());
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], a);
    }
}
