//! Trait boundary between the test262 runner and the engine under test.

use crate::test262::harness::try_inject_harness;
use crate::test262::skip::is_feature_supported;
use crate::Context;

/// Implement this for your engine to plug it into the test262 runner.
pub trait Test262Host {
    /// Execute a complete JS script (harness + test source).
    /// `Ok(())` if execution completes without throwing,
    /// `Err(message)` if it throws or fails to evaluate.
    fn run_script(&mut self, source: &str) -> Result<(), String>;

    /// Whether a test262 feature (frontmatter `features:` entry) is
    /// implemented. Returning false skips tests that require it.
    fn has_feature(&self, feature: &str) -> bool;
}

/// What happened when we tried to run a test.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum TestOutcome {
    Pass,
    Fail { reason: String },
    Skip { reason: String },
}

impl std::fmt::Display for TestOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestOutcome::Pass => write!(f, "PASS"),
            TestOutcome::Fail { reason } => write!(f, "FAIL: {}", reason),
            TestOutcome::Skip { reason } => write!(f, "SKIP: {}", reason),
        }
    }
}

/// Host backed by quench: fresh `Context` per script with builtins and harness injected.
pub struct QuenchHost;

impl QuenchHost {
    pub fn new() -> Self {
        QuenchHost
    }
}

impl Default for QuenchHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Test262Host for QuenchHost {
    fn run_script(&mut self, source: &str) -> Result<(), String> {
        let mut ctx = Context::new().map_err(|e| format!("{:?}", e))?;
        crate::builtins::register_builtins(&mut ctx);
        // Save strict mode before harness loading; restore before test eval so that
        // any strictness inadvertently set during harness eval (e.g. by a harness file
        // containing "use strict") cannot leak into the test itself.
        let prev_strict_before_harness = crate::interpreter::is_strict_mode();
        try_inject_harness(&mut ctx).map_err(|e| format!("harness load failure: {}", e))?;
        crate::interpreter::set_strict_mode(prev_strict_before_harness);
        // Force sloppy mode before test eval. The test source itself (if it begins with
        // "use strict") will set strict mode correctly via check_use_strict_directive.
        // This prevents a previous test's strict-mode run from leaking into the next
        // test's sloppy-mode run.
        crate::interpreter::set_strict_mode(false);
        ctx.eval(source).map(|_| ()).map_err(|e| format!("{:?}", e))
    }

    fn has_feature(&self, feature: &str) -> bool {
        is_feature_supported(feature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quench_host_runs_and_throws() {
        let mut host = QuenchHost::new();
        assert!(host.run_script("var x = 1 + 1;").is_ok());
        assert!(host.run_script("throw new Error('boom')").is_err());
    }

    #[test]
    fn repro_length_dflt() {
        let mut host = QuenchHost::new();
        let src = r#"
var f1 = (x = 42) => {};
var desc = Object.getOwnPropertyDescriptor(f1, 'length');
if (!desc) throw new Test262Error('desc undefined');
if (desc.value !== 0) throw new Test262Error('value: ' + desc.value);
if (desc.writable !== false) throw new Test262Error('writable: ' + desc.writable);
if (desc.enumerable !== false) throw new Test262Error('enumerable: ' + desc.enumerable);
if (desc.configurable !== true) throw new Test262Error('configurable: ' + desc.configurable);
"#;
        let r = host.run_script(src);
        assert!(r.is_ok(), "got: {:?}", r);
    }

    #[test]
    fn repro_length_dflt_verifyproperty() {
        let mut host = QuenchHost::new();
        // Simulate verifyProperty's isConfigurable check via delete then check.
        let src = r#"
var f1 = (x = 42) => {};
var origDesc = Object.getOwnPropertyDescriptor(f1, 'length');
delete f1.length;
var stillHas = Object.prototype.hasOwnProperty.call(f1, 'length');
if (origDesc.configurable !== true) throw new Test262Error('origConfigurable=' + origDesc.configurable);
if (stillHas) throw new Test262Error('still has length after delete');
if (f1.length !== 0) throw new Test262Error('length should be 0 even after delete');
"#;
        let r = host.run_script(src);
        assert!(r.is_ok(), "got: {:?}", r);
    }

    #[test]
    fn repro_length_dflt_via_harness() {
        let mut host = QuenchHost::new();
        let harness_src = r#"
var verifyProperty = function(obj, name, desc) {
  var originalDesc = Object.getOwnPropertyDescriptor(obj, name);
  if (!Object.prototype.hasOwnProperty.call(obj, name)) {
    throw new Error('should be own');
  }
  try { delete obj[name]; } catch (e) {}
  var stillHas = Object.prototype.hasOwnProperty.call(obj, name);
  var failures = [];
  if (desc.configurable !== undefined && desc.configurable !== stillHas) {
    failures.push('configurable: expected ' + desc.configurable + ', actual ' + stillHas);
  }
  if (failures.length) throw new Error(failures.join('; '));
  return true;
};
var f1 = (x = 42) => {};
verifyProperty(f1, 'length', { configurable: true });
"#;
        let r = host.run_script(harness_src);
        assert!(r.is_ok(), "got: {:?}", r);
    }

    #[test]
    fn repro_length_dflt_actual_test_source() {
        let mut host = QuenchHost::new();
        let test_source = r#"
var f1 = (x = 42) => {};

verifyProperty(f1, "length", {
  value: 0,
  writable: false,
  enumerable: false,
  configurable: true,
});

var f2 = (x = 42, y) => {};

verifyProperty(f2, "length", {
  value: 0,
  writable: false,
  enumerable: false,
  configurable: true,
});
"#;
        let r = host.run_script(test_source);
        assert!(r.is_ok(), "got: {:?}", r);
    }

    #[test]
    fn debug_arrow_length_own() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval(
            "var f1 = (x = 42) => {}; \
             Object.prototype.hasOwnProperty.call(f1, 'length');"
        );
        eprintln!("hasOwn length: {:?}", r);
    }

    #[test]
    fn debug_arrow_length_delete() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval(
            "var f1 = (x = 42) => {}; \
             var d = delete f1.length; \
             var stillHas = Object.prototype.hasOwnProperty.call(f1, 'length'); \
             [d, stillHas, f1.length];"
        ).unwrap();
        if let crate::value::Value::Object(o) = r {
            let e = o.borrow().elements.clone();
            eprintln!("delete results: d={:?}, stillHas={:?}, len={:?}", e[0], e[1], e[2]);
        }
    }

