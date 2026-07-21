//! Native function and constructor member access evaluation

use crate::value::{JsError, NativeConstructor, NativeFunction, Value};
use std::rc::Rc;

/// Evaluate member access on a native function
pub fn eval_native_function_member(
    nf: &Rc<NativeFunction>,
    prop_name: &str,
) -> Result<Value, JsError> {
    // Check custom properties first
    if let Some(val) = nf.get_property(prop_name) {
        return Ok(val);
    }
    match prop_name {
        "name" => Ok(Value::String({
            let n = nf.name.clone();
            if n.is_empty() {
                "anonymous".to_string()
            } else {
                n
            }
        })),
        "prototype" => eval_native_prototype(nf),
        "length" => Ok(Value::Number(0.0)),
        "call" => eval_native_call_method(nf),
        "apply" => eval_native_apply_method(nf),
        "bind" => eval_native_bind_method(nf),
        "toPrimitive" | "hasInstance" | "isConcatSpreadable" => eval_well_known_symbol(prop_name),
        _ => Ok(Value::Undefined),
    }
}

fn eval_native_prototype(nf: &Rc<NativeFunction>) -> Result<Value, JsError> {
    // Per ECMA-262, built-in functions that are not constructors
    // (parseInt, isNaN, isFinite, parseFloat, Function.prototype.{call,apply,bind},
    // and similar) MUST have `fn.prototype === undefined`. Only functions that
    // were constructed with an explicit prototype (NativeFunction::new_with_prototype)
    // expose one; otherwise return Undefined instead of lazy-creating an empty
    // prototype object.
    if let Some(proto) = nf.prototype.borrow().as_ref() {
        Ok(Value::Object(Rc::clone(proto)))
    } else {
        Ok(Value::Undefined)
    }
}

fn eval_well_known_symbol(prop_name: &str) -> Result<Value, JsError> {
    if let Some(sym) = crate::builtins::symbol::get_well_known_symbol_no_ctx(prop_name) {
        Ok(sym)
    } else {
        Ok(Value::Undefined)
    }
}

/// Handle NativeFunction.prototype.call
fn eval_native_call_method(nf: &Rc<NativeFunction>) -> Result<Value, JsError> {
    let nf_clone = nf.clone();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(Value::NativeFunction(nf_clone.clone()));
            let this_val = args.first().cloned().unwrap_or(Value::Undefined);
            crate::interpreter::set_this_value(this_val.clone());
            let remaining_args: Vec<Value> = args.into_iter().skip(1).collect();
            let result = crate::eval::function::call_native_function(
                nf_clone.clone(),
                remaining_args,
                this_val,
            );
            crate::interpreter::take_native_this();
            crate::interpreter::take_this_value();
            result
        },
    ))))
}

/// Handle NativeFunction.prototype.apply
fn eval_native_apply_method(nf: &Rc<NativeFunction>) -> Result<Value, JsError> {
    let nf_clone = nf.clone();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(Value::NativeFunction(nf_clone.clone()));
            let this_val = args.first().cloned().unwrap_or(Value::Undefined);
            crate::interpreter::set_this_value(this_val.clone());
            let args_array = args.get(1);
            let spread_args = if let Some(Value::Object(o)) = args_array {
                let o = o.borrow();
                (0..o.elements.len())
                    .filter_map(|i| o.elements.get(i).cloned())
                    .collect()
            } else {
                Vec::new()
            };
            let result = crate::eval::function::call_native_function(
                nf_clone.clone(),
                spread_args,
                this_val,
            );
            crate::interpreter::take_native_this();
            crate::interpreter::take_this_value();
            result
        },
    ))))
}

