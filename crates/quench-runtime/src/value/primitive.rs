//! ToPrimitive and ToObject — the core spec conversion operations.

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::JsError;
use crate::value::Value;

#[cfg(test)]
mod tests;

// ─── PrimitiveHint ───────────────────────────────────────────────────────────

/// Hint for ToPrimitive conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveHint {
    Default,
    Number,
    String,
}

// ─── to_primitive ────────────────────────────────────────────────────────────

/// Convert a Value to a primitive using JavaScript's ToPrimitive abstract operation.
pub fn to_primitive(value: &Value, hint: Option<&str>) -> Result<Value, JsError> {
    if let Some(prim) = primitive_direct(value) {
        return Ok(prim);
    }
    match value {
        Value::Object(obj) => to_primitive_object(obj, hint),
        Value::Function(f) => to_primitive_function(&Rc::new(f.clone()), hint),
        Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Generator(_)
        | Value::Class(_) => Ok(Value::String("[Function]".to_string())),
        _ => Ok(Value::Undefined),
    }
}

fn primitive_direct(v: &Value) -> Option<Value> {
    match v {
        Value::Undefined => Some(Value::Undefined),
        Value::Null => Some(Value::Null),
        Value::Boolean(b) => Some(Value::Boolean(*b)),
        Value::Number(n) => Some(Value::Number(*n)),
        Value::String(s) => Some(Value::String(s.clone())),
        Value::Symbol(s) => Some(Value::Symbol(s.clone())),
        Value::BigInt(bi) => Some(Value::BigInt(Rc::clone(bi))),
        _ => None,
    }
}

/// ToPrimitive for a user-defined JS Function. JS functions inherit
/// valueOf/toString from Object.prototype, but calling those on a function
/// recurses (valueOf returns `this`, toString returns "[object Function]").
/// We only honour OWN properties (e.g. `f.valueOf = function() { return 1 }`).
/// Inherited methods fall back to a textual representation.
fn to_primitive_function(
    f: &Rc<crate::value::function::ValueFunction>,
    hint: Option<&str>,
) -> Result<Value, JsError> {
    let hint = match hint {
        Some("string") => PrimitiveHint::String,
        Some("number") => PrimitiveHint::Number,
        _ => PrimitiveHint::Default,
    };

    let (first, second) = match hint {
        PrimitiveHint::Default | PrimitiveHint::Number => ("valueOf", "toString"),
        PrimitiveHint::String => ("toString", "valueOf"),
    };

    // Check own properties first (user-defined override)
    let first_method = f.get_property(first);
    let second_method = f.get_property(second);

    let this_val = Value::Function((**f).clone());

    let mut first_was_object = false;
    let mut first_found = false;
    if let Some(ref m) = first_method {
        first_found = true;
        let v = crate::eval::call_value_with_this(m.clone(), vec![], this_val.clone())?;
        if !matches!(v, Value::Object(_) | Value::Function(_)) {
            return Ok(v);
        }
        first_was_object = true;
    }
    let mut second_found = false;
    if let Some(ref m) = second_method {
        second_found = true;
        let v = crate::eval::call_value_with_this(m.clone(), vec![], this_val.clone())?;
        if !matches!(v, Value::Object(_) | Value::Function(_)) {
            return Ok(v);
        }
        if first_was_object {
            let (err, _) = crate::value::create_js_error_with_type(
                "Cannot convert object to primitive value",
                "TypeError",
            );
            crate::value::set_thrown_value(err);
            return Err(crate::value::JsError("TypeError".to_string()));
        }
    }

    // No own properties found — try inherited toString/valueOf from
    // Function.prototype (the [[Prototype]] of function objects), not from
    // the function's `.prototype` property (which is the prototype for
    // instances created via `new` and has Object.prototype as its own
    // [[Prototype]]).
    let func_proto = crate::builtins::get_function_prototype();
    if !first_found {
        if let Some(ref fp) = func_proto {
            if let Some(m) = fp.borrow().get(first) {
                let v = crate::eval::call_value_with_this(m, vec![], this_val.clone())?;
                if !matches!(v, Value::Object(_) | Value::Function(_)) {
                    return Ok(v);
                }
            }
        }
    }
    if !second_found {
        if let Some(ref fp) = func_proto {
            if let Some(m) = fp.borrow().get(second) {
                let v = crate::eval::call_value_with_this(m, vec![], this_val.clone())?;
                if !matches!(v, Value::Object(_) | Value::Function(_)) {
                    return Ok(v);
                }
            }
        }
    }

    // Final fallback: use source_text so `String(f)` and `f.toString()` agree.
    Ok(Value::String(f.source_text()))
}

fn to_primitive_object(
    obj: &Rc<RefCell<crate::value::object::Object>>,
    hint: Option<&str>,
) -> Result<Value, JsError> {
    let hint = match hint {
        Some("string") => PrimitiveHint::String,
        Some("number") => PrimitiveHint::Number,
        _ => PrimitiveHint::Default,
    };

    // Check Symbol.toPrimitive first.
    if let Some(v) = try_to_primitive_symbol(obj, hint)? {
        return Ok(v);
    }

    let (first, second) = match hint {
        PrimitiveHint::Default | PrimitiveHint::Number => ("valueOf", "toString"),
        PrimitiveHint::String => ("toString", "valueOf"),
    };

    if let Some(result) = try_method(obj, first)? {
        return Ok(result);
    }
    if let Some(result) = try_method(obj, second)? {
        return Ok(result);
    }

    // ES 7.1.1.1 OrdinaryToPrimitive: both methods were called and returned
    // non-primitive (object) values → throw TypeError (covers the case where
    // exactly one method exists and returns an object, and the case where
    // both exist and both return objects, and the case where neither exists).
    let (err, _) = crate::value::create_js_error_with_type(
        "Cannot convert object to primitive value",
        "TypeError",
    );
    crate::value::set_thrown_value(err);
    Err(crate::value::JsError("TypeError".to_string()))
}

