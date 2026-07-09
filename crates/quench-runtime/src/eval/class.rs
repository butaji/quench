//! Class expression evaluation
//!
//! This module handles class instantiation, prototype creation,
//! and class expression evaluation.

use crate::ast::{Class, Expression, Statement};
use crate::builtins;
use crate::env::Environment;
use crate::eval::statement::eval_statements;
use crate::interpreter::{predeclare_let_const, take_control_flow, ControlFlow};
use crate::value::{ClassValue, JsError, Object, ObjectKind, Value, ValueFunction};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate a class expression
pub fn eval_class_expr(class: &Class, env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    let new_value = ClassValue::from_ast(class);

    // Eagerly create the prototype for this class
    let _ = get_or_create_class_prototype(&new_value, env)?;

    Ok(Value::Class(new_value))
}

/// Instantiate a class from its AST representation
pub fn instantiate_class_from_ast_with_env(
    class: ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let proto_rc = get_or_create_class_prototype(&class, env)?;

    let mut instance = Object::new(ObjectKind::Ordinary);
    instance.prototype = Some(Rc::clone(&proto_rc));
    let instance_rc = Rc::new(RefCell::new(instance));

    proto_rc.borrow_mut().set("constructor", Value::Object(Rc::clone(&instance_rc)));

    let params = class.constructor_params.clone();
    let body = class.constructor_body.clone();
    let this_val = Value::Object(Rc::clone(&instance_rc));

    let mut call_env = Environment::with_parent(Rc::clone(env));
    call_env.current_scope_mut().set_this(this_val.clone());

    // Set the super class reference in the environment for super() calls
    if class.super_class.is_some() {
        let super_class_val = crate::eval::expression::eval_expression(
            class.super_class.as_ref().unwrap(),
            env,
        )?;
        call_env.set_super_class(super_class_val);
    }

    for (i, param) in params.iter().enumerate() {
        let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
        call_env.define(param.clone(), arg);
    }

    let args_obj = create_arguments_object_simple(args.clone());
    call_env.define("arguments".to_string(), args_obj);

    let call_env = Rc::new(RefCell::new(call_env));

    // Check if body is empty
    if body.is_empty() {
        // No constructor body - call super() if there's a superclass
        if let Some(super_class_val) = class.super_class.as_ref() {
            let super_val = crate::eval::expression::eval_expression(super_class_val, env)?;
            call_super_or_default(&super_val, args, &this_val, env)?;
        }
        Ok(this_val)
    } else {
        // Check if the first statement is an explicit super() call
        let first_is_super_call = check_first_is_super_call(&body);

        if first_is_super_call {
            // First statement is explicit super() call - just evaluate normally
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            let result = eval_statements(&body, &call_env, false)?;
            match result {
                Value::Object(_) | Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => Ok(result),
                _ => Ok(this_val),
            }
        } else {
            // No explicit super() call - call super() automatically first
            if let Some(super_class_val) = class.super_class.as_ref() {
                let super_val = crate::eval::expression::eval_expression(super_class_val, env)?;
                call_super_or_default(&super_val, args.clone(), &this_val, env)?;
            }
            // Then evaluate the constructor body
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            let result = eval_statements(&body, &call_env, false)?;
            match result {
                Value::Object(_) | Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => Ok(result),
                _ => Ok(this_val),
            }
        }
    }
}

