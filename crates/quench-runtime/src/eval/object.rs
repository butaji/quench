//! Object operations: assignment, property access, getter/setter calls.

use crate::ast::*;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::statement::eval_function_body;
use crate::value::{GetterStorage, JsError, Object, SetterStorage, Value};
use std::cell::RefCell;
use std::rc::Rc;

mod helpers;
pub use helpers::*;

/// Assign a value to a target (variable, member, or destructuring pattern).
pub fn assign_to(
    target: &Expression,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    match target {
        Expression::Identifier(name) => assign_to_identifier(name, value, env),
        Expression::Member {
            object,
            property,
            computed,
        } => assign_to_member(object, property, *computed, value, env),
        Expression::ArrayPattern(bindings) => assign_array_destructuring(bindings, value, env),
        Expression::ObjectPattern(props) => assign_object_destructuring(props, value, env),
        _ => Err(JsError("Invalid assignment target".to_string())),
    }
}

/// Extract the property name from a PropertyKey (public, used by call.rs).
pub fn extract_property_name(
    property: &PropertyKey,
    computed: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<String, JsError> {
    if computed {
        match property {
            PropertyKey::Computed(e) => {
                let val = eval_expression(e, env, in_arrow_function)?;
                match &val {
                    Value::Symbol(s) => {
                        Ok(s.desc.clone().map(|d| d.to_string()).unwrap_or_default())
                    }
                    _ => Ok(crate::value::to_js_string(&val)),
                }
            }
            _ => Err(JsError("Invalid computed property".to_string())),
        }
    } else {
        match property {
            PropertyKey::Ident(s) => Ok(s.clone()),
            PropertyKey::String(s) => Ok(s.clone()),
            PropertyKey::Number(n) => Ok(n.to_string()),
            PropertyKey::Computed(e) => Ok(crate::value::to_js_string(&eval_expression(
                e,
                env,
                in_arrow_function,
            )?)),
        }
    }
}

/// Evaluate a callee expression and determine the `this` value for the call.
pub fn eval_callee_with_this(
    callee: &Expression,
    env: &Rc<RefCell<Environment>>,
) -> Result<(Value, Value, bool), JsError> {
    match callee {
        Expression::Member {
            object,
            property,
            computed,
        } => {
            let obj_val = eval_expression(object, env, false)?;
            let prop_name = extract_property_name(property, *computed, env, false)?;
            let func = get_member_function(&obj_val, &prop_name, env)?;
            Ok((func, obj_val, false))
        }
        _ => {
            let func = eval_expression(callee, env, false)?;
            let is_direct = if let Expression::Identifier(name) = callee {
                crate::eval::literal::is_global_eval(name, env)
            } else {
                false
            };
            Ok((func, Value::Undefined, is_direct))
        }
    }
}

/// Call a getter function, returning the getter's value.
pub fn call_getter(
    obj: &Rc<RefCell<Object>>,
    getter_storage: &GetterStorage,
    _env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let this_val = getter_this_value(obj);
    if let Some(func) = &getter_storage.func {
        let call_site_strict = crate::interpreter::is_strict_mode();
        return crate::eval::function::call_value_impl(
            func.clone(),
            Vec::new(),
            this_val,
            call_site_strict,
        );
    }
    let closure = Rc::clone(&getter_storage.closure);
    let body = getter_storage.body.clone();
    let call_env = Environment::with_parent(closure);
    call_env.current_scope().borrow_mut().set_this(this_val);
    let call_env = Rc::new(RefCell::new(call_env));
    if body.is_empty() {
        Ok(Value::Undefined)
    } else {
        let prev_strict = crate::interpreter::is_strict_mode();
        crate::interpreter::set_strict_mode(getter_storage.strict);
        let result = eval_function_body(&body, &call_env, false);
        crate::interpreter::set_strict_mode(prev_strict);
        result
    }
}

/// Assign to a member expression (object.property).
pub fn assign_to_member(
    object: &Expression,
    property: &PropertyKey,
    computed: bool,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let prop_name = extract_property_name(property, computed, env, false)?;

    // Handle chained member: e.g. assert.deepEqual._compare = ...
    if let Expression::Member {
        object: parent_obj,
        property: parent_prop,
        computed: parent_computed,
    } = object
    {
        let parent_prop_name = extract_property_name(parent_prop, *parent_computed, env, false)?;
        let parent_val = eval_expression(parent_obj, env, false)?;

        // Value::Object parent: read, modify function property, write back.
        if let Value::Object(ref parent_o) = parent_val {
            let func_opt = {
                let parent_read = parent_o.borrow();
                parent_read.properties.get(&parent_prop_name).cloned()
            };
            if let Some(Value::Function(func)) = func_opt {
                let func = func;
                func.set_property(&prop_name, value.clone());
                parent_o
                    .borrow_mut()
                    .properties
                    .insert(parent_prop_name, Value::Function(func));
                return Ok(());
            }
        }

        // NativeFunction parent: clone property, modify, write back via set_property.
        if let Value::NativeFunction(ref nf) = parent_val {
            let prop_opt = nf.get_property(&parent_prop_name);
            if let Some(Value::Function(func)) = prop_opt {
                let func = func;
                func.set_property(&prop_name, value.clone());
                let _ = nf.set_property(&parent_prop_name, Value::Function(func));
                return Ok(());
            }
            // Nested NativeFunction property: clone inner, modify, write both.
            if let Some(Value::NativeFunction(inner_nf)) = nf.get_property(&parent_prop_name) {
                let _ = inner_nf.set_property(&prop_name, value.clone());
                let _ = nf.set_property(&parent_prop_name, Value::NativeFunction(inner_nf));
                return Ok(());
            }
        }
    }

    let obj_val = eval_expression(object, env, false)?;

    match obj_val {
        // Box primitives per ES §10.2.9 [[Set]] (ToObject coercion).
        Value::Number(_) | Value::Boolean(_) | Value::Symbol(_) | Value::String(_) => {
            assign_to_primitive_boxed(&obj_val, &prop_name, value, env)
        }
        Value::Object(o) => assign_to_object(&o, &prop_name, value, env),
        Value::Function(f) => assign_to_function(&f, &prop_name, value.clone()),
        Value::NativeFunction(nf) => assign_to_native_function(&nf, &prop_name, value.clone()),
        Value::NativeConstructor(nc) => {
            assign_to_native_constructor(&nc, &prop_name, value.clone())
        }
        Value::Class(class) => {
            class.set_static_field(&prop_name, value.clone());
            Ok(())
        }
        _ => Err(JsError(format!(
            "Cannot assign to property of non-object, got {:?}",
            obj_val
        ))),
    }
}

/// Assign to a boxed primitive (Number/Boolean/Symbol).
fn assign_to_primitive_boxed(
    obj_val: &Value,
    prop_name: &str,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let boxed = box_primitive_for_set(obj_val, env)?;

    // Check own setter first.
    let receiver_has_own_setter = boxed.borrow().get_setter(prop_name).is_some();
    if !receiver_has_own_setter {
        // Check prototype-of-prototype for proxy (e.g. Number.prototype proxy).
        if let Some(ref proto_rc) = boxed.borrow().prototype {
            if let Some(ref proto_of_proto_rc) = proto_rc.borrow().prototype {
                let proto_of_proto = proto_of_proto_rc.borrow();
                let handler_val = proto_of_proto.properties.get("__quench_proxy_handler");
                let target_val = proto_of_proto.properties.get("__quench_proxy_target");
                if let (Some(Value::Object(h)), Some(Value::Object(t))) =
                    (handler_val.cloned(), target_val.cloned())
                {
                    let this_val = Value::Object(Rc::clone(&boxed));
                    let success = call_proxy_set_trap(
                        &Value::Object(Rc::clone(&t)),
                        &h,
                        &this_val,
                        prop_name,
                        value.clone(),
                    )?;
                    if success {
                        return Ok(());
                    }
                    if crate::interpreter::is_strict_mode() {
                        let (_, js_err) = crate::value::error::create_js_error_with_type(
                            "Cannot set property (proxy set trap returned false)",
                            "TypeError",
                        );
                        return Err(js_err);
                    }
                    return Ok(());
                }
            }
        }
        // Check inherited proxy in prototype chain.
        if let Some((_, handler, target)) = find_proxy_in_prototype_chain(&boxed, prop_name) {
            let this_val = Value::Object(Rc::clone(&boxed));
            let success =
                call_proxy_set_trap(&target, &handler, &this_val, prop_name, value.clone())?;
            if success {
                return Ok(());
            }
            if crate::interpreter::is_strict_mode() {
                let (_, js_err) = crate::value::error::create_js_error_with_type(
                    "Cannot set property (proxy set trap returned false)",
                    "TypeError",
                );
                return Err(js_err);
            }
            return Ok(());
        }
    }
    boxed.borrow_mut().set(prop_name, value.clone());
    Ok(())
}

/// Assign to an ordinary object.
fn assign_to_object(
    o: &Rc<RefCell<Object>>,
    prop_name: &str,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    // Walk prototype chain for inherited setters (ES §10.2.9 [[Set]]).
    let mut prototype: Option<Rc<RefCell<Object>>> = Some(Rc::clone(o));
    let mut setter_clone: Option<SetterStorage> = None;
    while let Some(current) = prototype {
        {
            let obj_ref = current.borrow();
            if obj_ref.get_setter(prop_name).is_some() {
                setter_clone = obj_ref.get_setter(prop_name).cloned();
                break;
            }
            prototype = obj_ref.prototype.as_ref().map(Rc::clone);
        }
    }

    // Call inherited setter if found.
    if let Some(setter_storage) = setter_clone {
        call_setter(o, &setter_storage, value.clone(), env)?;
        return Ok(());
    }

    // Check proxy in prototype chain before own property path.
    if let Some((_, handler, target)) = find_proxy_in_prototype_chain(o, prop_name) {
        let this_val = Value::Object(Rc::clone(o));
        let success = call_proxy_set_trap(&target, &handler, &this_val, prop_name, value.clone())?;
        if success {
            return Ok(());
        }
        if crate::interpreter::is_strict_mode() {
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                "Cannot set property (proxy set trap returned false)",
                "TypeError",
            );
            return Err(js_err);
        }
        return Ok(());
    }

    // Reject inherited non-writable data properties.
    if has_readonly_prototype_property(o, prop_name) {
        if crate::interpreter::is_strict_mode() {
            let (_, error) = crate::value::error::create_js_error_with_type(
                "Cannot assign to read only property",
                "TypeError",
            );
            return Err(error);
        }
        return Ok(());
    }

    // Reject property sets on frozen objects.
    if crate::builtins::object_static::is_frozen_object(o) {
        return Ok(());
    }

    // Strict mode checks.
    if crate::interpreter::is_strict_mode() {
        let obj_ref = o.borrow();
        if let Some(flags) = obj_ref.get_descriptor(prop_name) {
            if !flags.writable {
                let (_, js_err) = crate::value::error::create_js_error_with_type(
                    "Cannot assign to read only property",
                    "TypeError",
                );
                return Err(js_err);
            }
        } else if !obj_ref.extensible && !obj_ref.properties.contains_key(prop_name) {
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                "Cannot add property to non-extensible object",
                "TypeError",
            );
            return Err(js_err);
        }
    }

    o.borrow_mut().set(prop_name, value.clone());

    // Mirror writes on globalThis into the global binding.
    let is_global_this = env
        .borrow()
        .get("globalThis")
        .map(|g| matches!(g, Value::Object(ref go) if Rc::ptr_eq(go, o)))
        .unwrap_or(false);
    if is_global_this && !env.borrow_mut().set(prop_name, value.clone()) {
        env.borrow_mut()
            .define(prop_name.to_string(), value.clone());
    }
    Ok(())
}

