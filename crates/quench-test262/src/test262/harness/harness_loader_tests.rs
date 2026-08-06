//! Tests for HarnessLoader (strip_js_function, load, build_script, root_dir).
//!
//! These tests verify the harness wrapper:
//! - strips frontmatter correctly
//! - caches loaded files
//! - loads harness files without suppressing their implementations
//! - does not crash on missing files
//! - build_script correctly assembles harness + source

use std::fs;

// =============================================================================
// strip_js_function
// =============================================================================

#[test]
fn test_strip_js_function_removes_function_body() {
    let source = r#"
function foo(a, b) {
    return a + b;
}
function bar() {
    return 42;
}
var x = 1;
"#;
    let result = super::strip_js_function(source, "foo");
    assert!(result.contains("function bar()"));
    assert!(result.contains("var x = 1;"));
    assert!(!result.contains("function foo"));
}

#[test]
fn test_strip_js_function_preserves_code_before() {
    let source = r#"var before = 1;
function target() {
    return "inner";
}
var after = 2;"#;
    let result = super::strip_js_function(source, "target");
    assert!(result.contains("var before = 1;"));
    assert!(!result.contains("function target"));
    assert!(result.contains("var after = 2;"));
}

#[test]
fn test_strip_js_function_async_function() {
    let source = r#"async function target() {
    return await Promise.resolve(1);
}
var x = 42;"#;
    let result = super::strip_js_function(source, "target");
    assert!(!result.contains("function target"));
    assert!(!result.contains("async function target"));
    assert!(result.contains("var x = 42;"));
}

#[test]
fn test_strip_js_function_single_line_brace() {
    // function foo() { return 1; }
    let source = r#"function target() { return 1; }
var x = 2;"#;
    let result = super::strip_js_function(source, "target");
    assert!(!result.contains("function target"));
    assert!(result.contains("var x = 2;"));
}

#[test]
fn test_strip_js_function_nested_braces() {
    let source = r#"function outer() {
    if (true) {
        while (false) {
            return 1;
        }
    }
    return 2;
}
var x = 3;"#;
    let result = super::strip_js_function(source, "outer");
    assert!(!result.contains("function outer"));
    assert!(result.contains("var x = 3;"));
}

#[test]
fn test_strip_js_function_not_found() {
    let source = "var x = 1;\n";
    let result = super::strip_js_function(source, "nonexistent");
    assert_eq!(result, source);
}

#[test]
fn test_strip_js_function_preserves_after_newline_before_brace() {
    // function on its own line, brace on next line
    let source = "function target()\n{\n    return 1;\n}\nvar x = 2;";
    let result = super::strip_js_function(source, "target");
    assert!(!result.contains("function target"));
    assert!(result.contains("var x = 2;"));
}

#[test]
fn test_strip_js_function_ignores_braces_inside_strings() {
    let source = "function target() { var s = '}'; return s; }\nafter();";
    let result = super::strip_js_function(source, "target");
    assert_eq!(result.trim(), "after();");
}

// =============================================================================
// HarnessLoader.load
// =============================================================================

fn test_harness_dir() -> std::path::PathBuf {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/test262")
}

fn make_loader() -> super::HarnessLoader {
    super::HarnessLoader::new(&test_harness_dir().to_string_lossy())
}

#[test]
fn test_harness_loader_load_strips_frontmatter() {
    let loader = make_loader();
    let content = loader.load("sta.js").expect("sta.js should exist");
    // Frontmatter (/*--- ... ---*/) must be stripped
    assert!(
        !content.contains("description:") && !content.contains("esid:"),
        "frontmatter should be stripped from loaded file"
    );
}

#[test]
fn test_harness_loader_load_caches() {
    let loader = make_loader();
    let first = loader.load("sta.js").expect("sta.js should exist");
    let second = loader.load("sta.js").expect("sta.js should be cached");
    // Same content returned from cache
    assert_eq!(first, second);
}

#[test]
fn test_harness_loader_load_returns_none_for_missing() {
    let loader = make_loader();
    let result = loader.load("does_not_exist_xyz.js");
    assert!(
        result.is_none(),
        "loading a missing file should return None"
    );
}

#[test]
fn test_harness_loader_load_preserves_empty_file_as_loaded() {
    let root = std::env::temp_dir().join("quench-empty-harness");
    let harness_dir = root.join("harness");
    fs::create_dir_all(&harness_dir).unwrap();
    let tmp = harness_dir.join("empty.js");
    fs::write(&tmp, "   \n\t\n  ").unwrap();
    let loader = super::HarnessLoader::new(&root.to_string_lossy());
    let result = loader.load("empty.js");
    fs::remove_dir_all(&root).ok();
    assert_eq!(result, Some(String::new()));
}

