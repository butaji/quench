//! test262 skip policy — zero skips. Every test runs; crashes become failures
//! via process isolation. See `runner/digest.rs` `inprocess_digest()`.

use crate::metadata::Test262Metadata;

const UNSUPPORTED_FEATURES: &[&str] = &[];

const CRASH_FILES: &[(&str, &str)] = &[];

pub fn crash_files() -> &'static [(&'static str, &'static str)] {
    CRASH_FILES
}

pub fn is_feature_supported(feature: &str) -> bool {
    !UNSUPPORTED_FEATURES.contains(&feature)
}

pub fn should_skip(meta: &Test262Metadata) -> Option<String> {
    for feature in &meta.features {
        if !is_feature_supported(feature) {
            return Some(format!("unsupported feature: {}", feature));
        }
    }
    None
}

pub fn should_skip_path(_path: &str) -> Option<String> {
    None
}

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
    fn path_skips_always_none() {
        assert!(should_skip_path("any/path.js").is_none());
        assert!(should_skip_path("").is_none());
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
}
