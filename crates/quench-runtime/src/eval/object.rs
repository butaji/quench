//! Object operations: assignment, property access, getter/setter calls

use crate::ast::*;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::statement::eval_function_body;
use crate::eval::string_methods::get_string_method;
use crate::value::{GetterStorage, JsError, Object, ObjectKind, SetterStorage, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Assign a value to a target (variable, member, or destructuring pattern)
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

fn assign_array_destructuring(
    bindings: &[BindingElement],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let arr_rc = match value {
        Value::Object(o) => o.clone(),
        Value::String(s) => {
            let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
            let len = chars.len();
            let mut arr = Object::new(ObjectKind::Array);
            arr.elements = chars;
            arr.properties
                .insert("length".to_string(), Value::Number(len as f64));
            Rc::new(RefCell::new(arr))
        }
        _ => return Err(JsError("Cannot destructure non-iterable value".to_string())),
    };
    for (i, binding) in bindings.iter().enumerate() {
        let elem_value = {
            let arr_ref = arr_rc.borrow();
            arr_ref.get(&i.to_string()).unwrap_or(Value::Undefined)
        };
        assign_binding_elem(binding, &elem_value, env)?;
    }
    Ok(())
}

fn assign_object_destructuring(
    props: &[(PropertyKey, BindingElement)],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let obj = match value {
        Value::Object(o) => o.clone(),
        _ => return Err(JsError("Cannot destructure non-object value".to_string())),
    };
    for (key, binding) in props {
        let key_str = extract_destructure_key(key, env)?;
        let prop_value = {
            let obj_ref = obj.borrow();
            obj_ref.get(&key_str).unwrap_or(Value::Undefined)
        };
        assign_binding_elem(binding, &prop_value, env)?;
    }
    Ok(())
}

fn extract_destructure_key(
    key: &PropertyKey,
    env: &Rc<RefCell<Environment>>,
) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(s) => Ok(s.clone()),
        PropertyKey::String(s) => Ok(s.clone()),
        PropertyKey::Number(n) => Ok(n.to_string()),
        PropertyKey::Computed(expr) => Ok(crate::value::to_js_string(&eval_expression(
            expr, env, false,
        )?)),
    }
}

fn assign_binding_elem(
    binding: &BindingElement,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    match binding {
        BindingElement::Identifier(name) => assign_to_identifier(name, value, env),
        BindingElement::ArrayPattern(bindings) => assign_array_destructuring(bindings, value, env),
        BindingElement::ObjectPattern(props) => assign_object_destructuring(props, value, env),
    }
}

fn assign_to_identifier(
    name: &str,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    if env.borrow().has(name) {
        if let Some(kind) = env.borrow().get_kind(name) {
            if kind == VarKind::Const {
                return Err(JsError(
                    "TypeError: Assignment to constant variable".to_string(),
                ));
            }
        }
        env.borrow_mut().set(name, value.clone());
    } else {
        env.borrow_mut().define(name.to_string(), value.clone());
    }
    Ok(())
}