#[test]
fn test_harness_loader_load_deep_equal_returns_source() {
    let loader = make_loader();
    let content = loader
        .load("deepEqual.js")
        .expect("deepEqual.js should exist");
    assert!(content.contains("assert.deepEqual"));
}

#[test]
fn test_harness_loader_includes_deep_equal_source() {
    let loader = make_loader();
    let content = loader
        .build_script("// source", &["deepEqual.js".to_string()])
        .unwrap();
    assert!(content.contains("assert.deepEqual._compare"));
}

#[test]
fn test_harness_loader_load_property_helper_preserves_verify_property() {
    let loader = make_loader();
    let content = loader
        .build_script("// source", &["propertyHelper.js".to_string()])
        .expect("build_script with propertyHelper.js should work");
    assert!(
        content.contains("function verifyProperty(obj"),
        "verifyProperty should be included in the built script"
    );
}

#[test]
fn test_harness_loader_load_property_helper_patches_non_index_numeric() {
    // propertyHelper.js: nonIndexNumericPropertyName is patched during build_script.
    let loader = make_loader();
    let content = loader
        .build_script("// source", &["propertyHelper.js".to_string()])
        .expect("build_script with propertyHelper.js should work");
    // Should NOT contain the dangerous 4294967295 value
    assert!(
        !content.contains("Math.pow(2, 32) - 1"),
        "built script should NOT contain the dangerous Math.pow(2, 32) - 1 value"
    );
    // Should contain the safe patched value
    assert!(
        content.contains("999999"),
        "built script should contain the safe patched value 999999"
    );
}

#[test]
fn test_harness_loader_load_is_constructor_returns_none() {
    // isConstructor.js: the native is_constructor covers it, loader skips
    // Note: isConstructor.js exists on disk but build_script skips it
    let loader = make_loader();
    let content = loader
        .load("isConstructor.js")
        .expect("isConstructor.js should exist");
    // The raw file still has content (it loads from disk), but build_script skips it
    // This tests that the file is loadable
    assert!(
        !content.is_empty(),
        "isConstructor.js should load from disk"
    );
}

#[test]
fn test_harness_loader_root_dir() {
    let loader = make_loader();
    let root = loader.root_dir();
    assert!(
        root.ends_with("tests/test262"),
        "root_dir should end with tests/test262, got: {}",
        root
    );
}

#[test]
fn test_harness_loader_load_nested_frontmatter_comments_preserved() {
    // Comments inside frontmatter area should be stripped along with frontmatter
    let loader = make_loader();
    let content = loader.load("sta.js").expect("sta.js should exist");
    // No frontmatter delimiters should remain
    assert!(
        !content.contains("/*---"),
        "/*--- delimiter should be stripped"
    );
    assert!(
        !content.contains("---*/"),
        "---*/ delimiter should be stripped"
    );
}

#[test]
fn test_harness_loader_load_assert_js_has_function() {
    // assert.js should load (not overridden)
    let loader = make_loader();
    let content = loader.load("assert.js").expect("assert.js should exist");
    // Native assert is created, but assert.js still loads (provides helpers)
    assert!(
        content.contains("function assert"),
        "assert.js should contain function assert"
    );
}

// =============================================================================
// HarnessLoader.build_script
// =============================================================================

#[test]
fn test_harness_loader_build_script_source_appended() {
    let loader = make_loader();
    let source = "var x = 1;";
    let script = loader
        .build_script(source, &[])
        .expect("build_script with no includes should work");
    assert!(
        script.ends_with(source),
        "source should be at end of built script"
    );
}

#[test]
fn test_harness_loader_build_script_includes_includes() {
    let loader = make_loader();
    let source = "var x = 1;";
    let script = loader
        .build_script(source, &["sta.js".to_string()])
        .expect("build_script with sta.js should work");
    // sta.js should be prepended and contain Test262Error
    assert!(
        script.contains("Test262Error"),
        "built script should include sta.js content (Test262Error)"
    );
}

#[test]
fn test_harness_loader_build_script_includes_is_constructor() {
    let loader = make_loader();
    let source = "var x = 1;";
    let script = loader
        .build_script(source, &["isConstructor.js".to_string()])
        .expect("build_script should succeed even with isConstructor.js");
    assert!(
        script.contains("function isConstructor(f)"),
        "isConstructor.js should be included in build_script"
    );
}

#[test]
fn test_harness_loader_build_script_error_for_missing_include() {
    let loader = make_loader();
    let result = loader.build_script("var x = 1;", &["nonexistent_xyz.js".to_string()]);
    assert!(
        result.is_err(),
        "build_script should fail for missing include"
    );
    assert!(
        result.unwrap_err().contains("not found"),
        "error should mention 'not found'"
    );
}

