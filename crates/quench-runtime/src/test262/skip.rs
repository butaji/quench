//! test262 skip policy — every skip is counted, never a pass.
//!
//! Path skips are temporary debt for known process-killing crashes.
//! Feature skips are empty: unsupported features must fail loudly so digests
//! drive implementation. Force-run path skips with `TEST262_NOSKIP=1`.

use crate::test262::metadata::Test262Metadata;
use std::path::Path;

/// Features that abort the process (not merely fail). Empty by default —
/// prefer failing tests over silent skips so digests stay honest.
const UNSUPPORTED_FEATURES: &[&str] = &[];

/// File basenames that stack-overflow / abort the process in-process.
const CRASH_FILES: &[(&str, &str)] = &[
    ("prototype-wiring.js", "known crash: stack overflow"),
    ("prototype-setter.js", "known crash: stack overflow"),
    (
        "this-access-restriction-2.js",
        "known crash: stack overflow",
    ),
    ("this-access-restriction.js", "known crash: stack overflow"),
    ("this-check-ordering.js", "known crash: stack overflow"),
    ("restricted-properties.js", "known crash: stack overflow"),
    (
        "static-init-arguments-functions.js",
        "known crash: stack overflow",
    ),
    (
        "static-init-arguments-methods.js",
        "known crash: stack overflow",
    ),
    (
        "static-init-arguments-eval.js",
        "known crash: stack overflow",
    ),
    ("tco.js", "known crash: stack overflow"),
];

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
pub fn should_skip_path(path: &str) -> Option<String> {
    if noskip_enabled() {
        return None;
    }
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    CRASH_FILES
        .iter()
        .find(|(file, _)| *file == name)
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
    fn crash_files_are_skipped_by_path() {
        let r = should_skip_path("foo/tco.js");
        assert!(r.unwrap().contains("crash"));
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
}
