//! test262 skip policy — minimal skips for features not yet implemented.

use crate::test262::metadata::Test262Metadata;

/// Returns false for features that are not yet implemented.
pub fn is_feature_supported(feature: &str) -> bool {
    match feature {
        "hashbang" => false,  // hashbang comments not yet supported
        _ => true,
    }
}

/// Returns a skip reason if the test should be skipped based on metadata.
pub fn should_skip(meta: &Test262Metadata) -> Option<String> {
    for feature in &meta.features {
        if !is_feature_supported(feature) {
            return Some(format!("unsupported feature: {}", feature));
        }
    }
    None
}

/// Always returns None — no source-level skips.
pub fn should_skip_source(_source: &str) -> Option<String> {
    None
}

/// Always returns None — no path-level skips.
pub fn should_skip_path(_path: &str) -> Option<String> {
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
