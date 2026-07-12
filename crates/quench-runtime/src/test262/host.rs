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
        try_inject_harness(&mut ctx).map_err(|e| format!("harness load failure: {}", e))?;
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
        assert!(result.is_ok(), "assert.throws(TypeError) failed: {:?}", result);
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
        assert!(result.is_ok(), "custom error name test failed: {:?}", result);
    }

    #[test]
    fn thrown_error_has_name_property() {
        // Verify that `new TypeError()` actually sets .name on the object.
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"var n = new TypeError().name; if (n !== "TypeError") throw new Error("got: " + n)"#);
        assert!(result.is_ok(), "new TypeError().name failed: {:?}", result);
    }

    #[test]
    fn constructor_property_chain_for_errors() {
        // Verify that local TypeError instances have constructor === local TypeError,
        // NOT the global TypeError. This is how assert.throws distinguishes custom
        // errors from built-in errors even when they share the same name.
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"
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
"#);
        assert!(result.is_ok(), "constructor chain test failed: {:?}", result);
    }

    #[test]
    fn harness_exact_assert_throws_custom_typeerror() {
        // Reproduce the exact harness file logic step by step
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"
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
"#);
        assert!(result.is_ok(), "harness exact test failed: {:?}", result);
    }

    #[test]
    fn strict_eval_legacy_octal_should_throw_syntax_error() {
        // Reproducer for test262 7.8.3-3gs.js: eval in strict mode with
        // legacy octal literals should throw SyntaxError, not ReferenceError.
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"
"use strict";
var a;
assert.throws(SyntaxError, function() {
  eval("a = 0x1;a = 01;");
});
"#);
        assert!(result.is_ok(), "strict eval with legacy octal should throw SyntaxError: {:?}", result);
    }

    #[test]
    fn assert_throws_with_plain_function() {
        // Test assert.throws with a plain JS function that throws SyntaxError
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"
assert.throws(SyntaxError, function() {
  throw new SyntaxError("test");
});
"#);
        assert!(result.is_ok(), "assert.throws with SyntaxError should work: {:?}", result);
    }

    #[test]
    fn assert_throws_with_plain_function_throwing_syntax_error() {
        // Test assert.throws with a plain JS function that throws SyntaxError
        let mut host = QuenchHost::new();
        // Note: no "use strict" prefix
        let result = host.run_script(r#"
assert.throws(ReferenceError, function() {
  console.log(x);
});
"#);
        assert!(result.is_ok(), "assert.throws with ReferenceError should work: {:?}", result);
    }

    #[test]
    fn assert_throws_eval_syntax_error_directly() {
        // What if we catch the eval error directly?
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"
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
"#);
        assert!(result.is_ok(), "try/catch eval SyntaxError should work: {:?}", result);
    }

    #[test]
    fn eval_legacy_octal_simple() {
        // Minimal test: eval in strict mode throws on legacy octal?
        let mut host = QuenchHost::new();
        let result = host.run_script(r#""use strict"; eval("01;")"#);
        assert!(result.is_err(), "strict eval with legacy octal should error");
    }

    #[test]
    fn eval_legacy_octal_catch() {
        // Can we catch it? Try WITHOUT native assert/throws
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"
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
"#);
        assert!(result.is_ok(), "eval octal catch should work: {:?}", result);
    }

    #[test]
    fn syntax_error_direct() {
        // Minimal test: create SyntaxError and check constructor.name
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"
var e = new SyntaxError("test");
e.name;
console.log(e.name);
"#);
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
        assert!(result.is_err(), "strict eval with legacy octal should error");
    }

    #[test]
    fn eval_no_harness_catch() {
        // Test eval catch WITHOUT harness
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let result = ctx.eval(r#"
"use strict";
var caught = null;
try {
  eval("01;");
} catch(e) {
  caught = e;
}
if (caught === null) throw new Error("nothing caught");
caught.constructor.name;
"#);
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
        assert!(result.is_err(), "strict eval with var and legacy octal should error: {:?}", result);
    }

    #[test]
    fn syntax_error_as_function_returns_object() {
        // Does SyntaxError() (without new) return an object?
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"
var e = SyntaxError("test");
if (e === undefined) throw new Error("SyntaxError() returned undefined");
if (typeof e !== "object") throw new Error("SyntaxError() returned " + typeof e);
if (e.constructor !== SyntaxError) throw new Error("wrong constructor: " + e.constructor.name);
"#);
        assert!(result.is_ok(), "SyntaxError as function should return object: {:?}", result);
    }




    #[test]
    fn strict_eval_legacy_octal_direct() {
        // Direct eval in strict mode with legacy octal
        let mut host = QuenchHost::new();
        let result = host.run_script(r#""use strict"; eval("01;")"#);
        let err_msg = format!("{:?}", result);
        assert!(result.is_err(), "strict eval with legacy octal should error: {:?}", result);
        assert!(err_msg.contains("SyntaxError"), "error should be SyntaxError, got: {}", err_msg);
    }

    #[test]
    fn strict_eval_with_var_declared() {
        // Same as the test262 test but without assert.throws
        let mut host = QuenchHost::new();
        let result = host.run_script(r#""use strict"; var a; eval("a = 0x1; a = 01;")"#);
        assert!(result.is_err(), "should throw, got: {:?}", result);
        let err_msg = format!("{:?}", result);
        assert!(err_msg.contains("SyntaxError"), "should be SyntaxError, got: {}", err_msg);
    }

    #[test]
    fn sloppy_eval_legacy_octal_allowed() {
        // In sloppy mode, legacy octals are allowed (no error)
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"eval("01;")"#);
        assert!(result.is_ok(), "sloppy eval with legacy octal should be ok: {:?}", result);
    }

    #[test]
    fn eval_from_function_in_strict_mode() {
        // eval called from within a function defined in strict mode
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"
"use strict";
function test() {
  eval("01;");
}
test();
"#);
        assert!(result.is_err(), "should throw, got: {:?}", result);
        let err_msg = format!("{:?}", result);
        assert!(err_msg.contains("SyntaxError"), "should be SyntaxError, got: {}", err_msg);
    }

    #[test]
    fn eval_from_function_with_var() {
        // Same as above with var a;
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"
"use strict";
var a;
function test() {
  eval("a = 0x1; a = 01;");
}
test();
"#);
        assert!(result.is_err(), "should throw, got: {:?}", result);
        let err_msg = format!("{:?}", result);
        assert!(err_msg.contains("SyntaxError"), "should be SyntaxError, got: {}", err_msg);
    }

    #[test]
    fn instanceof_regexp_with_harness() {
        // Test instanceof with RegExp literal with harness loaded
        let mut host = QuenchHost::new();
        let result = host.run_script(r#"
var re = /(?:)/;
re instanceof RegExp;
"#);
        println!("Result: {:?}", result);
        assert!(result.is_ok(), "instanceof regexp with harness failed: {:?}", result);
    }

}