/// Call the super constructor or use default behavior
fn call_super_or_default(super_val: &Value, args: Vec<Value>, this_val: &Value, env: &Rc<RefCell<Environment>>) -> Result<(), JsError> {
    match super_val {
        Value::Class(super_class) => {
            // Use call_super_constructor to ensure 'this' is properly bound
            call_super_constructor(super_class.clone(), args, this_val.clone(), env)?;
        }
        Value::Object(o) => {
            if let Some(Value::Function(constructor)) = o.borrow().get("constructor") {
                crate::eval::function::call_value_with_this(
                    Value::Function(constructor.clone()),
                    args,
                    this_val.clone(),
                )?;
            }
        }
        Value::NativeConstructor(nc) => {
            // For native constructors, call with 'this' binding
            crate::eval::function::call_value_with_this(
                Value::NativeConstructor(nc.clone()),
                args,
                this_val.clone(),
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// Instantiate a class from its AST representation (legacy signature)
pub fn instantiate_class_from_ast(
    class: ClassValue,
    args: Vec<Value>,
) -> Result<Value, JsError> {
    instantiate_class_from_ast_with_env(
        class,
        args,
        &Rc::new(RefCell::new(Environment::new())),
    )
}

/// Call a super constructor with the given arguments and 'this' binding
pub fn call_super_constructor(
    class: ClassValue,
    args: Vec<Value>,
    this_val: Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let proto_rc = get_or_create_class_prototype(&class, env)?;

    let params = class.constructor_params.clone();
    let body = class.constructor_body.clone();

    let mut call_env = Environment::with_parent(Rc::clone(env));
    call_env.current_scope_mut().set_this(this_val.clone());

    for (i, param) in params.iter().enumerate() {
        let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
        call_env.define(param.clone(), arg);
    }

    let args_obj = create_arguments_object_simple(args);
    call_env.define("arguments".to_string(), args_obj);

    let call_env = Rc::new(RefCell::new(call_env));

    if body.is_empty() {
        // Empty constructor - just return this
        Ok(this_val)
    } else {
        // Evaluate constructor body
        predeclare_let_const(&body, &mut call_env.borrow_mut());
        let result = eval_statements(&body, &call_env, false)?;
        match result {
            Value::Object(_) | Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => Ok(result),
            _ => Ok(this_val),
        }
    }
}

/// Check if the first statement in a constructor body is an explicit super() call
fn check_first_is_super_call(body: &[Statement]) -> bool {
    if let Some(Statement::Expression(expr)) = body.first() {
        if let Expression::Call { callee, .. } = expr.as_ref() {
            if let Expression::Identifier(id) = callee.as_ref() {
                return id == "super";
            }
        }
    }
    false
}

/// Create a simple arguments object
fn create_arguments_object_simple(args: Vec<Value>) -> Value {
    let mut obj = Object::new(ObjectKind::Ordinary);
    for (i, arg) in args.iter().enumerate() {
        obj.set(&i.to_string(), arg.clone());
    }
    obj.set("length", Value::Number(args.len() as f64));
    Value::Object(Rc::new(RefCell::new(obj)))
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

/// Get prototype from a class value (used for extends)
fn get_prototype_from_class_val(val: &Value) -> Option<Rc<RefCell<Object>>> {
    match val {
        Value::Object(o) => {
            let proto = o.borrow().get("prototype");
            if let Some(Value::Object(proto_obj)) = proto {
                Some(proto_obj.clone())
            } else {
                None
            }
        }
        Value::Class(class) => {
            let cell = class.prototype_cell.borrow();
            if let Some(ref proto) = *cell {
                return Some(Rc::clone(proto));
            }
            None
        }
        _ => None,
    }
}

/// Create a prototype for a class value (helper for extends)
fn create_class_prototype_helper_with_env(
    class: &ClassValue,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    let parent_proto = if let Some(ref super_class) = class.super_class {
        let super_class_val = crate::eval::expression::eval_expression(super_class, env)?;
        get_prototype_from_class_val(&super_class_val)
    } else {
        builtins::get_object_prototype()
    };

    let mut proto = if let Some(parent) = parent_proto {
        Object::with_prototype(ObjectKind::Ordinary, parent)
    } else {
        Object::new(ObjectKind::Ordinary)
    };

    let closure = Rc::clone(env);
    for (name, params, body) in &class.methods {
        let func = ValueFunction::new(
            Some(prop_key_to_string(name)),
            params.clone(),
            body.clone(),
            Rc::clone(&closure),
        );
        proto.set(&prop_key_to_string(name), Value::Function(func));
    }

    for (name, body) in &class.getters {
        let key = prop_key_to_string(name);
        proto.set_getter(&key, Rc::new(body.clone()), Rc::clone(&closure));
    }

    for (name, param, body) in &class.setters {
        let key = prop_key_to_string(name);
        proto.set_setter(&key, param.clone(), Rc::new(body.clone()), Rc::clone(&closure));
    }

    Ok(Rc::new(RefCell::new(proto)))
}

/// Legacy helper for creating prototype without environment (for operators.rs)
pub fn create_class_prototype_helper(class: &ClassValue) -> Result<Rc<RefCell<Object>>, JsError> {
    create_class_prototype_helper_with_env(class, &Rc::new(RefCell::new(Environment::new())))
}

/// Helper to convert PropertyKey to string
fn prop_key_to_string(key: &crate::ast::PropertyKey) -> String {
    match key {
        crate::ast::PropertyKey::Ident(s) => s.clone(),
        crate::ast::PropertyKey::String(s) => s.clone(),
        crate::ast::PropertyKey::Number(n) => n.to_string(),
        crate::ast::PropertyKey::Computed(_) => "[computed]".to_string(),
    }
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
        _ => Ok(None),
    }
}
