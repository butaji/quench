use super::*;
use crate::value::{create_js_error_with_type, set_thrown_value, take_thrown_value};

#[test]
fn typeof_callable_object_is_function() {
    let value = crate::Context::new()
        .unwrap()
        .eval("typeof String")
        .unwrap();
    assert_eq!(value, Value::String("function".to_string()));
}

#[test]
fn loose_equality_propagates_object_coercion_errors() {
    let result = crate::Context::new()
        .unwrap()
        .eval("({ valueOf: function() { return {}; }, toString: function() { return {}; } } == 1)");
    assert!(result.is_err());
}

#[test]
fn unsigned_shift_propagates_left_call_throw_before_rhs() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval(
        "(function() { throw new Error('left'); })() >>> (function() { throw new Error('right'); })()",
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().0, "Error: left");
}

#[test]
fn unsigned_shift_rejects_symbol_from_rhs_after_coercion() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval(
        "(function() { return { valueOf: function() { return 1; } }; })() >>> (function() { return { valueOf: function() { return Symbol('x'); } }; })()",
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().0.contains("TypeError"));
}

#[test]
fn relational_operator_rejects_symbol_after_primitive_conversion() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("1 < Symbol('x')");
    assert!(result.is_err());
    assert!(result.unwrap_err().0.contains("TypeError"));
}

#[test]
fn bigint_shift_accepts_negative_shift_counts() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("0b101n << -1n").unwrap(),
        Value::BigInt(std::rc::Rc::new(2.into()))
    );
}

#[test]
fn bigint_relational_comparison_preserves_precision() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("9007199254740993n > 9007199254740992n").unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn bigint_number_relational_comparison_preserves_precision() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("9007199254740993n > 9007199254740992").unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn bigint_string_relational_comparison_rejects_number_only_strings() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(ctx.eval("'0.' < 1n").unwrap(), Value::Boolean(false));
    assert_eq!(ctx.eval("'0x10' < 17n").unwrap(), Value::Boolean(true));
    assert_eq!(ctx.eval("0n < '1n'").unwrap(), Value::Boolean(false));
}

#[test]
fn instanceof_propagates_object_has_instance_getter_error() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval(
        "var F = {}; Object.defineProperty(F, Symbol.hasInstance, { get: function() { throw 7; } }); 0 instanceof F;",
    );
    assert!(result.is_err());
    let error = crate::value::take_thrown_value().unwrap();
    assert_eq!(crate::value::to_js_string(&error), "7");
}

/// When `valueOf` throws, `eval_add` must surface the error AND leave the
/// thrown value intact for the surrounding try/catch to retrieve.
#[test]
fn eval_add_propagates_valueof_throw_and_preserves_thrown_value() {
    let (stale, _js_err) = create_js_error_with_type("stale", "Error");
    set_thrown_value(stale);

    let left = Value::Number(1.0);
    let right = Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
        crate::value::Object::new(crate::value::ObjectKind::Ordinary),
    )));
    let _ = take_thrown_value();
    let (fresh, _) = create_js_error_with_type("boom", "Error");
    set_thrown_value(fresh);
    let result = eval_add(&left, &right);
    assert!(
        result.is_err(),
        "eval_add must return Err when thrown_value is set"
    );
    assert!(
        get_thrown_value().is_some(),
        "eval_add must not consume thrown_value"
    );
    let _ = take_thrown_value();
}

/// Per spec §7.1.1: @@toPrimitive getter throws, other side must not run.
#[test]
fn eval_add_short_circuits_when_left_toprim_getter_throws() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval(
        "var callCount = 0; \
         var thrower = {}; \
         var counter = {}; \
         Object.defineProperty(thrower, Symbol.toPrimitive, { get: function() { throw new Error('x'); } }); \
         Object.defineProperty(counter, Symbol.toPrimitive, { get: function() { callCount += 1; } }); \
         var thrown; \
         try { thrower + counter; } catch (e) { thrown = e; } \
         ({ callCount: callCount, msg: thrown ? thrown.message : 'undefined' });",
    );
    let value = result.unwrap();
    let crate::value::Value::Object(obj) = value else {
        panic!("expected object result")
    };
    let obj = obj.borrow();
    assert_eq!(obj.get("callCount"), Some(crate::value::Value::Number(0.0)));
    assert_eq!(
        obj.get("msg"),
        Some(crate::value::Value::String("x".to_string()))
    );
}

