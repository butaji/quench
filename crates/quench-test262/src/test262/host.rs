//! Trait boundary between the test262 runner and the engine under test.

use std::path::Path;

#[cfg(test)]
use crate::harness::try_inject_harness;
use quench_runtime::value::error::take_thrown_value;
#[cfg(test)]
use quench_runtime::Context;
use quench_runtime::Value;

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

/// Structured diagnostic information about a test failure.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TestFailure {
    /// Human-readable error message (used for digest grouping).
    pub message: String,
    /// Error type extracted from the JS error object, e.g. "TypeError", "Test262Error".
    pub error_type: Option<String>,
    /// The `.message` property of the JS error object (the spec-level error detail).
    pub error_message: Option<String>,
    /// JS stack trace, e.g. from `.stack` on the error object.
    pub js_stack: Option<String>,
    /// Path to the test source file.
    pub source_path: Option<String>,
    /// Approximate line number where the failure occurred (1-based).
    pub source_line: Option<usize>,
    /// Source code context surrounding the failure (up to ~N lines).
    pub source_context: String,
}

impl TestFailure {
    /// Build a minimal TestFailure from just a message string (backward compat).
    pub fn from_message(msg: impl Into<String>) -> Self {
        TestFailure {
            message: msg.into(),
            error_type: None,
            error_message: None,
            js_stack: None,
            source_path: None,
            source_line: None,
            source_context: String::new(),
        }
    }

    /// Build a TestFailure from a message and a thrown JS Value.
    /// Extracts `.name`, `.message`, and `.stack` from the error object.
    pub fn from_thrown(msg: impl Into<String>, thrown: Value) -> Self {
        let msg = msg.into();
        let (error_type, error_message, js_stack) = extract_error_properties(&thrown);
        TestFailure {
            message: msg,
            error_type,
            error_message,
            js_stack,
            source_path: None,
            source_line: None,
            source_context: String::new(),
        }
    }

    /// Attach source context from a test file path and optional line hint.
    /// When no hint_line is given, tries to locate the failing line by
    /// searching the source for keywords extracted from the error message.
    pub fn with_source(mut self, path: &Path, hint_line: Option<usize>) -> Self {
        let path_s = path.to_string_lossy().to_string();
        self.source_path = Some(path_s.clone());
        if let Ok(source) = std::fs::read_to_string(path) {
            let lines: Vec<&str> = source.lines().collect();
            let total = lines.len();
            // Determine which line to highlight: provided hint, or auto-detect
            // from the error message keywords.
            let target = hint_line.or_else(|| self.locate_message_in_source(&lines));
            let (start, end) = if let Some(hl) = target {
                let hl = hl.max(1).min(total);
                let s = hl.saturating_sub(10);
                let e = (hl + 10).min(total);
                (s, e)
            } else {
                (0, total.min(30))
            };
            let ctx_lines: Vec<String> = lines[start..end]
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let ln = start + i + 1;
                    let marker = if target == Some(ln) { " → " } else { "   " };
                    format!("{}{:4}: {}", marker, ln, l)
                })
                .collect();
            self.source_context = ctx_lines.join("\n");
            self.source_line = target;
        }
        self
    }

    /// Try to locate the failing line in source by matching error message
    /// keywords against source lines. Falls back to assertion function calls.
    /// Returns a 1-based line number.
    fn locate_message_in_source(&self, lines: &[&str]) -> Option<usize> {
        let body = self.message_body();
        let keywords = extract_keywords(&body);
        if let Some(idx) = best_keyword_line(lines, &keywords) {
            return Some(idx + 1);
        }
        if is_assert_throws_wrapper_message(&body) {
            if let Some(idx) = lines.iter().rposition(|l| l.contains("assert.throws")) {
                return Some(idx + 1);
            }
        }
        fallback_assertion_line(lines).map(|idx| idx + 1)
    }
}