/// Assign to a function value.
fn assign_to_function(
    f: &crate::value::ValueFunction,
    prop_name: &str,
    value: Value,
) -> Result<(), JsError> {
    if f.is_arrow && (prop_name == "caller" || prop_name == "arguments") {
        let msg = "'caller' and 'arguments' are restricted properties and cannot be set on arrow functions".to_string();
        let (err, js_err) = crate::value::create_js_error_with_type(&msg, "TypeError");
        crate::value::set_thrown_value(err);
        return Err(js_err);
    }
    if f.get_property(prop_name).is_some() && (prop_name == "length" || prop_name == "name") {
        if crate::interpreter::is_strict_mode() {
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                "Cannot assign to read only property",
                "TypeError",
            );
            return Err(js_err);
        }
        return Ok(());
    }
    f.set_property(prop_name, value);
    Ok(())
}

/// Assign to a native function.
fn assign_to_native_function(
    nf: &crate::value::NativeFunction,
    prop_name: &str,
    value: Value,
) -> Result<(), JsError> {
    if crate::interpreter::is_strict_mode() && (prop_name == "length" || prop_name == "name") {
        let (_, error) = crate::value::error::create_js_error_with_type(
            "Cannot assign to read only property",
            "TypeError",
        );
        return Err(error);
    }
    let _ = nf.set_property(prop_name, value);
    Ok(())
}