// =====================================================================
// eval_binary_op — arithmetic
// =====================================================================

#[test]
fn binary_add_number_plus_number() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("2 + 3").unwrap();
    assert_eq!(r, crate::value::Value::Number(5.0));
}

#[test]
fn binary_add_string_concat() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("'hello' + ' ' + 'world'").unwrap();
    assert_eq!(r, crate::value::Value::String("hello world".to_string()));
}

#[test]
fn binary_add_number_plus_string() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("5 + ' items'").unwrap();
    assert_eq!(r, crate::value::Value::String("5 items".to_string()));
}

#[test]
fn binary_add_string_plus_number() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("'count: ' + 42").unwrap();
    assert_eq!(r, crate::value::Value::String("count: 42".to_string()));
}

#[test]
fn binary_add_object_with_value_of() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx
        .eval("var o = { valueOf: function() { return 10; } }; 5 + o;")
        .unwrap();
    assert_eq!(r, crate::value::Value::Number(15.0));
}

#[test]
fn binary_add_object_with_to_string() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx
        .eval("var o = { toString: function() { return '42'; } }; 'result=' + o;")
        .unwrap();
    assert_eq!(r, crate::value::Value::String("result=42".to_string()));
}

#[test]
fn binary_add_date_hint_string() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx
        .eval("var d = new Date(2024, 0, 1); d + ' is a date';")
        .unwrap();
    let s = match r {
        crate::value::Value::String(s) => s,
        _ => panic!("expected string"),
    };
    assert!(
        s.starts_with("Date @ "),
        "expected 'Date @ ...', got: {}",
        s
    );
    assert!(s.contains(" is a date"));
}

#[test]
fn binary_add_symbol_left_throws() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("Symbol('x') + 1");
    assert!(r.is_err(), "Symbol + number must throw");
}

#[test]
fn binary_add_symbol_right_throws() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("1 + Symbol('x')");
    assert!(r.is_err(), "number + Symbol must throw");
}

#[test]
fn binary_add_symbol_string_concat_throws() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("'prefix: ' + Symbol('x')");
    assert!(r.is_err(), "string + Symbol must throw");
}

#[test]
fn binary_subtract() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("10 - 3").unwrap(),
        crate::value::Value::Number(7.0)
    );
    assert_eq!(
        ctx.eval("0.1 - 0.2").unwrap(),
        crate::value::Value::Number(-0.1)
    );
}

#[test]
fn binary_multiply() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("6 * 7").unwrap(),
        crate::value::Value::Number(42.0)
    );
    assert_eq!(
        ctx.eval("Number.MIN_VALUE * 0.1").unwrap(),
        crate::value::Value::Number(0.0)
    );
}

#[test]
fn binary_divide() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("20 / 4").unwrap(),
        crate::value::Value::Number(5.0)
    );
    assert_eq!(ctx.eval("7 / 2").unwrap(), crate::value::Value::Number(3.5));
}

#[test]
fn binary_divide_by_zero() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("1 / 0").unwrap(),
        crate::value::Value::Number(f64::INFINITY)
    );
    assert_eq!(
        ctx.eval("-1 / 0").unwrap(),
        crate::value::Value::Number(f64::NEG_INFINITY)
    );
}

#[test]
fn binary_modulo() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("17 % 5").unwrap(),
        crate::value::Value::Number(2.0)
    );
}

#[test]
fn binary_modulo_by_zero() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("5 % 0").unwrap();
    let n = match r {
        crate::value::Value::Number(n) => n,
        _ => panic!("expected number"),
    };
    assert!(n.is_nan(), "5 % 0 must produce NaN, got {}", n);
}

// =====================================================================
// eval_binary_op — equality
// =====================================================================