    #[test]
    fn tmp_strict_fn_eval() {
        let mut host = QuenchHost::new();
        let r = host.run_script(
            r#"
var s = "none";
(function() {
  'use strict';
  try { undefinedGlobalXYZ = 1; s = "sloppy"; }
  catch (e) { s = "strict:" + (e instanceof ReferenceError); }
})();
if (s !== "strict:true") throw new Error("strict not active: " + s);
"#,
        );
        assert!(r.is_ok(), "got: {:?}", r);
    }

    #[test]
    fn test_new_math_error_type() {
        let mut host = QuenchHost::new();
        let result = host.run_script("new Math");
        println!("new Math result: {:?}", result);
        assert!(result.is_err(), "new Math should throw");
    }

    #[test]
    fn test_assert_throws_new_math_inside_function() {
        // Reproducer for test262 S8.6.2_A7.js:
        // assert.throws(TypeError, function() { new Math; })
        // The test262 runner says "Expected TypeError but got ReferenceError"
        // This means Math is not found inside the assert.throws callback.
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
assert.throws(TypeError, function() {
  var objMath = new Math;
});
"#,
        );
        assert!(
            result.is_ok(),
            "assert.throws(TypeError, function() {{ new Math; }}) failed: {:?}",
            result
        );
    }

    #[test]
    fn test_math_accessible_inside_callback() {
        // Test that Math is accessible inside a function called from assert.throws
        let mut host = QuenchHost::new();
        // Direct test: Math.abs should work inside function
        let result = host.run_script(
            r#"
assert.sameValue(Math.abs(-5), 5);
"#,
        );
        assert!(result.is_ok(), "Math.abs failed: {:?}", result);

        // Inside assert.throws callback
        let result2 = host.run_script(
            r#"
assert.throws(TypeError, function() {
  // Math should be accessible here
  if (typeof Math === 'undefined') {
    throw new ReferenceError('Math is not defined');
  }
  new Math;
});
"#,
        );
        assert!(
            result2.is_ok(),
            "Math inside callback failed: {:?}",
            result2
        );
    }

    #[test]
    fn assert_throws_typeerror_intrinsic() {
        // Reproducer: assert.throws(TypeError, fn) with the intrinsic TypeError.
        // The harness wraps assert.throws as a NativeFunction, which calls the
        // test function directly. If the thrown object's .name is empty, the
        // name comparison fails.
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
assert.throws(TypeError, function() {
  throw new TypeError();
}, 'Throws TypeError');
"#,
        );
        assert!(
            result.is_ok(),
            "assert.throws(TypeError) failed: {:?}",
            result
        );
    }

    #[test]
    fn assert_throws_custom_typeerror_name_collision() {
        // With instanceof-style matching: a local TypeError is NOT an instance
        // of the global TypeError, even if they share the same .name.
        // The first assert.throws passes (local err instanceof local TypeError).
        // The second assert.throws must use the local TypeError to match.
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var intrinsicTypeError = TypeError;
(function() {
  function TypeError() {}
  // Throws local TypeError — matches local TypeError, not global
  assert.throws(TypeError, function() {
    throw new TypeError();
  }, 'Throws an instance of the matching custom TypeError');
  // Throws global TypeError — matches global TypeError (instanceof)
  assert.throws(intrinsicTypeError, function() {
    throw new intrinsicTypeError();
  }, 'Global TypeError matches global TypeError');
})();
"#,
        );
        assert!(result.is_ok(), "name collision test failed: {:?}", result);
    }

    #[test]
    fn custom_function_constructor_name() {
        // Test that a function declaration has its .name property set
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"function TypeError() {}
var n = TypeError.name;
if (n !== "TypeError") throw new Error("expected TypeError, got: " + n)
"#,
        );
        assert!(result.is_ok(), "function .name test failed: {:?}", result);
    }

    #[test]
    fn custom_error_instance_name() {
        // Test that new CustomError() has .name = "CustomError"
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"function CustomError() {}
var e = new CustomError();
var n = e.name;
if (n !== "CustomError") throw new Error("expected CustomError, got: " + n)
"#,
        );
        assert!(
            result.is_ok(),
            "custom error name test failed: {:?}",
            result
        );
    }

    #[test]
    fn thrown_error_has_name_property() {
        // Verify that `new TypeError()` actually sets .name on the object.
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"var n = new TypeError().name; if (n !== "TypeError") throw new Error("got: " + n)"#,
        );
        assert!(result.is_ok(), "new TypeError().name failed: {:?}", result);
    }

    #[test]
    fn constructor_property_chain_for_errors() {
        // Verify that local TypeError instances have constructor === local TypeError,
        // NOT the global TypeError. This is how assert.throws distinguishes custom
        // errors from built-in errors even when they share the same name.
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var intrinsicTypeError = TypeError;
(function() {
  function TypeError() {}
  var localErr = new TypeError();
  // localErr.constructor should be the local TypeError function, not the global one
  if (localErr.constructor !== TypeError) {
    throw new Error("localErr.constructor !== local TypeError");
  }
  // localErr.constructor should NOT be the global TypeError
  if (localErr.constructor === intrinsicTypeError) {
    throw new Error("localErr.constructor === global TypeError (should not)");
  }
  // global TypeError instance should have constructor === global
  var globalErr = new intrinsicTypeError();
  if (globalErr.constructor !== intrinsicTypeError) {
    throw new Error("globalErr.constructor !== global TypeError");
  }
})();
"#,
        );
        assert!(
            result.is_ok(),
            "constructor chain test failed: {:?}",
            result
        );
    }

    #[test]
    fn harness_exact_assert_throws_custom_typeerror() {
        // Reproduce the exact harness file logic step by step
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var intrinsicTypeError = TypeError;
var threw = false;

(function() {
  function TypeError() {}

  assert.throws(TypeError, function() {
    throw new TypeError();
  }, 'Throws an instance of the matching custom TypeError');

  try {
    assert.throws(intrinsicTypeError, function() {
      throw new TypeError();
    });
  } catch (err) {
    threw = true;
    if (err.constructor !== Test262Error) {
      throw new Error(
        'Expected a Test262Error but a "' + err.constructor.name +
        '" was thrown.'
      );
    }
  }

  if (threw === false) {
    throw new Error('Expected a Test262Error, but no error was thrown.');
  }
})();
"#,
        );
        assert!(result.is_ok(), "harness exact test failed: {:?}", result);
    }

    #[test]
    fn assert_deep_equal_callable() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
