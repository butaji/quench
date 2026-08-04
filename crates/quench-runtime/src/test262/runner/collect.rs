//! Collect every `.js` test under a stage directory (no silent filtering).

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
    fn stage_zero_inventory_is_complete_and_unique() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/test262/test/harness");
        let tests = collect_tests(&root);
        assert_eq!(tests.len(), 116);
        let unique = tests.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), tests.len());
    }
}
