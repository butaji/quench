//! Native assert helpers (sameValue, throws, compareArray)

use std::rc::Rc;

use crate::value::same_value;
use crate::{JsError, Value};

/// assert.sameValue - SameValue check (NaN equals NaN, +0 != -0)
pub fn assert_same_value(args: Vec<Value>) -> Result<Value, JsError> {
    let a = args.first().cloned().unwrap_or(Value::Undefined);
    let b = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !same_value(&a, &b) {
        let message = args
            .get(2)
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        let msg = format!(
            "sameValue failed: {} !== {} - {}",
            debug_string(&a),
            debug_string(&b),
            message
        );
        let (err_val, js_err) =
            crate::value::error::create_js_error_with_type(&msg, "Test262Error");
        if let Value::Object(o) = &err_val {
            o.borrow_mut()
                .set("name", Value::String("Test262Error".to_string()));
        }
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    Ok(Value::Undefined)
}

fn get_error_name(v: &Value) -> String {
    match v {
        Value::NativeConstructor(nc) => nc.name().to_string(),
        Value::Function(f) => f.name.clone().unwrap_or_default(),
        Value::Object(obj) => obj
            .borrow()
            .get("name")
            .map(|val| crate::value::to_js_string(&val))
            .unwrap_or_default(),
        _ => crate::value::to_js_string(v),
    }
}

/// assert.throws - verifies a function throws the expected error type
pub fn assert_throws(args: Vec<Value>) -> Result<Value, JsError> {
    let expected_ctr = args.first().cloned().unwrap_or(Value::Undefined);
    let fn_value = args.get(1).cloned().unwrap_or(Value::Undefined);
    let message = args
        .get(2)
        .map(crate::value::to_js_string)
        .unwrap_or_default();

    let result = match &fn_value {
        Value::NativeFunction(_)
        | Value::Function(_)
        | Value::Object(_)
        | Value::Generator(_) | Value::Class(_)
        | Value::NativeConstructor(_) => {
            crate::eval::call_value_with_this(fn_value.clone(), vec![], Value::Undefined)
        }
        _ => {
            let msg = "assert.throws: expected a function".to_string();
            let (err_val, js_err) =
                crate::value::error::create_js_error_with_type(&msg, "Test262Error");
            if let Value::Object(o) = &err_val {
                o.borrow_mut()
                    .set("name", Value::String("Test262Error".to_string()));
            }
            crate::value::set_thrown_value(err_val);
            return Err(js_err);
        }
    };

    match result {
        Ok(_) => {
            let msg = format!(
                "Expected {} to be thrown but no exception was thrown. {}",
                get_error_name(&expected_ctr),
                message
            );
            let (err_val, js_err) =
                crate::value::error::create_js_error_with_type(&msg, "Test262Error");
            if let Value::Object(o) = &err_val {
                o.borrow_mut()
                    .set("name", Value::String("Test262Error".to_string()));
            }
            crate::value::set_thrown_value(err_val);
            Err(js_err)
        }
        Err(js_err) => {
            let thrown = match crate::value::get_thrown_value() {
                Some(v) => v,
                None => {
                    let msg = &js_err.0;
                    let err_type = msg.split(':').next().unwrap_or("Error");
                    let (err_val, _) =
                        crate::value::error::create_js_error_with_type(&js_err.0, err_type);
                    crate::value::set_thrown_value(err_val.clone());
                    err_val
                }
            };

            let check_result = check_error_instance(&thrown, &expected_ctr);
            if check_result {
                crate::value::take_thrown_value();
                Ok(Value::Undefined)
            } else {
                // Create Test262Error with original as cause, WITHOUT overwriting thrown value.
                // Old code called set_thrown_value(err_val) which replaced the original,
                // breaking cross-realm tests that check the original error's constructor.
                let msg = if get_error_name(&thrown) == get_error_name(&expected_ctr) {
                    format!(
                        "Expected {} but got a different error constructor with the same name. {}",
                        get_error_name(&expected_ctr),
                        message
                    )
                } else {
                    format!(
                        "Expected {} but got {}. {}",
                        get_error_name(&expected_ctr),
                        get_error_name(&thrown),
                        message
                    )
                };
                let (err_val, js_err) =
                    crate::value::error::create_js_error_with_type(&msg, "Test262Error");
                if let Value::Object(o) = &err_val {
                    o.borrow_mut()
                        .set("name", Value::String("Test262Error".to_string()));
                    // Attach original error as cause so cross-realm tests can inspect it
                    o.borrow_mut().set("cause", thrown.clone());
                }
                // Set thrown value to the Test262Error so eval_try_catch's take_thrown_value()
                // returns the Test262Error (not the original cross-realm error still sitting
                // in the thread-local). The cause property still carries the original error
                // for cross-realm tests that need to inspect it.
                crate::value::set_thrown_value(err_val.clone());
                Err(js_err)
            }
        }
    }
}

/// Check if thrown error matches the expected constructor per assert.throws spec.
///
/// Algorithm: walks expected.prototype chain, checking thrown.constructor at each level.
/// This correctly distinguishes same-named constructors from different realms via
/// pointer identity. No name-based fallback — same name ≠ same constructor.
fn check_error_instance(thrown: &Value, expected: &Value) -> bool {
    // Get thrown's .constructor property
    let thrown_ctor = match thrown {
        Value::Object(o) => {
            let c = o.borrow().get("constructor");
            if c.is_none() {
                return false;
            }
            c.unwrap()
        }
        Value::Function(f) => {
            // JS functions store properties in their properties HashMap
            f.get_property("constructor").unwrap_or(Value::Undefined)
        }
        Value::NativeFunction(f) => Value::NativeFunction(Rc::clone(f)),
        Value::NativeConstructor(f) => Value::NativeConstructor(Rc::clone(f)),
        _ => return false,
    };

    // Null/undefined constructor cannot match
    if matches!(&thrown_ctor, Value::Undefined | Value::Null) {
        return false;
    }

    // Level 0: thrown.constructor === expected (identity check)
    if ptr_eq_value(&thrown_ctor, expected) {
        return true;
    }

    // Level 1: Walk expected.prototype's [[Prototype]] chain for inheritance match Walk expected.prototype's [[Prototype]] chain for inheritance match
    let mut current = get_prototype_from_function(expected);
    while let Some(Value::Object(obj)) = current {
        // Check constructor at this level
        let ctor = obj.borrow().get("constructor");
        if let Some(c) = ctor {
            if ptr_eq_value(&thrown_ctor, &c) {
                return true;
            }
        }
        // Move to next prototype before dropping obj
        let next_proto = obj.borrow().prototype.clone();
        drop(obj);
        current = next_proto.map(Value::Object);
    }

    false
}

fn get_prototype_from_function(f: &Value) -> Option<Value> {
    match f {
        Value::NativeConstructor(nc) => Some(Value::Object(Rc::clone(&nc.prototype))),
        Value::Function(vf) => Some(Value::Object(vf.get_prototype())),
        _ => None,
    }
}

/// Compare two Values for function identity, handling Value::Object vs Value::Function
/// wrapping the same underlying function.
fn ptr_eq_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(o_a), Value::Object(o_b)) => Rc::ptr_eq(o_a, o_b),
        (Value::Function(f_a), Value::Function(f_b)) => f_a.identity_ptr() == f_b.identity_ptr(),
        (Value::NativeFunction(f_a), Value::NativeFunction(f_b)) => {
            Rc::ptr_eq(&f_a.func, &f_b.func)
        }
        (Value::NativeConstructor(f_a), Value::NativeConstructor(f_b)) => {
            Rc::ptr_eq(f_a.func_rc(), f_b.func_rc())
        }
        _ => false,
    }
}