const LOCATE_STOP_WORDS: &[&str] = &[
    "Actual", "expected", "should", "have", "same", "contents", "this", "that", "with", "from",
    "been", "call", "called", "value", "values", "throw", "thrown", "error", "failed", "after",
    "before", "true", "false",
];

const LOCATE_ASSERTION_PATTERNS: &[&str] = &[
    "assert.compareArray",
    "assert.sameValue",
    "assert.throws",
    "assert.notSameValue",
    "assert.deepEqual",
    "assert(false",
    "assert(true",
    "$DONOTEVALUATE",
    "verifyProperty",
];

impl TestFailure {
    fn message_body(&self) -> String {
        let raw = self
            .message
            .strip_prefix("JsError(\"")
            .and_then(|s| s.rsplit_once("\")"))
            .map(|(inner, _)| inner)
            .unwrap_or(&self.message);
        raw.split_once(':')
            .map(|(_, rest)| rest.trim().to_string())
            .unwrap_or_else(|| raw.to_string())
    }
}

fn extract_keywords(body: &str) -> Vec<String> {
    body.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() > 4 && !LOCATE_STOP_WORDS.contains(w))
        .map(|w| w.to_string())
        .collect()
}

fn best_keyword_line(lines: &[&str], keywords: &[String]) -> Option<usize> {
    let mut scored: Vec<(usize, usize)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let count = keywords
                .iter()
                .filter(|kw| line.contains(kw.as_str()))
                .count();
            if count > 0 {
                Some((i, count))
            } else {
                None
            }
        })
        .collect();
    scored.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    scored.first().map(|(i, _)| *i)
}

fn is_assert_throws_wrapper_message(body: &str) -> bool {
    body.contains("Thrown value was not an object!")
        || body.contains(" but got a ")
        || body.contains(" to be thrown but no exception was thrown at all")
}

fn fallback_assertion_line(lines: &[&str]) -> Option<usize> {
    LOCATE_ASSERTION_PATTERNS
        .iter()
        .find_map(|pat| lines.iter().position(|l| l.contains(pat)))
}

/// Extract structured properties from a thrown JS error Value.
fn extract_error_properties(val: &Value) -> (Option<String>, Option<String>, Option<String>) {
    match val {
        Value::Object(obj) => {
            let obj = obj.borrow();
            let name = obj.get("name").and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            });
            let message = obj.get("message").and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            });
            let stack = obj.get("stack").and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            });
            (name, message, stack)
        }
        _ => (None, None, None),
    }
}

/// After an eval failure, capture the thrown JS error's properties.
/// Must be called immediately after the failed eval, before the thrown value is consumed.
/// Returns (error_type, error_message, js_stack).
pub fn capture_thrown_diagnostics() -> (Option<String>, Option<String>, Option<String>) {
    if let Some(thrown) = take_thrown_value() {
        extract_error_properties(&thrown)
    } else {
        (None, None, None)
    }
}