#[test]
fn binary_eq_loose_equality() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("1 == 1").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("1 == '1'").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("null == undefined").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("false == 0").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("'5' == 5").unwrap(),
        crate::value::Value::Boolean(true)
    );
}

#[test]
fn binary_neq_loose_inequality() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("1 != 2").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("1 != '1'").unwrap(),
        crate::value::Value::Boolean(false)
    );
}

#[test]
fn binary_strict_eq_true() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("1 === 1").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("true === true").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("'x' === 'x'").unwrap(),
        crate::value::Value::Boolean(true)
    );
}

#[test]
fn binary_strict_neq_false_when_equal() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("1 !== 1").unwrap(),
        crate::value::Value::Boolean(false)
    );
}

#[test]
fn binary_strict_eq_different_types() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("1 === '1'").unwrap(),
        crate::value::Value::Boolean(false)
    );
    assert_eq!(
        ctx.eval("0 === false").unwrap(),
        crate::value::Value::Boolean(false)
    );
    assert_eq!(
        ctx.eval("null === undefined").unwrap(),
        crate::value::Value::Boolean(false)
    );
}

// =====================================================================
// eval_binary_op — relational
// =====================================================================

#[test]
fn binary_lt_numeric() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("3 < 5").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("5 < 3").unwrap(),
        crate::value::Value::Boolean(false)
    );
    assert_eq!(
        ctx.eval("NaN < 5").unwrap(),
        crate::value::Value::Boolean(false)
    );
}

#[test]
fn binary_gt_numeric() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("5 > 3").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("3 > 5").unwrap(),
        crate::value::Value::Boolean(false)
    );
}

#[test]
fn binary_le_numeric() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("3 <= 5").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("5 <= 5").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("6 <= 5").unwrap(),
        crate::value::Value::Boolean(false)
    );
}

#[test]
fn binary_ge_numeric() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("5 >= 3").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("5 >= 5").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("3 >= 5").unwrap(),
        crate::value::Value::Boolean(false)
    );
}

#[test]
fn binary_relational_string_vs_string() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("'apple' < 'banana'").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("'z' > 'a'").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("'abc' < 'abd'").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("'test' < 'test'").unwrap(),
        crate::value::Value::Boolean(false)
    );
    assert_eq!(
        ctx.eval("'test' <= 'test'").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("'A' < 'a'").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("'Z' > 'a'").unwrap(),
        crate::value::Value::Boolean(false)
    );
    assert_eq!(
        ctx.eval("'10' < '9'").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("'' < ''").unwrap(),
        crate::value::Value::Boolean(false)
    );
    assert_eq!(
        ctx.eval("'' <= ''").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("'\\u{10000}' < '\\uFFFF'").unwrap(),
        crate::value::Value::Boolean(true)
    );
}

#[test]
fn relational_comparison_coerces_left_operand_first() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval(
            "'use strict'; \
             var x = { valueOf: function() { throw 'x'; } }; \
             var y = { valueOf: function() { throw 'y'; } }; \
             try { x < y; 'none'; } catch (e) { e; }",
        )
        .unwrap();
    assert_eq!(result, crate::value::Value::String("x".into()));
}

// =====================================================================
// eval_in_op
// =====================================================================

#[test]
fn binary_in_found_in_object() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("'name' in { name: 'Alice' }").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("'age' in { name: 'Alice' }").unwrap(),
        crate::value::Value::Boolean(false)
    );
}

#[test]
fn binary_in_non_object() {
    let mut ctx = crate::Context::new().unwrap();
    assert!(ctx.eval("'foo' in null").is_err());
    assert!(ctx.eval("'foo' in undefined").is_err());
    assert!(ctx.eval("'foo' in 42").is_err());
}

#[test]
fn binary_in_primitive_rhs_throws_type_error() {
    let mut ctx = crate::Context::new().unwrap();
    assert!(ctx.eval("'foo' in null").is_err());
    assert!(ctx.eval("'foo' in undefined").is_err());
    assert!(ctx.eval("'foo' in 42").is_err());
}

#[test]
fn binary_in_native_constructor_checks_static_properties() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("'MAX_VALUE' in Number"),
        Ok(crate::value::Value::Boolean(true))
    );
}

