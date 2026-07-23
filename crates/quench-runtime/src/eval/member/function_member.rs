//! Function member access evaluation

use crate::value::{
    create_js_error_with_type, set_thrown_value, JsError, NativeFunction, Object, Value,
    ValueFunction,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate member access on a JS function
pub fn eval_function_member(f: &ValueFunction, prop_name: &str) -> Result<Value, JsError> {
    // Check custom properties first (e.g., sameValue, notSameValue on assert)
    if let Some(val) = f.get_property(prop_name) {
        return Ok(val);
    }
    if f.is_arrow && (prop_name == "arguments" || prop_name == "caller") {
        let msg = format!(
            "'caller' and 'arguments' are restricted properties and cannot be accessed on arrow functions"
        );
        let (err, js_err) = create_js_error_with_type(&msg, "TypeError");
        set_thrown_value(err);
        return Err(js_err);
    }
    // ES spec §16.1: class methods (functions from MethodDefinition syntax) have
    // restricted 'caller' and 'arguments' properties.
    if f.is_method && (prop_name == "arguments" || prop_name == "caller") {
        let (err, js_err) = create_js_error_with_type(
            "'caller' and 'arguments' are restricted properties and cannot be accessed on this function",
            "TypeError",
        );
        set_thrown_value(err);
        return Err(js_err);
    }
    match prop_name {
        "name" => Ok(Value::String(f.name.clone().unwrap_or_default())),
        "length" => eval_function_length(f),
        "prototype" => Ok(Value::Object(f.get_prototype())),
        "call" => eval_callable_call_method(Value::Function(f.clone())),
        "apply" => eval_callable_apply_method(Value::Function(f.clone())),
        "bind" => eval_callable_bind_method(Value::Function(f.clone())),
        _ => {
            // Per ES spec, property lookup on a function object follows the
            // [[Prototype]] chain, which for all functions is Function.prototype.
            // f.get_prototype() returns the .prototype property (for new instances),
            // NOT the function's own [[Prototype]]. Use Function.prototype instead.
            if let Some(func_proto) = crate::builtins::get_function_prototype() {
                Ok(func_proto
                    .borrow()
                    .get(prop_name)
                    .unwrap_or(Value::Undefined))
            } else {
                Ok(Value::Undefined)
            }
        }
    }
}

fn eval_function_length(f: &ValueFunction) -> Result<Value, JsError> {
    let mut count = 0;
    for p in &f.params {
        if p.default.is_some() {
            break;
        }
        count += 1;
    }
    Ok(Value::Number(count as f64))
}

/// Function.prototype.call / apply / bind for any callable (`Value::Function` or `Value::Class`).
pub fn eval_callable_proto_method(target: Value, prop_name: &str) -> Result<Value, JsError> {
    match prop_name {
        "call" => eval_callable_call_method(target),
        "apply" => eval_callable_apply_method(target),
        "bind" => eval_callable_bind_method(target),
        _ => Ok(Value::Undefined),
    }
}

fn callable_name(target: &Value) -> String {
    match target {
        Value::Function(f) => f.name.clone().unwrap_or_default(),
        Value::Class(c) => c.name.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

fn callable_length(target: &Value) -> usize {
    match target {
        Value::Function(f) => {
            let mut count = 0;
            for p in &f.params {
                if p.default.is_some() {
                    break;
                }
                count += 1;
            }
            count
        }
        Value::Class(c) => c.constructor_params.len(),
        _ => 0,
    }
}

fn store_bound_metadata(
    bound: &NativeFunction,
    target: &Value,
    bound_this: &Value,
    bound_args: &[Value],
) {
    let _ = bound.set_property("__quench_bound_target", target.clone());
    let _ = bound.set_property("__quench_bound_this", bound_this.clone());
    let mut arr = Object::new(crate::value::ObjectKind::Ordinary);
    for (i, arg) in bound_args.iter().enumerate() {
        arr.elements.resize(i + 1, Value::Undefined);
        arr.elements[i] = arg.clone();
    }
    let _ = bound.set_property(
        "__quench_bound_args",
        Value::Object(Rc::new(RefCell::new(arr))),
    );
}

pub fn bound_callable_target(func: &Value) -> Option<(Value, Value, Vec<Value>)> {
    let Value::NativeFunction(nf) = func else {
        return None;
    };
    let target = nf.get_property("__quench_bound_target")?;
    let bound_this = nf
        .get_property("__quench_bound_this")
        .unwrap_or(Value::Undefined);
    let bound_args = nf
        .get_property("__quench_bound_args")
        .and_then(|v| {
            if let Value::Object(o) = v {
                let o = o.borrow();
                Some(
                    (0..o.elements.len())
                        .filter_map(|i| o.elements.get(i).cloned())
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default();
    Some((target, bound_this, bound_args))
}

/// Handle Function.prototype.call
pub fn eval_callable_call_method(target: Value) -> Result<Value, JsError> {
    let target_for_call = target.clone();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(target_for_call.clone());
            let this_val = args.first().cloned().unwrap_or(Value::Undefined);
            crate::interpreter::set_this_value(this_val.clone());
            let remaining_args: Vec<Value> = args.into_iter().skip(1).collect();
            let result = crate::eval::call_value_with_this(
                target_for_call.clone(),
                remaining_args,
                this_val,
            );
            crate::interpreter::take_native_this();
            crate::interpreter::take_this_value();
            result
        },
    ))))
}

/// Handle Function.prototype.apply
pub fn eval_callable_apply_method(target: Value) -> Result<Value, JsError> {
    let target_for_call = target.clone();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(target_for_call.clone());
            let this_val = args.first().cloned().unwrap_or(Value::Undefined);
            crate::interpreter::set_this_value(this_val.clone());
            let spread_args = if let Some(Value::Object(o)) = args.get(1) {
                let o = o.borrow();
                (0..o.elements.len())
                    .filter_map(|i| o.elements.get(i).cloned())
                    .collect()
            } else {
                Vec::new()
            };
            let result =
                crate::eval::call_value_with_this(target_for_call.clone(), spread_args, this_val);
            crate::interpreter::take_native_this();
            crate::interpreter::take_this_value();
            result
        },
    ))))
}