fn assign_to_member(
    object: &Expression,
    property: &PropertyKey,
    computed: bool,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let prop_name = extract_property_name(property, computed, env, false)?;

    // Handle the case where object is itself a member expression (chained access)
    // e.g., assert.deepEqual._compare where we need to update assert.deepEqual
    if let Expression::Member {
        object: parent_obj,
        property: parent_prop,
        computed: parent_computed,
    } = object
    {
        let parent_prop_name = extract_property_name(parent_prop, *parent_computed, env, false)?;
        let parent_val = eval_expression(parent_obj, env, false)?;

        if let Value::Object(ref parent_o) = parent_val {
            // Clone the function out, modify it, put it back
            let func_opt = {
                let parent_read = parent_o.borrow();
                parent_read.properties.get(&parent_prop_name).cloned()
            };

            if let Some(Value::Function(func)) = func_opt {
                func.set_property(&prop_name, value.clone());
                // Put the modified function back
                parent_o
                    .borrow_mut()
                    .properties
                    .insert(parent_prop_name, Value::Function(func));
                return Ok(());
            }

            // Handle NativeFunction properties (e.g. assert.deepEqual._compare where assert is an Object)
            if let Some(Value::NativeFunction(nf)) = func_opt {
                nf.set_property(&prop_name, value.clone());
                // Put the modified native function back
                parent_o
                    .borrow_mut()
                    .properties
                    .insert(parent_prop_name, Value::NativeFunction(nf));
                return Ok(());
            }
        }

        // Same for native-function parents (e.g. assert.deepEqual._compare = ...):
        // reading a function property yields a clone, so write the modified
        // function back onto the shared Rc<NativeFunction>.
        if let Value::NativeFunction(ref nf) = parent_val {
            if let Some(Value::Function(func)) = nf.get_property(&parent_prop_name) {
                func.set_property(&prop_name, value.clone());
                nf.set_property(&parent_prop_name, Value::Function(func));
                return Ok(());
            }
            // Handle NativeFunction properties (e.g. assert.deepEqual._compare where assert and deepEqual are both NativeFunction)
            if let Some(Value::NativeFunction(inner_nf)) = nf.get_property(&parent_prop_name) {
                inner_nf.set_property(&prop_name, value.clone());
                nf.set_property(&parent_prop_name, Value::NativeFunction(inner_nf));
                return Ok(());
            }
        }
    }

    let obj_val = eval_expression(object, env, false)?;

    match obj_val {
        Value::Object(o) => {
            let has_setter = {
                let obj_ref = o.borrow();
                obj_ref.get_setter(&prop_name).is_some()
            };
            if has_setter {
                let setter_clone = {
                    let obj_ref = o.borrow();
                    obj_ref.get_setter(&prop_name).cloned()
                };
                if let Some(setter_storage) = setter_clone {
                    call_setter(&o, &setter_storage, value.clone(), env)?;
                    return Ok(());
                }
            }
            // Try to set function property in place if the property exists and is a function
            if o.borrow_mut()
                .set_function_property(&prop_name, &prop_name, value.clone())
            {
                // Already modified the function in place via set_function_property
                return Ok(());
            }
            // Reject property sets on frozen objects
            if crate::builtins::object_static::is_frozen_object(&o) {
                return Ok(());
            }
            o.borrow_mut().set(&prop_name, value.clone());
            // Mirror writes on the globalThis object into the global binding,
            // so identifier resolution (which checks env scopes before the
            // globalThis fallback) stays in sync: `globalThis.x = v; x` === v.
            let is_global_this = env
                .borrow()
                .get("globalThis")
                .map(|g| matches!(g, Value::Object(ref go) if Rc::ptr_eq(go, &o)))
                .unwrap_or(false);
            if is_global_this && !env.borrow_mut().set(&prop_name, value.clone()) {
                env.borrow_mut().define(prop_name.clone(), value.clone());
            }
            Ok(())
        }
        Value::Function(ref f) => {
            f.set_property(&prop_name, value.clone());
            Ok(())
        }
        Value::NativeFunction(ref nf) => {
            nf.set_property(&prop_name, value.clone());
            Ok(())
        }
        Value::NativeConstructor(ref nc) => {
            nc.set_property(&prop_name, value.clone());
            Ok(())
        }
        _ => Err(JsError(format!(
            "Cannot assign to property of non-object, got {:?}",
            obj_val
        ))),
    }
}