// =====================================================================
// eval_instanceof
// =====================================================================

#[test]
fn binary_instanceof_basic() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("({}) instanceof Object").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("([]) instanceof Array").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(
        ctx.eval("([]) instanceof Object").unwrap(),
        crate::value::Value::Boolean(true)
    );
}

#[test]
fn binary_instanceof_unrelated() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("({}) instanceof Array").unwrap(),
        crate::value::Value::Boolean(false)
    );
}

#[test]
fn string_constructor_has_instance_does_not_recurse() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("({}) instanceof String"),
        Ok(crate::value::Value::Boolean(false))
    );
}

#[test]
fn inherited_custom_has_instance_is_called() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval(
            "var base = {}; Object.defineProperty(base, Symbol.hasInstance, { value: function() { return true; } }); var ctor = Object.create(base); ({}) instanceof ctor"
        ),
        Ok(crate::value::Value::Boolean(true))
    );
}

#[test]
fn instanceof_non_callable_rhs_throws_type_error() {
    let mut ctx = crate::Context::new().unwrap();
    assert!(ctx.eval("1 instanceof Math").is_err());
    assert!(ctx.eval("true instanceof true").is_err());
    assert!(ctx.eval("1 instanceof null").is_err());
}

#[test]
fn instanceof_throws_when_function_prototype_is_not_object() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval(
        "var F = new Function(); F.prototype = undefined;\
         try { ({} instanceof F); false; } catch (e) { e instanceof TypeError; }",
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[test]
fn wrapped_bigint_shift_stays_bigint() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("Object(2n) << 1n");
    assert_eq!(result.unwrap(), Value::BigInt(std::rc::Rc::new(4.into())));
}

#[test]
fn wrapped_bigint_bitwise_and_stays_bigint() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("Object(2n) & 3n");
    assert_eq!(result.unwrap(), Value::BigInt(std::rc::Rc::new(2.into())));
}

#[test]
fn wrapped_bigint_multiplication_stays_bigint() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("Object(2n) * 3n");
    assert_eq!(result.unwrap(), Value::BigInt(std::rc::Rc::new(6.into())));
}

#[test]
fn wrapped_bigint_unary_operations_stay_bigint() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("-Object(2n) === -2n && ~Object(2n) === -3n");
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[test]
fn unsigned_shift_rejects_wrapped_bigint() {
    let mut ctx = crate::Context::new().unwrap();
    let result =
        ctx.eval("try { Object(2n) >>> 0n; false; } catch (e) { e instanceof TypeError; }");
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[test]
fn binary_instanceof_null_undefined() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("null instanceof Object").unwrap(),
        crate::value::Value::Boolean(false)
    );
    assert_eq!(
        ctx.eval("undefined instanceof Object").unwrap(),
        crate::value::Value::Boolean(false)
    );
}

#[test]
fn binary_instanceof_custom_ctor() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("function C() {} var inst = new C(); inst instanceof C;");
    assert_eq!(r.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn binary_instanceof_subclass_extends_array() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let sub_is_sub = ctx
        .eval("class Subclass extends Array {} var sub = new Subclass(); sub instanceof Subclass")
        .unwrap();
    let sub_is_arr = ctx
        .eval("class Subclass extends Array {} var sub = new Subclass(); sub instanceof Array")
        .unwrap();
    if sub_is_sub != crate::value::Value::Boolean(true) {
        panic!("sub instanceof Subclass = {:?}", sub_is_sub);
    }
    assert_eq!(sub_is_arr, crate::value::Value::Boolean(true));
}

#[test]
fn binary_instanceof_subclass_extends_function() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval(
            "class Subclass extends Function {} \
             var sub = new Subclass(); \
             sub instanceof Subclass && sub instanceof Function",
        )
        .unwrap();
    assert_eq!(result, crate::value::Value::Boolean(true));
}

#[test]
fn binary_instanceof_subclass() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval(
        "function Base() {} \
         function Derived() {} \
         Derived.prototype = Object.create(Base.prototype); \
         var inst = new Derived(); \
         inst instanceof Base;",
    );
    assert_eq!(r.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn binary_instanceof_class() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("class C {} var inst = new C(); inst instanceof C;");
    assert_eq!(r.unwrap(), crate::value::Value::Boolean(true));
}

