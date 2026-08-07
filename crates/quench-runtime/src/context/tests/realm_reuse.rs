//! Tests for realm context reuse and cross-realm eval.

#[cfg(test)]
use crate::test262::harness::inject_harness;
#[cfg(test)]
use crate::value as value_mod;
#[cfg(test)]
use crate::{Context, Value};

/// Test that Object.setPrototypeOf modifies the SAME Number that eval uses.
#[cfg(test)]
#[test]
fn test_set_prototype_of_persists_in_eval() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    // Modify Number.prototype via setPrototypeOf
    let result = ctx.eval(
        r#"
        Object.setPrototypeOf(Number.prototype, null);
    "#,
    );
    assert!(result.is_ok(), "setPrototypeOf failed: {:?}", result);

    // Get Number's prototype after modification — should be null now
    let result = ctx.eval("Object.getPrototypeOf(Number.prototype)");
    assert!(result.is_ok(), "getPrototypeOf after failed: {:?}", result);
    let proto_after = result.unwrap();

    // proto_after should be null (not the original proto)
    assert_eq!(
        proto_after,
        Value::Null,
        "setPrototypeOf should modify Number.prototype so subsequent eval sees null"
    );
}

/// Test that modifying other.Number.prototype is visible inside other.eval.
#[cfg(test)]
#[test]
fn test_other_eval_sees_set_prototype_of() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    // All in one eval to ensure the realm global is accessible
    let result = ctx.eval(
        r#"
        var other = $262.createRealm().global;
        Object.setPrototypeOf(other.Number.prototype, null);
        var protoInRealm = other.eval('Object.getPrototypeOf(Number.prototype)');
        protoInRealm;
        "#,
    );
    assert!(result.is_ok(), "combined eval failed: {:?}", result);
    let proto_in_realm = result.unwrap();

    assert_eq!(
        proto_in_realm,
        Value::Null,
        "other.eval should see the setPrototypeOf(modified Number.prototype)"
    );
}

/// Test that other.Number and other.eval('Number') are the same object.
#[cfg(test)]
#[test]
fn test_other_number_same_as_other_eval_number() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    // First check with JS ===
    let result = ctx.eval(
        r#"
        var other = $262.createRealm().global;
        var n1 = other.Number;
        var n2 = other.eval('Number');
        n1 === n2;
        "#,
    );
    assert!(result.is_ok(), "combined eval failed: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::Boolean(true),
        "other.Number and other.eval('Number') should be identical"
    );
}

/// Test at Rust level: other.Number and other.eval('Number') via same_value.
#[cfg(test)]
#[test]
fn test_realm_eval_number_identity_via_rust() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    // Get other.Number and other.eval('Number') as separate values
    let result = ctx.eval(
        r#"
        var other = $262.createRealm().global;
        var n1 = other.Number;
        var n2 = other.eval('Number');
        // Return a tuple so we can check both
        [n1, n2];
        "#,
    );
    assert!(result.is_ok(), "eval failed: {:?}", result);
    let val = result.unwrap();
    if let Value::Object(arr) = val {
        let arr_b = arr.borrow();
        if let (Some(n1_val), Some(n2_val)) = (arr_b.get("0"), arr_b.get("1")) {
            // Use same_value which does ptr comparison for NativeConstructor
            let n1_clone = n1_val.clone();
            let n2_clone = n2_val.clone();
            let same = value_mod::same_value(&n1_clone, &n2_clone);
            assert!(
                same,
                "same_value(other.Number, other.eval('Number')) = false: n1={:?} n2={:?}",
                n1_val, n2_val
            );
        }
    }
}

/// Test eval can access Number in the realm's own context.
#[cfg(test)]
#[test]
fn test_realm_eval_can_access_number() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    let result = ctx.eval(
        r#"
        var other = $262.createRealm().global;
        var n = other.eval('typeof Number');
        n;
        "#,
    );
    assert!(result.is_ok(), "eval typeof failed: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::String("function".to_string()),
        "realm eval should see Number as a function"
    );
}