assert.deepEqual(null, null);
assert.deepEqual(undefined, undefined);
assert.deepEqual("a", "a");
assert.deepEqual(1, 1);
assert.deepEqual(true, true);
"#,
        );
        assert!(
            result.is_ok(),
            "assert.deepEqual basic calls failed: {:?}",
            result
        );
    }

    #[test]
    fn assert_deep_equal_with_symbol() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var s1 = Symbol();
assert.deepEqual(s1, s1);
"#,
        );
        assert!(
            result.is_ok(),
            "assert.deepEqual with Symbol failed: {:?}",
            result
        );
    }

    #[test]
    fn harness_deep_equal_primitives_via_loader() {
        use crate::test262::harness::HarnessLoader;
        use crate::test262::metadata::Test262Metadata;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");
        let test_path = repo_root.join("tests/test262/test/harness/deepEqual-primitives.js");

        let source = std::fs::read_to_string(&test_path).unwrap();
        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
        let meta = Test262Metadata::parse(&source).unwrap();
        let script = harness.build_script(&source, &meta.includes).unwrap();

        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        assert!(
            result.is_ok(),
            "deepEqual-primitives via loader failed: {:?}",
            result
        );
    }

    #[test]
    fn deep_equal_js_eval_failure_leaves_native() {
        // Check what assert.deepEqual is after deepEqual.js fails to load
        let mut host = QuenchHost::new();
        let result = host.run_script("typeof assert.deepEqual;");
        // Should be "function" (native stub) not "undefined"
        assert!(
            result.is_ok(),
            "typeof assert.deepEqual failed: {:?}",
            result
        );
    }

    #[test]
    fn deep_equal_js_loads_via_harness_loader() {
        use crate::test262::harness::HarnessLoader;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");

        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
        assert!(
            harness.load("deepEqual.js").is_some(),
            "deepEqual.js should load"
        );

        let mut host = QuenchHost::new();
        let result = host.run_script("assert.deepEqual;");
        assert!(
            result.is_ok(),
            "assert.deepEqual should exist: {:?}",
            result
        );
    }

    #[test]
    fn deep_equal_js_plus_assert_js_in_script() {
        use crate::test262::harness::HarnessLoader;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");

        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
        let script = harness
            .build_script(
                r#"
var s1 = Symbol();
assert.deepEqual(s1, s1);
"#,
                &["deepEqual.js".to_string()],
            )
            .unwrap();

        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        assert!(
            result.is_ok(),
            "assert.js + deepEqual.js + test failed: {:?}",
            result
        );
    }

    #[test]
    fn deep_equal_object_symbol_vs_symbol() {
        // deepEqual-primitives.js: Object(s1) vs s1
        use crate::test262::harness::HarnessLoader;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");

        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
        let script = harness
            .build_script(
                r#"
var s1 = Symbol();
assert.deepEqual(Object(s1), s1);
"#,
                &["deepEqual.js".to_string()],
            )
            .unwrap();

        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        assert!(
            result.is_ok(),
            "Object(symbol) vs symbol failed: {:?}",
            result
        );
    }

    #[test]
    fn deep_equal_symbol_vs_string() {
        // deepEqual-primitives.js: assert.deepEqual(s1, "Symbol()") should throw
        use crate::test262::harness::HarnessLoader;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");

        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
        let script = harness
            .build_script(
                r#"
var s1 = Symbol();
assert.throws(Test262Error, function() { assert.deepEqual(s1, "Symbol()"); });
"#,
                &["deepEqual.js".to_string()],
            )
            .unwrap();

        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        assert!(
            result.is_ok(),
            "symbol vs string should throw: {:?}",
            result
        );
    }

    #[test]
    fn deep_equal_js_plus_assert_js_full() {
        // The exact full build_script output that deepEqual-primitives.js uses
        use crate::test262::harness::HarnessLoader;
        use crate::test262::metadata::Test262Metadata;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");
        let test_path = repo_root.join("tests/test262/test/harness/deepEqual-primitives.js");

        let source = std::fs::read_to_string(&test_path).unwrap();
        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
        let meta = Test262Metadata::parse(&source).unwrap();

        // Test each include individually
        for inc in &meta.includes {
            let script = harness
                .build_script("assert.deepEqual;", &[inc.clone()])
                .unwrap();
            let mut host = QuenchHost::new();
            let r = host.run_script(&script);
            assert!(r.is_ok(), "include {} failed: {:?}", inc, r);
        }

        let full_script = harness.build_script(&source, &meta.includes).unwrap();
        let mut host = QuenchHost::new();
        let result = host.run_script(&full_script);
        assert!(result.is_ok(), "full script failed: {:?}", result);
    }

    #[test]
    fn deep_equal_bisect_groups() {
        // Bisect which group of assertions fails
        use crate::test262::harness::HarnessLoader;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");
        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());

        // Group 1: lines 1-10 (basic primitives + symbol self-equal)
        let g1 = r#"
var s1 = Symbol();
var s2 = Symbol();
assert.deepEqual(null, null);
assert.deepEqual(undefined, undefined);
assert.deepEqual("a", "a");
assert.deepEqual(1, 1);
assert.deepEqual(true, true);
assert.deepEqual(s1, s1);
assert.deepEqual(Object("a"), "a");
assert.deepEqual(Object(1), 1);
assert.deepEqual(Object(true), true);
assert.deepEqual(Object(s1), s1);
"#;
        let script1 = harness
            .build_script(g1, &["deepEqual.js".to_string()])
            .unwrap();
        let mut host = QuenchHost::new();
        let r1 = host.run_script(&script1);
        assert!(r1.is_ok(), "Group 1 failed: {:?}", r1);

        // Group 2: lines 26-30 (throws: null/0, undefined/0, "", 1/1, "1"/"2")
        let g2 = r#"
