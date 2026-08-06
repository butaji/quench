//! Literal expression evaluation
//!
//! Handles evaluation of literal expressions: numbers, strings, booleans,
//! null, undefined, identifiers, object/array literals, RegExp literals,
//! and function/arrow function expressions.

/// Check if `name` resolves to the global `eval` function (direct eval).
/// Returns true only if the identifier `name` resolves to the actual built-in
/// `eval` function — not a local alias like `var my_eval = eval`.
/// For "eval": walks the environment chain and resolves the identifier to get
/// its value. Returns true only if the value is a native function named "eval".
/// For other names: returns false (never direct eval).
pub fn is_global_eval(name: &str, env: &Rc<RefCell<Environment>>) -> bool {
    if name != "eval" {
        return false;
    }
    // Walk environment chain to find the binding and resolve it
    let mut current: Option<Rc<RefCell<Environment>>> = Some(Rc::clone(env));
    while let Some(e) = current {
        if let Some(val) = e.borrow().get(name) {
            // Found the binding. Check if the VALUE is the global eval function.
            // The global eval is a NativeFunction named "eval". Local aliases
            // (var my_eval = eval) have the same value but are indirect eval.
            if let Value::NativeFunction(nf) = val {
                // The global eval function has name "eval". Local aliases
                // (var my_eval = eval) have the same value but are indirect eval.
                return nf.name == "eval";
            }
            return false;
        }
        current = e.borrow().get_parent();
    }
    false
}

