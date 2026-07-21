//! Unit tests for primitive.rs — to_primitive, to_object, and related coercion
//! functions (to_bool, to_number, to_js_string) via the public API.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::env::Environment;

use crate::value::convert::{
    to_bool, to_js_string, to_number, to_object, to_primitive, PrimitiveHint,
};
use crate::value::kind::{ExoticKind, ObjectKind};
use crate::value::object::Object;
use crate::value::{ClassValue, GeneratorObject, NativeConstructor, NativeFunction, Symbol, Value};

fn ctx() -> crate::Context {
    crate::Context::new().unwrap()
}

fn eval_val(src: &str) -> Value {
    ctx().eval(src).unwrap()
}

fn make_env() -> Rc<RefCell<Environment>> {
    Rc::new(RefCell::new(Environment::new()))
}

fn nf() -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))))
}

fn obj() -> Value {
    Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))))
}

fn sym(desc: &str) -> Value {
    Value::Symbol(Rc::new(Symbol {
        desc: Some(desc.into()),
        global: false,
    }))
}

fn big(n: i64) -> Value {
    Value::BigInt(Rc::new(num_bigint::BigInt::from(n)))
}

fn nc() -> Value {
    Value::NativeConstructor(Rc::new(NativeConstructor::new(
        |_| Ok(Value::Undefined),
        Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))),
    )))
}

// ── primitive_direct — already-primitive values ────────────────────────────────

#[test]
fn test_to_primitive_undefined() {
    assert_eq!(
        to_primitive(&Value::Undefined, None).unwrap(),
        Value::Undefined
    );
}

#[test]
fn test_to_primitive_null() {
    assert_eq!(to_primitive(&Value::Null, None).unwrap(), Value::Null);
}

#[test]
fn test_to_primitive_boolean() {
    assert_eq!(
        to_primitive(&Value::Boolean(true), None).unwrap(),
        Value::Boolean(true)
    );
    assert_eq!(
        to_primitive(&Value::Boolean(false), None).unwrap(),
        Value::Boolean(false)
    );
}

