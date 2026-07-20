//! Private helper functions for expression evaluation.
//! All functions here are internal helpers; public API lives in the parent `expression.rs`.

use crate::ast::*;
use crate::env::Environment;
use crate::eval::call::extract_property_name;
use crate::eval::statement::eval_statement;
use crate::value::{to_bool, to_number, JsError, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate logical compound assignment (||=, &&=, ??=)
pub fn eval_logical_compound_assign(
    op: &crate::ast::CompoundOp,
    left: &Expression,
    left_val: &Value,
    right: &Expression,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    match op {
        crate::ast::CompoundOp::LogicalOrAssign => {
            if to_bool(left_val) {
                Ok(left_val.clone())
            } else {
                let right_val =
                    crate::eval::expression::eval_expression(right, env, in_arrow_function)?;
                crate::eval::object::assign_to(left, &right_val, env)?;
                Ok(right_val)
            }
        }
        crate::ast::CompoundOp::LogicalAndAssign => {
            if !to_bool(left_val) {
                Ok(left_val.clone())
            } else {
                let right_val =
                    crate::eval::expression::eval_expression(right, env, in_arrow_function)?;
                crate::eval::object::assign_to(left, &right_val, env)?;
                Ok(right_val)
            }
        }
        crate::ast::CompoundOp::NullishCoalescingAssign => match left_val {
            Value::Null | Value::Undefined => {
                let right_val =
                    crate::eval::expression::eval_expression(right, env, in_arrow_function)?;
                crate::eval::object::assign_to(left, &right_val, env)?;
                Ok(right_val)
            }
            _ => Ok(left_val.clone()),
        },
        _ => Err(JsError("Invalid logical compound assignment".to_string())),
    }
}

/// Evaluate a unary expression
pub fn eval_unary_expr(
    op: UnaryOp,
    argument: &Expression,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    if op == UnaryOp::Typeof {
        if let Expression::Identifier(name) = argument {
            if in_arrow_function && name == "arguments" {
                return Err(JsError(format!("ReferenceError: {} is not defined", name)));
            }
            if name != "this" {
                if !env.borrow().has(name) {
                    return Ok(Value::String("undefined".to_string()));
                }
                if env.borrow().is_tdz(name) {
                    return Err(JsError(format!(
                        "ReferenceError: cannot access '{}' before initialization",
                        name
                    )));
                }
            }
        }
    }
    if op == UnaryOp::Delete {
        return eval_delete(argument, env, in_arrow_function);
    }
    let val = crate::eval::expression::eval_expression(argument, env, in_arrow_function)?;
    crate::eval::operators::eval_unary_op(op, &val)
}

/// Evaluate a delete expression
pub fn eval_delete(
    expr: &Expression,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    match expr {
        Expression::Member {
            object,
            property,
            computed,
        } => {
            let obj_val = crate::eval::expression::eval_expression(object, env, in_arrow_function)?;
            let prop_key =
                extract_property_name(property.clone(), *computed, env, in_arrow_function)?;
            match obj_val {
                Value::Null | Value::Undefined => Err(JsError(
                    "TypeError: Cannot delete property of null or undefined".to_string(),
                )),
                Value::Object(obj_rc) => {
                    let deleted = obj_rc.borrow_mut().delete(&prop_key);
                    Ok(Value::Boolean(deleted))
                }
                Value::Function(f) => {
                    if matches!(prop_key.as_str(), "length" | "name") {
                        let removed = f.remove_property(&prop_key);
                        Ok(Value::Boolean(removed))
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                Value::Class(c) => {
                    if prop_key == "name" || prop_key == "prototype" {
                        c.deleted_properties.borrow_mut().insert(prop_key.clone());
                        Ok(Value::Boolean(true))
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                Value::NativeFunction(nf) => {
                    let configurable = prop_key == "name" || prop_key == "length";
                    if configurable {
                        nf.as_ref().remove_property(&prop_key);
                    }
                    Ok(Value::Boolean(configurable))
                }
                Value::NativeConstructor(_nc) => Ok(Value::Boolean(
                    prop_key == "name" || prop_key == "prototype",
                )),
                _ => Ok(Value::Boolean(false)),
            }
        }
        Expression::Identifier(name) => {
            if crate::interpreter::is_strict_mode() {
                return Err(JsError(format!(
                    "SyntaxError: cannot delete property '{}'",
                    name
                )));
            }
            let kind = env.borrow().get_kind(name);
            if matches!(kind, Some(VarKind::Var | VarKind::Let | VarKind::Const)) {
                return Ok(Value::Boolean(false));
            }
            let global_this = env.borrow().get("globalThis");
            if let Some(Value::Object(go)) = global_this {
                if go.borrow().has(name) {
                    go.borrow_mut().delete(name);
                    return Ok(Value::Boolean(true));
                }
            }
            Ok(Value::Boolean(true))
        }
        _ => Ok(Value::Boolean(false)),
    }
}

/// Evaluate an update expression (++ or --)
pub fn eval_update(
    op: UpdateOp,
    argument: &Expression,
    prefix: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let current = crate::eval::expression::eval_expression(argument, env, in_arrow_function)?;
    let current_num = to_number(&current);
    let new_val = match op {
        UpdateOp::Increment => current_num + 1.0,
        UpdateOp::Decrement => current_num - 1.0,
    };
    crate::eval::object::assign_to(argument, &Value::Number(new_val), env)?;
    if prefix {
        Ok(Value::Number(new_val))
    } else {
        Ok(Value::Number(current_num))
    }
}

/// Evaluate a sequence expression (comma operator)
pub fn eval_sequence(
    exprs: &[Expression],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let mut last = Value::Undefined;
    for e in exprs {
        last = crate::eval::expression::eval_expression(e, env, in_arrow_function)?;
    }
    Ok(last)
}

/// Evaluate a block expression
pub fn eval_block_expr(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let mut last = Value::Undefined;
    for stmt in stmts {
        last = eval_statement(stmt, env, false, in_arrow_function)?;
    }
    Ok(last)
}