fn is_primitive(v: &Value) -> bool {
    matches!(
        v,
        Value::Undefined
            | Value::Null
            | Value::Boolean(_)
            | Value::Number(_)
            | Value::String(_)
            | Value::Symbol(_)
    )
}

fn get_array_elements(arr: &Value) -> Option<Vec<Value>> {
    match arr {
        Value::Object(obj) => {
            let obj = obj.borrow();
            let len = obj.get("length")?;
            let len = match len {
                Value::Number(n) => n as usize,
                _ => return None,
            };
            Some(
                (0..len)
                    .map(|i| obj.get(&i.to_string()).unwrap_or(Value::Undefined))
                    .collect(),
            )
        }
        _ => None,
    }
}

fn fmt_array(arr: &[Value]) -> String {
    let parts: Vec<String> = arr.iter().map(crate::value::to_js_string).collect();
    format!("[{}]", parts.join(", "))
}

/// assert.compareArray - verifies two arrays have same elements (SameValue)
pub fn assert_compare_array(args: Vec<Value>) -> Result<Value, JsError> {
    let actual = args.first().cloned().unwrap_or(Value::Undefined);
    let expected = args.get(1).cloned().unwrap_or(Value::Undefined);
    let message = args
        .get(2)
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    let mk_err = |msg: String| -> Result<Value, JsError> {
        let (err_val, js_err) =
            crate::value::error::create_js_error_with_type(&msg, "Test262Error");
        if let Value::Object(o) = &err_val {
            o.borrow_mut()
                .set("name", Value::String("Test262Error".to_string()));
        }
        crate::value::set_thrown_value(err_val);
        Err(js_err)
    };
    if is_primitive(&actual) {
        return mk_err(format!(
            "Actual argument [{}] shouldn't be primitive. {}",
            debug_string(&actual),
            message
        ));
    }
    if is_primitive(&expected) {
        return mk_err(format!(
            "Expected argument [{}] shouldn't be primitive. {}",
            debug_string(&expected),
            message
        ));
    }
    let actual_elems = get_array_elements(&actual)
        .ok_or_else(|| JsError("Actual is not array-like".to_string()))?;
    let expected_elems = get_array_elements(&expected)
        .ok_or_else(|| JsError("Expected is not array-like".to_string()))?;
    if actual_elems.len() != expected_elems.len() {
        return mk_err(format!(
            "Actual {} and expected {} should have the same contents. {}",
            fmt_array(&actual_elems),
            fmt_array(&expected_elems),
            message
        ));
    }
    for i in 0..actual_elems.len() {
        if !same_value(&actual_elems[i], &expected_elems[i]) {
            return mk_err(format!(
                "Actual {} and expected {} should have same contents. {}",
                fmt_array(&actual_elems),
                fmt_array(&expected_elems),
                message
            ));
        }
    }
    Ok(Value::Undefined)
}

