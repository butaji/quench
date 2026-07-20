//! Expression evaluation
//!
//! Main expression evaluator that dispatches to specialized modules
//! based on expression type.

use crate::ast::*;
use crate::env::Environment;
use crate::eval::call::{eval_call, eval_member, eval_new};
use crate::eval::class::eval_class_expr;
use crate::eval::iteration::{eval_for_in, eval_for_of};
use crate::eval::jsx::{eval_jsx_element, eval_jsx_fragment};
use crate::eval::literal::{
    eval_array_literal, eval_identifier, eval_object_literal, eval_regexp_literal,
};
pub use crate::eval::literal::{eval_property_key, get_super_value};
use crate::eval::operators::eval_binary_op;
pub use crate::eval::statement::eval_statements;
use crate::value::{to_bool, JsError, Value, ValueFunction};
use num_bigint::BigInt;
use std::cell::RefCell;
use std::rc::Rc;

pub mod helpers;
pub use helpers::*;

#[cfg(test)]
mod tests;

/// Evaluate an expression
pub fn eval_expression(
    expr: &Expression,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    match expr {
        Expression::Number(n) => Ok(Value::Number(*n)),
        Expression::String(s) => Ok(Value::String(s.clone())),
        Expression::Boolean(b) => Ok(Value::Boolean(*b)),
        Expression::Null => Ok(Value::Null),
        Expression::Undefined => Ok(Value::Undefined),
        Expression::RegExp { pattern, flags } => eval_regexp_literal(pattern, flags),
        Expression::BigInt(raw) => {
            let raw = raw.strip_suffix('n').unwrap_or(raw);
            let (digits, radix) = if raw.starts_with("0x") || raw.starts_with("0X") {
                (&raw[2..], 16)
            } else if raw.starts_with("0b") || raw.starts_with("0B") {
                (&raw[2..], 2)
            } else if raw.starts_with("0o") || raw.starts_with("0O") {
                (&raw[2..], 8)
            } else {
                (raw, 10)
            };
            let bi = BigInt::parse_bytes(digits.as_bytes(), radix)
                .ok_or_else(|| JsError(format!("Invalid BigInt literal: {}", raw)))?;
            Ok(Value::BigInt(std::rc::Rc::new(bi)))
        }
        Expression::Identifier(name) => eval_identifier(name, env, in_arrow_function),
        Expression::Object(props) => eval_object_literal(props, env, in_arrow_function),
        Expression::Array(elements) => eval_array_literal(elements, env, in_arrow_function),
        Expression::FunctionExpression {
            name,
            params,
            body,
            is_async,
            is_generator,
        } => {
            let closure = capture_env_for_closure(env);
            let mut func = ValueFunction::new(
                name.clone(),
                params.clone(),
                body.clone(),
                closure,
                *is_async,
                *is_generator,
            );
            func.strict = crate::interpreter::is_strict_mode();
            Ok(Value::Function(func))
        }
        Expression::ArrowFunction { params, body } => {
            let closure = capture_env_for_closure(env);
            let mut func = ValueFunction::new_arrow(params.clone(), body.clone(), closure);
            func.strict = crate::interpreter::is_strict_mode();
            func.set_property("name", Value::String(String::new()));
            Ok(Value::Function(func))
        }
        Expression::Binary { op, left, right } => {
            let left_val = eval_expression(left, env, in_arrow_function)?;
            match op {
                BinaryOp::And => {
                    if !to_bool(&left_val) {
                        return Ok(left_val);
                    }
                }
                BinaryOp::Or => {
                    if to_bool(&left_val) {
                        return Ok(left_val);
                    }
                }
                BinaryOp::NullishCoalescing
                    if !matches!(left_val, Value::Null | Value::Undefined) =>
                {
                    return Ok(left_val);
                }
                _ => {}
            }
            let right_val = eval_expression(right, env, in_arrow_function)?;
            eval_binary_op(*op, &left_val, &right_val)
        }
        Expression::Unary { op, argument } => {
            eval_unary_expr(*op, argument, env, in_arrow_function)
        }
        Expression::Assignment { left, right } => {
            if crate::interpreter::is_strict_mode() {
                if let Expression::Identifier(name) = left.as_ref() {
                    if matches!(name.as_str(), "NaN" | "undefined" | "Infinity") {
                        let (_, error) = crate::value::error::create_js_error_with_type(
                            &format!("Cannot assign to '{}' in strict mode", name),
                            "TypeError",
                        );
                        return Err(error);
                    }
                }
            }
            let identifier_scope = match left.as_ref() {
                Expression::Identifier(name) => env.borrow().binding_scope(name),
                _ => None,
            };
            let right_val = eval_expression(right, env, in_arrow_function)?;
            if let Expression::Member {
                object,
                property,
                computed,
            } = left.as_ref()
            {
                if !*computed {
                    if let Expression::Identifier(name) = object.as_ref() {
                        let prop_name = match property {
                            crate::ast::PropertyKey::Ident(s) => Some(s.clone()),
                            crate::ast::PropertyKey::String(s) => Some(s.clone()),
                            crate::ast::PropertyKey::Number(n) => Some(n.to_string()),
                            _ => None,
                        };
                        if let Some(prop) = prop_name {
                            if env
                                .borrow_mut()
                                .set_property(name, &prop, right_val.clone())
                            {
                                return Ok(right_val);
                            }
                        }
                    }
                }
            }
            if let (Expression::Identifier(name), Some(scope)) = (left.as_ref(), identifier_scope) {
                if scope.borrow().object_binding_has(name) == Some(false)
                    && crate::interpreter::is_strict_mode()
                {
                    let (_, error) = crate::value::error::create_js_error_with_type(
                        &format!("{} is not defined", name),
                        "ReferenceError",
                    );
                    return Err(error);
                }
                if scope.borrow_mut().set_object_property(
                    name,
                    right_val.clone(),
                    crate::interpreter::is_strict_mode(),
                ) == Some(true)
                {
                    return Ok(right_val);
                }
                if !scope.borrow_mut().set(
                    name.clone(),
                    right_val.clone(),
                    crate::interpreter::is_strict_mode(),
                ) {
                    if scope.borrow().get_kind(name) == Some(VarKind::Const) {
                        let (_, error) = crate::value::error::create_js_error_with_type(
                            &format!("Assignment to constant variable '{}'", name),
                            "TypeError",
                        );
                        return Err(error);
                    }
                    crate::eval::object::assign_to(left, &right_val, env)?;
                }
                return Ok(right_val);
            }
            crate::eval::object::assign_to(left, &right_val, env)?;
            Ok(right_val)
        }
        Expression::CompoundAssignment { op, left, right } => {
            let left_val = eval_expression(left, env, in_arrow_function)?;
            let right_val = eval_expression(right, env, in_arrow_function)?;
            let result = eval_binary_op(op.to_binary(), &left_val, &right_val)?;
            crate::eval::object::assign_to(left, &result, env)?;
            Ok(result)
        }
        Expression::LogicalCompoundAssignment { op, left, right } => {
            let left_val = eval_expression(left, env, in_arrow_function)?;
            let result =
                eval_logical_compound_assign(op, left, &left_val, right, env, in_arrow_function)?;
            Ok(result)
        }
        Expression::Call { callee, arguments } => {
            eval_call(callee, arguments, env, in_arrow_function)
        }
        Expression::Member {
            object,
            property,
            computed,
        } => eval_member(object, property, *computed, env, in_arrow_function),
        Expression::Conditional {
            condition,
            consequent,
            alternate,
        } => {
            if to_bool(&eval_expression(condition, env, in_arrow_function)?) {
                eval_expression(consequent, env, in_arrow_function)
            } else {
                eval_expression(alternate, env, in_arrow_function)
            }
        }
        Expression::Update {
            op,
            argument,
            prefix,
        } => eval_update(*op, argument, *prefix, env, in_arrow_function),
        Expression::New {
            constructor,
            arguments,
        } => eval_new(constructor, arguments, env, in_arrow_function),
        Expression::Sequence(exprs) => eval_sequence(exprs, env, in_arrow_function),
        Expression::BlockExpr(stmts) => eval_block_expr(stmts, env, in_arrow_function),
        Expression::ArrayPattern(_) => Err(JsError(
            "Array pattern must be used in assignment context".to_string(),
        )),
        Expression::ObjectPattern(_) => Err(JsError(
            "Object pattern must be used in assignment context".to_string(),
        )),
        Expression::ForOf {
            variable,
            iterable,
            body,
        } => eval_for_of(variable, iterable, body, env, in_arrow_function),
        Expression::ForIn {
            variable,
            object,
            body,
        } => eval_for_in(variable, object, body, env, in_arrow_function),
        Expression::JsxElement {
            tag,
            props,
            children,
        } => eval_jsx_element(tag, props, children, env),
        Expression::JsxFragment { children } => eval_jsx_fragment(children, env),
        Expression::Class(class) => eval_class_expr(class, env, None),
        Expression::Spread(_) => Err(JsError(
            "Spread must be used inside an array literal context".to_string(),
        )),
        Expression::Elision => Err(JsError(
            "Array elision must be used inside an array literal context".to_string(),
        )),
    }
}

/// Build the environment captured by a closure.
pub fn capture_env_for_closure(env: &Rc<RefCell<Environment>>) -> Rc<RefCell<Environment>> {
    let captured = env.borrow().capture_env();
    Rc::new(RefCell::new(captured))
}
