//! Class expression evaluation
//!
//! This module handles class instantiation, prototype creation,
//! and class expression evaluation.

use crate::ast::{Class, VarKind};
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::value::{ClassValue, JsError, Object, Value};
use std::cell::RefCell;
use std::rc::Rc;

pub mod helpers;
pub use helpers::*;

#[allow(dead_code)]
fn class_static_field_this_name() {
    let _ = 42;
}

/// Evaluate a class expression. The `inferred_name` parameter provides the
/// inferred class name per ES §14.6.13 step 18 when the class is anonymous
/// and the surrounding context supplies the name.
pub fn eval_class_expr(
    class: &Class,
    env: &Rc<RefCell<Environment>>,
    inferred_name: Option<&str>,
) -> Result<Value, JsError> {
    let mut new_value = ClassValue::from_ast(class);
    if let Some(name) = inferred_name {
        new_value.set_name(name);
    }

    let class_name = class.name.as_deref().or(inferred_name);

    let class_scope = if let Some(name) = class_name {
        let scope_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(env))));
        scope_env
            .borrow_mut()
            .current_scope()
            .borrow_mut()
            .declare_var(name.to_string(), VarKind::Const);
        let class_val = Value::Class(new_value.clone());
        scope_env
            .borrow_mut()
            .current_scope()
            .borrow_mut()
            .initialize_declared(name, class_val);
        scope_env
    } else {
        Rc::clone(env)
    };

    let _ = get_or_create_class_prototype(&new_value, &class_scope)?;

    let class_value = Value::Class(new_value.clone());
    for (name, value_expr) in &new_value.static_fields {
        let child_env: Rc<RefCell<Environment>> =
            Rc::new(RefCell::new(Environment::with_parent(Rc::clone(env))));
        child_env
            .borrow_mut()
            .current_scope()
            .borrow_mut()
            .set_this(class_value.clone());
        let field_value = eval_expression(value_expr, &child_env, false)?;
        let key_str = prop_key_to_string(name, &child_env, true)?;
        if key_str == "prototype" || key_str == "constructor" {
            return Err(JsError(format!(
                "TypeError: static class field may not be named '{}'",
                key_str
            )));
        }
        new_value.set_static_field(&key_str, field_value);
    }

    Ok(Value::Class(new_value))
}

#[allow(dead_code)]
fn infer_class_name_from_env(_env: &Rc<RefCell<Environment>>) -> Option<String> {
    None
}

/// Instantiate a class from its AST representation
pub fn instantiate_class_from_ast_with_env(
    class: ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if class.instance_fields.is_empty() {
        instantiate_simple(&class, args, env)
    } else {
        instantiate_with_fields(&class, args, env)
    }
}

/// Instantiate a class from its AST representation (legacy signature)
pub fn instantiate_class_from_ast(class: ClassValue, args: Vec<Value>) -> Result<Value, JsError> {
    let env = crate::context::get_current_env()
        .unwrap_or_else(|| Rc::new(RefCell::new(Environment::new())));
    instantiate_class_from_ast_with_env(class, args, &env)
}

/// Call a super constructor with the given arguments and 'this' binding
pub fn call_super_constructor(
    class: ClassValue,
    args: Vec<Value>,
    this_val: Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let _proto_rc = get_or_create_class_prototype(&class, env)?;

    let _params = class.constructor_params.clone();
    let body = class.constructor_body.clone();

    let mut call_env = Environment::with_parent(Rc::clone(env));
    call_env
        .current_scope()
        .borrow_mut()
        .set_this(this_val.clone());

    for (i, param) in _params.iter().enumerate() {
        let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
        call_env.define(param.clone(), arg);
    }

    let args_obj = create_arguments_object_simple(args);
    call_env.define("arguments".to_string(), args_obj);

    let call_env = Rc::new(RefCell::new(call_env));

    if body.is_empty() {
        Ok(this_val)
    } else {
        crate::interpreter::predeclare_let_const(&body, &mut call_env.borrow_mut());
        let result = crate::eval::statement::eval_function_body(&body, &call_env, false)?;
        finish_constructor(result, &this_val)
    }
}

/// Get or create the prototype for a class, caching it in the ClassValue
pub fn get_or_create_class_prototype(
    class: &ClassValue,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    {
        let cell = class.prototype_cell.borrow();
        if let Some(ref proto) = *cell {
            return Ok(Rc::clone(proto));
        }
    }

    let proto_rc = create_class_prototype_helper_with_env(class, env)?;

    {
        let mut cell = class.prototype_cell.borrow_mut();
        *cell = Some(Rc::clone(&proto_rc));
    }

    Ok(proto_rc)
}

/// Legacy helper for creating prototype without environment
pub fn create_class_prototype_helper(class: &ClassValue) -> Result<Rc<RefCell<Object>>, JsError> {
    create_class_prototype_helper_with_env(class, &Rc::new(RefCell::new(Environment::new())))
}

/// Get constructor prototype from a value
pub fn get_constructor_prototype(val: &Value) -> Result<Option<Rc<RefCell<Object>>>, JsError> {
    match val {
        Value::Object(o) => {
            let proto = o.borrow().get("prototype");
            if let Some(Value::Object(proto_obj)) = proto {
                Ok(Some(proto_obj.clone()))
            } else {
                Ok(None)
            }
        }
        Value::Function(f) => Ok(Some(f.get_prototype())),
        Value::NativeConstructor(nc) => Ok(Some(Rc::clone(&nc.prototype))),
        Value::NativeFunction(nf) => {
            // Prototype is set by JS harness via Test262Error.prototype = ... (set_property).
            // Also set constructor on the prototype so instanceof works.
            if let Some(Value::Object(proto_obj)) = nf.get_property("prototype") {
                Ok(Some(proto_obj))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests;