pub fn debug_string(v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", s),
        Value::Object(_) | Value::Generator(_) => "[object]".to_string(),
        Value::Function(_) => "[Function]".to_string(),
        Value::NativeFunction(_) => "[NativeFunction]".to_string(),
        Value::NativeConstructor(_) => "[NativeConstructor]".to_string(),
        Value::Symbol(s) => format!("Symbol({})", s.desc.as_deref().unwrap_or("")),
        Value::Class(_) => "[Class]".to_string(),
        Value::BigInt(bi) => format!("{}n", bi),
    }
}

#[cfg(test)]
mod tests {
    use crate::test262::harness::try_inject_harness;
    use crate::test262::host::Test262Host;
    use crate::Value;

    fn harness_ctx() -> crate::Context {
        let mut ctx = crate::Context::new().unwrap();
        try_inject_harness(&mut ctx).unwrap();
        ctx
    }

    /// Direct test: assert(false) must throw a Test262Error
    #[test]
    fn test_assert_false_throws() {
        let mut ctx = harness_ctx();
        match ctx.eval("assert(false)") {
            Ok(v) => panic!("assert(false) should throw but returned: {:?}", v),
            Err(_e) => {}
        }
    }

    /// Direct test: assert(false) in try/catch
    #[test]
    fn test_assert_false_in_try_catch() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            var threw = false;
            try {
              assert(false);
            } catch(err) {
              threw = true;
            }
            threw
        "#,
        );
        let _ = result;
    }

    /// Mimic QuenchHost::run_script exactly to see if assert(false) throws
    #[test]
    fn test_quench_host_style_assert_false() {
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let prev_strict = crate::interpreter::is_strict_mode();
        crate::interpreter::set_strict_mode(false);
        try_inject_harness(&mut ctx).expect("harness should load");
        crate::interpreter::set_strict_mode(prev_strict);
        crate::interpreter::set_strict_mode(false);

        // Direct assert(false) call
        match ctx.eval("assert(false)") {
            Ok(v) => panic!(
                "assert(false) via QuenchHost-style should throw but returned: {:?}",
                v
            ),
            Err(_e) => {}
        }

        // try/catch variant
        let _result = ctx.eval(
            r#"
            var threw = false;
            try {
              assert(false);
            } catch(err) {
              threw = true;
            }
            threw
        "#,
        );
    }

    /// What is `err` in the catch block when running the built script?
    /// Compare: with frontmatter vs without.
    #[test]
    fn test_catch_err_value_with_and_without_frontmatter() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let test262_dir = manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/test262");
        let harness = crate::test262::HarnessLoader::new(test262_dir.to_str().unwrap());

        let test_path = test262_dir.join("test/harness/assert-false.js");
        let source = std::fs::read_to_string(&test_path).unwrap();
        let script = harness.build_script(&source, &[]).unwrap();

        // Test WITH frontmatter (built script)
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        crate::interpreter::set_strict_mode(false);
        try_inject_harness(&mut ctx).expect("harness");
        let _r1 = ctx.eval(&script);

        // Test WITHOUT frontmatter (strip it manually)
        let source_no_fm = if let Some(s) = source.find("/*---") {
            if let Some(e) = source[s..].find("---*/") {
                let end = s + e + 5;
                format!("{}{}", &source[..s], &source[end..])
            } else {
                source.clone()
            }
        } else {
            source.clone()
        };
        let mut ctx2 = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx2);
        crate::interpreter::set_strict_mode(false);
        try_inject_harness(&mut ctx2).expect("harness");
        let _r2 = ctx2.eval(&source_no_fm);

        // Also test: inline with $debug
        let mut ctx3 = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx3);
        crate::interpreter::set_strict_mode(false);
        try_inject_harness(&mut ctx3).expect("harness");
        let r3 = ctx3.eval(
            r#"
var threw = false;
var errType = 'NOT_SET';
try {
  assert(false);
} catch(err) {
  threw = true;
  errType = typeof err;
  $debug = errType + ':' + (err === undefined ? 'UNDEF' : 'DEFINED');
}
threw + ',' + errType
"#,
        );
        let _ = r3;
    }

    /// Minimal reproduction: throw in try, check catch param in handler.
    /// This tests the full pipeline (parse → eval_try_catch) without harness.
    #[test]
    fn test_minimal_throw_catch_binding() {
        let mut ctx = crate::Context::new().unwrap();
        // Test 1: throw 42, catch(err) { return err; }
        let r1 = ctx.eval(
            r#"
            var result = 'NOT_SET';
            try {
                throw 42;
            } catch(err) {
                result = err;
            }
            result
            "#,
        );
        assert!(r1.is_ok(), "should not throw: {:?}", r1);
        assert_eq!(r1.unwrap(), crate::Value::Number(42.0));

        // Test 2: throw Error, catch(err) { return typeof err; }
        let r2 = ctx.eval(
            r#"
            var result = 'NOT_SET';
            try {
                throw new Error('boom');
            } catch(err) {
                result = typeof err;
            }
            result
            "#,
        );
        assert!(r2.is_ok(), "should not throw: {:?}", r2);
        assert_eq!(r2.unwrap(), crate::Value::String("object".to_string()));
    }

    /// With harness: does assert(false) throw, and does catch see it?
    #[test]
    fn test_assert_false_throws_and_catch_sees_error() {
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        crate::interpreter::set_strict_mode(false);
        try_inject_harness(&mut ctx).expect("harness");

        // Test: assert(false) throws, catch(err) { return typeof err; }
        let r = ctx.eval(
            r#"
            var result = 'NOT_SET';
            try {
                assert(false);
            } catch(err) {
                result = typeof err;
                $debug = 'catch saw: ' + (typeof err) + ' (isUndefined=' + (err === undefined) + ')';
            }
            result
            "#,
        );
        let _ = ctx.eval("$debug");

        // assert(false) should throw, and catch should see an object (the Test262Error)
        assert!(
            r.is_ok(),
            "outer try-catch should catch assert(false): {:?}",
            r
        );
        // result should be 'object', not 'NOT_SET' or anything else
        let v = r.unwrap();
        assert_eq!(
            v,
            crate::Value::String("object".to_string()),
            "catch should see error object, got: {:?}",
            v
        );
    }

    /// Verify build_script prepends harness files to test source (with frontmatter intact).
    #[test]
    fn test_build_script_includes_harness() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let test262_dir = manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/test262");
        let harness = crate::test262::HarnessLoader::new(test262_dir.to_str().unwrap());

        let test_path = test262_dir.join("test/harness/assert-false.js");
        let source = std::fs::read_to_string(&test_path).unwrap();

        // Source has frontmatter
        assert!(
            source.contains("/*---") && source.contains("---*/"),
            "assert-false.js should have frontmatter"
        );

        // build_script prepends harness includes; frontmatter stays in source (it's a JS comment)
        let script = harness
            .build_script(&source, &["assert.js".to_string()])
            .unwrap();
        assert!(
            script.contains("function assert"),
            "built script should include assert.js"
        );
    }

    /// Reproduce the EXACT runner path: HarnessLoader.build_script + QuenchHost.run_script
    #[test]
    fn test_runner_path_assert_false() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let test262_dir = manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/test262");
        let harness = crate::test262::HarnessLoader::new(test262_dir.to_str().unwrap());

        let test_path = test262_dir.join("test/harness/assert-false.js");
        let source = std::fs::read_to_string(&test_path).unwrap();

        // Parse includes from frontmatter (same as runner) - assert-false.js has NO includes!
        let includes: Vec<String> = vec![];
        let script = harness
            .build_script(&source, &includes)
            .expect("build_script should succeed");

        // Run via QuenchHost (exactly like runner)
        let mut host = crate::test262::QuenchHost::new();
        let result = host.run_script(&script);
        assert!(
            result.is_ok(),
            "assert-false.js should pass via runner path, got: {:?}",
            result
        );
    }

    /// Debug: exactly replicate QuenchHost setup step by step, then eval the script
    #[test]
    fn test_replicate_quench_host_step_by_step() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let test262_dir = manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/test262");
        let harness = crate::test262::HarnessLoader::new(test262_dir.to_str().unwrap());

        let test_path = test262_dir.join("test/harness/assert-false.js");
        let source = std::fs::read_to_string(&test_path).unwrap();
        let includes: Vec<String> = vec![];
        let script = harness
            .build_script(&source, &includes)
            .expect("build_script should succeed");

        // Step 1: fresh context
        let mut ctx = crate::Context::new().expect("Context::new");
        // Step 2: register_builtins
        crate::builtins::register_builtins(&mut ctx);
        // Step 3: try_inject_harness
        crate::test262::harness::try_inject_harness(&mut ctx).expect("harness");
        // Step 4: set_strict_mode(false)
        crate::interpreter::set_strict_mode(false);
        // Step 5: eval
        let r = ctx.eval(&script);
        assert!(r.is_ok(), "step-by-step should pass, got {:?}", r);

        // Compare: same script but WITHOUT frontmatter comment
        let inline_script = r#"