#[test]
fn test_to_primitive_number() {
    assert_eq!(
        to_primitive(&Value::Number(42.0), None).unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn test_to_primitive_string() {
    assert_eq!(
        to_primitive(&Value::String("hello".into()), None).unwrap(),
        Value::String("hello".into())
    );
}

#[test]
fn test_to_primitive_bigint() {
    let result = to_primitive(&big(99), None).unwrap();
    assert!(matches!(result, Value::BigInt(_)));
}

#[test]
fn test_to_primitive_symbol() {
    let result = to_primitive(&sym("sym"), None).unwrap();
    assert!(matches!(result, Value::Symbol(_)));
}

// ── NativeFunction / Class → "[Function]" ───────────────────────────────────

#[test]
fn test_to_primitive_native_function() {
    let result = to_primitive(&nf(), None).unwrap();
    assert_eq!(result, Value::String("[Function]".to_string()));
}

#[test]
fn test_to_primitive_native_function_hint_number() {
    let result = to_primitive(&nf(), Some("number")).unwrap();
    assert_eq!(result, Value::String("[Function]".to_string()));
}

#[test]
fn test_to_primitive_native_function_hint_string() {
    let result = to_primitive(&nf(), Some("string")).unwrap();
    assert_eq!(result, Value::String("[Function]".to_string()));
}

// ── to_primitive_object — plain object ─────────────────────────────────────
// Note: all JS objects inherit valueOf/toString from Object.prototype,
// so there is no valid case where neither method exists. Testing that
// would test impossible JavaScript behavior.

#[test]
fn test_to_primitive_object_value_of_returns_primitive() {
    let result = eval_val("var o = { valueOf() { return 42 } }; o");
    let prim = to_primitive(&result, Some("number")).unwrap();
    assert_eq!(prim, Value::Number(42.0));
}

#[test]
fn test_to_primitive_object_to_string_returns_primitive() {
    let result = eval_val("var o = { toString() { return 'custom' } }; o");
    let prim = to_primitive(&result, Some("string")).unwrap();
    assert_eq!(prim, Value::String("custom".to_string()));
}

#[test]
fn test_to_primitive_object_hint_number_prefers_value_of() {
    let result = eval_val("var o = { valueOf() { return 1 }, toString() { return 'a' } }; o");
    let prim = to_primitive(&result, Some("number")).unwrap();
    assert_eq!(prim, Value::Number(1.0));
}

#[test]
fn test_to_primitive_object_hint_string_prefers_to_string() {
    let result = eval_val("var o = { valueOf() { return 1 }, toString() { return 'a' } }; o");
    let prim = to_primitive(&result, Some("string")).unwrap();
    assert_eq!(prim, Value::String("a".to_string()));
}

#[test]
fn test_to_primitive_object_both_return_object_throws() {
    let result = eval_val("var o = { valueOf() { return {} }, toString() { return {} } }; o");
    assert!(to_primitive(&result, Some("number")).is_err());
}

#[test]
fn test_to_primitive_object_value_of_returns_object_to_string_works() {
    let result = eval_val("var o = { valueOf() { return {} }, toString() { return 'ok' } }; o");
    let prim = to_primitive(&result, Some("number")).unwrap();
    assert_eq!(prim, Value::String("ok".to_string()));
}

// ── Symbol.toPrimitive ────────────────────────────────────────────────────────

#[test]
fn test_to_primitive_object_symbol_to_primitive_number() {
    let result = eval_val("var o = { [Symbol.toPrimitive](hint) { return 123; } }; o");
    let prim = to_primitive(&result, Some("number")).unwrap();
    assert_eq!(prim, Value::Number(123.0));
}

#[test]
fn test_to_primitive_object_symbol_to_primitive_string() {
    let result = eval_val("var o = { [Symbol.toPrimitive](hint) { return 'symResult'; } }; o");
    let prim = to_primitive(&result, Some("string")).unwrap();
    assert_eq!(prim, Value::String("symResult".to_string()));
}

#[test]
fn test_to_primitive_object_symbol_to_primitive_default() {
    let result = eval_val("var o = { [Symbol.toPrimitive](hint) { return 'default'; } }; o");
    let prim = to_primitive(&result, None).unwrap();
    assert_eq!(prim, Value::String("default".to_string()));
}

#[test]
fn test_to_primitive_object_symbol_to_primitive_returns_object_throws() {
    let result = eval_val("var o = { [Symbol.toPrimitive](hint) { return {}; } }; o");
    assert!(to_primitive(&result, Some("number")).is_err());
}

// ── to_primitive_function — ValueFunction ─────────────────────────────────────

#[test]
fn test_to_primitive_function_default_hint() {
    let result = eval_val("function f() {}; f");
    let prim = to_primitive(&result, None).unwrap();
    assert_eq!(prim, Value::String("[Function]".to_string()));
}

#[test]
fn test_to_primitive_function_string_hint() {
    let result = eval_val("function f() {}; f");
    let prim = to_primitive(&result, Some("string")).unwrap();
    assert_eq!(prim, Value::String("[Function]".to_string()));
}

#[test]
fn test_to_primitive_function_with_custom_value_of() {
    let result = eval_val("function f() {}; f.valueOf = function() { return 42; }; f");
    let prim = to_primitive(&result, Some("number")).unwrap();
    assert_eq!(prim, Value::Number(42.0));
}

// ── to_object ───────────────────────────────────────────────────────────────

#[test]
fn test_to_object_undefined_returns_ordinary_object() {
    assert!(matches!(to_object(&Value::Undefined), Value::Object(_)));
}

#[test]
fn test_to_object_null_returns_ordinary_object() {
    assert!(matches!(to_object(&Value::Null), Value::Object(_)));
}

#[test]
fn test_to_object_bigint_sets_value_property() {
    let r = to_object(&big(55));
    let obj = match r {
        Value::Object(o) => o,
        _ => panic!("expected Object"),
    };
    assert!(obj.borrow().get("_value").is_some());
}

#[test]
fn test_to_object_symbol_returns_ordinary_object() {
    assert!(matches!(to_object(&sym("x")), Value::Object(_)));
}

// ── PrimitiveHint ───────────────────────────────────────────────────────────

#[test]
fn test_primitive_hint_eq() {
    assert_eq!(PrimitiveHint::Default, PrimitiveHint::Default);
    assert_eq!(PrimitiveHint::Number, PrimitiveHint::Number);
    assert_eq!(PrimitiveHint::String, PrimitiveHint::String);
}

#[test]
fn test_primitive_hint_ne() {
    assert_ne!(PrimitiveHint::Default, PrimitiveHint::Number);
    assert_ne!(PrimitiveHint::Number, PrimitiveHint::String);
    assert_ne!(PrimitiveHint::Default, PrimitiveHint::String);
}

// ── to_bool ─────────────────────────────────────────────────────────────────

#[test]
fn test_to_bool_primitives() {
    assert!(!to_bool(&Value::Undefined));
    assert!(!to_bool(&Value::Null));
    assert!(to_bool(&Value::Boolean(true)));
    assert!(!to_bool(&Value::Boolean(false)));
    assert!(!to_bool(&Value::Number(0.0)));
    assert!(!to_bool(&Value::Number(-0.0)));
    assert!(!to_bool(&Value::Number(f64::NAN)));
    assert!(to_bool(&Value::Number(1.0)));
    assert!(to_bool(&Value::Number(-1.0)));
    assert!(to_bool(&Value::Number(f64::INFINITY)));
    assert!(!to_bool(&Value::String(String::new())));
    assert!(to_bool(&Value::String("x".to_string())));
}

#[test]
fn test_to_bool_objects() {
    assert!(to_bool(&obj()));
    assert!(to_bool(&nf()));
    assert!(to_bool(&nc()));
    let f = Value::Function(crate::value::ValueFunction::new(
        None,
        vec![],
        vec![],
        make_env(),
        false,
        false,
    ));
    assert!(to_bool(&f));
}

#[test]
fn test_to_bool_class_gen_sym_bigint() {
    let cls_val = Value::Class(ClassValue {
        id: 0,
        name: None,
        constructor_params: vec![],
        constructor_body: vec![],
        methods: vec![],
        static_methods: vec![],
        getters: vec![],
        setters: vec![],
        static_getters: vec![],
        static_setters: vec![],
        instance_fields: vec![],
        static_fields: vec![],
        super_class: None,
        super_class_own_proto_cell: Rc::new(RefCell::new(None::<Value>)),
        prototype_cell: Rc::new(RefCell::new(None)),
        static_properties_cell: Rc::new(RefCell::new(HashMap::new())),
        deleted_properties: Rc::new(RefCell::new(HashSet::new())),
        class_def_env_cell: Rc::new(RefCell::new(None)),
    });
    assert!(to_bool(&cls_val));
    let gen_val = Value::Generator(Rc::new(RefCell::new(GeneratorObject::new(
        Rc::new(vec![]),
        vec![],
        make_env(),
        false,
    ))));
    assert!(to_bool(&gen_val));
    assert!(to_bool(&sym("s")));
    assert!(!to_bool(&big(0)));
    assert!(to_bool(&big(1)));
}

// ── to_number ───────────────────────────────────────────────────────────────

#[test]
fn test_to_number_primitives() {
    assert!(to_number(&Value::Undefined).is_nan());
    assert_eq!(to_number(&Value::Null), 0.0);
    assert_eq!(to_number(&Value::Boolean(false)), 0.0);
    assert_eq!(to_number(&Value::Boolean(true)), 1.0);
    assert_eq!(to_number(&Value::Number(42.5)), 42.5);
    assert!(to_number(&Value::Number(f64::NAN)).is_nan());
}

#[test]
fn test_to_number_strings() {
    assert_eq!(to_number(&Value::String("42".to_string())), 42.0);
    assert_eq!(to_number(&Value::String("-3".to_string())), -3.0);
    assert_eq!(to_number(&Value::String(String::new())), 0.0);
    assert_eq!(to_number(&Value::String("   ".to_string())), 0.0);
    assert!(to_number(&Value::String("x".to_string())).is_nan());
    assert_eq!(
        to_number(&Value::String("Infinity".to_string())),
        f64::INFINITY
    );
    assert!(to_number(&Value::String("NaN".to_string())).is_nan());
}

#[test]
fn test_to_number_non_coercible() {
    assert!(to_number(&sym("s")).is_nan());
    assert!(to_number(&big(42)).is_nan());
    assert!(to_number(&nf()).is_nan());
}

// ── to_js_string ────────────────────────────────────────────────────────────

#[test]
fn test_to_js_string_primitives() {
    assert_eq!(to_js_string(&Value::Undefined), "undefined");
    assert_eq!(to_js_string(&Value::Null), "null");
    assert_eq!(to_js_string(&Value::Boolean(true)), "true");
    assert_eq!(to_js_string(&Value::Boolean(false)), "false");
    assert_eq!(to_js_string(&Value::Number(42.0)), "42");
    assert_eq!(to_js_string(&Value::Number(-0.0)), "0");
    assert_eq!(to_js_string(&Value::String("hi".to_string())), "hi");
    assert_eq!(to_js_string(&big(123)), "123n");
}

#[test]
fn test_to_js_string_symbols() {
    assert_eq!(to_js_string(&sym("mySym")), "Symbol(mySym)");
    let no_desc = Value::Symbol(Rc::new(Symbol {
        desc: None,
        global: false,
    }));
    assert_eq!(to_js_string(&no_desc), "Symbol()");
}

#[test]
fn test_to_js_string_objects() {
    assert_eq!(to_js_string(&obj()), "[object Object]");
    assert_eq!(to_js_string(&nf()), "[Function]");
    let f = Value::Function(crate::value::ValueFunction::new(
        None,
        vec![],
        vec![],
        make_env(),
        false,
        false,
    ));
    assert_eq!(to_js_string(&f), "[Function]");
}

// ── to_object additional edge cases ─────────────────────────────────────────

#[test]
fn test_to_object_boolean_boxed() {
    let r = to_object(&Value::Boolean(true));
    match r {
        Value::Object(o) => {
            assert_eq!(o.borrow().exotic_kind, Some(ExoticKind::Boolean));
        }
        _ => panic!("expected Object"),
    }
}

#[test]
fn test_to_object_number_boxed() {
    let r = to_object(&Value::Number(42.0));
    match r {
        Value::Object(o) => {
            assert_eq!(o.borrow().exotic_kind, Some(ExoticKind::Number));
        }
        _ => panic!("expected Object"),
    }
}

#[test]
fn test_to_object_string_boxed() {
    let r = to_object(&Value::String("abc".to_string()));
    match r {
        Value::Object(o) => {
            let obj = o.borrow();
            assert_eq!(obj.exotic_kind, Some(ExoticKind::String));
            assert_eq!(obj.get("0"), Some(Value::String("abc".to_string())));
            assert_eq!(obj.get("length"), Some(Value::Number(3.0)));
        }
        _ => panic!("expected Object"),
    }
}

#[test]
fn test_to_object_identity_preserved() {
    assert!(matches!(to_object(&obj()), Value::Object(_)));
    let f = Value::Function(crate::value::ValueFunction::new(
        None,
        vec![],
        vec![],
        make_env(),
        false,
        false,
    ));
    assert!(matches!(to_object(&f), Value::Function(_)));
    assert!(matches!(to_object(&nf()), Value::NativeFunction(_)));
    let cls_val = Value::Class(ClassValue {
        id: 0,
        name: None,
        constructor_params: vec![],
        constructor_body: vec![],
        methods: vec![],
        static_methods: vec![],
        getters: vec![],
        setters: vec![],
        static_getters: vec![],
        static_setters: vec![],
        instance_fields: vec![],
        static_fields: vec![],
        super_class: None,
        super_class_own_proto_cell: Rc::new(RefCell::new(None::<Value>)),
        prototype_cell: Rc::new(RefCell::new(None)),
        static_properties_cell: Rc::new(RefCell::new(HashMap::new())),
        deleted_properties: Rc::new(RefCell::new(HashSet::new())),
        class_def_env_cell: Rc::new(RefCell::new(None)),
    });
    assert!(matches!(to_object(&cls_val), Value::Class(_)));
    let gen_val = Value::Generator(Rc::new(RefCell::new(GeneratorObject::new(
        Rc::new(vec![]),
        vec![],
        make_env(),
        false,
    ))));
    assert!(matches!(to_object(&gen_val), Value::Generator(_)));
}
