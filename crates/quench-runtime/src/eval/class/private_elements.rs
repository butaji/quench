//! Private instance method/accessor installation (PrivateMethodOrAccessorAdd).

use crate::ast::{Expression, Statement};
use crate::env::Environment;
use crate::eval::class::helpers::{
    private_field_add, prop_key_to_string, storage_key_for_property,
};
use crate::eval::expression::capture_env_for_closure;
use crate::value::{ClassValue, JsError, Object, Value, ValueFunction};
use std::cell::RefCell;
use std::rc::Rc;

fn private_element_exists(obj: &Object, key: &str) -> bool {
    obj.properties.contains_key(key)
        || obj.getters.contains_key(key)
        || obj.setters.contains_key(key)
}

fn ensure_private_add(obj: &Rc<RefCell<Object>>, key: &str) -> Result<(), JsError> {
    if !crate::value::is_private_name_key(key) {
        return Ok(());
    }
    let o = obj.borrow();
    if private_element_exists(&o, key) {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "Private method or accessor already defined",
            "TypeError",
        );
        return Err(js_err);
    }
    if !o.extensible {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "Cannot add private field to non-extensible object",
            "TypeError",
        );
        return Err(js_err);
    }
    Ok(())
}

/// Install private instance methods, getters, and setters on `instance`.
pub fn install_instance_private_elements(
    class: &ClassValue,
    instance: &Rc<RefCell<Object>>,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let closure = class.get_class_def_env().unwrap_or_else(|| Rc::clone(env));
    let member_closure = capture_env_for_closure(&closure);

    for (name, params, body, is_async, is_generator) in &class.methods {
        let key_str = prop_key_to_string(name, &closure, false)?;
        if !key_str.starts_with('#') {
            continue;
        }
        let storage_key = storage_key_for_property(name, &key_str);
        ensure_private_add(instance, &storage_key)?;
        let mut func = ValueFunction::new(
            Some(key_str.clone()),
            params.clone(),
            body.clone(),
            Rc::clone(&member_closure),
            *is_async,
            *is_generator,
        );
        func.strict = true;
        func.is_method = true;
        private_field_add(instance, &storage_key, Value::Function(func))?;
    }

    for (name, body) in &class.getters {
        let key_str = prop_key_to_string(name, &closure, false)?;
        if !key_str.starts_with('#') {
            continue;
        }
        let storage_key = storage_key_for_property(name, &key_str);
        ensure_private_add(instance, &storage_key)?;
        instance.borrow_mut().set_getter(
            &storage_key,
            Rc::new(body.clone()),
            Rc::clone(&member_closure),
            true,
        );
    }

    for (name, param, body) in &class.setters {
        let key_str = prop_key_to_string(name, &closure, false)?;
        if !key_str.starts_with('#') {
            continue;
        }
        let storage_key = storage_key_for_property(name, &key_str);
        ensure_private_add(instance, &storage_key)?;
        instance.borrow_mut().set_setter(
            &storage_key,
            param.clone(),
            Rc::new(body.clone()),
            Rc::clone(&member_closure),
            true,
        );
    }
    Ok(())
}

pub fn program_contains_super_call(body: &[Statement]) -> bool {
    body.iter().any(stmt_contains_super_call)
}

fn stmt_contains_super_call(stmt: &Statement) -> bool {
    match stmt {
        Statement::Expression(expr) | Statement::Return(Some(expr)) => {
            expr_contains_super_call(expr)
        }
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
                    .is_some_and(|a| stmt_contains_super_call(a))
        }
        Statement::While { body, .. }
        | Statement::For { body, .. }
        | Statement::ForIn { body, .. } => stmt_contains_super_call(body),
        Statement::Try {
            body,
            handler,
            finalizer,
            ..
        } => {
            stmt_contains_super_call(body)
                || handler
                    .as_ref()
                    .is_some_and(|h| stmt_contains_super_call(h))
                || finalizer
                    .as_ref()
                    .is_some_and(|f| stmt_contains_super_call(f))
        }
        _ => false,
    }
}

fn expr_contains_super_call(expr: &Expression) -> bool {
    match expr {
        Expression::Identifier(id) => id == "super",
        Expression::Call { callee, .. } => expr_contains_super_call(callee),
        Expression::Member { object, .. } => expr_contains_super_call(object),
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
        Expression::Sequence(exprs) => exprs.iter().any(expr_contains_super_call),
        _ => false,
    }
}