var threw = false;
try {
  assert(false);
} catch(err) {
  threw = true;
  if (err.constructor !== Test262Error) {
    throw new Error(
      'Expected a Test262Error, but a "' + err.constructor.name +
      '" was thrown.'
    );
  }
}
if (threw === false) {
  throw new Error('Expected a Test262Error, but no error was thrown.');
}
"#;
        let r2 = ctx.eval(inline_script);
        assert!(r2.is_ok(), "inline should pass, got {:?}", r2);
    }

    /// Verify assert(false) throws and catch sees Test262Error via QuenchHost path
    #[test]
    fn test_quench_host_exact_v2() {
        let script = r#"
var threw = false;
try {
  assert(false);
} catch(err) {
  threw = true;
  if (err && err.constructor !== Test262Error) {
    throw new Error(
      'Expected a Test262Error, but a "' + err.constructor.name +
      '" was thrown.'
    );
  }
}
if (threw === false) {
  throw new Error('Expected a Test262Error, but no error was thrown.');
}
"#;
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        crate::interpreter::set_strict_mode(false);
        try_inject_harness(&mut ctx).expect("harness loads");
        crate::interpreter::set_strict_mode(false);
        let result = ctx.eval(script);
        assert!(
            result.is_ok(),
            "QuenchHost path should pass, got {:?}",
            result
        );
    }

    /// Verify thrown_value is consumed by JS try/catch and not leaked between evals
    #[test]
    fn test_thrown_value_consumed_by_js_catch() {
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        crate::interpreter::set_strict_mode(false);
        try_inject_harness(&mut ctx).expect("harness loads");

        // No thrown value before
        let before = crate::value::take_thrown_value();
        assert!(before.is_none(), "no thrown value before eval");

        // assert(false) throws, but JS try/catch consumes it
        let result = ctx.eval(
            r#"
            var threw = false;
            var caughtType = 'NOT_CAUGHT';
            try {
              assert(false);
            } catch(err) {
              threw = true;
              caughtType = typeof err;
            }
            threw + ',' + caughtType
        "#,
        );
        assert!(result.is_ok(), "eval should succeed: {:?}", result);
        assert_eq!(
            result.unwrap(),
            crate::Value::String("true,object".to_string())
        );

        // thrown_value should be consumed by JS catch
        let after = crate::value::take_thrown_value();
        assert!(
            after.is_none(),
            "thrown value should be consumed: {:?}",
            after
        );
    }

    #[test]
    fn test_check_error_instance_error_vs_typeerror() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            assert.throws(Error, function() { throw new TypeError() })
        "#,
        );
        assert!(
            result.is_err(),
            "Error expected TypeError, should have failed"
        );
    }

    #[test]
    fn test_check_error_instance_same_type() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            assert.throws(TypeError, function() { throw new TypeError() })
        "#,
        );
        assert!(
            result.is_ok(),
            "TypeError expected TypeError, should have passed, got: {:?}",
            result
        );
    }

    #[test]
    fn test_check_error_instance_local_ctor() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            (function() {
                function TypeError() {}
                assert.throws(TypeError, function() { throw new TypeError() })
            })()
        "#,
        );
        assert!(
            result.is_ok(),
            "local TypeError should match local TypeError"
        );
    }

    #[test]
    fn test_check_error_instance_local_vs_global() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            (function() {
                function TypeError() {}
                assert.throws(TypeError, function() {
                    throw new globalThis.TypeError()
                })
            })()
        "#,
        );
        assert!(
            result.is_err(),
            "local TypeError should NOT match global TypeError"
        );
    }

    #[test]
    fn test_throws_cross_realm_typeerror() {
        // assert.throws(TypeError, fn) where fn throws a cross-realm TypeError
        // should FAIL because cross-realm TypeError !== local TypeError (different identity)
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            var realmGlobal = $262.createRealm().global;
            assert.throws(TypeError, function() {
                throw new realmGlobal.TypeError()
            })
        "#,
        );
        assert!(
            result.is_err(),
            "cross-realm TypeError should NOT match local TypeError, got: {:?}",
            result
        );
    }

    #[test]
    fn test_throws_same_realm_typeerror() {
        // assert.throws(TypeError, fn) where fn throws a same-realm TypeError
        // should PASS because they're the exact same constructor
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            assert.throws(TypeError, function() {
                throw new TypeError()
            })
        "#,
        );
        assert!(
            result.is_ok(),
            "same-realm TypeError should match, got: {:?}",
            result
        );
    }

    #[test]
    fn test_create_realm_works() {
        // Verify $262.createRealm() actually creates a separate realm
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            var realmGlobal = $262.createRealm().global;
            var realmTypeError = realmGlobal.TypeError;
            var localTypeError = TypeError;
            // They should be different objects
            realmTypeError !== localTypeError
        "#,
        );
        assert!(result.is_ok(), "createRealm check failed: {:?}", result);
        let val = result.unwrap();
        assert!(
            matches!(val, Value::Boolean(true)),
            "realm TypeError should differ from local TypeError, got {:?}",
            val
        );
    }

    #[test]
    fn test_error_constructor_on_instance() {
        // Verify that thrown error objects have a constructor property
        // that matches the global TypeError (identity check)
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            var err = new TypeError("test");
            // err.constructor should be the same as globalThis.TypeError
            var globalTypeError = globalThis.TypeError;
            err.constructor === globalTypeError
        "#,
        );
        assert!(result.is_ok(), "check failed: {:?}", result);
        let val = result.unwrap();
        assert!(
            matches!(val, Value::Boolean(true)),
            "err.constructor should equal globalThis.TypeError, got {:?}",
            val
        );
    }

    /// Verify that the cross-realm TypeError thrown inside assert.throws
    /// is NOT matched by the local TypeError (assert.throws must fail).
    #[test]
    fn test_assert_throws_cross_realm_typeerror_is_rejected() {
        let mut ctx = harness_ctx();
        // Just the assert.throws call — should FAIL (return Err)
        let result = ctx.eval(
            r#"
            var realmGlobal = $262.createRealm().global;
            assert.throws(TypeError, function() {
              throw new realmGlobal.TypeError();
            });
        "#,
        );
        assert!(
            result.is_err(),
            "assert.throws(cross-realm TypeError) should fail, got: {:?}",
            result
        );
    }

    /// Reproduce assert-throws-same-realm.js: when assert.throws(TypeError, fn) fails
    /// because fn throws a cross-realm TypeError (different constructor), assert.throws
    /// throws a Test262Error. The outer catch catches it, and err.constructor === Test262Error
    /// (not Error). So the outer catch completes normally.
    #[test]
    fn test_assert_throws_failure_wraps_as_test262_error() {
        let mut ctx = harness_ctx();

        // Step 1: Verify that `throw new Test262Error(...)` produces an object with
        // Test262Error.constructor === Test262Error (pointer identity).
        let constructor_check = ctx.eval(
            r#"
            var err = new Test262Error('test');
            err.constructor === Test262Error
        "#,
        );
        assert!(
            matches!(constructor_check, Ok(Value::Boolean(true))),
            "new Test262Error().constructor === Test262Error should be true, got: {:?}",
            constructor_check
        );

        // Step 2: Verify that assert.throws(TypeError, fn) throws when fn throws cross-realm TypeError.
        let throws_result = ctx.eval(
            r#"
            var realmGlobal = $262.createRealm().global;
            try {
              assert.throws(TypeError, function() {
                throw new realmGlobal.TypeError();
              });
              'no throw'
            } catch (e) {
              'caught: ' + e.name
            }
        "#,
        );
        assert!(
            matches!(throws_result, Ok(Value::String(ref s)) if s.contains("Test262Error")),
            "assert.throws should throw Test262Error when fn throws cross-realm TypeError. Got: {:?}",
            throws_result
        );

        // Test that assert.throws throws a Test262Error for cross-realm errors.
        // When a function throws a TypeError from a different realm, assert.throws
        // cannot verify the instanceof relationship, so it throws a Test262Error.
        // The wrapped error's .constructor must be the main realm's Test262Error.
        let result = ctx.eval(
            r#"
            var realmGlobal = $262.createRealm().global;
            var wrapped = null;

            try {
              assert.throws(TypeError, function() {
                throw new realmGlobal.TypeError();
              });
              'no throw'
            } catch (err) {
              wrapped = err;
              // Re-throw so we can inspect in Rust
              throw err;
            }

            // Should not reach here
            'unexpected'
        "#,
        );
        assert!(
            result.is_err(),
            "assert.throws should throw for cross-realm TypeError"
        );

        // Verify the wrapped error has the correct constructor
        let ctor_check = ctx.eval(
            r#"
            var realmGlobal = $262.createRealm().global;
            var ctorIsCorrect = false;

            try {
              assert.throws(TypeError, function() {
                throw new realmGlobal.TypeError();
              });
            } catch (err) {
              ctorIsCorrect = (err.constructor === Test262Error);
            }

            ctorIsCorrect
        "#,
        );
        assert!(
            matches!(ctor_check, Ok(Value::Boolean(true))),
            "wrapped error's .constructor should be Test262Error, got: {:?}",
            ctor_check
        );
    }

    /// What object is actually returned by the assert.throws function itself?
    #[test]
    fn test_assert_throws_returns_undefined_on_success() {
        let mut ctx = harness_ctx();
        // assert.throws with matching error returns undefined
        let result = ctx.eval(
            r#"
            var ret = assert.throws(TypeError, function() { throw new TypeError(); });
            ret === undefined
        "#,
        );
        assert!(result.is_ok(), "check failed: {:?}", result);
    }

    /// Verify Test262Error produces objects with correct constructor identity
    #[test]
    fn test_test262_error_constructor_identity() {
        let mut ctx = harness_ctx();
        let result = ctx
            .eval(
                r#"
            var e = new Test262Error("hello");
            JSON.stringify({
                constructorIsSame: e.constructor === Test262Error,
                constructorName: e.constructor ? e.constructor.name : "null",
                errName: e.name,
                errMessage: e.message
            })
        "#,
            )
            .unwrap();
        let expected = r#"{"constructorIsSame":true,"constructorName":"Test262Error","errName":"Test262Error","errMessage":"hello"}"#;
        assert_eq!(result, crate::Value::String(expected.to_string()));
    }

    /// Test Promise.then chaining with async functions - mimics asyncHelpers-asyncTest-returns-undefined.js
    #[test]
    fn test_async_function_promise_chaining() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let test262_dir = manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/test262");
        let harness = crate::test262::HarnessLoader::new(test262_dir.to_str().unwrap());

        // Same prelude the runner adds for async tests
        let prelude = r#"var $DONE = function(error) { if (error !== undefined && error !== null) throw error; };