/// Handle Function.prototype.bind
pub fn eval_callable_bind_method(target: Value) -> Result<Value, JsError> {
    let target_for_bind = target.clone();
    let bound_name = format!("bound {}", callable_name(&target));
    let bound_len = callable_length(&target);
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(target_for_bind.clone());
            let bound_this = args.first().cloned().unwrap_or(Value::Undefined);
            crate::interpreter::set_this_value(bound_this.clone());
            let bound_args: Vec<Value> = args.iter().skip(1).cloned().collect();
            let inner_target = target_for_bind.clone();
            let name_for_inner = bound_name.clone();
            let inner_bound_this = bound_this.clone();
            let inner_bound_args = bound_args.clone();
            let bound_func = NativeFunction::new(move |call_args: Vec<Value>| {
                crate::interpreter::set_native_this(inner_target.clone());
                crate::interpreter::set_this_value(inner_bound_this.clone());
                let all_args: Vec<Value> =
                    inner_bound_args.iter().cloned().chain(call_args).collect();
                let result = crate::eval::call_value_with_this(
                    inner_target.clone(),
                    all_args,
                    inner_bound_this.clone(),
                );
                crate::interpreter::take_native_this();
                crate::interpreter::take_this_value();
                result
            });
            let _ = bound_func.set_property("name", Value::String(name_for_inner));
            let _ = bound_func.set_property(
                "length",
                Value::Number(bound_len.saturating_sub(bound_args.len()) as f64),
            );
            store_bound_metadata(&bound_func, &target_for_bind, &bound_this, &bound_args);
            crate::interpreter::take_native_this();
            crate::interpreter::take_this_value();
            Ok(Value::NativeFunction(Rc::new(bound_func)))
        },
    ))))
}

#[cfg(test)]
mod tests {
    use crate::Context;
    use crate::Value;

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── eval_function_member: name ────────────────────────────────────────────

    #[test]
    fn function_name_named() {
        let r = eval("function foo() {} foo.name").unwrap();
        assert_eq!(r, Value::String("foo".into()));
    }

    #[test]
    fn function_name_anonymous() {
        let r = eval("(function() {}).name").unwrap();
        assert_eq!(r, Value::String("".into()));
    }

    // ─── eval_function_member: length ──────────────────────────────────────────

    #[test]
    fn function_length_no_defaults() {
        let r = eval("function f(a, b, c) {} f.length").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn function_length_with_defaults() {
        // ES spec: stop counting at first param with a default
        let r = eval("function f(a, b = 1, c) {} f.length").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn function_length_zero_params() {
        let r = eval("function f() {} f.length").unwrap();
        assert_eq!(r, Value::Number(0.0));
    }

    // ─── eval_function_member: prototype ─────────────────────────────────────────

    #[test]
    fn function_has_prototype_property() {
        let r = eval("function f() {} typeof f.prototype").unwrap();
        assert_eq!(r, Value::String("object".into()));
    }

    // ─── eval_function_member: call ─────────────────────────────────────────────

    #[test]
    fn function_call_sets_this() {
        let r = eval("function f() { return this.x; } f.call({x: 42})").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn function_call_passes_args() {
        let r = eval("function f(a, b) { return a + b; } f.call(null, 3, 4)").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn function_call_no_args() {
        let r = eval("function f() { return 99; } f.call()").unwrap();
        assert_eq!(r, Value::Number(99.0));
    }

    // ─── eval_function_member: apply ────────────────────────────────────────────

    #[test]
    fn function_apply_sets_this() {
        let r = eval("function f() { return this.y; } f.apply({y: 7})").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn function_apply_spreads_array() {
        let r = eval("function f(a, b, c) { return a + b + c; } f.apply(null, [1, 2, 3])").unwrap();
        assert_eq!(r, Value::Number(6.0));
    }

    #[test]
    fn function_apply_empty_array() {
        let r = eval("function f() { return 'ok'; } f.apply(null, [])").unwrap();
        assert_eq!(r, Value::String("ok".into()));
    }

    // ─── eval_function_member: bind ─────────────────────────────────────────────

    #[test]
    fn function_bind_prepends_args() {
        let r = eval("function f(a, b) { return a + b; } f.bind(null, 1)(2)").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn function_bind_sets_this() {
        let r = eval("function f() { return this.z; } f.bind({z: 55})()").unwrap();
        assert_eq!(r, Value::Number(55.0));
    }

    // ─── eval_function_member: caller/arguments on arrow ───────────────────────

    #[test]
    fn arrow_function_caller_restricted() {
        let r = eval("var f = () => 1; f.caller");
        assert!(r.is_err());
    }

    #[test]
    fn arrow_function_arguments_restricted() {
        let r = eval("var f = () => 1; f.arguments");
        assert!(r.is_err());
    }

    // ─── eval_function_member: fallback to Function.prototype ──────────────────

    #[test]
    fn function_member_unknown_falls_back_to_function_proto() {
        // toString is inherited from Function.prototype
        let r = eval("function f() {} typeof f.toString").unwrap();
        assert_eq!(r, Value::String("function".into()));
    }
}
