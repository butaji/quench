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
        Value::NativeFunction(_) | Value::NativeConstructor(_) | Value::Generator(_) | Value::Class(_) => {
            Ok(Value::String("[Function]".to_string()))
        }
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

    // Only check OWN properties — walking the prototype chain and calling
    // Object.prototype.valueOf/toString on a function recurses infinitely.
    let first_method = f.get_property(first);
    let second_method = f.get_property(second);

    let this_val = Value::Function((**f).clone());

    let mut first_was_object = false;
    if let Some(m) = first_method {
        let v = crate::eval::call_value_with_this(m, vec![], this_val.clone())?;
        if !matches!(v, Value::Object(_)) {
            return Ok(v);
        }
        first_was_object = true;
    }
    if let Some(m) = second_method {
        let v = crate::eval::call_value_with_this(m, vec![], this_val.clone())?;
        if !matches!(v, Value::Object(_)) {
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
    // Fallback: match to_js_string's representation for Value::Function so that
    // `f + ""` and `f.toString() + ""` agree.
    Ok(Value::String("[Function]".to_string()))
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

    let first_called = obj.borrow().get(first).is_some();
    let second_called = obj.borrow().get(second).is_some();

    if let Some(result) = try_method(obj, first)? {
        return Ok(result);
    }
    if let Some(result) = try_method(obj, second)? {
        return Ok(result);
    }

    // Both methods were called and returned non-primitive (object) values —
    // per ES spec, ToPrimitive must throw TypeError.
    if first_called && second_called {
        let (err, _) = crate::value::create_js_error_with_type(
            "Cannot convert object to primitive value",
            "TypeError",
        );
        crate::value::set_thrown_value(err);
        return Err(crate::value::JsError("TypeError".to_string()));
    }

    Ok(Value::String("[object Object]".to_string()))
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
    let to_prim_method = crate::eval::member::eval_object_member(
        obj,
        symbol_key.desc.as_deref().unwrap_or(""),
        None,
    )?;
    if matches!(to_prim_method, Value::Undefined) {
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
pub fn to_object(value: &Value) -> Value {
    match value {
        Value::Undefined | Value::Null => Value::Object(Rc::new(RefCell::new(
            crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary),
        ))),
        Value::Boolean(_b) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::Boolean);
            Value::Object(Rc::new(RefCell::new(obj)))
        }
        Value::Number(_n) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::Number);
            Value::Object(Rc::new(RefCell::new(obj)))
        }
        Value::String(s) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::String);
            obj.properties
                .insert("0".to_string(), Value::String(s.clone()));
            obj.elements = vec![Value::String(s.clone())];
            obj.properties
                .insert("length".to_string(), Value::Number(s.len() as f64));
            Value::Object(Rc::new(RefCell::new(obj)))
        }
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Generator(_) | Value::Class(_) => value.clone(),
        Value::Symbol(_s) => Value::Object(Rc::new(RefCell::new(
            crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary),
        ))),
        Value::BigInt(_) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::BigInt);
            obj.properties.insert("_value".to_string(), value.clone());
            Value::Object(Rc::new(RefCell::new(obj)))
        }
    }
}