fn extract_property_name(
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
                    Value::Symbol(s) => Ok(s.clone()),
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

/// Evaluate a callee expression and extract the function and "this" binding.
pub fn eval_callee_with_this(
    callee: &Expression,
    env: &Rc<RefCell<Environment>>,
) -> Result<(Value, Value), JsError> {
    match callee {
        Expression::Member {
            object,
            property,
            computed,
        } => {
            let obj_val = eval_expression(object, env, false)?;
            let prop_name = extract_property_name(property, *computed, env, false)?;
            let func = get_member_function(&obj_val, &prop_name, env)?;
            Ok((func, obj_val))
        }
        _ => {
            let func = eval_expression(callee, env, false)?;
            Ok((func, Value::Undefined))
        }
    }
}

fn get_member_function(
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match obj_val {
        Value::Object(o) => crate::eval::member::eval_object_member(o, prop_name),
        Value::String(s) => get_string_method(s, prop_name, env),
        Value::Number(_) => get_number_method(obj_val, prop_name, env),
        Value::Function(f) => crate::eval::member::eval_function_member(f, prop_name),
        Value::NativeFunction(nf) => {
            crate::eval::member::eval_native_function_member(nf, prop_name)
        }
        Value::NativeConstructor(nc) => {
            crate::eval::member::eval_native_constructor_member(nc, prop_name)
        }
        Value::Class(class) => {
            if let Some(val) = class.get_static_field(prop_name) {
                return Ok(val);
            }
            for (name, params, body) in &class.static_methods {
                if name_matches_prop(name, prop_name) {
                    let params_vec: Vec<Param> = params.iter().map(|p| Param::new(p)).collect();
                    let mut func = crate::value::ValueFunction::new(
                        Some(prop_name.to_string()),
                        params_vec,
                        body.clone(),
                        Rc::clone(env),
                    );
                    // Class bodies are always strict mode (ES spec 15.7).
                    func.strict = true;
                    return Ok(Value::Function(func));
                }
            }
            let proto = crate::eval::class::get_or_create_class_prototype(class, env)?;
            crate::eval::member::eval_object_member(&proto, prop_name)
        }
        _ => Ok(Value::Undefined),
    }
}

fn name_matches_prop(key: &crate::ast::PropertyKey, name: &str) -> bool {
    match key {
        crate::ast::PropertyKey::Ident(s) => s == name,
        crate::ast::PropertyKey::String(s) => s == name,
        crate::ast::PropertyKey::Number(n) => n.to_string() == name,
        crate::ast::PropertyKey::Computed(_) => false,
    }
}

fn get_number_method(
    _obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(Value::Object(ref num_obj)) = env.borrow().get("Number") {
        let num_obj = num_obj.borrow();
        if let Some(Value::Object(ref proto)) = num_obj.get("prototype") {
            let proto_obj = proto.borrow();
            if let Some(val) = proto_obj.get(prop_name) {
                return Ok(val);
            }
        }
    }
    Ok(Value::Undefined)
}

/// Call a getter function with the object as "this"
pub fn call_getter(
    obj: &Rc<RefCell<Object>>,
    getter_storage: &GetterStorage,
    _env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(func) = &getter_storage.func {
        return crate::eval::function::call_value_with_this(
            func.clone(),
            Vec::new(),
            Value::Object(Rc::clone(obj)),
        );
    }
    let closure = Rc::clone(&getter_storage.closure);
    let body = getter_storage.body.clone();
    let mut call_env = Environment::with_parent(closure);
    call_env
        .current_scope_mut()
        .set_this(Value::Object(Rc::clone(obj)));
    let call_env = Rc::new(RefCell::new(call_env));
    if body.is_empty() {
        Ok(Value::Undefined)
    } else {
        eval_function_body(&body, &call_env, false)
    }
}

/// Call a setter function with the object as "this" and the value as the parameter
pub fn call_setter(
    obj: &Rc<RefCell<Object>>,
    setter_storage: &SetterStorage,
    value: Value,
    _env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(func) = &setter_storage.func {
        return crate::eval::function::call_value_with_this(
            func.clone(),
            vec![value],
            Value::Object(Rc::clone(obj)),
        );
    }
    let closure = Rc::clone(&setter_storage.closure);
    let body = setter_storage.body.clone();
    let param = setter_storage.param.clone();
    let mut call_env = Environment::with_parent(Rc::clone(&closure));
    call_env
        .current_scope_mut()
        .set_this(Value::Object(Rc::clone(obj)));
    call_env.define(param, value);
    let call_env = Rc::new(RefCell::new(call_env));
    if body.is_empty() {
        Ok(Value::Undefined)
    } else {
        eval_function_body(&body, &call_env, false)
    }
}