var s1 = Symbol();
assert.throws(Test262Error, function () { assert.deepEqual(null, 0); });
assert.throws(Test262Error, function () { assert.deepEqual(undefined, 0); });
assert.throws(Test262Error, function () { assert.deepEqual("", 0); });
assert.throws(Test262Error, function () { assert.deepEqual("1", 1); });
assert.throws(Test262Error, function () { assert.deepEqual("1", "2"); });
"#;
        let script2 = harness
            .build_script(g2, &["deepEqual.js".to_string()])
            .unwrap();
        let mut host = QuenchHost::new();
        let r2 = host.run_script(&script2);
        assert!(r2.is_ok(), "Group 2 failed: {:?}", r2);

        // Group 3: lines 31-34 (throws: true/1, true/false, s1/"Symbol()", s1/s2)
        let g3 = r#"
var s1 = Symbol();
var s2 = Symbol();
assert.throws(Test262Error, function () { assert.deepEqual(true, 1); });
assert.throws(Test262Error, function () { assert.deepEqual(true, false); });
assert.throws(Test262Error, function () { assert.deepEqual(s1, "Symbol()"); });
assert.throws(Test262Error, function () { assert.deepEqual(s1, s2); });
"#;
        let script3 = harness
            .build_script(g3, &["deepEqual.js".to_string()])
            .unwrap();
        let mut host = QuenchHost::new();
        let r3 = host.run_script(&script3);
        assert!(r3.is_ok(), "Group 3 failed: {:?}", r3);

        // Groups 1+2 combined
        let combined = format!("{}{}", g1.trim(), &g2[1..]);
        let script_comb = harness
            .build_script(&combined, &["deepEqual.js".to_string()])
            .unwrap();
        let mut host = QuenchHost::new();
        let r_comb = host.run_script(&script_comb);
        assert!(r_comb.is_ok(), "Groups 1+2 failed: {:?}", r_comb);

        // Groups 1+3 combined
        let combined13 = format!("{}{}", g1.trim(), &g3[1..]);
        let script13 = harness
            .build_script(&combined13, &["deepEqual.js".to_string()])
            .unwrap();
        let mut host = QuenchHost::new();
        let r13 = host.run_script(&script13);
        assert!(r13.is_ok(), "Groups 1+3 failed: {:?}", r13);
    }

    #[test]
    fn deep_equal_bisect_line_by_line() {
        // Test each assertion individually through the harness
        use crate::test262::harness::HarnessLoader;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");
        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());

        let cases: &[(&str, &str)] = &[
            ("null==null", r#"assert.deepEqual(null, null);"#),
            (
                "undefined==undefined",
                r#"assert.deepEqual(undefined, undefined);"#,
            ),
            (r#""a"=="a""#, r#"assert.deepEqual("a", "a");"#),
            ("1==1", r#"assert.deepEqual(1, 1);"#),
            ("true==true", r#"assert.deepEqual(true, true);"#),
            ("s1==s1", r#"var s1=Symbol();assert.deepEqual(s1,s1);"#),
            (
                r#"Object("a")=="a""#,
                r#"var s1=Symbol();assert.deepEqual(Object("a"),"a");"#,
            ),
            (
                "Object(1)==1",
                r#"var s1=Symbol();assert.deepEqual(Object(1),1);"#,
            ),
            (
                "Object(true)==true",
                r#"var s1=Symbol();assert.deepEqual(Object(true),true);"#,
            ),
            (
                "Object(s1)==s1",
                r#"var s1=Symbol();assert.deepEqual(Object(s1),s1);"#,
            ),
            (
                "null!=0 throws",
                r#"var s1=Symbol();assert.throws(Test262Error,function(){assert.deepEqual(null,0);});"#,
            ),
            (
                "undefined!=0 throws",
                r#"var s1=Symbol();assert.throws(Test262Error,function(){assert.deepEqual(undefined,0);});"#,
            ),
            (
                r#"""!=0 throws"#,
                r#"var s1=Symbol();assert.throws(Test262Error,function(){assert.deepEqual("",0);});"#,
            ),
            (
                r#""1"!=1 throws"#,
                r#"var s1=Symbol();assert.throws(Test262Error,function(){assert.deepEqual("1",1);});"#,
            ),
            (
                r#""1"!="2" throws"#,
                r#"var s1=Symbol();assert.throws(Test262Error,function(){assert.deepEqual("1","2");});"#,
            ),
            (
                "true!=1 throws",
                r#"var s1=Symbol();assert.throws(Test262Error,function(){assert.deepEqual(true,1);});"#,
            ),
            (
                "true!=false throws",
                r#"var s1=Symbol();assert.throws(Test262Error,function(){assert.deepEqual(true,false);});"#,
            ),
            (
                r#"s1!="Symbol()" throws"#,
                r#"var s1=Symbol();assert.throws(Test262Error,function(){assert.deepEqual(s1,"Symbol()");});"#,
            ),
            (
                "s1!=s2 throws",
                r#"var s1=Symbol();var s2=Symbol();assert.throws(Test262Error,function(){assert.deepEqual(s1,s2);});"#,
            ),
        ];

        for (name, code) in cases {
            let script = harness
                .build_script(code, &["deepEqual.js".to_string()])
                .unwrap();
            let mut host = QuenchHost::new();
            let result = host.run_script(&script);
            let status = if result.is_ok() { "PASS" } else { "FAIL" };
            println!("[{}] {}: {:?}", status, name, result);
            assert!(result.is_ok(), "FAILED {}: {:?}", name, result);
        }
    }

    #[test]
    fn strict_eval_legacy_octal_should_throw_syntax_error() {
        // Reproducer for test262 7.8.3-3gs.js: eval in strict mode with
        // legacy octal literals should throw SyntaxError, not ReferenceError.
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
"use strict";
var a;
assert.throws(SyntaxError, function() {
  eval("a = 0x1;a = 01;");
});
"#,
        );
        assert!(
            result.is_ok(),
            "strict eval with legacy octal should throw SyntaxError: {:?}",
            result
        );
    }

    #[test]
    fn assert_throws_with_plain_function() {
        // Test assert.throws with a plain JS function that throws SyntaxError
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
assert.throws(SyntaxError, function() {
  throw new SyntaxError("test");
});
"#,
        );
        assert!(
            result.is_ok(),
            "assert.throws with SyntaxError should work: {:?}",
            result
        );
    }

    #[test]
    fn assert_throws_with_plain_function_throwing_syntax_error() {
        // Test assert.throws with a plain JS function that throws SyntaxError
        let mut host = QuenchHost::new();
        // Note: no "use strict" prefix
        let result = host.run_script(
            r#"
assert.throws(ReferenceError, function() {
  console.log(x);
});
"#,
        );
        assert!(
            result.is_ok(),
            "assert.throws with ReferenceError should work: {:?}",
            result
        );
    }

    #[test]
    fn assert_throws_eval_syntax_error_directly() {
        // What if we catch the eval error directly?
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
"use strict";
var a;
try {
  eval("a = 0x1;a = 01;");
  throw new Error("Expected error");
} catch(e) {
  // e should be a SyntaxError
  if (e.constructor !== SyntaxError) {
    throw new Error("Expected SyntaxError but got " + e.constructor.name + ": " + e.message);
  }
}
"#,
        );
        assert!(
            result.is_ok(),
            "try/catch eval SyntaxError should work: {:?}",
            result
        );
    }

    #[test]
    fn eval_legacy_octal_sloppy() {
        // Sloppy mode should allow legacy octal
        let mut host = QuenchHost::new();
        let result = host.run_script("var x = 01;");
        assert!(
            result.is_ok(),
            "sloppy mode with legacy octal should work: {:?}",
            result
        );
    }

    #[test]
    fn eval_legacy_octal_simple() {
        // Minimal test: eval in strict mode throws on legacy octal?
        let mut host = QuenchHost::new();
        let result = host.run_script(r#""use strict"; eval("01;")"#);
        assert!(
            result.is_err(),
            "strict eval with legacy octal should error"
        );
    }

    #[test]
    fn eval_legacy_octal_catch() {
        // Can we catch it? Try WITHOUT native assert/throws
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
"use strict";
var caught = null;
try {
  eval("01;");
} catch(e) {
  caught = e;
}
if (caught === null) throw new Error("nothing caught");
if (caught.constructor !== SyntaxError) {
  throw new Error("wrong type: " + caught.constructor.name + " / " + caught.message);
}
"#,
        );
        assert!(result.is_ok(), "eval octal catch should work: {:?}", result);
    }

    #[test]
    fn syntax_error_direct() {
        // Minimal test: create SyntaxError and check constructor.name
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var e = new SyntaxError("test");
e.name;
console.log(e.name);
"#,
        );
        assert!(result.is_ok(), "SyntaxError direct failed: {:?}", result);
    }

    #[test]
    fn syntax_error_name_prop() {
        // Check SyntaxError constructor has name property
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"console.log(SyntaxError.name); SyntaxError.name"#);
        assert!(result.is_ok(), "SyntaxError.name failed: {:?}", result);
    }

    #[test]
    fn eval_no_harness() {
        // Test eval WITHOUT the full harness (just Context + builtins)
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let result = ctx.eval(r#""use strict"; eval("01;")"#);
        assert!(
            result.is_err(),
            "strict eval with legacy octal should error"
        );
    }

    #[test]
    fn eval_no_harness_catch() {
        // Test eval catch WITHOUT harness
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let result = ctx.eval(
            r#"
"use strict";
var caught = null;
try {
  eval("01;");
} catch(e) {
  caught = e;
}
if (caught === null) throw new Error("nothing caught");
caught.constructor.name;
"#,
        );
        let s = format!("{:?}", result);
        if result.is_err() {
            panic!("eval catch failed: {}", s);
        }
        // result should be the constructor name string
        let v = result.unwrap();
        let name = crate::value::to_js_string(&v);
        if !name.contains("SyntaxError") {
            panic!("wrong constructor name: {}", name);
        }
    }

    #[test]
    fn eval_legacy_octal_with_var() {
        // The exact pattern from test262
        let mut host = QuenchHost::new();
        let result = host.run_script(r#""use strict"; var a; eval("a = 0x1; a = 01;")"#);
        assert!(
            result.is_err(),
            "strict eval with var and legacy octal should error: {:?}",
            result
        );
    }

    #[test]
    fn syntax_error_as_function_returns_object() {
        // Does SyntaxError() (without new) return an object?
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var e = SyntaxError("test");
if (e === undefined) throw new Error("SyntaxError() returned undefined");
if (typeof e !== "object") throw new Error("SyntaxError() returned " + typeof e);
if (e.constructor !== SyntaxError) throw new Error("wrong constructor: " + e.constructor.name);
"#,
        );
        assert!(
            result.is_ok(),
            "SyntaxError as function should return object: {:?}",
            result
        );
    }

    #[test]
    fn strict_eval_legacy_octal_direct() {
        // Direct eval in strict mode with legacy octal
        let mut host = QuenchHost::new();
        let result = host.run_script(r#""use strict"; eval("01;")"#);
        let err_msg = format!("{:?}", result);
        assert!(
            result.is_err(),
            "strict eval with legacy octal should error: {:?}",
            result
        );
        assert!(
            err_msg.contains("SyntaxError"),
            "error should be SyntaxError, got: {}",
            err_msg
        );
    }

    #[test]
    fn strict_eval_with_var_declared() {
        // Same as the test262 test but without assert.throws
        let mut host = QuenchHost::new();
        let result = host.run_script(r#""use strict"; var a; eval("a = 0x1; a = 01;")"#);
        assert!(result.is_err(), "should throw, got: {:?}", result);
        let err_msg = format!("{:?}", result);
        assert!(
            err_msg.contains("SyntaxError"),
            "should be SyntaxError, got: {}",
            err_msg
        );
    }

    #[test]
    fn sloppy_eval_legacy_octal_allowed() {
        // In sloppy mode, legacy octals are allowed (no error)
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"eval("01;")"#);
        assert!(
            result.is_ok(),
            "sloppy eval with legacy octal should be ok: {:?}",
            result
        );
    }

    #[test]
    fn eval_from_function_in_strict_mode() {
        // eval called from within a function defined in strict mode
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
"use strict";
function test() {
  eval("01;");
}
test();
"#,
        );
        assert!(result.is_err(), "should throw, got: {:?}", result);
        let err_msg = format!("{:?}", result);
        assert!(
            err_msg.contains("SyntaxError"),
            "should be SyntaxError, got: {}",
            err_msg
        );
    }

    #[test]
    fn eval_from_function_with_var() {
        // Same as above with var a;
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
"use strict";
var a;
function test() {
  eval("a = 0x1; a = 01;");
}
test();
"#,
        );
        assert!(result.is_err(), "should throw, got: {:?}", result);
        let err_msg = format!("{:?}", result);
        assert!(
            err_msg.contains("SyntaxError"),
            "should be SyntaxError, got: {}",
            err_msg
        );
    }

    #[test]
    fn instanceof_regexp_with_harness() {
        // Test instanceof with RegExp literal with harness loaded
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var re = /(?:)/;
re instanceof RegExp;
"#,
        );
        println!("Result: {:?}", result);
        assert!(
            result.is_ok(),
            "instanceof regexp with harness failed: {:?}",
            result
        );
    }

    #[test]
    fn sloppy_legacy_octal_via_harness() {
        // Reproducer: sloppy mode with legacy octal via harness loader
        // This mimics what the test262 runner does for the legacy-octal-integer test
        use crate::test262::harness::HarnessLoader;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");

        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
        let script = harness.build_script("var x = 01;", &[]).unwrap();
        assert!(
            !script.starts_with("\"use strict\""),
            "sloppy script should not have strict directive"
        );

        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        println!(
            "Script start (first 80 chars): {:?}",
            &script[..80.min(script.len())]
        );
        println!("Result: {:?}", result);
        assert!(
            result.is_ok(),
            "sloppy mode with harness should allow legacy octal: {:?}",
            result
        );
    }

    #[test]
    fn strict_mode_unicode_escaped_identifier_no_octal() {
        // Reproducer for the part-unicode-10.0.0-escaped.js failure:
        // "use strict" + harness + Unicode escaped identifiers should NOT
        // trigger "legacy octal literals are not allowed in strict mode".
        use crate::test262::harness::HarnessLoader;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");
        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());

        // This test has no includes
        let source = r#"// Copyright 2024 Mathias Bynens. All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.