/// Assign to a native constructor.
fn assign_to_native_constructor(
    nc: &crate::value::NativeConstructor,
    prop_name: &str,
    value: Value,
) -> Result<(), JsError> {
    let readonly = is_readonly_constructor_property(&nc.name(), prop_name);
    if crate::interpreter::is_strict_mode() && readonly {
        let (_, error) = crate::value::error::create_js_error_with_type(
            "Cannot assign to read only property",
            "TypeError",
        );
        return Err(error);
    }
    if !readonly {
        nc.set_property(prop_name, value);
    }
    Ok(())
}

/// Call a setter function with the object as "this" and the value as the parameter.
pub fn call_setter(
    obj: &Rc<RefCell<Object>>,
    setter_storage: &SetterStorage,
    value: Value,
    _env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let this_val = getter_this_value(obj);
    if let Some(func) = &setter_storage.func {
        let call_site_strict = crate::interpreter::is_strict_mode();
        return crate::eval::function::call_value_impl(
            func.clone(),
            vec![value],
            this_val,
            call_site_strict,
        );
    }
    let closure = Rc::clone(&setter_storage.closure);
    let body = setter_storage.body.clone();
    let param = setter_storage.param.clone();
    let mut call_env = Environment::with_parent(Rc::clone(&closure));
    call_env.current_scope().borrow_mut().set_this(this_val);
    call_env.define(param, value);
    let call_env = Rc::new(RefCell::new(call_env));
    if body.is_empty() {
        Ok(Value::Undefined)
    } else {
        let prev_strict = crate::interpreter::is_strict_mode();
        crate::interpreter::set_strict_mode(setter_storage.strict);
        let result = eval_function_body(&body, &call_env, false);
        crate::interpreter::set_strict_mode(prev_strict);
        result
    }
}

#[cfg(test)]
mod tests;
