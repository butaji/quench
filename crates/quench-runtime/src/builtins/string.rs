//! String built-in - shared String.prototype object

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_primitive, NativeFunction, Object, ObjectKind, PropertyFlags, Value};
use crate::Context;
use crate::JsError;

pub mod methods;

use methods::install_string_methods;

// Thread-local storage for String.prototype (created once, shared)
thread_local! {
    static STRING_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the String.prototype object
pub fn get_string_prototype() -> Option<Rc<RefCell<Object>>> {
    STRING_PROTOTYPE.with(|sp| sp.borrow().clone())
}

/// Convert a JS value to a number, propagating errors.
/// Unlike to_number() which returns NaN on error, this propagates the error.
fn to_number_or_err(v: &Value) -> Result<f64, JsError> {
    let prim = to_primitive(v, Some("number"))?;
    match prim {
        Value::Number(n) => Ok(n),
        Value::Boolean(true) => Ok(1.0),
        Value::Boolean(false) => Ok(0.0),
        Value::Null => Ok(0.0),
        Value::Symbol(_) => Err(JsError("Cannot convert symbol to number".to_string())),
        Value::String(s) => {
            let n = s.trim().parse::<f64>().unwrap_or(f64::NAN);
            Ok(n)
        }
        _ => Ok(f64::NAN),
    }
}

/// Register String.fromCharCode and String.fromCodePoint methods
fn register_string_static_methods(string_obj: &Rc<RefCell<Object>>) {
    let from_char_code = NativeFunction::new(|args| -> Result<Value, JsError> {
        let mut chars = String::new();
        for v in args.iter() {
            let code = to_number_or_err(v)? as u16;
            let ch = std::char::from_u32(code as u32).unwrap_or('\u{FFFD}');
            chars.push(ch);
        }
        Ok(Value::String(chars))
    });
    from_char_code.define_property(
        "name",
        Value::String("fromCharCode".to_string()),
        PropertyFlags {
            value: Some(Value::String("fromCharCode".to_string())),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    string_obj.borrow_mut().set(
        "fromCharCode",
        Value::NativeFunction(Rc::new(from_char_code)),
    );

    let from_code_point = NativeFunction::new(|args| -> Result<Value, JsError> {
        let mut chars = String::new();
        for v in args.iter() {
            let code = to_number_or_err(v)? as u32;
            let ch = std::char::from_u32(code).unwrap_or('\u{FFFD}');
            chars.push(ch);
        }
        Ok(Value::String(chars))
    });
    from_code_point.define_property(
        "name",
        Value::String("fromCodePoint".to_string()),
        PropertyFlags {
            value: Some(Value::String("fromCodePoint".to_string())),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    string_obj.borrow_mut().set(
        "fromCodePoint",
        Value::NativeFunction(Rc::new(from_code_point)),
    );
}

/// Register the String object and String.prototype
pub fn register_string(_ctx: &mut Context) {
    let string_obj = Object::new(ObjectKind::Ordinary);
    let string_obj = Rc::new(RefCell::new(string_obj));

    register_string_static_methods(&string_obj);

    // Create String.prototype and attach methods
    let string_proto = Object::new(ObjectKind::Ordinary);
    let string_proto_rc = Rc::new(RefCell::new(string_proto));

    install_string_methods(&string_proto_rc);
    // String.prototype must inherit from Object.prototype.
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        string_proto_rc.borrow_mut().prototype = Some(object_proto);
    }
    string_obj
        .borrow_mut()
        .set("prototype", Value::Object(Rc::clone(&string_proto_rc)));

    STRING_PROTOTYPE.with(|sp| {
        *sp.borrow_mut() = Some(Rc::clone(&string_proto_rc));
    });

    // Note: String global is registered by date::register_type_converters
    // with proper constructor behavior for new String()
}

#[cfg(test)]
mod tests {
    use crate::Context;
    use crate::Value;

    #[test]
    fn test_string_subclass_explicit_super() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                r#"
            class MyStr extends String {
                constructor() {
                    super("test262");
                }
            }
            var s = new MyStr();
            [s.hasOwnProperty("length"), s.toString(), s.length];
        "#,
            )
            .unwrap();
        match r {
            Value::Object(arr_rc) => {
                let arr = arr_rc.borrow();
                assert!(
                    matches!(arr.get("0"), Some(Value::Boolean(true))),
                    "expected s.hasOwnProperty('length') to be true, got {:?}",
                    arr.get("0")
                );
                assert!(
                    matches!(arr.get("1"), Some(Value::String(s)) if s == "test262"),
                    "expected s.toString() to be 'test262', got {:?}",
                    arr.get("1")
                );
                assert!(
                    matches!(arr.get("2"), Some(Value::Number(n)) if (n - 7.0).abs() < 1e-10),
                    "expected s.length to be 7, got {:?}",
                    arr.get("2")
                );
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    #[test]
    fn test_string_subclass_no_args() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                r#"
            class S extends String {}
            var s = new S();
            [s.hasOwnProperty("length"), s.length];
        "#,
            )
            .unwrap();
        match r {
            Value::Object(arr_rc) => {
                let arr = arr_rc.borrow();
                assert!(
                    matches!(arr.get("0"), Some(Value::Boolean(true))),
                    "expected s.hasOwnProperty('length') to be true, got {:?}",
                    arr.get("0")
                );
                assert!(
                    matches!(arr.get("1"), Some(Value::Number(n)) if (n - 0.0).abs() < 1e-10),
                    "expected s.length to be 0, got {:?}",
                    arr.get("1")
                );
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    #[test]
    fn test_string_subclass_trim() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                r#"
            class S extends String {}
            var s = new S(' test262 ');
            s.trim();
        "#,
            )
            .unwrap();
        assert_eq!(r, Value::String("test262".to_string()));
    }

    #[test]
    fn test_string_new_length_own_property() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                r#"
            var s = new String("test262");
            [s.hasOwnProperty("length"), s.length, s.toString()];
        "#,
            )
            .unwrap();
        match r {
            Value::Object(arr_rc) => {
                let arr = arr_rc.borrow();
                assert!(
                    matches!(arr.get("0"), Some(Value::Boolean(true))),
                    "expected s.hasOwnProperty('length') to be true, got {:?}",
                    arr.get("0")
                );
                assert!(
                    matches!(arr.get("1"), Some(Value::Number(n)) if (n - 7.0).abs() < 1e-10),
                    "expected s.length to be 7, got {:?}",
                    arr.get("1")
                );
                assert!(
                    matches!(arr.get("2"), Some(Value::String(s)) if s == "test262"),
                    "expected s.toString() to be 'test262', got {:?}",
                    arr.get("2")
                );
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }
}
