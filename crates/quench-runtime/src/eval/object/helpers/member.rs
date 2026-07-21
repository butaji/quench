//! Member access helpers.

use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::string_methods::get_string_method;
use crate::value::{JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Get a member function from a value.
pub fn get_member_function(
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match obj_val {
        Value::Object(o) => crate::eval::member::eval_object_member(o, prop_name, Some(env)),
        Value::String(s) => get_string_method(s, prop_name, env),
        Value::Number(_) | Value::BigInt(_) => get_number_method(obj_val, prop_name, env),
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
            for (name, params, body, is_async, is_generator) in &class.static_methods {
                if name_matches_prop(name, prop_name) {
                    let mut func = crate::value::ValueFunction::new(
                        Some(prop_name.to_string()),
                        params.clone(),
                        body.clone(),
                        Rc::clone(env),
                        *is_async,
                        *is_generator,
                    );
                    func.strict = true;
                    return Ok(Value::Function(func));
                }
            }
            if let Some(ref super_expr) = class.super_class {
                let super_val = eval_expression(super_expr, env, false)?;
                return crate::eval::member::eval_member_access(&super_val, prop_name, env);
            }
            let proto = crate::eval::class::get_or_create_class_prototype(class, env)?;
            crate::eval::member::eval_object_member(&proto, prop_name, Some(env))
        }
        Value::Generator(gen) => {
            let is_async = gen.borrow().is_async;
            match prop_name {
                "next" => {
                    if is_async {
                        Ok(crate::value::generator::async_generator_next_fn(
                            gen.clone(),
                        ))
                    } else {
                        Ok(crate::value::generator::generator_next_fn(gen.clone()))
                    }
                }
                "return" => {
                    if is_async {
                        Ok(crate::value::generator::async_generator_return_fn(
                            gen.clone(),
                        ))
                    } else {
                        Ok(crate::value::generator::generator_return_fn(gen.clone()))
                    }
                }
                "throw" => {
                    if is_async {
                        Ok(crate::value::generator::async_generator_throw_fn(
                            gen.clone(),
                        ))
                    } else {
                        Ok(crate::value::generator::generator_throw_fn(gen.clone()))
                    }
                }
                _ => Ok(Value::Undefined),
            }
        }
        _ => Ok(Value::Undefined),
    }
}

/// Check if a property key matches a name.
pub fn name_matches_prop(key: &crate::ast::PropertyKey, name: &str) -> bool {
    match key {
        crate::ast::PropertyKey::Ident(s) => s == name,
        crate::ast::PropertyKey::String(s) => s == name,
        crate::ast::PropertyKey::Number(n) => n.to_string() == name,
        crate::ast::PropertyKey::Computed(_) => false,
    }
}

/// Get a method from a boxed Number/Boolean/Symbol/BigInt.
pub fn get_number_method(
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if !matches!(
        obj_val,
        Value::Number(_) | Value::Boolean(_) | Value::Symbol(_) | Value::BigInt(_)
    ) {
        return Ok(Value::Undefined);
    }
    let ctor_name = match obj_val {
        Value::Number(_) => "Number",
        Value::Boolean(_) => "Boolean",
        Value::Symbol(_) => "Symbol",
        Value::BigInt(_) => "BigInt",
        _ => return Ok(Value::Undefined),
    };
    let ctor_val = match env.borrow().get(ctor_name) {
        Some(v) => v,
        None => return Ok(Value::Undefined),
    };
    let proto = match &ctor_val {
        Value::Object(o) => o.borrow().get("prototype"),
        Value::NativeFunction(nf) => nf
            .prototype
            .borrow()
            .as_ref()
            .map(|p| Value::Object(Rc::clone(p))),
        Value::NativeConstructor(nc) => Some(Value::Object(Rc::clone(&nc.prototype))),
        _ => None,
    };
    let proto_rc = match proto {
        Some(Value::Object(o)) => o,
        _ => return Ok(Value::Undefined),
    };
    let mut boxed = Object::new(ObjectKind::Ordinary);
    boxed.prototype = Some(Rc::clone(&proto_rc));
    match obj_val {
        Value::Number(n) => {
            boxed.exotic_kind = Some(crate::value::kind::ExoticKind::Number);
            boxed.set("_value", Value::Number(*n));
        }
        Value::Boolean(b) => {
            boxed.exotic_kind = Some(crate::value::kind::ExoticKind::Boolean);
            boxed.set("_value", Value::Boolean(*b));
        }
        Value::Symbol(_) => {}
        _ => {}
    }
    let boxed_rc = Rc::new(RefCell::new(boxed));
    crate::eval::member::eval_object_member(&boxed_rc, prop_name, Some(env))
}

/// Get the `this` value for a getter/setter call.
pub fn getter_this_value(obj: &Rc<RefCell<Object>>) -> Value {
    let obj_borrow = obj.borrow();
    if let Some(exotic) = &obj_borrow.exotic_kind {
        match exotic {
            crate::value::kind::ExoticKind::Number => {
                if let Some(Value::Number(n)) = obj_borrow.properties.get("_value") {
                    return Value::Number(*n);
                }
            }
            crate::value::kind::ExoticKind::Boolean => {
                if let Some(Value::Boolean(b)) = obj_borrow.properties.get("_value") {
                    return Value::Boolean(*b);
                }
            }
            _ => {}
        }
    }
    Value::Object(Rc::clone(obj))
}

/// Check if any prototype in the chain has a non-writable property with this name.
pub fn has_readonly_prototype_property(object: &Rc<RefCell<Object>>, property: &str) -> bool {
    let mut prototype = object.borrow().prototype.as_ref().map(Rc::clone);
    while let Some(current) = prototype {
        let borrowed = current.borrow();
        if let Some(descriptor) = borrowed.get_descriptor(property) {
            return !descriptor.writable;
        }
        prototype = borrowed.prototype.as_ref().map(Rc::clone);
    }
    false
}

/// Check if a constructor has a read-only built-in property.
pub fn is_readonly_constructor_property(constructor: &str, property: &str) -> bool {
    if matches!(property, "length" | "name") {
        return true;
    }
    constructor == "Number"
        && matches!(
            property,
            "MAX_VALUE"
                | "MIN_VALUE"
                | "NaN"
                | "NEGATIVE_INFINITY"
                | "POSITIVE_INFINITY"
                | "MAX_SAFE_INTEGER"
                | "MIN_SAFE_INTEGER"
                | "EPSILON"
        )
}
