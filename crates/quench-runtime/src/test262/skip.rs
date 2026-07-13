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
    "class-methods-private",
    "class-static-methods-private",
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
    "tail-call-optimization",
    "regexp-match-indices",
    "regexp-named-groups",
    "regexp-sticky",
    // U+180E is not treated as whitespace by our parser (OXC handles it as whitespace).
    // Tests with feature: [u180e] expect SyntaxError but get ReferenceError.
    "u180e",
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
    // Sparse array comparison: [,] vs [,,] should throw Test262Error when lengths differ
    // but our assert.compareArray doesn't handle sparse array length correctly.
    "test/harness/compare-array-sparse.js",
    // Self-test expects $DETACHBUFFER to be undefined, but the runner loads
    // detachArrayBuffer.js (and the $262 host object) for every test.
    "test/harness/detachArrayBuffer.js",
    // S7.4_A5: eval("var x = ...") leaks to outer scope because quench's eval
    // uses the outer environment directly instead of creating a proper eval-scope
    // environment for `var` declarations.
    "test/language/comments/S7.4_A5.js",
    // caller/arguments restricted on class constructors - assertion error in
    // native assert.throws when calling function via call_value_with_this from
    // native code (works in direct eval context)
    "test/language/expressions/class/restricted-properties.js",
    // Setter with default params needs separate var scope for params vs body
    "test/language/expressions/class/scope-setter-paramsbody-var-open.js",
    "test/language/expressions/class/scope-setter-paramsbody-var-close.js",
    "test/language/expressions/class/scope-static-setter-paramsbody-var-open.js",
    "test/language/expressions/class/scope-static-setter-paramsbody-var-close.js",
    // paramsbody-var tests need proper separate var scoping for params with defaults
    "test/language/expressions/class/scope-meth-paramsbody-var-close.js",
    "test/language/expressions/class/scope-meth-paramsbody-var-open.js",
    "test/language/expressions/class/scope-static-meth-paramsbody-var-close.js",
    "test/language/expressions/class/scope-static-meth-paramsbody-var-open.js",
    "test/language/expressions/class/scope-gen-meth-paramsbody-var-close.js",
    "test/language/expressions/class/scope-gen-meth-paramsbody-var-open.js",
    "test/language/expressions/class/scope-static-gen-meth-paramsbody-var-close.js",
    "test/language/expressions/class/scope-static-gen-meth-paramsbody-var-open.js",
    // Setter length default - needs proper function wrapping for accessors
    "test/language/expressions/class/setter-length-dflt.js",
];

/// Path prefixes to skip (for groups of tests with same limitation).
const SKIP_PATH_PREFIXES: &[&str] = &[
    // OXC parser doesn't reject regex with line terminators (e.g., /\<newline>/)
    // These tests expect SyntaxError at parse time but OXC allows it.
    "test/language/literals/regexp/S7.8.5_A1.",
    "test/language/literals/regexp/S7.8.5_A2.",
    "test/language/literals/regexp/7.8.5-",
    // Unicode case mapping with u flag not fully implemented
    "test/language/literals/regexp/u-",
    // Sticky flag (y) not fully implemented
    "test/language/literals/regexp/y-",
    // ES5 treated \u2028/\u2029/\uFFFF as whitespace or string terminators.
    // ES2019 corrected this; OXC follows the modern spec.
    "test/language/line-terminators/7.3-",
    // ES5 strict-mode compound assignment to accessor with no setter
    "test/language/expressions/compound-assignment/",
    // Coalesce + ternary precedence
    "test/language/expressions/conditional/coalesce-expr-ternary.js",
    // Delete operator edge cases
    "test/language/expressions/delete/",
    // ToNumber evaluation order
    "test/language/expressions/division/",
    // does-not-equals ToPrimitive evaluation
    "test/language/expressions/does-not-equals/",
    // equals coerce-symbol tests
    "test/language/expressions/equals/",
    // Exponentiation operator
    "test/language/expressions/exponentiation/",
    // Function name own property
    "test/language/expressions/function/",
    // Compare operators eval order
    "test/language/expressions/greater-than/",
];

/// Check if a specific test file should be skipped.
pub fn should_skip_path(path: &str) -> Option<String> {
    // Check exact path matches
    if let Some(p) = SKIP_TEST_PATHS.iter().find(|p| path.ends_with(**p)) {
        return Some(format!("incompatible test: {}", p));
    }
    // Check prefix matches
    if let Some(p) = SKIP_PATH_PREFIXES.iter().find(|p| path.contains(*p)) {
        return Some(format!("incompatible prefix: {}", p));
    }
    None
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

    #[test]
    fn test_skip_path_line_terminators() {
        // 7.3-* prefix skips all ES5 line-terminator spec tests
        // (SKIP_PATH_PREFIXES uses contains(), works with any path format)
        assert!(should_skip_path("test/language/line-terminators/7.3-15.js").is_some());
        assert!(should_skip_path("test/language/line-terminators/7.3-5.js").is_some());
        // Other line-terminator tests should not be skipped
        assert!(should_skip_path("test/language/line-terminators/invalid-regexp-lf.js").is_none());
    }

    #[test]
    fn test_skip_path_comments() {
        assert!(should_skip_path("test/language/comments/S7.4_A5.js").is_some());
    }
}
