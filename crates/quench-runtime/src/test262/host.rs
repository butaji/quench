//! Trait boundary between the test262 runner and the engine under test.

use crate::test262::harness::try_inject_harness;
use crate::Context;

/// Implement this for your engine to plug it into the test262 runner.
pub trait Test262Host: Send {
    /// Execute a complete JS script (harness + test source) in script mode.
    /// `Ok(())` if execution completes without throwing,
    /// `Err(message)` if it throws or fails to evaluate.
    fn run_script(&mut self, source: &str) -> Result<(), String>;

    /// Execute a complete ES module (harness + test source) in module mode.
    /// Used for tests with `flags: [module]`. Return value follows `run_script`.
    fn run_module_script(&mut self, source: &str) -> Result<(), String>;
}

/// What happened when we tried to run a test.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum TestOutcome {
    Pass,
    Fail {
        reason: String,
    },
    /// Documented skip — never counted as a pass.
    Skip {
        reason: String,
    },
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
        let prev_strict = crate::interpreter::is_strict_mode();
        try_inject_harness(&mut ctx).map_err(|e| format!("harness load failure: {}", e))?;
        crate::interpreter::set_strict_mode(prev_strict);
        crate::interpreter::set_strict_mode(false);
        ctx.eval(source).map(|_| ()).map_err(|e| format!("{:?}", e))
    }

    fn run_module_script(&mut self, source: &str) -> Result<(), String> {
        let mut ctx = Context::new().map_err(|e| format!("{:?}", e))?;
        crate::builtins::register_builtins(&mut ctx);
        let prev_strict = crate::interpreter::is_strict_mode();
        try_inject_harness(&mut ctx).map_err(|e| format!("harness load failure: {}", e))?;
        crate::interpreter::set_strict_mode(prev_strict);
        crate::interpreter::set_strict_mode(false);
        ctx.eval_es_module(source)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_skip_is_not_pass() {
        let s = TestOutcome::Skip {
            reason: "known crash".into(),
        };
        assert_ne!(s, TestOutcome::Pass);
        assert!(s.to_string().starts_with("SKIP:"));
    }

    #[test]
    fn fn_name_method_test262_case() {
        use crate::test262::harness::HarnessLoader;
        use crate::test262::runner::default_test262_dir;
        use crate::test262::runner::run_single_test;
        let harness = HarnessLoader::new(&default_test262_dir());
        let path = std::path::PathBuf::from(default_test262_dir())
            .join("test/language/statements/class/definition/fn-name-method.js");
        let mut host = QuenchHost::new();
        let outcome = run_single_test(&mut host, &harness, &path);
        assert_eq!(outcome, TestOutcome::Pass, "fn-name-method: {:?}", outcome);
    }

    #[test]
    fn fn_name_method_static_id_via_build_script() {
        use crate::test262::harness::HarnessLoader;
        use crate::test262::runner::default_test262_dir;
        let harness = HarnessLoader::new(&default_test262_dir());
        let ph = harness
            .build_script("", &["propertyHelper.js".to_string()])
            .unwrap();
        let script = format!(
            "{ph}class A {{ static id() {{}} }} \
             verifyProperty(A.id, 'name', {{ value: 'id', writable: false, enumerable: false, configurable: true }});"
        );
        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        assert!(result.is_ok(), "static A.id verifyProperty: {:?}", result);
    }

    #[test]
    fn fn_name_method_via_build_script_first_two() {
        use crate::test262::harness::HarnessLoader;
        use crate::test262::runner::default_test262_dir;
        let harness = HarnessLoader::new(&default_test262_dir());
        let ph = harness
            .build_script("", &["propertyHelper.js".to_string()])
            .unwrap();
        let script = format!(
            "{ph}var namedSym = Symbol('test262'); var anonSym = Symbol(); \
             class A {{ id() {{}} [anonSym]() {{}} [namedSym]() {{}} }} \
             verifyProperty(A.prototype.id, 'name', {{ value: 'id', writable: false, enumerable: false, configurable: true }}); \
             verifyProperty(A.prototype[anonSym], 'name', {{ value: '', writable: false, enumerable: false, configurable: true }});"
        );
        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        assert!(result.is_ok(), "first two verifyProperty: {:?}", result);
    }

    #[test]
    fn fn_name_method_via_build_script() {
        use crate::test262::harness::HarnessLoader;
        use crate::test262::metadata::Test262Metadata;
        use crate::test262::runner::default_test262_dir;
        use std::fs;
        let path = std::path::PathBuf::from(default_test262_dir())
            .join("test/language/statements/class/definition/fn-name-method.js");
        let source = fs::read_to_string(&path).unwrap();
        let meta = Test262Metadata::parse(&source).unwrap();
        let harness = HarnessLoader::new(&default_test262_dir());
        let script = harness.build_script(&source, &meta.includes).unwrap();
        let mut host = QuenchHost::new();
        let result = host.run_script(&script);
        assert!(result.is_ok(), "build_script fn-name-method: {:?}", result);
    }

    #[test]
    fn quench_host_cptn_decl_class_completion() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "assert.sameValue(eval('class C {}'), undefined);\n\
             assert.sameValue(eval('1; class C {}'), 1);",
        );
        assert!(
            result.is_ok(),
            "cptn-decl class completion via eval: {:?}",
            result
        );
    }

    #[test]
    fn quench_host_runs_and_throws() {
        let mut host = QuenchHost::new();
        assert!(host.run_script("var x = 1 + 1;").is_ok());
        assert!(host.run_script("throw new Error('boom')").is_err());
    }

    #[test]
    fn quench_host_runs_module() {
        let mut host = QuenchHost::new();
        assert!(host.run_module_script("export default 42;").is_ok());
    }

    #[test]
    fn quench_host_verify_property_symbol_accessor() {
        // Reproduce verifyProperty-restore-accessor-symbol.js scenario
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var obj;
var prop = Symbol(1);
var desc = { enumerable: true, configurable: true, get: function() { return 42; }, set: function() {} };

obj = {};
Object.defineProperty(obj, prop, desc);

// Check hasOwnProperty
var hasOwn = Object.prototype.hasOwnProperty.call(obj, prop);
if (hasOwn !== true) throw new Error('hasOwnProperty should be true, got ' + hasOwn);

// Check getter invocation
var val = obj[prop];
if (val !== 42) throw new Error('obj[prop] should return 42, got ' + val + ' (type: ' + typeof val + ')');

// Check getOwnPropertyDescriptor
var desc2 = Object.getOwnPropertyDescriptor(obj, prop);
if (typeof desc2.get !== 'function') throw new Error('desc2.get should be function');
"#,
        );
        assert!(result.is_ok(), "Symbol accessor test failed: {:?}", result);
    }

    #[test]
    fn quench_host_same_value_function_identity() {
        // Test assert.sameValue with function identity (the core of verifyProperty)
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var obj = {};
Object.defineProperty(obj, 'foo', {
    enumerable: true,
    configurable: true,
    get: function() { return 99; },
    set: function() {}
});
var d = Object.getOwnPropertyDescriptor(obj, 'foo');
// assert.sameValue should succeed when comparing the same function object
assert.sameValue(d.get, d.get, 'function identity');
assert.sameValue(d.set, d.set, 'setter identity');
// assert.sameValue should fail for different values
var threw = false;
try {
    assert.sameValue(d.get, d.set);
} catch(e) {
    threw = true;
}
if (!threw) throw new Error('sameValue(d.get, d.set) should throw');
"#,
        );
        assert!(
            result.is_ok(),
            "sameValue function identity test failed: {:?}",
            result
        );
    }

    #[test]
    fn quench_host_symbol_accessor_same_value() {
        // Test assert.sameValue with Symbol-keyed accessor descriptor
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var obj = {};
var sym = Symbol('test');
Object.defineProperty(obj, sym, {
    enumerable: true,
    configurable: true,
    get: function() { return 42; },
    set: function() {}
});
var d = Object.getOwnPropertyDescriptor(obj, sym);
// The getter function should be preserved
assert.sameValue(typeof d.get, 'function', 'getter is a function');
assert.sameValue(typeof d.set, 'function', 'setter is a function');
assert.sameValue(d.get(), 42, 'getter returns 42');
"#,
        );
        assert!(
            result.is_ok(),
            "Symbol accessor sameValue test failed: {:?}",
            result
        );
    }

    /// Reproduce cpn-class-decl-accessors-computed-property-name-from-function-declaration.js
    /// This mimics the test262 harness path exactly to see if C[f()] = 1 returns undefined.
    #[test]
    fn quench_host_class_computed_setter() {
        let mut host = QuenchHost::new();
        // C[f()] = 1 should return 1 (the RHS), not undefined
        let result = host.run_script(
            r#"
function f() {}
class C {
    get [f()]() { return 1; }
    set [f()](v) { return 1; }
    static get [f()]() { return 1; }
    static set [f()](v) { return 1; }
}
var c = new C();
var r1 = C[f()] = 1;
var r2 = c[f()] = 1;
if (r1 !== 1) throw new Error('C[f()] = 1 returned ' + r1 + ', expected 1');
if (r2 !== 1) throw new Error('c[f()] = 1 returned ' + r2 + ', expected 1');
"#,
        );
        assert!(
            result.is_ok(),
            "computed setter assignment should return RHS: {:?}",
            result
        );
    }

    #[test]
    fn quench_host_class_computed_setter_via_assert() {
        let mut host = QuenchHost::new();
        // Same as above but using assert.sameValue (like the actual test262 test)
        let result = host.run_script(
            r#"
function f() {}
class C {
    get [f()]() { return 1; }
    set [f()](v) { return 1; }
    static get [f()]() { return 1; }
    static set [f()](v) { return 1; }
}
var c = new C();
assert.sameValue(C[f()] = 1, 1);
assert.sameValue(c[f()] = 1, 1);
"#,
        );
        assert!(
            result.is_ok(),
            "computed setter assert.sameValue should pass: {:?}",
            result
        );
    }

    /// Regression test: assignment to class setter via String() conversion
    /// must return the RHS value (1), not the setter's return value.
    /// Previously failed: C[String(f())] = 1 returned undefined.
    #[test]
    fn quench_host_class_computed_setter_string_conversion() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