/// Test that Number is not undefined in realm eval.
#[cfg(test)]
#[test]
fn test_realm_eval_number_not_undefined() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    let result = ctx.eval(
        r#"
        var other = $262.createRealm().global;
        var n = other.eval('Number === undefined');
        n;
        "#,
    );
    assert!(
        result.is_ok(),
        "eval Number===undefined failed: {:?}",
        result
    );
    assert_eq!(
        result.unwrap(),
        Value::Boolean(false),
        "Number should NOT be undefined in realm eval"
    );
}

/// Test eval can access Number.prototype.
#[cfg(test)]
#[test]
fn test_realm_eval_number_prototype_exists() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    let result = ctx.eval(
        r#"
        var other = $262.createRealm().global;
        var proto = other.eval('Number.prototype !== undefined');
        proto;
        "#,
    );
    assert!(
        result.is_ok(),
        "eval Number.prototype check failed: {:?}",
        result
    );
    assert_eq!(
        result.unwrap(),
        Value::Boolean(true),
        "Number.prototype should be accessible in realm eval"
    );
}

/// Test eval can access Number.prototype directly.
#[cfg(test)]
#[test]
fn test_realm_eval_number_prototype_value() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    let result = ctx.eval(
        r#"
        var other = $262.createRealm().global;
        var proto = other.eval('typeof Number.prototype');
        proto;
        "#,
    );
    assert!(
        result.is_ok(),
        "eval typeof Number.prototype failed: {:?}",
        result
    );
    assert_eq!(
        result.unwrap(),
        Value::String("object".to_string()),
        "Number.prototype should be an object"
    );
}

/// Test setPrototypeOf on other.Number.prototype does NOT throw.
#[cfg(test)]
#[test]
fn test_set_prototype_of_on_realm_number() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    let result = ctx.eval(
        r#"
        var other = $262.createRealm().global;
        Object.setPrototypeOf(other.Number.prototype, null);
        "#,
    );
    assert!(result.is_ok(), "setPrototypeOf failed: {:?}", result);
}

/// Test that getPrototypeOf(Number.prototype) works in realm eval AFTER setPrototypeOf.
#[cfg(test)]
#[test]
fn test_get_prototype_of_after_set_prototype_of() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    let result = ctx.eval(
        r#"
        var other = $262.createRealm().global;
        Object.setPrototypeOf(other.Number.prototype, null);
        var proto = other.eval('Object.getPrototypeOf(Number.prototype)');
        proto;
        "#,
    );
    assert!(result.is_ok(), "getPrototypeOf failed: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::Null,
        "Number.prototype proto should be null after setPrototypeOf"
    );
}

/// Test that the proxy set trap fires for primitive property assignment
/// (same-realm version to isolate from realm-sharing complexity).
#[cfg(test)]
#[test]
fn test_same_realm_primitive_set_proxy_trap() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    let result = ctx.eval(
        r#"
        var count = 0;
        var spy = new Proxy({}, {
            set: function(target, prop, value) {
                count += 1;
                return true;
            }
        });
        Object.setPrototypeOf(Number.prototype, spy);
        0..test262 = null;
        count;
        "#,
    );
    assert!(result.is_ok(), "eval failed: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::Number(1.0),
        "proxy set trap should have been called once (same realm)"
    );
}

/// Test that assigning to a property on a primitive calls the proxy set trap
/// in the other realm's prototype chain.
#[cfg(test)]
#[test]
fn test_other_realm_primitive_set_proxy_trap() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx);

    let result = ctx.eval(
        r#"
        var other = $262.createRealm().global;
        var count = 0;
        var spy = new Proxy({}, {
            set: function(target, prop, value) {
                count += 1;
                return true;
            }
        });
        Object.setPrototypeOf(other.Number.prototype, spy);
        other.eval('0..test262 = null');
        count;
        "#,
    );
    assert!(result.is_ok(), "combined eval failed: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::Number(1.0),
        "proxy set trap should have been called once"
    );
}