// =====================================================================
// eval_binary_op — logical (short-circuit)
// =====================================================================

#[test]
fn binary_and_short_circuits() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("var x = 0; false && (x = 1); x;").unwrap();
    assert_eq!(r, crate::value::Value::Number(0.0));
}

#[test]
fn binary_and_returns_right_when_truthy() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("true && 'yes'").unwrap();
    assert_eq!(r, crate::value::Value::String("yes".to_string()));
}

#[test]
fn binary_and_returns_left_when_falsy() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("0 && 'yes'").unwrap();
    assert_eq!(r, crate::value::Value::Number(0.0));
}

#[test]
fn binary_or_short_circuits() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("var x = 0; true || (x = 1); x;").unwrap();
    assert_eq!(r, crate::value::Value::Number(0.0));
}

#[test]
fn binary_or_returns_left_when_truthy() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("'first' || 'second'").unwrap();
    assert_eq!(r, crate::value::Value::String("first".to_string()));
}

#[test]
fn binary_or_returns_right_when_falsy() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("false || 'fallback'").unwrap();
    assert_eq!(r, crate::value::Value::String("fallback".to_string()));
}

// =====================================================================
// eval_nullish
// =====================================================================

#[test]
fn binary_nullish_coalescing_null() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("null ?? 'fallback'").unwrap();
    assert_eq!(r, crate::value::Value::String("fallback".to_string()));
}

#[test]
fn binary_nullish_coalescing_undefined() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("undefined ?? 42").unwrap();
    assert_eq!(r, crate::value::Value::Number(42.0));
}

#[test]
fn binary_nullish_coalescing_returns_left() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("0 ?? 'fallback'").unwrap();
    assert_eq!(r, crate::value::Value::Number(0.0));
    let r2 = ctx.eval("false ?? true").unwrap();
    assert_eq!(r2, crate::value::Value::Boolean(false));
    let r3 = ctx.eval("'' ?? 'fallback'").unwrap();
    assert_eq!(r3, crate::value::Value::String("".to_string()));
}

// =====================================================================
// bitwise operators
// =====================================================================

#[test]
fn binary_bit_and() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("0xFF & 0x0F").unwrap(),
        crate::value::Value::Number(15.0)
    );
    assert_eq!(ctx.eval("5 & 3").unwrap(), crate::value::Value::Number(1.0));
}

#[test]
fn binary_bit_or() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("0x0F | 0xF0").unwrap(),
        crate::value::Value::Number(255.0)
    );
}

#[test]
fn binary_bit_xor() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("0xFF ^ 0x0F").unwrap(),
        crate::value::Value::Number(240.0)
    );
}

#[test]
fn binary_bitwise_mixed_bigint_and_number_throws() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("try { 1n & 1; 'no throw' } catch (e) { e instanceof TypeError }"),
        Ok(crate::value::Value::Boolean(true))
    );
}

#[test]
fn binary_bitwise_symbol_left_does_not_coerce_right() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("var trace=''; try { ({valueOf(){trace+='1';return Symbol()}}) & ({valueOf(){trace+='2';return 1}}) } catch(e) {} trace"),
        Ok(crate::value::Value::String("1".into()))
    );
}

// =====================================================================
// shift operators
// =====================================================================

#[test]
fn binary_shift_left() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("1 << 3").unwrap(),
        crate::value::Value::Number(8.0)
    );
    assert_eq!(
        ctx.eval("-1 << 2").unwrap(),
        crate::value::Value::Number(-4.0)
    );
}

#[test]
fn binary_shift_right_sign_propagating() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("-8 >> 2").unwrap(),
        crate::value::Value::Number(-2.0)
    );
}

#[test]
fn binary_shift_right_unsigned() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("-1 >>> 0").unwrap(),
        crate::value::Value::Number(4294967295.0)
    );
}

