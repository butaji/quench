//! Private helper functions for class operations.
//! All functions here are internal helpers; public API lives in the parent `class.rs`.

use crate::ast::{ArrowBody, Expression, Statement};
use crate::builtins;
use crate::env::Environment;
use crate::eval::expression::{capture_env_for_closure, eval_expression};
use crate::eval::statement::eval_function_body;
use crate::interpreter::{check_depth_guard, predeclare_let_const};
use crate::value::{ClassValue, JsError, Object, ObjectKind, Value, ValueFunction};
use std::cell::RefCell;
use std::rc::Rc;

/// Instantiate without instance fields (fast path)
pub fn instantiate_simple(
    class: &ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let _depth = check_depth_guard()?;
    let proto_rc = crate::eval::class::get_or_create_class_prototype(class, env)?;

    let mut instance = Object::new(ObjectKind::Ordinary);
    instance.prototype = Some(Rc::clone(&proto_rc));
    let instance_rc = Rc::new(RefCell::new(instance));

    let this_val = Value::Object(Rc::clone(&instance_rc));
    instance_rc
        .borrow_mut()
        .set("constructor", Value::Class(class.clone()));

    let body = class.constructor_body.clone();
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
        let body_calls_super = first_is_super || body_calls_super_call(&body);
        if body_calls_super {
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
pub fn instantiate_with_fields(
    class: &ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let _depth = check_depth_guard()?;
    let proto_rc = crate::eval::class::get_or_create_class_prototype(class, env)?;

    let mut instance = Object::new(ObjectKind::Ordinary);
    instance.prototype = Some(Rc::clone(&proto_rc));
    let instance_rc = Rc::new(RefCell::new(instance));

    let this_val = Value::Object(Rc::clone(&instance_rc));
    instance_rc
        .borrow_mut()
        .set("constructor", Value::Class(class.clone()));

    let body = class.constructor_body.clone();
    let call_env = build_constructor_env(class, &args, &this_val, env)?;
    let call_env = Rc::new(RefCell::new(call_env));

    let has_super = class.super_class.is_some();
    let body_calls_super = !body.is_empty() && body_calls_super_call(&body);

    let init_fields = || -> Result<(), JsError> {
        for (name, value_expr) in &class.instance_fields {
            let field_val = eval_expression(value_expr, &call_env, false)?;
            let key_str = prop_key_to_string(name, &call_env, false)?;
            instance_rc.borrow_mut().set(&key_str, field_val);
        }
        Ok(())
    };

    if has_super {
        if body_calls_super {
            call_env
                .borrow_mut()
                .set_pending_fields(class.instance_fields.clone());
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            eval_function_body(&body, &call_env, false)?;
        } else if body.is_empty() {
            let sv = eval_expression(class.super_class.as_ref().unwrap(), env, false)?;
            call_super_or_default(&sv, args, &this_val, env)?;
            init_fields()?;
        } else {
            let sv = eval_expression(class.super_class.as_ref().unwrap(), env, false)?;
            call_super_or_default(&sv, args.clone(), &this_val, env)?;
            init_fields()?;
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            eval_function_body(&body, &call_env, false)?;
        }
    } else {
        init_fields()?;
        if !body.is_empty() {
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            eval_function_body(&body, &call_env, false)?;
        }
    }

    Ok(this_val)
}

/// Build constructor environment (params, arguments, super reference)
pub fn build_constructor_env(
    class: &ClassValue,
    args: &[Value],
    this_val: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Environment, JsError> {
    let mut call_env = Environment::with_parent(Rc::clone(env));
    call_env
        .current_scope()
        .borrow_mut()
        .set_this_value(this_val.clone());

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
pub fn finish_constructor(result: Value, this_val: &Value) -> Result<Value, JsError> {
    match result {
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_) => Ok(result),
        _ => Ok(this_val.clone()),
    }
}

/// Call the super constructor or use default behavior
pub fn call_super_or_default(
    super_val: &Value,
    args: Vec<Value>,
    this_val: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    match super_val {
        Value::Class(super_class) => {
            crate::eval::class::call_super_constructor(
                super_class.clone(),
                args,
                this_val.clone(),
                env,
            )?;
        }
        Value::Object(o) => {
            if let Some(Value::Function(constructor)) = o.borrow().get("constructor") {
                crate::eval::function::call_value_with_this(
                    Value::Function(constructor.clone()),
                    args,
                    this_val.clone(),
                )?;
            } else if let Some(Value::NativeConstructor(nc)) = o.borrow().get("constructor") {
                crate::eval::function::call_value_with_this(
                    Value::NativeConstructor(nc.clone()),
                    args,
                    this_val.clone(),
                )?;
            }
        }
        Value::NativeConstructor(nc) => {
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

/// Check if the first statement in a constructor body is an explicit super() call
pub fn check_first_is_super_call(body: &[Statement]) -> bool {
    if let Some(Statement::Expression(expr)) = body.first() {
        if let Expression::Call { callee, .. } = expr.as_ref() {
            if let Expression::Identifier(id) = callee.as_ref() {
                return id == "super";
            }
        }
    }
    false
}

/// Check if the constructor body contains a super() call anywhere
pub fn body_calls_super_call(body: &[Statement]) -> bool {
    body.iter().any(stmt_contains_super_call)
}

fn stmt_contains_super_call(stmt: &Statement) -> bool {
    match stmt {
        Statement::Expression(expr) => expr_contains_super_call(expr),
        Statement::Block(stmts) => stmts.iter().any(stmt_contains_super_call),
        Statement::If {
            condition,
            consequent,
            alternate,
        } => {
            expr_contains_super_call(condition)
                || stmt_contains_super_call(consequent)
                || alternate
                    .as_ref()
                    .map(|a| stmt_contains_super_call(a))
                    .unwrap_or(false)
        }
        Statement::While { body, .. }
        | Statement::For { body, .. }
        | Statement::ForIn { body, .. } => stmt_contains_super_call(body),
        Statement::TryCatch { body, handler, .. } => {
            stmt_contains_super_call(body) || stmt_contains_super_call(handler)
        }
        Statement::Return(Some(expr)) => expr_contains_super_call(expr),
        _ => false,
    }
}

fn expr_contains_super_call(expr: &Expression) -> bool {
    match expr {
        Expression::Identifier(id) => id == "super",
        Expression::Call { callee, .. } => expr_contains_super_call(callee),
        Expression::Member { object, .. } => expr_contains_super_call(object),
        Expression::ArrowFunction { body, .. } => match body.as_ref() {
            ArrowBody::Expression(e) => expr_contains_super_call(e),
            ArrowBody::Block(stmts) => stmts.iter().any(stmt_contains_super_call),
        },
        Expression::Assignment { left, right, .. } => {
            expr_contains_super_call(left) || expr_contains_super_call(right)
        }
        Expression::Binary { left, right, .. } => {
            expr_contains_super_call(left) || expr_contains_super_call(right)
        }
        Expression::Unary { argument, .. } => expr_contains_super_call(argument),
        Expression::Conditional {
            condition,
            consequent,
            alternate,
        } => {
            expr_contains_super_call(condition)
                || expr_contains_super_call(consequent)
                || expr_contains_super_call(alternate)
        }
        _ => false,
    }
}

/// Create a simple arguments object
pub fn create_arguments_object_simple(args: Vec<Value>) -> Value {
    let mut obj = Object::new(ObjectKind::Ordinary);
    for (i, arg) in args.iter().enumerate() {
        obj.set(&i.to_string(), arg.clone());
    }
    obj.set("length", Value::Number(args.len() as f64));
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Per ES §7.2.4 IsConstructor: check if a value is a constructor
pub fn is_constructor_value(val: &Value) -> bool {
    match val {
        Value::Class(_) => true,
        Value::NativeConstructor(_) => true,
        Value::NativeFunction(nf) => nf.prototype.borrow().is_some(),
        Value::Function(f) => !f.is_arrow,
        Value::Object(o) => {
            o.borrow().get("prototype").is_some() && o.borrow().get("constructor").is_some()
        }
        _ => false,
    }
}

/// Get prototype from a class value (used for extends)
pub fn get_prototype_from_class_val(val: &Value) -> Option<Rc<RefCell<Object>>> {
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
                Some(Rc::clone(proto))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Create a prototype for a class value (helper for extends)
pub fn create_class_prototype_helper_with_env(
    class: &ClassValue,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    let parent_proto = if let Some(ref super_class) = class.super_class {
        let super_class_val = eval_expression(super_class, env, false)?;
        if !matches!(&super_class_val, Value::Null) && !is_constructor_value(&super_class_val) {
            return Err(JsError(
                "TypeError: superclass must be a constructor".to_string(),
            ));
        }
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
    closure.borrow_mut().push_scope();
    if let Some(ref super_class_expr) = class.super_class {
        let super_class_val = eval_expression(super_class_expr, env, false)?;
        closure.borrow_mut().set_super_class(super_class_val);
    }
    let member_closure = capture_env_for_closure(&closure);

    for (name, params, body) in &class.methods {
        let key_str = prop_key_to_string(name, &closure, false)?;
        let mut func = ValueFunction::new(
            Some(key_str.clone()),
            params.clone(),
            body.clone(),
            Rc::clone(&member_closure),
            false,
            false,
        );
        func.strict = true;
        proto.set(&key_str, Value::Function(func));
    }

    for (name, body) in &class.getters {
        let key = prop_key_to_string(name, &closure, false)?;
        proto.set_getter(&key, Rc::new(body.clone()), Rc::clone(&member_closure));
    }

    for (name, param, body) in &class.setters {
        let key = prop_key_to_string(name, &closure, false)?;
        proto.set_setter(
            &key,
            param.clone(),
            Rc::new(body.clone()),
            Rc::clone(&member_closure),
        );
    }

    Ok(Rc::new(RefCell::new(proto)))
}

/// Helper to convert PropertyKey to string, evaluating computed expressions
pub fn prop_key_to_string(
    key: &crate::ast::PropertyKey,
    env: &Rc<RefCell<Environment>>,
    in_arrow: bool,
) -> Result<String, JsError> {
    match key {
        crate::ast::PropertyKey::Ident(s) => Ok(s.clone()),
        crate::ast::PropertyKey::String(s) => Ok(s.clone()),
        crate::ast::PropertyKey::Number(n) => Ok(n.to_string()),
        crate::ast::PropertyKey::Computed(expr) => {
            let val = eval_expression(expr, env, in_arrow)?;
            Ok(crate::value::to_js_string(&val))
        }
    }
}
