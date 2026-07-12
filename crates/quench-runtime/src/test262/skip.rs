//! test262 skip policy — single source of truth for skip logic.

use crate::test262::metadata::Test262Metadata;

/// Features NOT implemented (case-insensitive match skips the test).
const SKIP_FEATURES: &[&str] = &[
    // Async / Generators
    "async-functions",
    "async-iteration",
    "generators",
    "for-await-of",
    "top-level-await",
    // Class - partial: public fields work, private NOT implemented
    "class-fields-private",
    "class-static-fields-private",
    "private-fields",
    "private-methods",
    // Built-in globals
    "BigInt",
    "Proxy",
    "Reflect",
    "WeakMap",
    "WeakSet",
    "WeakRef",
    "TypedArray",
    // Symbol well-known
    "Symbol.iterator",
    "Symbol.asyncIterator",
    "Symbol.match",
    "Symbol.matchAll",
    "Symbol.replace",
    "Symbol.search",
    "Symbol.species",
    "Symbol.split",
    "Symbol.toStringTag",
    "Symbol.unscopables",
    // Modules
    "import.meta",
    "dynamic-import",
    // Decorators
    "decorators",
    "decorators-support-transition",
    // Array.prototype
    "Array.prototype.groupBy",
    "Array.prototype.groupByToMap",
    "Array.prototype.toReversed",
    "Array.prototype.toSorted",
    "Array.prototype.toSpliced",
    "Array.prototype.with",
    // Intl
    "Intl.DateTimeFormat",
    "Intl.NumberFormat",
    "Intl.Segmenter",
    // Other
    "regexp-match-indices",
    "hashbang",
    "New Function.prototype.toString",
];

/// Flags that skip the test.
/// NOTE: "raw" and "async" are NOT skipped — runner.rs has dedicated handling
/// for both (raw: no harness prelude; async: $DONE setup). "module" is skipped
/// separately with reason "module-flag" because modules can't run as scripts.
const SKIP_FLAGS: &[&str] = &[
    "shellFunction",
    "CanBlockIsFalse",
    "CanBlockIsTrue",
    "generated",
];

/// Whether a feature is considered implemented.
pub fn is_feature_supported(feature: &str) -> bool {
    !SKIP_FEATURES
        .iter()
        .any(|f| feature.eq_ignore_ascii_case(f))
}

/// Check if a test should be skipped.
pub fn should_skip(meta: &Test262Metadata) -> Option<String> {
    // Modules cannot be executed as classic scripts.
    if meta.flags.iter().any(|f| f == "module") {
        return Some("module-flag".to_string());
    }
    for feature in &meta.features {
        if SKIP_FEATURES
            .iter()
            .any(|f| feature.eq_ignore_ascii_case(f))
        {
            return Some(format!("Unsupported feature: {}", feature));
        }
    }
    for flag in &meta.flags {
        if SKIP_FLAGS.contains(&flag.as_str()) {
            return Some(format!("Unsupported flag: {}", flag));
        }
    }
    None
}

/// Source-level skip: catches tests that use async syntax without declaring
/// the `async-functions` feature (common in test/harness tests, which are
/// only tagged `flags: [async]`). Remove once async functions are supported.
pub fn should_skip_source(source: &str) -> Option<String> {
    if source.contains("async function") || source.contains("async(") || source.contains("await ") {
        return Some("async-syntax".to_string());
    }
    None
}

/// Path-level skip for individual tests whose premise contradicts the
/// runner's harness model. Keep this list as short as possible.
const SKIP_TEST_PATHS: &[&str] = &[
    // Self-test expects $DETACHBUFFER to be undefined, but the runner loads
    // detachArrayBuffer.js (and the $262 host object) for every test.
    "test/harness/detachArrayBuffer.js",
];

/// Check if a specific test file should be skipped.
pub fn should_skip_path(path: &str) -> Option<String> {
    SKIP_TEST_PATHS
        .iter()
        .find(|p| path.ends_with(**p))
        .map(|p| format!("incompatible test: {}", p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skip_bigint() {
        let meta = Test262Metadata {
            features: vec!["BigInt".to_string()],
            ..Default::default()
        };
        assert!(should_skip(&meta).is_some());
    }

    #[test]
    fn test_no_skip_basic() {
        assert!(should_skip(&Test262Metadata::default()).is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let meta = Test262Metadata {
            features: vec!["bigint".to_string()],
            ..Default::default()
        };
        assert!(should_skip(&meta).is_some());
    }

    #[test]
    fn test_source_skip_async_syntax() {
        assert_eq!(
            should_skip_source("var p = (async function () { await x; })();"),
            Some("async-syntax".to_string())
        );
        assert!(should_skip_source("var x = 1 + 1;").is_none());
    }
}