/*---
author: Mathias Bynens
esid: sec-names-and-keywords
description: |
  Test that Unicode v10.0.0 ID_Continue characters are accepted as
  identifier part characters in escaped form, i.e.
  - \uXXXX or \u{XXXX} for BMP symbols
  - \u{XXXXXX} for astral symbols
info: |
  Generated by https://github.com/mathiasbynens/caniunicode
---*/

var _\u0AFA\u0AFB\u0AFC\u0AFD\u0AFE\u0AFF\u0D00\u0D3B\u0D3C\u1CF7\u1DF6\u1DF7\u1DF8\u1DF9\u{11A01}\u{11A02}\u{11A03}\u{11A04}\u{11A05}\u{11A06}\u{11A07}\u{11A08}\u{11A09}\u{11A0A}\u{11A33}\u{11A34}\u{11A35}\u{11A36}\u{11A37}\u{11A38}\u{11A39}\u{11A3B}\u{11A3C}\u{11A3D}\u{11A3E}\u{11A47}\u{11A51}\u{11A52}\u{11A53}\u{11A54}\u{11A55}\u{11A56}\u{11A57}\u{11A58}\u{11A59}\u{11A5A}\u{11A5B}\u{11A8A}\u{11A8B}\u{11A8C}\u{11A8D}\u{11A8E}\u{11A8F}\u{11A90}\u{11A91}\u{11A92}\u{11A93}\u{11A94}\u{11A95}\u{11A96}\u{11A97}\u{11A98}\u{11A99}\u{11D31}\u{11D32}\u{11D33}\u{11D34}\u{11D35}\u{11D36}\u{11D3A}\u{11D3C}\u{11D3D}\u{11D3F}\u{11D40}\u{11D41}\u{11D42}\u{11D43}\u{11D44}\u{11D45}\u{11D47}\u{11D50}\u{11D51}\u{11D52}\u{11D53}\u{11D54}\u{11D55}\u{11D56}\u{11D57}\u{11D58}\u{11D59};"#;

        let script = harness.build_script(source, &[]).unwrap();
        let strict_script = format!("\"use strict\";\n{}", script);

        let mut host = QuenchHost::new();
        let result = host.run_script(&strict_script);
        assert!(
            result.is_ok(),
            "strict mode with Unicode escaped identifiers failed: {:?}",
            result
        );
    }

    #[test]
    fn sloppy_legacy_octal_full_test_file() {
        // Reproducer: run the actual legacy-octal-integer.js test file in sloppy mode
        use crate::test262::harness::HarnessLoader;
        use crate::test262::metadata::Test262Metadata;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");

        let test_path =
            repo_root.join("tests/test262/test/language/literals/numeric/legacy-octal-integer.js");
        let source = std::fs::read_to_string(&test_path).unwrap();
        let meta = Test262Metadata::parse(&source).unwrap();

        println!("Test flags: {:?}", meta.flags);
        println!("Test includes: {:?}", meta.includes);
        assert!(
            meta.flags.contains(&"noStrict".to_string()),
            "test should have noStrict flag"
        );

        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
        let script = harness.build_script(&source, &meta.includes).unwrap();
        println!("Script starts with: {:?}", &script[..100.min(script.len())]);

        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        println!("Result: {:?}", result);
        assert!(
            result.is_ok(),
            "sloppy mode should allow legacy octal: {:?}",
            result
        );
    }

    #[test]
    fn run_single_test_legacy_octal() {
        // Reproducer: call run_single_test directly to see which path fails
        use crate::test262::harness::HarnessLoader;
        use crate::test262::runner::run_single_test;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");

        let test_path =
            repo_root.join("tests/test262/test/language/literals/numeric/legacy-octal-integer.js");

        let mut host = QuenchHost::new();
        let outcome = run_single_test(
            &mut host,
            &HarnessLoader::new(test262_dir.to_str().unwrap()),
            &test_path,
        );
        println!("Outcome: {:?}", outcome);
        assert!(
            matches!(outcome, TestOutcome::Pass),
            "expected Pass, got: {:?}",
            outcome
        );
    }

    #[test]
    fn test_s8_6_2_a7_exact_pipeline() {
        // Reproduce S8.6.2_A7.js exactly through the full runner pipeline
        use crate::test262::harness::HarnessLoader;
        use crate::test262::runner::run_single_test;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");
        let test_path = repo_root.join("tests/test262/test/language/types/object/S8.6.2_A7.js");

        let mut host = QuenchHost::new();
        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
        let outcome = run_single_test(&mut host, &harness, &test_path);

        println!("S8.6.2_A7 outcome: {:?}", outcome);
        assert!(
            matches!(outcome, TestOutcome::Pass),
            "S8.6.2_A7 expected Pass, got: {:?}",
            outcome
        );
    }

    #[test]
    fn test_new_math_through_pipeline() {
        // Minimal repro: new Math through full pipeline with harness
        use crate::test262::harness::HarnessLoader;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");

        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
        let script = harness
            .build_script(
                r#"
assert.throws(TypeError, function() {
  var objMath = new Math;
});
"#,
                &["assert.js".to_string()],
            )
            .unwrap();

        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        println!("new Math result: {:?}", result);
        assert!(
            result.is_ok(),
            "new Math through pipeline failed: {:?}",
            result
        );
    }

    #[test]
    fn test_new_math_no_harness() {
        // Check: does new Math work without harness?
        let mut host = QuenchHost::new();
        let result = host.run_script("var m = new Math;");
        println!("new Math (no harness): {:?}", result);
        assert!(result.is_err(), "new Math should throw, got: {:?}", result);
    }

    #[test]
    fn test_math_inside_plain_function() {
        // Does Math work inside a plain function (no assert.throws)?
        let mut host = QuenchHost::new();
        let result = host.run_script("(function() { return Math.abs(-5); })()");
        println!("Math inside plain function: {:?}", result);
        assert!(
            result.is_ok(),
            "Math in plain function failed: {:?}",
            result
        );
    }

    #[test]
    fn test_math_global_only() {
        // Does Math work at top level?
        let mut host = QuenchHost::new();
        let result = host.run_script("Math.abs(-5)");
        println!("Math at top level: {:?}", result);
        assert!(result.is_ok(), "Math at top level failed: {:?}", result);
    }

    #[test]
    fn test_math_new_vs_access() {
        // Does new Math work vs Math.abs() inside assert.throws callback?
        let mut host = QuenchHost::new();

        // Test 1: Math.abs() should work (found and callable)
        // assert.throws expects a throw but Math.abs(5) returns 5 → "no exception"
        let r1 = host.run_script("assert.throws(TypeError, function() { Math.abs(5); });");
        println!("Math.abs() in assert.throws: {:?}", r1);

        // Test 2: new Math should throw TypeError
        let r2 = host.run_script("assert.throws(TypeError, function() { new Math; });");
        println!("new Math in assert.throws: {:?}", r2);
        // Force print to see actual value
        let _ = r2.clone();
        assert!(r2.is_ok(), "new Math in assert.throws failed: {:?}", r2);
    }

    #[test]
    fn test_math_new_standalone_in_callback() {
        // Test: new Math without assert.throws (just try/catch)
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
(function() {
  try {
    var m = new Math;
    return "ok: " + typeof m;
  } catch(e) {
    return e.name + ": " + e.message;
  }
})();
"#,
        );
        println!("new Math standalone in callback: {:?}", result);
        // If ReferenceError: Math not found
        // If TypeError: Math found, correct error type
    }

    #[test]
    fn test_math_new_top_level_vs_callback() {
        // Compare: new Math at top level vs inside a function
        let mut host = QuenchHost::new();

        // Top level: should throw TypeError
        let r1 = host.run_script("new Math");
        println!("new Math top level: {:?}", r1);

        // Inside function (not assert.throws): should throw TypeError
        let r2 = host.run_script("(function() { new Math; })()");
        println!("new Math inside plain function: {:?}", r2);

        // Inside assert.throws: should throw TypeError (caught by assert.throws)
        let r3 = host.run_script("assert.throws(TypeError, function() { new Math; });");
        println!("new Math inside assert.throws: {:?}", r3);
    }

    #[test]
    fn test_math_abs_in_assert_throws() {
        // Does new Math work vs Math.abs() inside assert.throws callback?
        let mut host = QuenchHost::new();

        // Test: Check if Math is found inside the callback by returning its type
        let r0 = host.run_script(
            r#"
            (function() {
                return typeof Math;
            })()
        "#,
        );
        println!("typeof Math in callback: {:?}", r0);

        // Test 1: Math.abs() should work (found and callable)
        // assert.throws expects a throw but Math.abs(5) returns 5 → "no exception"
        let r1 = host.run_script("assert.throws(TypeError, function() { Math.abs(5); });");
        println!("Math.abs() in assert.throws: {:?}", r1);

        // Test 2: new Math should throw TypeError
        let r2 = host.run_script("assert.throws(TypeError, function() { new Math; });");
        println!("new Math in assert.throws: {:?}", r2);
        // Force print to see actual value
        let _ = r2.clone();
        assert!(r2.is_ok(), "new Math in assert.throws failed: {:?}", r2);
    }

    #[test]
    fn test_math_in_assert_throws_no_throw() {
        // Math.abs(5) = 5 (no throw) → assert.throws expects throw → FAIL
        // But if Math is not defined → ReferenceError → different failure
        let mut host = QuenchHost::new();
        let result = host.run_script("assert.throws(TypeError, function() { Math.abs(5); });");
        println!("Math in assert.throws (no throw): {:?}", result);
        // This should fail because Math.abs(5) = 5 (not a TypeError)
        // If it fails with ReferenceError, Math is missing
        // If it fails with assertion error, Math was found but wrong error type
        match &result {
            Ok(_) => println!("Unexpected: passed"),
            Err(e) => {
                if e.contains("ReferenceError") {
                    println!("DIAG: Math not found (ReferenceError)");
                } else if e.contains("TypeError") {
                    println!("DIAG: Math found, wrong error type");
                } else {
                    println!("DIAG: Other error: {}", e);
                }
            }
        }
    }

    #[test]
    fn trace_math_availability() {
        // Trace where Math disappears through the full pipeline
        use std::path::PathBuf;

        // Step 1: Fresh context + builtins only
        {
            let mut ctx = crate::Context::new().unwrap();
            crate::builtins::register_builtins(&mut ctx);
            let r = ctx.eval("typeof Math");
            println!("After builtins, typeof Math = {:?}", r);
            assert!(r.is_ok());
            assert_eq!(crate::value::to_js_string(&r.unwrap()), "object");
        }

        // Step 2: + try_inject_harness
        {
            let mut ctx = crate::Context::new().unwrap();
            crate::builtins::register_builtins(&mut ctx);
            crate::test262::harness::try_inject_harness(&mut ctx).unwrap();
            let r = ctx.eval("typeof Math");
            println!("After harness, typeof Math = {:?}", r);
            assert!(r.is_ok());
            assert_eq!(crate::value::to_js_string(&r.unwrap()), "object");
        }

        // Step 3: Full pipeline
        {
            let mut ctx = crate::Context::new().unwrap();
            crate::builtins::register_builtins(&mut ctx);
            crate::test262::harness::try_inject_harness(&mut ctx).unwrap();
            crate::interpreter::set_strict_mode(false);
            let r = ctx.eval("new Math");
            println!("After full pipeline, new Math = {:?}", r);
            assert!(r.is_err(), "new Math should throw TypeError");
        }
    }

    #[test]
    fn repro_coerce_symbol_to_prim_err() {
        // Reproducer for addition/get-symbol-to-prim-err.js
        // The thrower object's @@toPrimitive getter throws. When the
        // addition evaluates `thrower + counter`, the throw must propagate
        // BEFORE counter's @@toPrimitive getter is invoked.
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var thrower = {};
var counter = {};
var callCount = 0;

Object.defineProperty(thrower, Symbol.toPrimitive, {
  get: function() {
    throw new Test262Error();
  }
});
Object.defineProperty(counter, Symbol.toPrimitive, {
  get: function() {
    callCount += 1;
  }
});

var threw = false;
try {
  thrower + counter;
} catch(e) {
  threw = e.constructor === Test262Error;
}
if (!threw) throw new Error("addition should throw");
if (callCount !== 0) throw new Error("counter's getter was called: " + callCount);
"#,
        );
        assert!(
            result.is_ok(),
            "coerce-symbol-to-prim-err repro failed: {:?}",
            result
        );
    }

    #[test]
    fn repro_coerce_symbol_to_prim_err_full() {
        // Same as the test262 file, with the full assert.throws wrapper.
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var thrower = {};
var counter = {};
var log;

Object.defineProperty(thrower, Symbol.toPrimitive, {
  get: function() {
    log += 'accessThrower';
    return function() { throw new Test262Error(); };
  }
});
Object.defineProperty(counter, Symbol.toPrimitive, {
  get: function() {
    log += 'accessCounter';
    return function() { log += 'callCounter'; };
  }
});

log = '';

assert.throws(Test262Error, function() {
  thrower + counter;
}, 'error thrown by left-hand side');
assert.sameValue(log, 'accessThrower');
"#,
        );
        assert!(result.is_ok(), "coerce-symbol-to-prim-err full failed: {:?}", result);
    }

    #[test]
    fn repro_coerce_symbol_to_prim_err_via_loader() {
        // Run the actual test262 file via HarnessLoader (mimics the staged runner).
        use crate::test262::harness::HarnessLoader;
        use crate::test262::metadata::Test262Metadata;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");
        let test_path = repo_root.join(
            "tests/test262/test/language/expressions/addition/coerce-symbol-to-prim-err.js",
        );

        let source = std::fs::read_to_string(&test_path).unwrap();
        let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
        let meta = Test262Metadata::parse(&source).unwrap();
        let script = harness.build_script(&source, &meta.includes).unwrap();

        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        assert!(
            result.is_ok(),
            "coerce-symbol-to-prim-err via loader failed: {:?}",
            result
        );
    }

    #[test]
    fn trace_assert_throws_in_pipeline() {
        // Step-by-step: what does assert.throws(TypeError, fn() { new Math }) see?
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let test262_dir = repo_root.join("tests/test262");

        let harness = crate::test262::harness::HarnessLoader::new(test262_dir.to_str().unwrap());
        let script = harness
            .build_script(
                r#"
var result;
try {
  new Math;
  result = "no error";
} catch(e) {
  result = e.constructor.name + ": " + e.message;
}
result;
"#,
                &["assert.js".to_string()],
            )
            .unwrap();

        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        println!("What error did new Math throw? {:?}", result);
        assert!(result.is_ok(), "tracing test failed: {:?}", result);
    }

    #[test]
    fn block_let_shadows_outer_let_for_closure() {
        // Reproducer for test262 statements/block/scope-lex-close.js:
        // a `let` inside a block must create a new lexical binding that
        // shadows an outer `let` of the same name, even when a closure
        // captures the inner binding. The inner `x` must remain 'inside'
        // even though the outer `let x = 'outside'` is evaluated later.
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var probe;
{
  let x = 'inside';
  probe = function() { return x; };
}
let x = 'outside';
if (x !== 'outside') throw new Error('outer x wrong: ' + x);
if (probe() !== 'inside') throw new Error('probe() wrong: ' + probe());
"#,
        );
        assert!(
            result.is_ok(),
            "block let shadowing failed: {:?}",
            result
        );
    }

    #[test]
    fn block_let_does_not_leak_after_block() {
        // A `let` declared inside a block must NOT be visible after the
        // block exits (block-scoped, not function-scoped).
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
{
  let blockScoped = 'inside';
}
try {
  blockScoped;
  throw new Error('blockScoped leaked out of block');
} catch (e) {
  if (!(e instanceof ReferenceError)) {
    throw new Error('expected ReferenceError, got: ' + e);
  }
}
"#,
        );
        assert!(
            result.is_ok(),
            "block-scope leak test failed: {:?}",
            result
        );
    }
}