use crate::ast::*;
use crate::builtins;
use crate::env::Environment;
use crate::eval::iteration::get_iterator;
use crate::value::error::create_js_error_with_type;
use crate::value::{get_thrown_value, to_js_string, to_object, JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate an identifier expression
pub fn eval_identifier(
    name: &str,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    if name == "this" {
        crate::eval::class::helpers::check_this_access_allowed(env)?;
        return Ok(crate::interpreter::get_this_binding(env));
    }
    if name == "super" {
        return eval_super(env);
    }
    if name == "new.target" {
        if crate::interpreter::is_direct_eval() && crate::interpreter::is_eval_in_class_field() {
            return Ok(Value::Undefined);
        }
        // Per ES §13.2.6 GetNewTarget: arrow functions inherit new.target
        // via lexical scope (the enclosing function's env binding). For
        // ordinary functions, call_js_function_impl binds new.target in
        // the call env, so env.get resolves it correctly here.
        return Ok(env.borrow().get(name).unwrap_or(Value::Undefined));
    }
    // Arrow functions don't have their own 'arguments' binding
    if in_arrow_function && name == "arguments" {
        // Check if arguments exists in enclosing scope (arrow can access enclosing arguments)
        let found = env.borrow().get("arguments");
        if found.is_none() {
            let (_, js_err) = create_js_error_with_type(
                &format!("ReferenceError: {} is not defined", name),
                "ReferenceError",
            );
            return Err(js_err);
        }
        // Arrow can access enclosing arguments - fall through to normal lookup
    }
    if env.borrow().is_tdz(name) {
        let (value, js_err) = create_js_error_with_type(
            &format!(
                "ReferenceError: Cannot access '{}' before initialization",
                name
            ),
            "ReferenceError",
        );
        crate::value::set_thrown_value(value);
        return Err(js_err);
    }

    // Use get() which handles DeclaredOnly (hoisted var) → undefined
    // and truly unknown vars → None (caught below as ReferenceError).
    match env.borrow().get(name) {
        Some(v) => {
            if let Some(thrown) = get_thrown_value() {
                return Err(JsError(to_js_string(&thrown)));
            }
            Ok(v)
        }
        None => {
            if let Some(thrown) = get_thrown_value() {
                return Err(JsError(to_js_string(&thrown)));
            }

            // Fallback: try to get from Context's globals directly.
            // This handles cases where the environment chain doesn't have access
            // to globalThis (e.g., super constructor calls with isolated environments).
            if let Some(global_val) = crate::context::get_global_from_context(name) {
                return Ok(global_val);
            }
            let (value, js_err) =
                create_js_error_with_type(&format!("{} is not defined", name), "ReferenceError");
            crate::value::set_thrown_value(value);
            Err(js_err)
        }
    }
}

/// Get the super class value from the environment chain
fn get_super_from_env(env: &Rc<RefCell<Environment>>) -> Option<Value> {
    let mut current = Some(env.clone());
    while let Some(e) = current {
        if let Some(super_class) = e.borrow().get_super_class() {
            return Some(super_class);
        }
        current = e.borrow().get_parent();
    }
    None
}

/// Evaluate super keyword
fn eval_super(env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    get_super_from_env(env)
        .ok_or_else(|| JsError("ReferenceError: super is only valid in class methods".to_string()))
}

/// Evaluate a RegExp literal
pub fn eval_regexp_literal(pattern: &str, flags: &str) -> Result<Value, JsError> {
    use crate::value::PropertyFlags;
    use regress::Regex;
    // ES 11.8.5: regex literals must not contain line terminators
    if pattern.contains('\n')
        || pattern.contains('\r')
        || pattern.contains('\u{2028}')
        || pattern.contains('\u{2029}')
    {
        let (err_val, js_err) = crate::value::error::create_js_error_with_type(
            "Invalid regular expression: unexpected line terminator",
            "SyntaxError",
        );
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    let regress_flags: String = flags.chars().filter(|c| "imsu".contains(*c)).collect();
    let regex = match Regex::with_flags(pattern, regress_flags.as_str()) {
        Ok(regex) => regex,
        Err(_) => {
            let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                "Invalid regular expression",
                "SyntaxError",
            );
            crate::value::set_thrown_value(err_val);
            return Err(js_err);
        }
    };
    let mut obj = Object::new(ObjectKind::RegExp);
    obj.internal_regex_source = Some(pattern.to_string());
    obj.internal_regex_flags = Some(flags.to_string());
    obj.set("source", Value::String(pattern.to_string()));
    obj.set("global", Value::Boolean(flags.contains('g')));
    obj.set("ignoreCase", Value::Boolean(flags.contains('i')));
    obj.set("multiline", Value::Boolean(flags.contains('m')));
    // lastIndex must be writable, non-enumerable, non-configurable per spec
    obj.define(
        "lastIndex",
        Value::Number(0.0),
        PropertyFlags {
            value: Some(Value::Number(0.0)),
            writable: true,
            enumerable: false,
            configurable: false,
        },
    );
    obj.set("flags", Value::String(flags.to_string()));
    obj.internal_regex = Some(regex);
    let obj_rc = Rc::new(RefCell::new(obj));
    obj_rc.borrow_mut().prototype = Some(crate::builtins::regex::get_regexp_prototype());
    Ok(Value::Object(obj_rc))
}

/// Evaluate an object literal expression
pub fn eval_object_literal(
    props: &[(PropertyKey, PropertyValue)],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let keys_info: Vec<String> = props
        .iter()
        .map(|(k, v)| format!("{:?}({:?})", k, v))
        .collect();
    let _ = keys_info;
    let mut obj = Object::new(ObjectKind::Ordinary);
    if let Some(prototype) = builtins::get_object_prototype() {
        obj.prototype = Some(prototype);
    }
    let home = Rc::new(RefCell::new(obj));
    let literal_env = Rc::new(RefCell::new(env.borrow().capture_env()));
    literal_env
        .borrow_mut()
        .set_super_class(Value::Object(Rc::clone(&home)));
    for (key, value) in props {
        match value {
            PropertyValue::Spread(expr) => {
                let spread_val = after_expr_eval(crate::eval::expression::eval_expression(
                    expr,
                    &literal_env,
                    in_arrow_function,
                ))?;
                if crate::interpreter::peek_generator_yield() {
                    return Ok(Value::Object(Rc::clone(&home)));
                }
                copy_spread_into_object(&mut home.borrow_mut(), spread_val, &literal_env)?;
            }
            _ => {
                let key_str = eval_property_key(key, &literal_env, in_arrow_function)?;
                if crate::interpreter::peek_generator_yield() {
                    return Ok(Value::Object(Rc::clone(&home)));
                }
                match value {
                    PropertyValue::Value(expr)
                    | PropertyValue::Shorthand(expr)
                    | PropertyValue::Method(expr) => {
                        let mut val = crate::eval::expression::eval_expression(
                            expr,
                            &literal_env,
                            in_arrow_function,
                        )?;
                        crate::eval::class::helpers::set_function_name_for_field_initializer(
                            &mut val, key, &key_str, expr,
                        );
                        if matches!(value, PropertyValue::Method(_)) {
                            if let Value::Function(function) = &mut val {
                                function.is_method = true;
                            }
                        }
                        if key_str.contains('\0') {
                            home.borrow_mut().set_symbol(&key_str, val);
                            continue;
                        }
                        if matches!(key, PropertyKey::Computed(_)) && key_str == "__proto__" {
                            home.borrow_mut().define(
                                &key_str,
                                val,
                                crate::value::object::PropertyFlags {
                                    value: None,
                                    writable: true,
                                    enumerable: true,
                                    configurable: true,
                                },
                            );
                        } else if key_str == "__proto__" && matches!(value, PropertyValue::Value(_))
                        {
                            match val {
                                Value::Object(proto) => home.borrow_mut().prototype = Some(proto),
                                Value::Null => home.borrow_mut().prototype = None,
                                _ => {}
                            }
                        } else {
                            home.borrow_mut().set(&key_str, val);
                        }
                    }
                    PropertyValue::Getter { params: _, body } => {
                        let fn_name = crate::eval::class::helpers::accessor_function_name(
                            key,
                            &key_str,
                            &literal_env,
                            "get",
                        )?;
                        home.borrow_mut().set_getter(
                            &key_str,
                            Rc::new(body.clone()),
                            crate::eval::expression::capture_env_for_closure(&literal_env),
                            false,
                            Some(fn_name),
                        );
                    }
                    PropertyValue::Setter { param, body } => {
                        let fn_name = crate::eval::class::helpers::accessor_function_name(
                            key,
                            &key_str,
                            &literal_env,
                            "set",
                        )?;
                        home.borrow_mut().set_setter(
                            &key_str,
                            param.clone(),
                            Rc::new(body.clone()),
                            crate::eval::expression::capture_env_for_closure(&literal_env),
                            false,
                            Some(fn_name),
                        );
                    }
                    PropertyValue::Spread(_) => unreachable!(),
                }
            }
        }
    }
    Ok(Value::Object(home))
}

/// Evaluate a property key (identifier, string, number, or computed)
pub fn eval_property_key(
    key: &PropertyKey,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(s) => Ok(s.clone()),
        PropertyKey::String(s) => Ok(s.clone()),
        PropertyKey::Number(n) => Ok(crate::value::number_to_string(*n)),
        PropertyKey::Computed(e) => {
            let val = crate::eval::expression::eval_expression(e, env, in_arrow_function)?;
            crate::eval::ops::to_property_key(&val)
        }
    }
}