/// Handle NativeFunction.prototype.bind
fn eval_native_bind_method(nf: &Rc<NativeFunction>) -> Result<Value, JsError> {
    let nf_clone = nf.clone();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(Value::NativeFunction(nf_clone.clone()));
            let bound_this = args.first().cloned().unwrap_or(Value::Undefined);
            crate::interpreter::set_this_value(bound_this.clone());
            let bound_args: Vec<Value> = args.iter().skip(1).cloned().collect();
            let target_nf = nf_clone.clone();
            let result = Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
                move |call_args: Vec<Value>| {
                    crate::interpreter::set_native_this(Value::NativeFunction(target_nf.clone()));
                    crate::interpreter::set_this_value(bound_this.clone());
                    let all_args: Vec<Value> =
                        bound_args.iter().cloned().chain(call_args).collect();
                    let result = crate::eval::function::call_native_function(
                        target_nf.clone(),
                        all_args,
                        bound_this.clone(),
                    );
                    crate::interpreter::take_native_this();
                    crate::interpreter::take_this_value();
                    result
                },
            ))));
            crate::interpreter::take_native_this();
            crate::interpreter::take_this_value();
            result
        },
    ))))
}

/// Evaluate member access on a native constructor
pub fn eval_native_constructor_member(
    nc: &Rc<NativeConstructor>,
    prop_name: &str,
) -> Result<Value, JsError> {
    // Check accessor (getter/setter) first - Object.defineProperty can override
    if let Some(accessor) = nc.get_accessor(prop_name) {
        if let Some(getter) = accessor.getter {
            // Call the getter with this=nc
            let nc_val = Value::NativeConstructor(Rc::clone(nc));
            return crate::eval::call_value_with_this(getter, vec![], nc_val);
        }
        // Accessor with no getter returns undefined
        return Ok(Value::Undefined);
    }

    // Check static methods
    if let Some(val) = nc.get_static_method(prop_name) {
        return Ok(val);
    }

    // Check if this is the Function constructor
    let is_function_constructor = crate::builtins::function::get_function_prototype()
        .map(|fp| Rc::ptr_eq(&fp, &nc.prototype))
        .unwrap_or(false);

    match prop_name {
        "prototype" => Ok(Value::Object(Rc::clone(&nc.prototype))),
        "length" => {
            if is_function_constructor {
                Ok(Value::Number(1.0))
            } else {
                Ok(Value::Number(0.0))
            }
        }
        "name" => Ok(Value::String(nc.name().to_string())),
        _ => Ok(Value::Undefined),
    }
}

#[cfg(test)]
mod tests {
    use crate::Context;
    use crate::Value;

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── eval_native_function_member: name ────────────────────────────────────

    #[test]
    fn native_function_name_named() {
        let r = eval("Object.name").unwrap();
        assert_eq!(r, Value::String("Object".into()));
    }

    #[test]
    fn native_function_name_anonymous() {
        // Built-in anonymous functions
        let r = eval("Array.prototype.map.name").unwrap();
        assert_eq!(r, Value::String("map".into()));
    }

    // ─── eval_native_function_member: length ───────────────────────────────────

    #[test]
    fn native_function_length_is_zero() {
        let r = eval("Object.keys.length").unwrap();
        assert_eq!(r, Value::Number(0.0));
    }

    // ─── eval_native_function_member: prototype ────────────────────────────────

    #[test]
    fn native_function_without_prototype_returns_undefined() {
        // Most native functions have no prototype
        let r = eval("Object.keys.prototype").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    // ─── eval_native_function_member: length ────────────────────────────────────

    #[test]
    fn native_constructor_function_length_is_one() {
        let r = eval("Function.length").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn native_constructor_other_length_is_zero() {
        let r = eval("Object.length").unwrap();
        assert_eq!(r, Value::Number(0.0));
    }

    // ─── eval_native_constructor_member: prototype ──────────────────────────

    #[test]
    fn native_constructor_has_prototype() {
        let r = eval("typeof Object.prototype").unwrap();
        assert_eq!(r, Value::String("object".into()));
    }

    // ─── eval_native_constructor_member: name ─────────────────────────────────

    #[test]
    fn native_constructor_name() {
        let r = eval("Object.name").unwrap();
        assert_eq!(r, Value::String("Object".into()));
    }
}