function f() {}
class C {
    get [f()]() { return 1; }
    set [f()](v) { return 1; }
    static get [f()]() { return 1; }
    static set [f()](v) { return 1; }
}
var c = new C();
// All forms must return the assigned value, not the setter's return.
assert.sameValue(C[String(f())] = 1, 1, 'C[String(f())] = 1 must return 1');
assert.sameValue(C[f()] = 1, 1, 'C[f()] = 1 must return 1');
assert.sameValue(c[String(f())] = 1, 1, 'c[String(f())] = 1 must return 1');
assert.sameValue(c[f()] = 1, 1, 'c[f()] = 1 must return 1');
"#,
        );
        assert!(
            result.is_ok(),
            "computed setter with String() conversion must return RHS: {:?}",
            result
        );
    }

    #[test]
    fn quench_host_with_harness_verify_property_accessor_symbol() {
        // Full harness test: load assert.js + propertyHelper.js + run verifyProperty scenario
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
var __hasOwnProperty = Function.prototype.call.bind(Object.prototype.hasOwnProperty);
var __getOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
var __propertyIsEnumerable = Function.prototype.call.bind(Object.prototype.propertyIsEnumerable);

// Simplified verifyProperty that checks the accessor descriptor
function verifyProperty(obj, name, desc) {
    var originalDesc = __getOwnPropertyDescriptor(obj, name);

    if (!__hasOwnProperty(obj, name)) {
        throw new Error('should be own property');
    }

    if (typeof originalDesc.get !== 'function') {
        throw new Error('originalDesc.get should be function, got ' + typeof originalDesc.get);
    }
    if (typeof originalDesc.set !== 'function') {
        throw new Error('originalDesc.set should be function, got ' + typeof originalDesc.set);
    }
}

var obj = {};
var prop = Symbol(1);
var desc = { enumerable: true, configurable: true, get: function() { return 42; }, set: function() {} };

Object.defineProperty(obj, prop, desc);
verifyProperty(obj, prop, desc);
"#,
        );
        assert!(
            result.is_ok(),
            "verifyProperty accessor Symbol failed: {:?}",
            result
        );
    }
}