/// Get source context around a given line in a test file.
pub fn read_source_context(path: &Path, hint_line: Option<usize>, context_lines: usize) -> String {
    let Ok(source) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let lines: Vec<&str> = source.lines().collect();
    let total = lines.len();
    let (start, end) = if let Some(hl) = hint_line {
        let hl = hl.max(1).min(total);
        let s = hl.saturating_sub(context_lines);
        let e = (hl + context_lines).min(total);
        (s, e)
    } else {
        (0, total.min(context_lines * 2))
    };
    lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let ln = start + i + 1;
            let marker = if hint_line == Some(ln) {
                " → "
            } else {
                "   "
            };
            format!("{}{:4}: {}", marker, ln, l)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What happened when we tried to run a test.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum TestOutcome {
    Pass,
    Fail {
        #[serde(flatten)]
        failure: TestFailure,
    },
    /// Documented skip — never counted as a pass.
    Skip {
        reason: String,
    },
}

impl TestOutcome {
    /// Convenience: get the failure message if this is a Fail, else empty string.
    pub fn failure_message(&self) -> &str {
        match self {
            TestOutcome::Fail { failure } => &failure.message,
            _ => "",
        }
    }
}

impl std::fmt::Display for TestOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestOutcome::Pass => write!(f, "PASS"),
            TestOutcome::Fail { failure } => {
                write!(f, "FAIL: {}", failure.message)?;
                if let Some(ref et) = failure.error_type {
                    write!(f, " [{}]", et)?;
                }
                Ok(())
            }
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
        let mut ctx = crate::runner::execute::initialize_test_context(false)?;
        ctx.eval(source).map(|_| ()).map_err(|e| format!("{:?}", e))
    }

    fn run_module_script(&mut self, source: &str) -> Result<(), String> {
        let mut ctx = crate::runner::execute::initialize_test_context(false)?;
        ctx.eval_es_module(source)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_definition_null_proto_test262_case() {
        use crate::harness::HarnessLoader;
        use crate::runner::default_test262_dir;
        use crate::runner::run_single_test;
        let harness = HarnessLoader::new(&default_test262_dir());
        let path = std::path::PathBuf::from(default_test262_dir())
            .join("test/language/statements/class/subclass/class-definition-null-proto.js");
        let outcome = run_single_test(&harness, &path);
        assert_eq!(outcome, TestOutcome::Pass, "{:?}", outcome);
    }

    #[test]
    fn class_extends_null_proto_via_quench_host() {
        let mut host = QuenchHost::new();
        let r = host.run_script(
            "class Foo extends null {} \
             assert.sameValue(Object.getPrototypeOf(Foo.prototype), null);",
        );
        assert!(r.is_ok(), "{:?}", r);
    }

    #[test]
    fn outcome_skip_is_not_pass() {
        let s = TestOutcome::Skip {
            reason: "known crash".into(),
        };
        assert_ne!(s, TestOutcome::Pass);
        assert!(s.to_string().starts_with("SKIP:"));
    }

    #[test]
    fn dstr_array_pattern_with_null_value_throws_typeerror_object() {
        use crate::runner::execute::initialize_test_context;
        let mut ctx = initialize_test_context(false).expect("ctx");
        let script = r#"
            var f = ([[x]]) => {};
            var captured;
            try {
                f([null]);
                captured = "no-throw";
            } catch (thrown) {
                captured = [
                    typeof thrown === "object" && thrown !== null,
                    thrown && thrown.constructor && thrown.constructor.name,
                    thrown && thrown.constructor === TypeError,
                ];
            }
            globalThis.__dstr_result = captured;
        "#;
        ctx.eval(script).expect("script must run");
        let v = ctx.get_global("__dstr_result").expect("set");
        let Value::Object(obj) = v else {
            panic!("expected array, got {:?}", v);
        };
        let arr = obj.borrow();
        let is_object = matches!(arr.get("0").unwrap(), Value::Boolean(true));
        let name_val = arr.get("1").unwrap();
        let same_ctor = matches!(arr.get("2").unwrap(), Value::Boolean(true));
        assert!(
            is_object,
            "thrown must be an object (got typeof non-object)"
        );
        let Value::String(name) = name_val else {
            panic!("elem 1 must be string, got {:?}", name_val);
        };
        assert_eq!(name, "TypeError", "thrown must be a TypeError");
        assert!(same_ctor, "thrown.constructor must be TypeError");
    }

    #[test]
    fn unscopables_with_blocks_property_lookup_in_arrow() {
        use crate::runner::execute::initialize_test_context;
        let mut ctx = initialize_test_context(false).expect("ctx");
        // The arrow function has its own `var v = x` after the with block;
        // var hoisting makes v DeclaredOnly at function entry. Inside the
        // with(globalThis), looking up `v` while v is in unscopables must
        // skip the with-object AND find the (uninitialized) DeclaredOnly
        // arrow-scope binding, yielding undefined.
        let script = r#"
            var v = 1;
            globalThis[Symbol.unscopables] = { v: true };
            var ref = (x) => {
                with (globalThis) {
                    globalThis.__saw_v = v;
                }
                var v = x;
            };
            ref(0);
        "#;
        ctx.eval(script).expect("arrow with unscopables must run");
        let saw = ctx.get_global("__saw_v").expect("saw_v must be set");
        let Value::Undefined = saw else {
            panic!(
                "with(globalThis) under unscopables must yield undefined for v (DeclaredOnly), got {:?}",
                saw
            );
        };
    }

    #[test]
    fn member_assignment_on_null_throws_typeerror_object() {
        use crate::runner::execute::initialize_test_context;
        let mut ctx = initialize_test_context(false).expect("ctx");
        let script = r#"
            var count = 0;
            var base = null;
            var captured;
            try {
                base.prop = count += 1;
                captured = "no-throw";
            } catch (thrown) {
                captured = [
                    typeof thrown === "object" && thrown !== null,
                    thrown && thrown.constructor && thrown.constructor.name,
                    thrown && thrown.constructor === TypeError,
                ];
            }
            globalThis.__assign_result = captured;
            globalThis.__count_after = count;
        "#;
        ctx.eval(script).expect("script must run");
        let v = ctx.get_global("__assign_result").expect("set");
        let Value::Object(obj) = v else {
            panic!("expected array, got {:?}", v);
        };
        let arr = obj.borrow();
        let is_object = matches!(arr.get("0").unwrap(), Value::Boolean(true));
        let name_val = arr.get("1").unwrap();
        let same_ctor = matches!(arr.get("2").unwrap(), Value::Boolean(true));
        assert!(
            is_object,
            "thrown must be an object (got typeof non-object)"
        );
        let Value::String(name) = name_val else {
            panic!("elem 1 must be string, got {:?}", name_val);
        };
        assert_eq!(name, "TypeError", "thrown must be a TypeError");
        assert!(same_ctor, "thrown.constructor must be TypeError");
        // The right-hand side is evaluated before the assignment throws
        // (per ES PutValue semantics), so the count++ side effect persists.
        let count = ctx.get_global("__count_after").unwrap();
        let Value::Number(n) = count else {
            panic!("count must be a number, got {:?}", count);
        };
        assert_eq!(n, 1.0, "count++ side effect must persist on throw");
    }

    #[test]
    fn harness_string_underscore_native_helpers_are_present() {
        use crate::runner::execute::initialize_test_context;
        let mut ctx = initialize_test_context(false).expect("ctx");
        // The JS String.js builtin layer wraps `String.fromCharCode` /
        // `String.fromCodePoint` with JS functions that call the underscore
        // variants (`String.__fromCharCode`, `String.__fromCodePoint`).
        // If those native helpers are missing, every spec test that uses
        // `String.fromCharCode` blows up with "Cannot read property 'apply'
        // of undefined". This regression pin guards the bootstrap order.
        assert!(
            matches!(ctx.get_global("String"), Some(Value::Object(_))),
            "String global must be an object"
        );
        let r = ctx
            .eval("String.fromCharCode(65)")
            .expect("String.fromCharCode must work");
        assert_eq!(r, Value::String("A".into()));
        let r = ctx
            .eval("String.fromCharCode(65, 66)")
            .expect("String.fromCharCode(2-arg)");
        assert_eq!(r, Value::String("AB".into()));
    }

    #[test]
    fn harness_assert_settable_in_test_context() {
        use crate::runner::execute::initialize_test_context;
        let mut ctx = initialize_test_context(false).expect("ctx");
        let v = ctx.get_global("assert");
        assert!(v.is_some(), "assert must be defined");
        let v = v.unwrap();
        let (Value::Function(_) | Value::NativeFunction(_)) = v else {
            panic!("assert must be a function, got {:?}", v);
        };
        // Try to call assert.sameValue through eval.
        ctx.eval("assert.sameValue(1, 1);")
            .expect("assert.sameValue must be callable");
    }

    #[test]
    fn fn_name_method_test262_case() {
        use crate::harness::HarnessLoader;
        use crate::runner::default_test262_dir;
        use crate::runner::run_single_test;
        let harness = HarnessLoader::new(&default_test262_dir());
        let path = std::path::PathBuf::from(default_test262_dir())
            .join("test/language/statements/class/definition/fn-name-method.js");
        let outcome = run_single_test(&harness, &path);
        assert_eq!(outcome, TestOutcome::Pass, "fn-name-method: {:?}", outcome);
    }

    #[test]
    fn fn_name_method_static_id_via_build_script() {
        use crate::harness::HarnessLoader;
        use crate::runner::default_test262_dir;
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
        use crate::harness::HarnessLoader;
        use crate::runner::default_test262_dir;
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
        use crate::harness::HarnessLoader;
        use crate::metadata::Test262Metadata;
        use crate::runner::default_test262_dir;
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

    // =============================================================================
    // QuenchHost isolation and performance tests
    // =============================================================================

    /// QuenchHost must create a fresh Context per run_script call.
    #[test]
    fn test_quench_host_fresh_context_per_call() {
        let mut host1 = QuenchHost::new();
        let mut host2 = QuenchHost::new();

        // Set a marker on host1's context
        host1.run_script("var __marker = 'host1'").ok();
        // host2 should NOT see host1's marker
        let result = host2.run_script("typeof __marker === 'undefined'");
        assert_eq!(result, Ok(()), "host2 should not see host1's globals");
    }

    /// run_script sets non-strict mode (sloppy eval).
    #[test]
    fn test_quench_host_runs_sloppy() {
        let mut host = QuenchHost::new();
        // Strict mode would reject `with` statement
        let result = host.run_script("with ({}) {}");
        assert_eq!(
            result,
            Ok(()),
            "QuenchHost should run in sloppy mode (with statement allowed)"
        );
    }

    /// run_script runs in non-strict even when called from strict context.
    #[test]
    fn test_quench_host_sloppy_regardless_of_caller_strict() {
        // When QuenchHost::run_script is called, it explicitly sets strict=false
        let mut ctx = Context::new().unwrap();
        quench_runtime::builtins::register_builtins(&mut ctx);
        quench_runtime::interpreter::set_strict_mode(true);
        let prev = quench_runtime::interpreter::is_strict_mode();
        // This simulates what QuenchHost.run_script does internally
        quench_runtime::interpreter::set_strict_mode(false);
        let result = ctx.eval("with ({}) {}");
        quench_runtime::interpreter::set_strict_mode(prev);
        assert_eq!(
            result,
            Ok(quench_runtime::value::Value::Undefined),
            "QuenchHost should set sloppy mode regardless of caller's strictness"
        );
    }

    /// Thrown value from one run_script must not leak to the next.
    #[test]
    fn test_quench_host_thrown_value_isolated_between_calls() {
        let mut host = QuenchHost::new();

        // First call throws
        host.run_script("throw new Error('boom')").unwrap_err();

        // Second call should start clean (no stale thrown value)
        let result = host.run_script("var x = 1; x === 1");
        assert_eq!(
            result,
            Ok(()),
            "second call should start clean after first threw"
        );
    }

    /// Error from run_script is propagated as Err(String).
    #[test]
    fn test_quench_host_error_propagation() {
        let mut host = QuenchHost::new();
        let result = host.run_script("throw new Error('boom')");
        assert!(result.is_err(), "run_script should return Err on throw");
        let err = result.unwrap_err();
        assert!(
            err.contains("Error") || err.contains("boom"),
            "error message should contain the thrown error: {}",
            err
        );
    }

    /// Multiple runs of the same script all pass.
    #[test]
    fn test_quench_host_multiple_runs_same_result() {
        let mut host = QuenchHost::new();
        let script = "var n = (n || 0) + 1; n;";

        for i in 1..=3 {
            let result = host.run_script(script);
            assert_eq!(result, Ok(()), "run {} should succeed", i);
        }
    }

    /// Thrown value is consumed by successful try/catch and not leaked.
    #[test]
    fn test_quench_host_catch_consumes_thrown_value() {
        let mut host = QuenchHost::new();
        host.run_script(
            r#"
            var caught = false;
            try { throw new Error('caught'); } catch(e) { caught = true; }
            caught
            "#,
        )
        .ok();
        // Next run starts clean
        let result = host.run_script("var x = 42; x === 42");
        assert_eq!(result, Ok(()), "next run should be clean");
    }

    /// run_module_script runs ES module code.
    #[test]
    fn test_quench_host_run_module_script() {
        let mut host = QuenchHost::new();
        let result = host.run_module_script("export default 42;");
        assert_eq!(result, Ok(()), "module script should run: {:?}", result);
    }

    /// run_module_script rejects non-module code as error.
    #[test]
    fn test_quench_host_run_module_script_rejects_sloppy() {
        let mut host = QuenchHost::new();
        let result = host.run_module_script("var x = 1;");
        assert_eq!(result, Ok(()));
    }

    /// Test262Error must be properly initialized in QuenchHost context.
    #[test]
    fn test_quench_host_test262_error_initialized() {
        let mut host = QuenchHost::new();
        // Test262Error should be a constructor
        let result =
            host.run_script("var err = new Test262Error('test'); err.name === 'Test262Error'");
        assert_eq!(result, Ok(()), "Test262Error should work: {:?}", result);
    }

    /// Harness globals are available in every QuenchHost run.
    #[test]
    fn test_quench_host_harness_globals_available() {
        let mut host = QuenchHost::new();
        let checks = [
            "typeof assert === 'function'",
            "typeof Test262Error === 'function'",
            "typeof $262 === 'object'",
            "typeof print === 'function'",
            "typeof stop === 'function'",
            "typeof verifyProperty === 'function'",
            "typeof fnGlobalObject === 'function'",
            "typeof isConstructor === 'function'",
        ];

        for check in checks {
            let result = host.run_script(check);
            assert_eq!(
                result,
                Ok(()),
                "'{}' should be available in QuenchHost context",
                check
            );
        }
    }

    /// assert.sameValue and assert.throws work via QuenchHost.
    #[test]
    fn test_quench_host_assert_helpers_work() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "assert.sameValue(1, 1); \
             assert.sameValue(NaN, NaN); \
             assert.sameValue(-0, -0); \
             assert.throws(TypeError, function() { throw new TypeError(); }); \
             'all passed'",
        );
        assert_eq!(
            result,
            Ok(()),
            "assert helpers should work via QuenchHost: {:?}",
            result
        );
    }

    /// MAIN_REALM_TEST262_ERROR is set after QuenchHost::run_script.
    #[test]
    fn test_quench_host_sets_main_realm_host_error() {
        let mut ctx = Context::new().unwrap();
        quench_runtime::builtins::register_builtins(&mut ctx);
        quench_runtime::interpreter::set_strict_mode(false);
        try_inject_harness(&mut ctx).expect("harness ok");
        if let Some(te) = ctx.get_global("Test262Error") {
            quench_runtime::value::error::set_main_realm_host_error(te);
        }
        quench_runtime::interpreter::set_strict_mode(false);

        // Now eval something that throws a Test262Error
        let result = ctx.eval("assert(false, 'msg')");
        assert!(result.is_err(), "assert(false) should throw");

        // The thrown value should be a Test262Error
        let thrown = quench_runtime::value::take_thrown_value();
        assert!(
            thrown.is_some(),
            "thrown value should be set after assert(false)"
        );
    }

    /// createRealm does not pollute the main realm's Test262Error.
    #[test]
    fn test_quench_host_create_realm_preserves_main_test262_error() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            r#"
            // Create a realm with a modified Error
            var realm = $262.createRealm();
            realm.evalScript('Object.prototype.custom = 1;');

            // Main realm's Test262Error should still work
            var err = new Test262Error('main');
            err.name === 'Test262Error' && err.message === 'main'
            "#,
        );
        assert_eq!(
            result,
            Ok(()),
            "createRealm should not break main realm's Test262Error: {:?}",
            result
        );
    }

    #[test]
    fn s12_2_a11_via_run_single_test() {
        use crate::harness::HarnessLoader;
        use crate::runner::{default_test262_dir, run_single_test};
        let harness = HarnessLoader::new(&default_test262_dir());
        let path = std::path::PathBuf::from(default_test262_dir())
            .join("test/language/statements/variable/S12.2_A11.js");
        let source = std::fs::read_to_string(&path).expect("read");
        let mut host = QuenchHost::new();
        let direct_result = host.run_script(&source);
        assert_eq!(
            direct_result,
            Ok(()),
            "direct QuenchHost result: {:?}",
            direct_result
        );
        let outcome = run_single_test(&harness, &path);
        assert_eq!(outcome, TestOutcome::Pass, "S12.2_A11: {:?}", outcome);
    }

    fn locate(message: &str, source: &str) -> Option<usize> {
        let failure = TestFailure::from_message(message);
        let lines: Vec<&str> = source.lines().collect();
        failure.locate_message_in_source(&lines)
    }

    const STAGE0_DEEP_EQUAL_SYNTH: &str = "/*--- description: synth ---*/\n\
                                           var s1 = Symbol();\n\
                                           var s2 = Symbol('foo');\n\
                                           assert.throws(Test262Error, function () { assert.deepEqual(null, 0); });\n\
                                           assert.throws(Test262Error, function () { assert.deepEqual(undefined, 0); });\n\
                                           assert.throws(Test262Error, function () { assert.deepEqual(s1, \"Symbol()\"); });\n";

    #[test]
    fn locate_message_in_source_pins_exact_keyword_match() {
        let source = "var x = 1;\n\
                      var fooBar = 2;\n\
                      var z = 3;\n";
        let lines: Vec<&str> = source.lines().collect();
        let failure = TestFailure::from_message("TypeError: fooBar reference mismatch");
        assert_eq!(failure.locate_message_in_source(&lines), Some(2));
    }

    #[test]
    fn locate_message_in_source_prefers_inner_assertion_in_wrapper() {
        assert_eq!(
            locate(
                "JsError(\"Test262Error: Thrown value was not an object!\")",
                STAGE0_DEEP_EQUAL_SYNTH,
            ),
            Some(6),
            "STAGE0 deepEqual-primitives symptom should point at the inner assert.deepEqual line, not the first assert.throws wrapper",
        );
    }

    #[test]
    fn locate_message_in_source_returns_last_assert_throws_for_wrapper_message() {
        let source = "assert.throws(TypeError, function () { throw 1; });\n\
                      assert.throws(TypeError, function () { throw 2; });\n";
        assert_eq!(
            locate(
                "JsError(\"Test262Error: Thrown value was not an object!\")",
                source,
            ),
            Some(2),
            "when the harness's assert.throws wrapper is the failure surface, the LAST matching wrapper is the best guess — STAGE0 deepEqual-primitives.js symptom",
        );
    }

    #[test]
    fn locate_message_in_source_falls_back_to_first_assertion() {
        let source = "// prologue\n\
                      assert.sameValue(1, 2);\n\
                      assert.deepEqual(3, 4);\n";
        assert_eq!(
            locate("JsError(\"Test262Error: mystery\")", source),
            Some(2),
            "no-keyword fallback should still anchor on an assertion line",
        );
    }

    #[test]
    fn locate_message_in_source_returns_none_when_nothing_matches() {
        let source = "// nothing relevant\n";
        assert_eq!(
            locate(
                "JsError(\"Test262Error: Thrown value was not an object!\")",
                source
            ),
            None,
        );
    }

    #[test]
    fn locate_message_in_source_handles_js_error_wrapper() {
        let source = "var alpha = 1;\n\
                      var definedBeta = 2;\n\
                      alpha();\n";
        assert_eq!(
            locate(
                "JsError(\"TypeError: definedBeta reference undefined\")",
                source
            ),
            Some(2),
            "JsError(\"...\") wrapper must be stripped before keyword extraction",
        );
    }
}
