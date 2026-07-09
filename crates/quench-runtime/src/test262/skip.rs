//! test262 skip policy - determines which tests to skip

use crate::test262::metadata::Test262Metadata;

/// Features to skip (not yet supported by quench-runtime)
///
/// ## Completed audits (task 91)
/// - "template-literals" - implemented via lower/literals.rs
/// - "optional-chaining" - implemented via lower/opt_chain.rs
/// - "nullish-coalescing" - implemented via eval/operators.rs
/// - "optional-catch-binding" - implemented: ast.rs param=Option<String>,
///   eval/statement.rs handles None param
const SKIP_FEATURES: &[&str] = &[
    "Promise",
    "async-functions",
    "async-iteration",
    "generators",
    "class",
    "class-fields-private",
    "class-fields-public",
    "class-static-fields-private",
    "class-static-fields-public",
    "BigInt",
    "Proxy",
    "Reflect",
    "WeakMap",
    "WeakSet",
    "WeakRef",
    "TypedArray",
    "RegExp",
    "RegExp Unicode property escapes",
    "Symbol",
    "Symbol.iterator",
    "Symbol.asyncIterator",
    "Symbol.hasInstance",
    "Symbol.isConcatSpreadable",
    "Symbol.match",
    "Symbol.matchAll",
    "Symbol.replace",
    "Symbol.search",
    "Symbol.species",
    "Symbol.split",
    "Symbol.toPrimitive",
    "Symbol.toStringTag",
    "Symbol.unscopables",
    "default-parameters",
    "destructuring-binding",
    "spread",
    "spread-syntax",
    "for-await-of",
    "logical-assignment",
    "import.meta",
    "export-star",
    "export-from",
    "export-default",
    "dynamic-import",
];

/// Flags to skip
/// Note: "onlyStrict" is handled specially by runner.rs (adds "use strict")
const SKIP_FLAGS: &[&str] = &[
    "noStrict",
    "raw",
    "module",
    "async",
    "shellFunction",
];

/// Negative test phases to skip
const SKIP_NEGATIVE_PHASES: &[&str] = &["parse"];

/// Check if a test should be skipped based on its metadata
pub fn should_skip(meta: &Test262Metadata) -> Option<String> {
    // Check negative phase
    if let Some(ref neg) = meta.negative {
        if SKIP_NEGATIVE_PHASES.contains(&neg.phase.as_str()) {
            return Some(format!("Negative test phase: {}", neg.phase));
        }
    }

    // Check for unsupported features
    for feature in &meta.features {
        if SKIP_FEATURES.contains(&feature.as_str()) {
            return Some(format!("Unsupported feature: {}", feature));
        }
    }

    // Check for unsupported flags
    for flag in &meta.flags {
        if SKIP_FLAGS.contains(&flag.as_str()) {
            return Some(format!("Unsupported flag: {}", flag));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_skip_promise() {
        let meta = Test262Metadata {
            features: vec!["Promise".to_string()],
            ..Default::default()
        };
        assert!(should_skip(&meta).is_some());
    }

    #[test]
    fn test_should_not_skip_basic() {
        let meta = Test262Metadata::default();
        assert!(should_skip(&meta).is_none());
    }

    #[test]
    fn test_optional_catch_binding_not_skipped() {
        // optional-catch-binding is implemented - should NOT be skipped
        let meta = Test262Metadata {
            features: vec!["optional-catch-binding".to_string()],
            ..Default::default()
        };
        assert!(should_skip(&meta).is_none());
    }
}