fn try_to_primitive_symbol(
    obj: &Rc<RefCell<crate::value::object::Object>>,
    hint: PrimitiveHint,
) -> Result<Option<Value>, JsError> {
    let Some(to_prim_symbol) = crate::builtins::symbol::get_well_known_symbol_no_ctx("toPrimitive")
    else {
        return Ok(None);
    };
    let Value::Symbol(symbol_key) = to_prim_symbol else {
        return Ok(None);
    };
    let to_prim_method =
        crate::eval::member::eval_object_member(obj, &symbol_key.property_key(), None)?;
    if matches!(to_prim_method, Value::Undefined | Value::Null) {
        return Ok(None);
    }
    let hint_str = match hint {
        PrimitiveHint::Default => "default",
        PrimitiveHint::Number => "number",
        PrimitiveHint::String => "string",
    };
    let arg = Value::String(hint_str.to_string());
    let this_val = Value::Object(Rc::clone(obj));
    let result = crate::eval::call_value_with_this(to_prim_method, vec![arg], this_val)?;
    if !matches!(result, Value::Object(_)) {
        return Ok(Some(result));
    }
    let (err_val, js_err) = crate::value::error::create_js_error_with_type(
        "Cannot convert object to primitive value",
        "TypeError",
    );
    crate::value::set_thrown_value(err_val);
    Err(js_err)
}

fn try_method(
    obj: &Rc<RefCell<crate::value::object::Object>>,
    method_name: &str,
) -> Result<Option<Value>, JsError> {
    let method = obj.borrow().get(method_name);
    let Some(method) = method else {
        return Ok(None);
    };
    let this_val = Value::Object(Rc::clone(obj));
    match &method {
        Value::NativeFunction(nf) => {
            let result = nf.call(this_val, vec![])?;
            if !matches!(result, Value::Object(_)) {
                return Ok(Some(result));
            }
            Ok(None)
        }
        Value::Function(_) => {
            let result = crate::eval::call_value_with_this(method.clone(), vec![], this_val)?;
            if !matches!(result, Value::Object(_)) {
                return Ok(Some(result));
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

// ─── to_object ──────────────────────────────────────────────────────────────

/// ToObject per ECMAScript spec — converts primitives to boxed objects
pub fn to_object(value: &Value) -> Result<Value, JsError> {
    match value {
        Value::Undefined | Value::Null => {
            let (err, js_err) = crate::value::error::create_js_error_with_type(
                "Cannot convert undefined or null to object",
                "TypeError",
            );
            crate::value::set_thrown_value(err);
            Err(js_err)
        }
        Value::Boolean(_b) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::Boolean);
            crate::builtins::object::set_boxed_value(&mut obj, value.clone());
            obj.prototype = wrapper_prototype("Boolean");
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }
        Value::Number(_n) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::Number);
            crate::builtins::object::set_boxed_value(&mut obj, value.clone());
            obj.prototype = wrapper_prototype("Number");
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }
        Value::String(s) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::String);
            obj.prototype = crate::builtins::string::get_string_prototype();
            crate::builtins::object::set_boxed_value(&mut obj, Value::String(s.clone()));
            obj.elements = s.chars().map(|ch| Value::String(ch.to_string())).collect();
            for (index, value) in obj.elements.iter().enumerate() {
                let key = index.to_string();
                obj.properties.insert(key.clone(), value.clone());
                obj.descriptors.insert(
                    key,
                    crate::value::object::helpers::PropertyFlags {
                        writable: false,
                        enumerable: true,
                        configurable: false,
                        value: Some(value.clone()),
                    },
                );
            }
            obj.properties
                .insert("length".to_string(), Value::Number(s.len() as f64));
            obj.descriptors.insert(
                "length".to_string(),
                crate::value::object::helpers::PropertyFlags {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    value: Some(Value::Number(s.len() as f64)),
                },
            );
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Generator(_)
        | Value::Class(_) => Ok(value.clone()),
        Value::Symbol(_s) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            crate::builtins::object::set_boxed_value(&mut obj, value.clone());
            obj.prototype = wrapper_prototype("Symbol");
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }
        Value::BigInt(_) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::BigInt);
            crate::builtins::object::set_boxed_value(&mut obj, value.clone());
            obj.prototype = wrapper_prototype("BigInt");
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }
    }
}

fn wrapper_prototype(name: &str) -> Option<Rc<RefCell<crate::value::object::Object>>> {
    match crate::context::get_global_from_context(name) {
        Some(Value::NativeConstructor(constructor)) => Some(Rc::clone(&constructor.prototype)),
        Some(Value::NativeFunction(function)) => function.prototype.borrow().clone(),
        _ => None,
    }
}
