//! String built-in - shared String.prototype object

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::wtf8::append_wtf8_surrogate;
use crate::value::{to_primitive, NativeFunction, Object, ObjectKind, PropertyFlags, Value};
use crate::Context;
use crate::JsError;

pub mod methods;

use methods::install_string_methods;

// Thread-local storage for String.prototype (created once, shared)
thread_local! {
    static STRING_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
    static STRING_ITERATOR_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the StringIteratorPrototype so the bootstrap layer can re-parent it
/// onto the JS-self-hosted %IteratorPrototype% after Array.js / Iterator.js load.
pub fn get_string_iterator_prototype() -> Option<Rc<RefCell<Object>>> {
    STRING_ITERATOR_PROTOTYPE.with(|p| p.borrow().clone())
}

/// Get the String.prototype object
pub fn get_string_prototype() -> Option<Rc<RefCell<Object>>> {
    STRING_PROTOTYPE.with(|sp| sp.borrow().clone())
}

pub(crate) fn set_string_prototype(proto: Rc<RefCell<Object>>) {
    STRING_PROTOTYPE.with(|sp| *sp.borrow_mut() = Some(proto));
}

/// Save the thread-local prototype cache (realm snapshot support)
pub(crate) fn save_string_prototype() -> Option<Rc<RefCell<Object>>> {
    get_string_prototype()
}

/// Restore the thread-local prototype cache (realm snapshot support)
pub(crate) fn restore_string_prototype(proto: Option<Rc<RefCell<Object>>>) {
    STRING_PROTOTYPE.with(|sp| *sp.borrow_mut() = proto);
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
            if (0xd800..=0xdfff).contains(&code) {
                append_wtf8_surrogate(&mut chars, code);
            } else {
                let ch = std::char::from_u32(code as u32).unwrap_or('\u{FFFD}');
                chars.push(ch);
            }
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
        "__fromCharCode",
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
        "__fromCodePoint",
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
    register_string_iterator(&string_proto_rc);

    // Note: String global is registered by date::register_type_converters
    // with proper constructor behavior for new String()
}

pub(crate) fn register_string_iterator(string_proto: &Rc<RefCell<Object>>) {
    let Some(Value::Symbol(iterator_key)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("iterator")
    else {
        return;
    };
    let Some(Value::Symbol(tag_key)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
    else {
        return;
    };
    let iterator_proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    iterator_proto.borrow_mut().prototype = crate::builtins::iterator::get_iterator_prototype();
    STRING_ITERATOR_PROTOTYPE.with(|p| *p.borrow_mut() = Some(Rc::clone(&iterator_proto)));
    iterator_proto.borrow_mut().define(
        &tag_key.property_key(),
        Value::String("String Iterator".into()),
        PropertyFlags {
            writable: false,
            enumerable: false,
            configurable: true,
            ..Default::default()
        },
    );
    let next = NativeFunction::new(move |_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        let Value::Object(object) = this_val else {
            let msg = "TypeError: not a String Iterator";
            let (err, js_err) = crate::value::error::create_js_error_with_type(&msg, "TypeError");
            crate::value::set_thrown_value(err);
            return Err(js_err);
        };
        let Some(Value::Object(state)) = object.borrow().get_own("\0stringIteratorState") else {
            let msg = "TypeError: not a String Iterator";
            let (err, js_err) = crate::value::error::create_js_error_with_type(&msg, "TypeError");
            crate::value::set_thrown_value(err);
            return Err(js_err);
        };
        let mut index = match state.borrow().get("index") {
            Some(Value::Number(index)) => index as usize,
            _ => {
                let msg = "TypeError: not a String Iterator";
                let (err, js_err) =
                    crate::value::error::create_js_error_with_type(&msg, "TypeError");
                crate::value::set_thrown_value(err);
                return Err(js_err);
            }
        };
        let chars = match state.borrow().get("string") {
            Some(Value::String(string)) => {
                crate::value::wtf8::wtf8_for_of_iterate_preserving_pairs(&string)
            }
            _ => {
                let msg = "TypeError: not a String Iterator";
                let (err, js_err) =
                    crate::value::error::create_js_error_with_type(&msg, "TypeError");
                crate::value::set_thrown_value(err);
                return Err(js_err);
            }
        };
        let mut result = Object::new(ObjectKind::Ordinary);
        if let Some(character) = chars.get(index) {
            index += 1;
            state.borrow_mut().set("index", Value::Number(index as f64));
            result.set("value", character.clone());
            result.set("done", Value::Boolean(false));
        } else {
            result.set("value", Value::Undefined);
            result.set("done", Value::Boolean(true));
        }
        Ok(Value::Object(Rc::new(RefCell::new(result))))
    });
    next.define_property(
        "name",
        Value::String("next".into()),
        PropertyFlags {
            value: Some(Value::String("next".into())),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    next.define_property(
        "length",
        Value::Number(0.0),
        PropertyFlags {
            value: Some(Value::Number(0.0)),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    iterator_proto
        .borrow_mut()
        .set("next", Value::NativeFunction(Rc::new(next)));
    string_proto.borrow_mut().set_symbol(
        &iterator_key.property_key(),
        Value::NativeFunction(Rc::new(NativeFunction::new(move |_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let string = crate::value::to_js_string(&this_val);
            let mut state = Object::new(ObjectKind::Ordinary);
            state.set("string", Value::String(string));
            state.set("index", Value::Number(0.0));
            let mut iterator =
                Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&iterator_proto));
            iterator.set(
                "\0stringIteratorState",
                Value::Object(Rc::new(RefCell::new(state))),
            );
            Ok(Value::Object(Rc::new(RefCell::new(iterator))))
        }))),
    );
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
    fn string_prototype_methods_are_non_enumerable() {
        let mut ctx = Context::new().unwrap();
        let value = ctx
            .eval("[Object.getOwnPropertyDescriptor(String.prototype, 'startsWith').enumerable, Object.getOwnPropertyDescriptor(String.prototype, 'trimStart').enumerable].join('|')")
            .unwrap();
        assert_eq!(value, Value::String("false|false".into()));
    }

    #[test]
    fn string_concat_uses_string_hint_for_bigint_wrappers() {
        let mut ctx = Context::new().unwrap();
        let value = ctx
            .eval(
                r#"
            let gets = 0;
            let original = BigInt.prototype.toString;
            Object.defineProperty(BigInt.prototype, "toString", {
                get() {
                    ++gets;
                    return function() { return original.call(this) + "foo"; };
                },
            });
            "".concat(Object(1n)) + "|" + gets;
        "#,
            )
            .unwrap();
        assert_eq!(value, Value::String("1foo|1".into()));
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

    #[test]
    fn string_iterator_exposes_spec_prototype_and_next() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "var iterator = '𝌆a'[Symbol.iterator](); \
                 [Object.getPrototypeOf(iterator)[Symbol.toStringTag], iterator.next().value, iterator.next().value, iterator.next().done].join('|')",
            )
            .unwrap();
        assert_eq!(
            result,
            Value::String("String Iterator|𝌆|a|true".to_string())
        );
    }

    #[test]
    fn array_iterator_prototype_exposes_spec_tag() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval("Object.getPrototypeOf([][Symbol.iterator]())[Symbol.toStringTag]")
            .unwrap();
        assert_eq!(result, Value::String("Array Iterator".to_string()));
    }

    #[test]
    fn object_to_string_uses_array_iterator_tag() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval("Object.prototype.toString.call([][Symbol.iterator]())")
            .unwrap();
        assert_eq!(result, Value::String("[object Array Iterator]".to_string()));
    }

    #[test]
    fn string_iterator_prototype_matches_intrinsic_iterator_ancestry() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "var itrProto = Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())); \
                 var strItrProto = Object.getPrototypeOf(''[Symbol.iterator]()); \
                 Object.getPrototypeOf(strItrProto) === itrProto",
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn string_iterator_preserves_surrogate_pair_identity() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "var pair = '\\uD834\\uDF06'; \
                 pair === pair[Symbol.iterator]().next().value",
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn string_iterator_preserves_surrogate_pair_inside_string() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "var lo = '\\uD834'; var hi = '\\uDF06'; var pair = lo + hi; \
                 var string = 'a' + pair + 'b'; \
                 var iterator = string[Symbol.iterator](); \
                 iterator.next(); iterator.next().value === pair",
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }
}