#[test]
fn binary_shift_count_masked_to_5_bits() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("1 << 32").unwrap(),
        crate::value::Value::Number(1.0)
    );
    assert_eq!(
        ctx.eval("1 << 33").unwrap(),
        crate::value::Value::Number(2.0)
    );
}

// =====================================================================
// eval_unary_op
// =====================================================================

#[test]
fn unary_not() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("!true").unwrap(),
        crate::value::Value::Boolean(false)
    );
    assert_eq!(
        ctx.eval("!false").unwrap(),
        crate::value::Value::Boolean(true)
    );
    assert_eq!(ctx.eval("!!1").unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn unary_negation() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(ctx.eval("-42").unwrap(), crate::value::Value::Number(-42.0));
    assert_eq!(
        ctx.eval("-(-42)").unwrap(),
        crate::value::Value::Number(42.0)
    );
    assert_eq!(ctx.eval("-0").unwrap(), crate::value::Value::Number(-0.0));
}

#[test]
fn unary_plus() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(ctx.eval("+42").unwrap(), crate::value::Value::Number(42.0));
    assert_eq!(ctx.eval("+'5'").unwrap(), crate::value::Value::Number(5.0));
    assert_eq!(ctx.eval("+true").unwrap(), crate::value::Value::Number(1.0));
}

#[test]
fn unary_bitnot() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(ctx.eval("~0").unwrap(), crate::value::Value::Number(-1.0));
    assert_eq!(ctx.eval("~(-1)").unwrap(), crate::value::Value::Number(0.0));
    assert_eq!(
        ctx.eval("~-2147483649").unwrap(),
        crate::value::Value::Number(-2147483648.0)
    );
}

#[test]
fn unary_void() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(ctx.eval("void 0").unwrap(), crate::value::Value::Undefined);
    assert_eq!(
        ctx.eval("void 'expr'").unwrap(),
        crate::value::Value::Undefined
    );
}

// =====================================================================
// eval_typeof
// =====================================================================

#[test]
fn typeof_undefined() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("typeof undefined").unwrap(),
        crate::value::Value::String("undefined".to_string())
    );
}

#[test]
fn typeof_null() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("typeof null").unwrap(),
        crate::value::Value::String("object".to_string())
    );
}

#[test]
fn typeof_boolean() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("typeof false").unwrap(),
        crate::value::Value::String("boolean".to_string())
    );
}

#[test]
fn typeof_number() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("typeof 42").unwrap(),
        crate::value::Value::String("number".to_string())
    );
    assert_eq!(
        ctx.eval("typeof NaN").unwrap(),
        crate::value::Value::String("number".to_string())
    );
    assert_eq!(
        ctx.eval("typeof Infinity").unwrap(),
        crate::value::Value::String("number".to_string())
    );
}

#[test]
fn typeof_string() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("typeof 'hello'").unwrap(),
        crate::value::Value::String("string".to_string())
    );
}

#[test]
fn typeof_symbol() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("typeof Symbol('x')").unwrap(),
        crate::value::Value::String("symbol".to_string())
    );
}

#[test]
fn typeof_object() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("typeof {}").unwrap(),
        crate::value::Value::String("object".to_string())
    );
    assert_eq!(
        ctx.eval("typeof []").unwrap(),
        crate::value::Value::String("object".to_string())
    );
    assert_eq!(
        ctx.eval("typeof new Date()").unwrap(),
        crate::value::Value::String("object".to_string())
    );
}

#[test]
fn typeof_function() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("typeof function() {}").unwrap(),
        crate::value::Value::String("function".to_string())
    );
    assert_eq!(
        ctx.eval("typeof Array").unwrap(),
        crate::value::Value::String("function".to_string())
    );
}

#[test]
fn typeof_bigint() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("typeof 42n").unwrap(),
        crate::value::Value::String("bigint".to_string())
    );
}

// =====================================================================
// throw propagation in bit operations
// =====================================================================

#[test]
fn bitwise_op_left_to_number_throws() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx
        .eval(
            "var thrown; \
         try { \
           var o = { valueOf: function() { throw new Error('boom'); } }; \
           o & 1; \
         } catch(e) { thrown = e; } \
         thrown ? thrown.message : 'none';",
        )
        .unwrap();
    let s = match r {
        crate::value::Value::String(s) => s,
        _ => panic!("expected string"),
    };
    assert_eq!(s, "boom");
}

