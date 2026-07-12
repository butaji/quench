//! Class expression evaluation
//!
//! This module handles class instantiation, prototype creation,
//! and class expression evaluation.

use crate::ast::{Class, Expression, Param, Statement};
use crate::builtins;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::statement::eval_function_body;
use crate::interpreter::{check_depth_guard, predeclare_let_const};
use crate::value::{ClassValue, JsError, Object, ObjectKind, Value, ValueFunction};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate a class expression
pub fn eval_class_expr(class: &Class, env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    let new_value = ClassValue::from_ast(class);

    // Eagerly create the prototype for this class
    let _ = get_or_create_class_prototype(&new_value, env)?;

    // Initialize static fields on the class object
    for (name, value_expr) in &new_value.static_fields {
        let field_value = eval_expression(value_expr, env, false)?;
        let key_str = prop_key_to_string(name);
        new_value.set_static_field(&key_str, field_value);
    }

    Ok(Value::Class(new_value))
}

/// Instantiate a class from its AST representation
pub fn instantiate_class_from_ast_with_env(
    class: ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Fast path: no instance fields → use simpler logic
    if class.instance_fields.is_empty() {
        return instantiate_simple(&class, args, env);
    }
    instantiate_with_fields(&class, args, env)
}

/// Instantiate without instance fields (fast path)
fn instantiate_simple(
    class: &ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let _depth = check_depth_guard()?;
    let proto_rc = get_or_create_class_prototype(class, env)?;

    let mut instance = Object::new(ObjectKind::Ordinary);
    instance.prototype = Some(Rc::clone(&proto_rc));
    let instance_rc = Rc::new(RefCell::new(instance));

    proto_rc
        .borrow_mut()
        .set("constructor", Value::Object(Rc::clone(&instance_rc)));

    let _params = class.constructor_params.clone();
    let body = class.constructor_body.clone();
    let this_val = Value::Object(Rc::clone(&instance_rc));

    let call_env = build_constructor_env(class, &args, &this_val, env)?;
    let call_env = Rc::new(RefCell::new(call_env));

    if body.is_empty() {
        if let Some(ref sc) = class.super_class {
            let sv = eval_expression(sc, env, false)?;
            call_super_or_default(&sv, args, &this_val, env)?;
        }
        Ok(this_val)
    } else {
        let first_is_super = check_first_is_super_call(&body);
        if first_is_super {
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            let result = eval_function_body(&body, &call_env, false)?;
            finish_constructor(result, &this_val)
        } else {
            if let Some(ref sc) = class.super_class {
                let sv = eval_expression(sc, env, false)?;
                call_super_or_default(&sv, args, &this_val, env)?;
            }
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            let result = eval_function_body(&body, &call_env, false)?;
            finish_constructor(result, &this_val)
        }
    }
}

/// Instantiate with instance fields: fields init after super(), before body
fn instantiate_with_fields(
    class: &ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let _depth = check_depth_guard()?;
    let proto_rc = get_or_create_class_prototype(class, env)?;

    let mut instance = Object::new(ObjectKind::Ordinary);
    instance.prototype = Some(Rc::clone(&proto_rc));
    let instance_rc = Rc::new(RefCell::new(instance));

    proto_rc
        .borrow_mut()
        .set("constructor", Value::Object(Rc::clone(&instance_rc)));

    let body = class.constructor_body.clone();
    let this_val = Value::Object(Rc::clone(&instance_rc));

    let call_env = build_constructor_env(class, &args, &this_val, env)?;
    let call_env = Rc::new(RefCell::new(call_env));

    let has_super = class.super_class.is_some();
    let first_is_super = !body.is_empty() && check_first_is_super_call(&body);

    // Phase 1: evaluate constructor body (includes super() call handling)
    if body.is_empty() {
        if has_super {
            let sv = eval_expression(class.super_class.as_ref().unwrap(), env, false)?;
            call_super_or_default(&sv, args, &this_val, env)?;
        }
    } else if first_is_super {
        predeclare_let_const(&body, &mut call_env.borrow_mut());
        eval_function_body(&body, &call_env, false)?;
    } else {
        if has_super {
            let sv = eval_expression(class.super_class.as_ref().unwrap(), env, false)?;
            call_super_or_default(&sv, args.clone(), &this_val, env)?;
        }
        predeclare_let_const(&body, &mut call_env.borrow_mut());
        eval_function_body(&body, &call_env, false)?;
    }

    // Phase 2: initialize instance fields on 'this'
    for (name, value_expr) in &class.instance_fields {
        let field_val = eval_expression(value_expr, &call_env, false)?;
        let key_str = prop_key_to_string(name);
        instance_rc.borrow_mut().set(&key_str, field_val);
    }

    Ok(this_val)
}

/// Build constructor environment (params, arguments, super reference)
fn build_constructor_env(
    class: &ClassValue,
    args: &[Value],
    this_val: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Environment, JsError> {
    let mut call_env = Environment::with_parent(Rc::clone(env));
    call_env.current_scope_mut().set_this(this_val.clone());

    if let Some(ref sc) = class.super_class {
        let sv = eval_expression(sc, env, false)?;
        call_env.set_super_class(sv);
    }

    for (i, param) in class.constructor_params.iter().enumerate() {
        let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
        call_env.define(param.clone(), arg);
    }

    let args_obj = create_arguments_object_simple(args.to_vec());
    call_env.define("arguments".to_string(), args_obj);

    Ok(call_env)
}

/// Handle constructor return value (constructors return `this` by default)
fn finish_constructor(result: Value, this_val: &Value) -> Result<Value, JsError> {
    match result {
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_) => Ok(result),
        _ => Ok(this_val.clone()),
    }
}

/// Call the super constructor or use default behavior
fn call_super_or_default(
    super_val: &Value,
    args: Vec<Value>,
    this_val: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
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
pub fn instantiate_class_from_ast(class: ClassValue, args: Vec<Value>) -> Result<Value, JsError> {
    instantiate_class_from_ast_with_env(class, args, &Rc::new(RefCell::new(Environment::new())))
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
    call_env.current_scope_mut().set_this(this_val.clone());

    for (i, param) in _params.iter().enumerate() {
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
        let result = eval_function_body(&body, &call_env, false)?;
        match result {
            Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_) => Ok(result),
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
#[allow(dead_code)]
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
        let super_class_val = crate::eval::expression::eval_expression(super_class, env, false)?;
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
        let params_vec: Vec<Param> = params.iter().map(|p| Param::new(p)).collect();
        let mut func = ValueFunction::new(
            Some(prop_key_to_string(name)),
            params_vec,
            body.clone(),
            Rc::clone(&closure),
        );
        // Class bodies are always strict mode (ES spec 15.7).
        func.strict = true;
        proto.set(&prop_key_to_string(name), Value::Function(func));
    }

    for (name, body) in &class.getters {
        let key = prop_key_to_string(name);
        proto.set_getter(&key, Rc::new(body.clone()), Rc::clone(&closure));
    }

    for (name, param, body) in &class.setters {
        let key = prop_key_to_string(name);
        proto.set_setter(
            &key,
            param.clone(),
            Rc::new(body.clone()),
            Rc::clone(&closure),
        );
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