"#;
        // Simplified test
        let test_script = r#"
var doneCalls = 0;
var realDone = $DONE;
globalThis.$DONE = function () { doneCalls++; };

(async function () {
  asyncTest({});
})()
  .then(() => { doneCalls; })
  .then(realDone, realDone);
"#;
        let script = format!(
            "{}{}",
            prelude,
            harness
                .build_script(test_script, &["asyncHelpers.js".to_string()])
                .expect("build_script should succeed")
        );

        let mut host = crate::test262::QuenchHost::new();
        let result = host.run_script(&script);
        assert!(result.is_ok(), "async test should pass, got: {:?}", result);
    }

    /// Test: what happens when asyncTest({}) is called?
    #[test]
    fn test_async_test_with_non_function() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let test262_dir = manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/test262");
        let harness = crate::test262::HarnessLoader::new(test262_dir.to_str().unwrap());

        let prelude = r#"var $DONE = function(error) { if (error !== undefined && error !== null) throw error; };
"#;
        let test_script = r#"
var doneCalls = 0;
var realDone = $DONE;
globalThis.$DONE = function () { doneCalls++; };

// Direct call to asyncTest({})
var ret = asyncTest({});
"#;
        let script = format!(
            "{}{}",
            prelude,
            harness
                .build_script(test_script, &["asyncHelpers.js".to_string()])
                .expect("build_script should succeed")
        );

        let mut host = crate::test262::QuenchHost::new();
        let result = host.run_script(&script);
        assert!(
            result.is_ok(),
            "asyncTest({{}}) should work, got: {:?}",
            result
        );
    }

    /// Test: asyncTest({}) inside an async function
    #[test]
    fn test_async_test_inside_async_fn() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let test262_dir = manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/test262");
        let harness = crate::test262::HarnessLoader::new(test262_dir.to_str().unwrap());

        let prelude = r#"var $DONE = function(error) { if (error !== undefined && error !== null) throw error; };
"#;
        let test_script = r#"
var doneCalls = 0;
var realDone = $DONE;
globalThis.$DONE = function () { doneCalls++; };

// asyncTest({}) inside async function
(async function() {
  asyncTest({});
})();
"#;
        let script = format!(
            "{}{}",
            prelude,
            harness
                .build_script(test_script, &["asyncHelpers.js".to_string()])
                .expect("build_script should succeed")
        );

        let mut host = crate::test262::QuenchHost::new();
        let result = host.run_script(&script);
        assert!(
            result.is_ok(),
            "asyncTest inside async fn should work, got: {:?}",
            result
        );
    }
}