#[test]
fn shift_op_left_to_number_throws() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx
        .eval(
            "var o = { valueOf: function() { throw new Error('shift-boom'); } }; \
         var thrown; \
         try { o << 2; } catch(e) { thrown = e; } \
         thrown ? thrown.message : 'none';",
        )
        .unwrap();
    let s = match r {
        crate::value::Value::String(s) => s,
        _ => panic!("expected string"),
    };
    assert_eq!(s, "shift-boom");
}

#[test]
fn signed_shift_wraps_to_int32() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("(-2147483648 << 0) === -2147483648 && (-2147483648 << 1) === 0");
    assert_eq!(result, Ok(crate::value::Value::Boolean(true)));
}

#[test]
fn arithmetic_coerces_each_operand_once_in_order() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval(
            "var log = []; \
             var left = { valueOf: function() { log.push('left'); return 7; } }; \
             var right = { valueOf: function() { log.push('right'); return 2; } }; \
             left - right; log.join(',');",
        )
        .unwrap();
    assert_eq!(
        result,
        crate::value::Value::String("left,right".to_string())
    );
}

#[test]
fn bigint_addition_preserves_bigint_precision() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("0xFEDCBA9876543210n + 0xFEDCBA9876543210n === 0x1FDB97530ECA86420n"),
        Ok(crate::value::Value::Boolean(true))
    );
}

#[test]
fn null_to_primitive_method_falls_back_to_ordinary_conversion() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("({ [Symbol.toPrimitive]: null, valueOf: function() { return 2; } }) + 1"),
        Ok(crate::value::Value::Number(3.0))
    );
}

#[test]
fn bigint_addition_with_string_uses_string_concatenation() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("1n + 'x'"),
        Ok(crate::value::Value::String("1x".to_string()))
    );
}

#[test]
fn bigint_shift_with_number_throws_type_error() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("try { 1n << 1; false; } catch (error) { error instanceof TypeError; }"),
        Ok(crate::value::Value::Boolean(true))
    );
}

#[test]
fn arithmetic_symbol_conversion_throws_type_error_after_operand_evaluation() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval(
            "var trace = ''; try { (function() { trace += '1'; return { valueOf: function() { trace += '3'; return Symbol('1'); } }; })() / (function() { trace += '2'; return 1; })(); } catch (error) { trace + ':' + (error instanceof TypeError); }"
        ),
        Ok(crate::value::Value::String("123:true".to_string()))
    );
}

#[test]
fn arithmetic_symbol_conversion_uses_harness_type_error() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    assert_eq!(
        ctx.eval("try { (function() { return { valueOf: function() { return Symbol('1'); } }; })() / 1; false } catch (e) { e instanceof TypeError }") ,
        Ok(crate::value::Value::Boolean(true))
    );
}

#[test]
fn shift_symbol_conversion_does_not_evaluate_right_operand() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    assert_eq!(
        ctx.eval("try { ({ valueOf: function() { return Symbol('1'); } }) << ({ valueOf: function() { throw new Error('rhs'); } }); false } catch (e) { e instanceof TypeError }") ,
        Ok(crate::value::Value::Boolean(true))
    );
}

#[test]
fn shift_primitive_conversion_propagates_custom_error() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    assert_eq!(
        ctx.eval("function MyError() {} try { ({ [Symbol.toPrimitive]: function() { throw new MyError(); } }) << 0n; false } catch (e) { e instanceof MyError }") ,
        Ok(crate::value::Value::Boolean(true))
    );
}

#[test]
fn function_prototype_is_callable() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("typeof Function.prototype"),
        Ok(crate::value::Value::String("function".to_string()))
    );
}

#[test]
fn in_with_symbol_key_and_primitive_rhs_throws_type_error_object() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("try { Symbol.toStringTag in Symbol(); } catch (error) { typeof error === 'object' && error instanceof TypeError; }"),
        Ok(Value::Boolean(true))
    );
}