/// Evaluate an array literal expression
pub fn eval_array_literal(
    elements: &[Expression],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let mut arr = Object::new_array(0);
    for elem_expr in elements.iter() {
        match elem_expr {
            Expression::Spread(spread_expr) => {
                let spread_val = after_expr_eval(crate::eval::expression::eval_expression(
                    spread_expr,
                    env,
                    in_arrow_function,
                ))?;
                if crate::interpreter::peek_generator_yield() {
                    return Ok(Value::Object(Rc::new(RefCell::new(arr))));
                }
                let items = get_iterator(&spread_val)?;
                for item in items {
                    let idx = arr.elements.len();
                    arr.set(&idx.to_string(), item);
                }
            }
            Expression::Elision => {
                // Array hole: advances length but contributes no own property.
                let idx = arr.elements.len();
                arr.elements.push(Value::Undefined);
                arr.holes.insert(idx);
                arr.define_array_length(arr.elements.len() as f64);
            }
            _ => {
                let value = after_expr_eval(crate::eval::expression::eval_expression(
                    elem_expr,
                    env,
                    in_arrow_function,
                ))?;
                if crate::interpreter::peek_generator_yield() {
                    return Ok(Value::Object(Rc::new(RefCell::new(arr))));
                }
                let idx = arr.elements.len();
                arr.set(&idx.to_string(), value);
            }
        }
    }
    if let Some(prototype) = builtins::get_array_prototype() {
        arr.prototype = Some(prototype);
    }
    Ok(Value::Object(Rc::new(RefCell::new(arr))))
}

