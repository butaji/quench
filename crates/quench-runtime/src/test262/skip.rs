//! test262 skip policy — no skips; every test is attempted.

use crate::test262::metadata::Test262Metadata;

/// Returns true for all features — no tests are skipped by feature.
pub fn is_feature_supported(_feature: &str) -> bool {
    true
}

/// Returns None — no tests are skipped by metadata.
pub fn should_skip(_meta: &Test262Metadata) -> Option<String> {
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
    fn test_no_skip() {
        assert!(should_skip(&Test262Metadata::default()).is_none());
        assert!(should_skip_source("async function foo() {}").is_none());
        assert!(should_skip_path("anything.js").is_none());
    }
}
