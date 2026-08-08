use super::*;

// ── JsError type ─────────────────────────────────────────────────────

#[test]
fn js_error_new() {
    let err = JsError::new("something went wrong");
    assert_eq!(err.0, "something went wrong");
}

#[test]
fn js_error_new_from_owned_string() {
    let err = JsError::new("owned".to_string());
    assert_eq!(err.0, "owned");
}

#[test]
fn js_error_debug() {
    let err = JsError("hello".to_string());
    assert_eq!(format!("{:?}", err), "JsError(\"hello\")");
}

#[test]
fn js_error_display() {
    let err = JsError("hello".to_string());
    assert_eq!(format!("{}", err), "hello");
}

#[test]
fn js_error_from_str() {
    let err: JsError = "test error".into();
    assert_eq!(err.0, "test error");
}

#[test]
fn js_error_from_string() {
    let err: JsError = "test error".to_string().into();
    assert_eq!(err.0, "test error");
}

#[test]
fn js_error_std_error_trait() {
    let err = JsError("std error".to_string());
    // The Error trait requires Display; just verify trait works
    assert_eq!(err.to_string(), "std error");
}

// ── Thrown value ─────────────────────────────────────────────────────

#[test]
fn thrown_value_set_then_take() {
    let val = Value::Boolean(true);
    set_thrown_value(val.clone());
    assert_eq!(take_thrown_value(), Some(val));
}

#[test]
fn thrown_value_take_clears() {
    set_thrown_value(Value::Boolean(true));
    assert!(take_thrown_value().is_some());
    assert!(take_thrown_value().is_none());
}

#[test]
fn thrown_value_get_does_not_consume() {
    let val = Value::Number(42.0);
    set_thrown_value(val.clone());
    assert_eq!(get_thrown_value(), Some(val.clone()), "first peek");
    assert_eq!(
        get_thrown_value(),
        Some(val.clone()),
        "second peek unchanged"
    );
    assert_eq!(take_thrown_value(), Some(val), "take consumes");
    assert!(take_thrown_value().is_none(), "empty after take");
}

#[test]
fn thrown_value_overwrite() {
    set_thrown_value(Value::Number(1.0));
    set_thrown_value(Value::Number(2.0));
    assert_eq!(take_thrown_value(), Some(Value::Number(2.0)));
}

// ── create_js_error ──────────────────────────────────────────────────

#[test]
fn create_js_error_full() {
    let (_val, js_err) = create_js_error("test message");
    assert_eq!(js_err.0, "Error: test message");

    // Check the thrown value's structure (Value::Object does not impl Eq by pointer)
    let thrown = take_thrown_value().expect("thrown value set");
    match &thrown {
        Value::Object(obj) => {
            assert_eq!(
                obj.borrow().get("message"),
                Some(Value::String("test message".to_string()))
            );
            assert_eq!(
                obj.borrow().get("name"),
                Some(Value::String("Error".to_string()))
            );
        }
        other => panic!("expected Value::Object, got {:?}", other),
    }
}

// ── create_js_error_with_type ────────────────────────────────────────

#[test]
fn create_js_error_with_type_prefix() {
    let (_val, js_err) = create_js_error_with_type("bad", "TypeError");
    assert_eq!(js_err.0, "TypeError: bad");
}

#[test]
fn create_js_error_with_type_syntax_error() {
    let (_val, js_err) = create_js_error_with_type("unexpected token", "SyntaxError");
    assert_eq!(js_err.0, "SyntaxError: unexpected token");
}

#[test]
fn create_js_error_with_type_thrown_object() {
    let (_val, _js_err) = create_js_error_with_type("err", "RangeError");
    let thrown = take_thrown_value().expect("thrown value should be set");
    match &thrown {
        Value::Object(obj) => {
            assert_eq!(
                obj.borrow().get("name"),
                Some(Value::String("RangeError".to_string()))
            );
            assert_eq!(
                obj.borrow().get("message"),
                Some(Value::String("err".to_string()))
            );
        }
        other => panic!("expected Value::Object, got {:?}", other),
    }
}

// ── register_error_constructor ───────────────────────────────────────

#[test]
fn register_error_constructor_uses_error_prototype() {
    let mut ctor = Object::new(ObjectKind::Ordinary);
    ctor.set("name", Value::String("Error".to_string()));
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    register_error_constructor(
        Value::Object(Rc::new(RefCell::new(ctor))),
        Rc::clone(&proto),
    );

    let (val, _js_err) = create_js_error("proto test");
    match &val {
        Value::Object(obj) => {
            assert!(
                obj.borrow().prototype.is_some(),
                "Error object should have prototype when registered"
            );
        }
        other => panic!("expected Value::Object, got {:?}", other),
    }
    take_thrown_value();
}

#[test]
fn register_error_constructor_uses_type_error_prototype() {
    let mut ctor = Object::new(ObjectKind::Ordinary);
    ctor.set("name", Value::String("TypeError".to_string()));
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    register_error_constructor(
        Value::Object(Rc::new(RefCell::new(ctor))),
        Rc::clone(&proto),
    );

    let (val, _js_err) = create_js_error_with_type("te", "TypeError");
    match &val {
        Value::Object(obj) => {
            assert!(
                obj.borrow().prototype.is_some(),
                "TypeError object should have prototype when registered"
            );
        }
        other => panic!("expected Value::Object, got {:?}", other),
    }
    take_thrown_value();
}

#[test]
fn register_error_constructor_unknown_name_fallback_to_error() {
    let mut ctor = Object::new(ObjectKind::Ordinary);
    ctor.set("name", Value::String("CustomError".to_string()));
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    register_error_constructor(
        Value::Object(Rc::new(RefCell::new(ctor))),
        Rc::clone(&proto),
    );

    let (val, _js_err) = create_js_error("fallback");
    match &val {
        Value::Object(obj) => {
            assert!(
                obj.borrow().prototype.is_some(),
                "unknown name should fall back to ERROR_PROTOTYPE"
            );
        }
        other => panic!("expected Value::Object, got {:?}", other),
    }
    take_thrown_value();
}

// ── Test262Error ─────────────────────────────────────────────────────

#[test]
fn test262_error_set_and_get() {
    set_test262_error(Value::Null);
    assert_eq!(get_test262_error(), Some(Value::Null));
}

#[test]
fn test262_error_overwrite() {
    set_test262_error(Value::Boolean(false));
    set_test262_error(Value::Boolean(true));
    assert_eq!(get_test262_error(), Some(Value::Boolean(true)));
}