fn after_expr_eval(result: Result<Value, JsError>) -> Result<Value, JsError> {
    let val = result?;
    if crate::interpreter::peek_generator_yield() {
        return Ok(Value::Undefined);
    }
    Ok(val)
}

fn copy_spread_into_object(
    target: &mut Object,
    source: Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    if matches!(source, Value::Null | Value::Undefined) {
        return Ok(());
    }
    let source_obj = match source {
        Value::Object(o) => o,
        other => {
            let Value::Object(o) = to_object(&other)? else {
                return Ok(());
            };
            o
        }
    };
    if let Some(keys) = crate::eval::object::proxy_own_keys(&source_obj)? {
        for key in keys {
            if crate::eval::object::proxy_property_is_enumerable(&source_obj, &key)? == Some(false)
            {
                continue;
            }
            let property_key = match &key {
                Value::String(key) => key.clone(),
                Value::Symbol(symbol) => symbol.property_key(),
                _ => continue,
            };
            let value = crate::eval::member::eval_object_member_value(&source_obj, &key, None)?;
            if property_key.contains('\0') {
                target.set_symbol(&property_key, value);
            } else {
                target.set(&property_key, value);
            }
        }
        return Ok(());
    }
    let src = source_obj.borrow();
    for i in 0..src.elements.len() {
        if src.holes.contains(&i) || !src.is_enumerable(&i.to_string()) {
            continue;
        }
        let key = i.to_string();
        let val = spread_property_value(&source_obj, &key, &src, env)?;
        target.set(&key, val);
    }
    for key in crate::value::object::enumerable_own_keys(&src) {
        if !src.is_enumerable(&key) {
            continue;
        }
        let val = spread_property_value(&source_obj, &key, &src, env)?;
        target.set(&key, val);
    }
    for key in src
        .symbol_properties
        .keys()
        .chain(src.getters.keys())
        .chain(src.setters.keys())
    {
        if !key.contains('\0') || !src.is_enumerable(key) {
            continue;
        }
        let val = spread_property_value(&source_obj, key, &src, env)?;
        target.set_symbol(key, val);
    }
    for key in src.properties.keys() {
        if key.contains('\0') && !src.is_enumerable(key) {
            continue;
        }
        if key.contains('\0') {
            let val = spread_property_value(&source_obj, key, &src, env)?;
            target.set_symbol(key, val);
        }
    }
    Ok(())
}

fn spread_property_value(
    obj: &Rc<RefCell<Object>>,
    key: &str,
    src: &Object,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(getter) = src.get_getter(key) {
        return crate::eval::object::call_getter(obj, getter, env);
    }
    if let Some(value) = src.symbol_properties.get(key) {
        return Ok(value.clone());
    }
    if let Some(idx) = crate::value::object::helpers::as_array_index(key) {
        if idx < src.elements.len() && !src.holes.contains(&idx) {
            return Ok(src.elements[idx].clone());
        }
    }
    Ok(src.properties.get(key).cloned().unwrap_or(Value::Undefined))
}

/// Get the super class value from the environment (public for use by expression.rs)
pub fn get_super_value(env: &Rc<RefCell<Environment>>) -> Option<Value> {
    get_super_from_env(env)
}
