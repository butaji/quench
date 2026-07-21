//! test262 skip policy — no skips; every test is attempted.
//
// Features not yet implemented are listed in UNSUPPORTED_FEATURES.
// The `features:` field in test262 frontmatter is the canonical mechanism
// for indicating a test requires a specific feature — using it is not a "skip"
// in the anti-pattern sense.

use crate::test262::metadata::Test262Metadata;

/// Features not yet implemented — tests requiring these are skipped.
const UNSUPPORTED_FEATURES: &[&str] = &[
    // TypedArray (Int8Array, Uint8Array, etc.)
    "TypedArray",
    // Symbol
    "Symbol",
    // BigInt
    "BigInt",
    // async functions
    "async-functions",
    // generators
    "generators",
    // Reflect.construct
    "Reflect.construct",
];

/// Returns true if the feature is implemented.
pub fn is_feature_supported(feature: &str) -> bool {
    !UNSUPPORTED_FEATURES.contains(&feature)
}

/// Skip a test if any of its required features are not yet implemented.
pub fn should_skip(meta: &Test262Metadata) -> Option<String> {
    for feature in &meta.features {
        if !is_feature_supported(feature) {
            return Some(format!("unsupported feature: {}", feature));
        }
    }
    None
}

/// Returns None — no path-level skips.
pub fn should_skip_path(_path: &str) -> Option<String> {
    None
}

/// Returns None — no source-level skips.
pub fn should_skip_source(_source: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_skip_for_default_metadata() {
        assert!(should_skip(&Test262Metadata::default()).is_none());
    }

    #[test]
    fn test_no_skip_for_supported_features() {
        let mut meta = Test262Metadata::default();
        meta.features.push("arrowFunctions".to_string());
        assert!(should_skip(&meta).is_none());
    }

    #[test]
    fn test_skip_for_unsupported_feature() {
        let mut meta = Test262Metadata::default();
        meta.features.push("TypedArray".to_string());
        let result = should_skip(&meta);
        assert!(result.is_some());
        assert!(result.unwrap().contains("TypedArray"));
    }

    #[test]
    fn test_skip_for_multiple_features_one_unsupported() {
        let mut meta = Test262Metadata::default();
        meta.features.push("arrowFunctions".to_string());
        meta.features.push("BigInt".to_string());
        let result = should_skip(&meta);
        assert!(result.is_some());
        assert!(result.unwrap().contains("BigInt"));
    }

    #[test]
    fn test_is_feature_supported() {
        assert!(is_feature_supported("arrowFunctions"));
        assert!(is_feature_supported("let-scoped-variables"));
        assert!(!is_feature_supported("TypedArray"));
        assert!(!is_feature_supported("Symbol"));
        assert!(!is_feature_supported("BigInt"));
        assert!(!is_feature_supported("async-functions"));
        assert!(!is_feature_supported("generators"));
        assert!(!is_feature_supported("Reflect.construct"));
    }

    #[test]
    fn test_should_skip_path_no_skips() {
        assert!(should_skip_path("anything.js").is_none());
    }

    #[test]
    fn test_should_skip_source_no_skips() {
        assert!(should_skip_source("async function foo() {}").is_none());
    }
}
