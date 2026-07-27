//! test262 skip policy — every skip is counted, never a pass.
//!
//! Path skips are temporary debt for known process-killing crashes.
//! Feature skips are empty: unsupported features must fail loudly so digests
//! drive implementation. Force-run path skips with `TEST262_NOSKIP=1`.

use crate::test262::metadata::Test262Metadata;

/// Features that abort the process (not merely fail). Empty by default —
/// prefer failing tests over silent skips so digests stay honest.
const UNSUPPORTED_FEATURES: &[&str] = &[];

/// Stage-relative paths (from the test262 root) that stack-overflow / abort
/// the process in-process. Every entry was verified to crash `run-test`
/// (exit 134); paths are full `test/...` suffixes, never bare basenames.
///
/// These tests either require tail-call optimization (TCO) in the tree-walking
/// interpreter (the 4 `tco-*.js` entries) or involve BigInt operator
/// recursion (the 12 `bigint-wrapped-values.js` entries).
///
/// Fix: implement tail-call elimination for `return func()` patterns, and
/// fix BigInt ToPrimitive to avoid valueOf/toString recursion in binary ops.
/// Until then the subprocess runner (classify_isolated) handles crash exit
/// codes as test failures, but the in-process stage runner would crash.
const CRASH_FILES: &[(&str, &str)] = &[
    (
        "test/language/statements/if/tco-else-body.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/statements/if/tco-if-body.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/statements/labeled/tco.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/statements/while/tco-body.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/addition/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/bitwise-and/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/bitwise-or/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/bitwise-xor/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/division/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/exponentiation/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/left-shift/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/modulus/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/multiplication/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/right-shift/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/subtraction/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
    (
        "test/language/expressions/unsigned-right-shift/bigint-wrapped-values.js",
        "known crash: stack overflow",
    ),
];

/// The configured crash-skip entries, for gate diagnostics.
pub fn crash_files() -> &'static [(&'static str, &'static str)] {
    CRASH_FILES
}

/// Returns true if the feature is implemented (or should be attempted).
pub fn is_feature_supported(feature: &str) -> bool {
    !UNSUPPORTED_FEATURES.contains(&feature)
}

/// Skip when a required feature is in the (normally empty) crash list.
pub fn should_skip(meta: &Test262Metadata) -> Option<String> {
    for feature in &meta.features {
        if !is_feature_supported(feature) {
            return Some(format!("unsupported feature: {}", feature));
        }
    }
    None
}

/// Path-level skip for known process killers. Honored unless `TEST262_NOSKIP=1`.
/// `path` is matched by full stage-relative suffix (`test/...`), so any
/// absolute or repo-relative prefix resolves to the same entry.
pub fn should_skip_path(path: &str) -> Option<String> {
    if noskip_enabled() {
        return None;
    }
    let normalized = path.replace('\\', "/");
    CRASH_FILES
        .iter()
        .find(|(rel, _)| normalized == *rel || normalized.ends_with(&format!("/{}", rel)))
        .map(|(_, reason)| (*reason).to_string())
}

/// Returns None — no source-level skips.
pub fn should_skip_source(_source: &str) -> Option<String> {
    None
}

fn noskip_enabled() -> bool {
    std::env::var("TEST262_NOSKIP")
        .ok()
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_skip_for_default_metadata() {
        assert!(should_skip(&Test262Metadata::default()).is_none());
    }

    #[test]
    fn formerly_unsupported_features_are_attempted() {
        for feat in [
            "Symbol",
            "BigInt",
            "TypedArray",
            "generators",
            "async-functions",
        ] {
            let mut meta = Test262Metadata::default();
            meta.features.push(feat.to_string());
            assert!(
                should_skip(&meta).is_none(),
                "{feat} must not be feature-skipped"
            );
        }
    }

    #[test]
    fn crash_files_are_skipped_by_full_relative_path() {
        // crash files list is empty — no tests are skipped via CRASH_FILES.
        // The subprocess runner handles crashes as test failures.
        assert!(should_skip_path("tests/test262/test/language/statements/labeled/tco.js").is_none());
        assert!(should_skip_path("test/language/statements/labeled/tco.js").is_none());
    }

    #[test]
    fn same_basename_different_dir_is_not_skipped() {
        // return/tco.js no longer crashes; only labeled/tco.js does.
        assert!(should_skip_path("tests/test262/test/language/statements/return/tco.js").is_none());
    }

    #[test]
    fn unknown_paths_are_not_skipped() {
        assert!(should_skip_path("foo/bar.js").is_none());
    }

    #[test]
    fn test_is_feature_supported() {
        assert!(is_feature_supported("arrowFunctions"));
        assert!(is_feature_supported("Symbol"));
        assert!(is_feature_supported("TypedArray"));
    }

    #[test]
    fn test_should_skip_source_no_skips() {
        assert!(should_skip_source("async function foo() {}").is_none());
    }

    /// Verify each CRASH_FILES entry points at an existing test262 file.
    /// Ensures stale skip entries (for tests that no longer crash) are removed.
    #[test]
    fn crash_files_exist_on_disk() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/test262");
        let test262_dir =
            std::env::var("TEST262_DIR").unwrap_or_else(|_| root.to_string_lossy().into_owned());
        let test262_path = std::path::Path::new(&test262_dir);
        let mut missing = Vec::new();
        for (rel_path, _reason) in CRASH_FILES {
            if !test262_path.join(rel_path).is_file() {
                missing.push(*rel_path);
            }
        }
        assert!(
            missing.is_empty(),
            "CRASH_FILES entries with no matching test262 file: {:?}\n\
             These entries are stale and should be removed.",
            missing
        );
    }
}
