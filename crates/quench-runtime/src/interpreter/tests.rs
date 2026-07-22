//! Unit tests for interpreter helpers.

#[allow(unused_imports)]
use crate::interpreter::has_legacy_octal;
use crate::interpreter::{self, ControlFlow};
use crate::value::Value;

#[test]
fn test_reset_depth() {
    crate::interpreter::reset_depth();
}

#[test]
fn test_has_legacy_octal() {
    assert!(has_legacy_octal("01"), "01 is legacy octal");
    assert!(has_legacy_octal("07"), "07 is legacy octal");
    assert!(!has_legacy_octal("0x1"), "0x1 is hex, not octal");
    assert!(!has_legacy_octal("0X1"), "0X1 is hex, not octal");
    assert!(!has_legacy_octal("0b1"), "0b1 is binary, not octal");
    assert!(!has_legacy_octal("0B1"), "0B1 is binary, not octal");
    assert!(!has_legacy_octal("0o1"), "0o1 is octal, not legacy");
    assert!(!has_legacy_octal("0O1"), "0O1 is octal, not legacy");
    assert!(!has_legacy_octal("0n"), "0n is bigint, not octal");
    assert!(has_legacy_octal("a = 01;"), "a = 01; has octal");
    assert!(
        has_legacy_octal("\"use strict\";\na = 01;"),
        "with strict prefix"
    );
    assert!(
        !has_legacy_octal("\"use strict\";\nvar threw = false;"),
        "strict source, no octal"
    );
    // Copyright year must NOT be flagged
    assert!(
        !has_legacy_octal("// Copyright (C) 2015 the V8 project authors."),
        "copyright 2015"
    );
    assert!(
        !has_legacy_octal("// Copyright (C) 2016 the V8 project authors."),
        "copyright 2016"
    );
    // Numbers with embedded 01/07 are not octals
    assert!(!has_legacy_octal("var x = 2015;"), "2015 not octal");
    assert!(!has_legacy_octal("var x = 1007;"), "1007 not octal");
    assert!(!has_legacy_octal("var x = 1.07;"), "1.07 not octal");
    assert!(!has_legacy_octal("var x = 0.07;"), "0.07 not octal");
    // Strings with embedded octals must not be flagged
    assert!(
        !has_legacy_octal(r#"assert.sameValue(decimalToHexString(1), "0001");"#),
        "0001 string"
    );
    assert!(
        !has_legacy_octal(r#"var hex = "0123456789ABCDEF";"#),
        "hex string"
    );
    assert!(
        !has_legacy_octal(r#"assert.sameValue(decimalToPercentHexString(1), "%01");"#),
        "%01 string"
    );
    // Full test source
    assert!(
        !has_legacy_octal(
            r#""use strict";
function decimalToHexString(n) {
  var hex = "0123456789ABCDEF";
  return "%" + hex[(n >> 4) & 0xf] + hex[n & 0xf];
}
assert.sameValue(decimalToHexString(1), "0001");
assert.sameValue(decimalToPercentHexString(1), "%01");"#
        ),
        "decimalToHexString.js"
    );
    // Template literal with embedded 0 — not an octal
    assert!(
        !has_legacy_octal(r#"var s = `prefix 01 suffix`;"#),
        "template literal"
    );
    // Block comment with embedded 0 — not an octal
    assert!(
        !has_legacy_octal(r#"/* octal: 01 in comment */"#),
        "block comment"
    );
    // Regex literal with \u02C1
    assert!(
        !has_legacy_octal(
            r#""use strict";
var UnicodeIDStart = /[a-zA-Z\xF6\xF8-\u02C1]/u;"#
        ),
        "regex with \\u02C1"
    );
    // [native code] matcher
    assert!(
        !has_legacy_octal(
            r#""use strict";
var re = /\[native code\]/"#
        ),
        "native code regex"
    );
    // UTF-8 multi-byte
    assert!(
        !has_legacy_octal("var _\u{0AFA}\u{0AFB}\u{0AFC};"),
        "UTF-8 multi-byte"
    );
    // Regex char class 01
    assert!(
        !has_legacy_octal(r#"var re = /[01]/"#),
        "regex char class 01"
    );
    // Non-octal decimals (08, 09, 018, etc.)
    assert!(!has_legacy_octal("08"), "08 not octal");
    assert!(!has_legacy_octal("09"), "09 not octal");
    assert!(!has_legacy_octal("018"), "018 not octal");
    assert!(!has_legacy_octal("019"), "019 not octal");
    assert!(
        !has_legacy_octal("assert.sameValue(08, 8);"),
        "08 in assert"
    );
    assert!(
        !has_legacy_octal("assert.sameValue(018, 18);"),
        "018 in assert"
    );
    // Numeric separators
    assert!(!has_legacy_octal("var x = 00_01;"), "00_01 not octal");
    assert!(
        !has_legacy_octal("assert.sameValue(10.00_01e2, 10.0001e2);"),
        "10.00_01e2 not octal"
    );
    // Actual octals must be detected
    assert!(has_legacy_octal("var x = 01;"), "01 in code is octal");
    assert!(
        has_legacy_octal("assert.sameValue(01, 1);"),
        "01 in assert is octal"
    );
}

// ---------------------------------------------------------------------------
// Control flow tests
// ---------------------------------------------------------------------------

#[test]
fn test_control_flow_break() {
    interpreter::set_control_flow(ControlFlow::Break);
    assert_eq!(interpreter::take_control_flow(), Some(ControlFlow::Break));
    // Second take returns None (consumed)
    assert_eq!(interpreter::take_control_flow(), None);
}

#[test]
fn test_control_flow_continue() {
    interpreter::set_control_flow(ControlFlow::Continue(None));
    assert_eq!(
        interpreter::take_control_flow(),
        Some(ControlFlow::Continue(None))
    );
    assert_eq!(interpreter::take_control_flow(), None);
}

#[test]
fn test_control_flow_return() {
    let val = Value::Number(42.0);
    interpreter::set_control_flow(ControlFlow::Return(val));
    let result = interpreter::take_control_flow();
    assert!(result.is_some());
    match result.unwrap() {
        ControlFlow::Return(v) => assert_eq!(v, Value::Number(42.0)),
        other => panic!("expected Return, got {:?}", other),
    }
    assert_eq!(interpreter::take_control_flow(), None);
}

#[test]
fn test_control_flow_return_undefined() {
    interpreter::set_control_flow(ControlFlow::Return(Value::Undefined));
    let result = interpreter::take_control_flow();
    match result.unwrap() {
        ControlFlow::Return(v) => assert_eq!(v, Value::Undefined),
        other => panic!("expected Return(Undefined), got {:?}", other),
    }
}

#[test]
fn test_control_flow_return_null() {
    interpreter::set_control_flow(ControlFlow::Return(Value::Null));
    let result = interpreter::take_control_flow();
    match result.unwrap() {
        ControlFlow::Return(v) => assert_eq!(v, Value::Null),
        other => panic!("expected Return(Null), got {:?}", other),
    }
}

#[test]
fn test_control_flow_return_boolean() {
    interpreter::set_control_flow(ControlFlow::Return(Value::Boolean(true)));
    let result = interpreter::take_control_flow();
    match result.unwrap() {
        ControlFlow::Return(v) => assert_eq!(v, Value::Boolean(true)),
        other => panic!("expected Return(true), got {:?}", other),
    }
}

#[test]
fn test_control_flow_no_flow_take_returns_none() {
    // Ensure no leftover from previous tests
    let _ = interpreter::take_control_flow();
    assert_eq!(interpreter::take_control_flow(), None);
}

#[test]
fn test_is_control_flow_set_true() {
    let _ = interpreter::take_control_flow(); // clear
    interpreter::set_control_flow(ControlFlow::Break);
    assert!(interpreter::is_control_flow_set());
    // is_control_flow_set must not consume the value
    assert_eq!(interpreter::take_control_flow(), Some(ControlFlow::Break));
}

#[test]
fn test_is_control_flow_set_false() {
    let _ = interpreter::take_control_flow(); // clear
    assert!(!interpreter::is_control_flow_set());
}

#[test]
fn test_control_flow_overwrite() {
    interpreter::set_control_flow(ControlFlow::Break);
    // Overwrite with Continue before consuming
    interpreter::set_control_flow(ControlFlow::Continue(None));
    assert_eq!(
        interpreter::take_control_flow(),
        Some(ControlFlow::Continue(None))
    );
}

// ---------------------------------------------------------------------------
// Depth / recursion limit tests
// ---------------------------------------------------------------------------

#[test]
fn test_depth_check_release() {
    interpreter::reset_depth();
    assert!(interpreter::check_depth().is_ok());
    interpreter::release_depth();
}

#[test]
fn test_depth_check_increments() {
    interpreter::reset_depth();
    assert!(interpreter::check_depth().is_ok());
    assert!(interpreter::check_depth().is_ok());
    interpreter::release_depth();
    interpreter::release_depth();
}

#[test]
fn test_depth_release_underflow() {
    interpreter::reset_depth();
    // Releasing when depth is 0 must not panic
    interpreter::release_depth();
    interpreter::release_depth(); // should saturate at 0
                                  // After underflow, check_depth should still work
    assert!(interpreter::check_depth().is_ok());
    interpreter::release_depth();
}

#[test]
fn test_depth_reset() {
    interpreter::reset_depth();
    assert!(interpreter::check_depth().is_ok());
    assert!(interpreter::check_depth().is_ok());
    assert!(interpreter::check_depth().is_ok());
    interpreter::reset_depth();
    // Reset brings depth back to 0, so check_depth should succeed
    assert!(interpreter::check_depth().is_ok());
    interpreter::release_depth();
}

#[test]
fn test_depth_guard_created_and_dropped() {
    interpreter::reset_depth();
    {
        let guard = interpreter::check_depth_guard();
        assert!(guard.is_ok());
        // guard drops here, releasing depth
    }
    // After guard drops, depth should be back to 0
    assert!(interpreter::check_depth().is_ok());
    interpreter::release_depth();
}

#[test]
fn test_depth_guard_releases_on_drop() {
    interpreter::reset_depth();
    assert!(interpreter::check_depth().is_ok()); // depth = 1
    interpreter::release_depth(); // depth = 0

    // Create a guard
    assert!(interpreter::check_depth_guard().is_ok()); // depth = 1 (guard holds)

    // Now check_depth should see depth=1, still ok
    assert!(interpreter::check_depth().is_ok()); // depth = 2
    interpreter::release_depth(); // depth = 1
}

#[test]
fn test_depth_check_exceeds_max() {
    interpreter::reset_depth();
    // Lower the max depth temporarily
    crate::interpreter::set_max_call_depth(3);
    assert!(interpreter::check_depth().is_ok()); // 0 -> 1
    assert!(interpreter::check_depth().is_ok()); // 1 -> 2
    assert!(interpreter::check_depth().is_ok()); // 2 -> 3
    let result = interpreter::check_depth(); // 3 -> should fail (3 >= 3)
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().0, "Maximum call stack size exceeded");
    // Restore default
    crate::interpreter::set_max_call_depth(10000);
    // Clean up accumulated depth
    interpreter::release_depth();
    interpreter::release_depth();
    interpreter::release_depth();
}

// ---------------------------------------------------------------------------
// Strict mode tests
// ---------------------------------------------------------------------------

#[test]
fn test_strict_mode_default_false() {
    interpreter::set_strict_mode(false);
    assert!(!interpreter::is_strict_mode());
}

#[test]
fn test_strict_mode_roundtrip() {
    interpreter::set_strict_mode(true);
    assert!(interpreter::is_strict_mode());
    interpreter::set_strict_mode(false);
    assert!(!interpreter::is_strict_mode());
}

#[test]
fn test_strict_mode_toggle() {
    interpreter::set_strict_mode(true);
    assert!(interpreter::is_strict_mode());
    interpreter::set_strict_mode(false);
    assert!(!interpreter::is_strict_mode());
    interpreter::set_strict_mode(true);
    assert!(interpreter::is_strict_mode());
}

// ---------------------------------------------------------------------------
// This value tests
// ---------------------------------------------------------------------------

#[test]
fn test_this_value_default_none() {
    interpreter::take_this_value(); // clear
    assert_eq!(interpreter::get_this_value(), None);
}

#[test]
fn test_this_value_roundtrip() {
    let val = Value::String("test".to_string());
    interpreter::set_this_value(val.clone());
    assert_eq!(interpreter::get_this_value(), Some(val));
}

#[test]
fn test_this_value_overwrite() {
    interpreter::set_this_value(Value::Null);
    interpreter::set_this_value(Value::Boolean(false));
    assert_eq!(interpreter::get_this_value(), Some(Value::Boolean(false)));
}

// ---------------------------------------------------------------------------
// Generator resume value tests
// ---------------------------------------------------------------------------

#[test]
fn test_generator_resume_value_default_undefined() {
    let _ = interpreter::take_generator_resume_value(); // clear
    assert_eq!(interpreter::take_generator_resume_value(), Value::Undefined);
}

#[test]
fn test_generator_resume_value_roundtrip() {
    let val = Value::Number(std::f64::consts::PI);
    interpreter::set_generator_resume_value(val.clone());
    assert_eq!(interpreter::take_generator_resume_value(), val);
}

#[test]
fn test_generator_resume_value_overwrite() {
    interpreter::set_generator_resume_value(Value::Null);
    interpreter::set_generator_resume_value(Value::Boolean(true));
    assert_eq!(
        interpreter::take_generator_resume_value(),
        Value::Boolean(true)
    );
}

#[test]
fn test_generator_resume_value_consumes_on_take() {
    interpreter::set_generator_resume_value(Value::Number(99.0));
    let _ = interpreter::take_generator_resume_value(); // consume
    assert_eq!(interpreter::take_generator_resume_value(), Value::Undefined);
}

// ---------------------------------------------------------------------------
// Generator yield value tests
// ---------------------------------------------------------------------------

#[test]
fn test_generator_yield_default_none() {
    let _ = interpreter::take_generator_yield(); // clear
    assert_eq!(interpreter::take_generator_yield(), None);
}

#[test]
fn test_generator_yield_roundtrip() {
    let val = Value::String("yielded".to_string());
    interpreter::set_generator_yield(val.clone());
    assert_eq!(interpreter::take_generator_yield(), Some(val));
}

#[test]
fn test_generator_yield_overwrite() {
    interpreter::set_generator_yield(Value::Number(1.0));
    interpreter::set_generator_yield(Value::Number(2.0));
    assert_eq!(
        interpreter::take_generator_yield(),
        Some(Value::Number(2.0))
    );
}

#[test]
fn test_generator_yield_consumes_on_take() {
    interpreter::set_generator_yield(Value::Null);
    let _ = interpreter::take_generator_yield(); // consume
    assert_eq!(interpreter::take_generator_yield(), None);
}

// ---------------------------------------------------------------------------
// Generator return value tests
// ---------------------------------------------------------------------------

#[test]
fn test_generator_return_default_none() {
    let _ = interpreter::take_generator_return(); // clear
    assert_eq!(interpreter::take_generator_return(), None);
}

#[test]
fn test_generator_return_roundtrip() {
    let val = Value::Number(42.0);
    interpreter::set_generator_return(val.clone());
    assert_eq!(interpreter::take_generator_return(), Some(val));
}

#[test]
fn test_generator_return_overwrite() {
    interpreter::set_generator_return(Value::Boolean(false));
    interpreter::set_generator_return(Value::Boolean(true));
    assert_eq!(
        interpreter::take_generator_return(),
        Some(Value::Boolean(true))
    );
}

#[test]
fn test_generator_return_consumes_on_take() {
    interpreter::set_generator_return(Value::Number(-1.0));
    let _ = interpreter::take_generator_return(); // consume
    assert_eq!(interpreter::take_generator_return(), None);
}

// ---------------------------------------------------------------------------
// Current eval env tests
// ---------------------------------------------------------------------------

#[test]
fn test_current_eval_env_default_none() {
    interpreter::set_current_eval_env(None);
    assert!(interpreter::get_current_eval_env().is_none());
}

#[test]
fn test_current_eval_env_roundtrip() {
    use crate::env::Environment;
    use std::cell::RefCell;
    use std::rc::Rc;

    let env = Rc::new(RefCell::new(Environment::new()));
    interpreter::set_current_eval_env(Some(env.clone()));
    let result = interpreter::get_current_eval_env();
    assert!(result.is_some());
    // Compare Rc pointers
    assert!(Rc::ptr_eq(&env, &result.unwrap()));
}

#[test]
fn test_current_eval_env_clear() {
    use crate::env::Environment;
    use std::cell::RefCell;
    use std::rc::Rc;

    let env = Rc::new(RefCell::new(Environment::new()));
    interpreter::set_current_eval_env(Some(env));
    interpreter::set_current_eval_env(None);
    assert!(interpreter::get_current_eval_env().is_none());
}

// ---------------------------------------------------------------------------
// Direct eval tests
// ---------------------------------------------------------------------------

#[test]
fn test_direct_eval_default_false() {
    interpreter::set_direct_eval(false);
    assert!(!interpreter::is_direct_eval());
}

#[test]
fn test_direct_eval_roundtrip() {
    interpreter::set_direct_eval(true);
    assert!(interpreter::is_direct_eval());
    interpreter::set_direct_eval(false);
    assert!(!interpreter::is_direct_eval());
}

#[test]
fn test_direct_eval_toggle() {
    interpreter::set_direct_eval(true);
    assert!(interpreter::is_direct_eval());
    interpreter::set_direct_eval(false);
    assert!(!interpreter::is_direct_eval());
    interpreter::set_direct_eval(true);
    assert!(interpreter::is_direct_eval());
}

// ---------------------------------------------------------------------------
// DepthGuard RAII behavior tests
// ---------------------------------------------------------------------------

#[test]
fn test_depth_guard_struct_name() {
    // Verify DepthGuard is a valid type (constructible via check_depth_guard)
    interpreter::reset_depth();
    let guard = interpreter::check_depth_guard().unwrap();
    // guard is alive here; drop happens at end of scope
    drop(guard);
    // After dropping, depth is back to 0
    assert!(interpreter::check_depth().is_ok());
    interpreter::release_depth();
}

#[test]
fn test_depth_guard_nested() {
    interpreter::reset_depth();
    {
        let _g1 = interpreter::check_depth_guard().unwrap(); // depth = 1
        {
            let _g2 = interpreter::check_depth_guard().unwrap(); // depth = 2
            assert!(interpreter::check_depth().is_ok()); // depth = 3
            interpreter::release_depth(); // depth = 2
        } // _g2 drops -> depth = 1
    } // _g1 drops -> depth = 0
      // Now depth should be 0
    assert!(interpreter::check_depth().is_ok()); // depth = 1
    interpreter::release_depth(); // depth = 0
}

// ---------------------------------------------------------------------------
// take_this_value tests
// ---------------------------------------------------------------------------

#[test]
fn test_take_this_value_consumes() {
    interpreter::set_this_value(Value::Number(7.0));
    let v = interpreter::get_this_value();
    assert_eq!(v, Some(Value::Number(7.0)));
    // get_this_value does not consume
    let v2 = interpreter::get_this_value();
    assert_eq!(v2, Some(Value::Number(7.0)));
    interpreter::take_this_value();
    // After take_this_value, should be None
    assert_eq!(interpreter::get_this_value(), None);
}

// ---------------------------------------------------------------------------
// Control Flow Yield variants
// ---------------------------------------------------------------------------

#[test]
fn test_control_flow_yield_variant() {
    // ControlFlow::Yield is defined in the enum; test set/take roundtrip
    interpreter::set_control_flow(ControlFlow::Yield(Value::Number(1.0)));
    let result = interpreter::take_control_flow();
    match result {
        Some(ControlFlow::Yield(v)) => assert_eq!(v, Value::Number(1.0)),
        other => panic!("expected Yield, got {:?}", other),
    }
}

#[test]
fn test_control_flow_yield_delegate_variant() {
    interpreter::set_control_flow(ControlFlow::YieldDelegate(Value::String("abc".to_string())));
    let result = interpreter::take_control_flow();
    match result {
        Some(ControlFlow::YieldDelegate(v)) => {
            assert_eq!(v, Value::String("abc".to_string()))
        }
        other => panic!("expected YieldDelegate, got {:?}", other),
    }
}