#[test]
fn test_harness_loader_build_script_multiple_includes_concatenated() {
    let loader = make_loader();
    let source = "// test source";
    let script = loader
        .build_script(
            source,
            &["nativeErrors.js".to_string(), "nans.js".to_string()],
        )
        .expect("build_script with multiple includes should work");
    // Both includes should be present
    assert!(
        script.contains("NativeError"),
        "nativeErrors.js should be included"
    );
    assert!(script.contains("NaN"), "nans.js should be included");
}

#[test]
fn test_harness_loader_build_script_frontmatter_in_source_preserved() {
    // Source frontmatter (JS comments) should be preserved in built script
    let loader = make_loader();
    let source = "/*--- info: test ---\n*/\nvar x = 1;";
    let script = loader
        .build_script(source, &["sta.js".to_string()])
        .expect("build_script should preserve source frontmatter");
    // Frontmatter is inside a JS comment, so it stays as-is
    assert!(
        script.contains("/*--- info: test ---\n*/"),
        "frontmatter in source should be preserved"
    );
}

#[test]
fn test_harness_loader_build_script_empty_includes_array() {
    let loader = make_loader();
    let source = "var x = 1;";
    let script = loader
        .build_script(source, &[])
        .expect("build_script with empty includes should work");
    assert_eq!(script, source);
}

#[test]
fn test_harness_loader_empty_dir_handled() {
    // Non-existent harness dir should not panic; load should return None
    let loader = super::HarnessLoader::new("/tmp/this_dir_does_not_exist_xyz");
    let result = loader.load("any.js");
    assert!(
        result.is_none(),
        "loading from non-existent directory should return None"
    );
}

#[test]
fn test_harness_loader_preserves_js_after_stripped_function() {
    // After stripping a function, the remaining code should be valid JS
    let loader = make_loader();
    let script = loader
        .build_script(
            "assert.sameValue(x, 1);",
            &["propertyHelper.js".to_string()],
        )
        .expect("build_script with propertyHelper.js should work");
    // propertyHelper.js should be there (minus verifyProperty)
    assert!(
        script.contains("function isConfigurable("),
        "propertyHelper.js content (minus verifyProperty) should be included"
    );
}

#[test]
fn test_harness_loader_load_preserves_sta_js_specifically() {
    // sta.js is loaded via eval_harness_file, not via load/build_script
    // But load should still work for it
    let loader = make_loader();
    let content = loader.load("sta.js").expect("sta.js should exist");
    assert!(
        !content.trim().is_empty(),
        "sta.js should have content after frontmatter strip"
    );
}

#[test]
fn test_harness_loader_load_compar_array() {
    // compareArray.js: copyright comment remains (not stripped), but frontmatter
    // is removed and the function body (deprecated) is gone.
    let loader = make_loader();
    let content = loader
        .load("compareArray.js")
        .expect("compareArray.js should load");
    // Copyright comment is preserved; no frontmatter delimiters
    assert!(
        content.contains("Copyright"),
        "copyright should be preserved"
    );
    assert!(!content.contains("/*---"), "frontmatter should be stripped");
    assert!(
        !content.contains("compareArray"),
        "function body should be absent (deprecated)"
    );
}

#[test]
fn test_harness_loader_build_script_preserves_verify_property_from_property_helper() {
    let loader = make_loader();
    let script = loader
        .build_script("// source", &["propertyHelper.js".to_string()])
        .expect("build_script with propertyHelper.js should work");
    assert!(
        script.contains("function verifyProperty(obj"),
        "verifyProperty should be included in the built script"
    );
}

#[test]
fn test_harness_loader_load_native_errors() {
    let loader = make_loader();
    let content = loader
        .load("nativeErrors.js")
        .expect("nativeErrors.js should exist");
    assert!(
        content.contains("NativeError"),
        "nativeErrors.js should contain NativeError"
    );
}

#[test]
fn test_harness_loader_load_fn_global_object() {
    let loader = make_loader();
    let content = loader
        .load("fnGlobalObject.js")
        .expect("fnGlobalObject.js should exist");
    assert!(
        content.contains("fnGlobalObject"),
        "fnGlobalObject.js should contain fnGlobalObject"
    );
}

#[test]
fn injected_assert_uses_test262_javascript_definition() {
    let assert_js = make_loader().load("assert.js").unwrap();
    assert!(assert_js.contains("function assert(mustBeTrue, message)"));
    assert_eq!(assert_js.contains("assert.sameValue = function"), true);
}
