//! Expression evaluation
//!
//! Main expression evaluator that dispatches to specialized modules
//! based on expression type.

#![allow(clippy::complexity)]

use crate::ast::*;
use crate::env::Environment;
use crate::eval::call::{eval_call, eval_member, eval_new, extract_property_name};
use crate::eval::class::eval_class_expr;
use crate::eval::iteration::{eval_for_in, eval_for_of};
use crate::eval::jsx::{eval_jsx_element, eval_jsx_fragment};
use crate::eval::literal::{
    eval_array_literal, eval_identifier, eval_object_literal, eval_regexp_literal,
};
pub use crate::eval::literal::{eval_property_key, get_super_value};
use crate::eval::operators::{eval_binary_op, eval_unary_op};
use crate::eval::statement::eval_statement;
pub use crate::eval::statement::eval_statements;
use crate::value::{to_bool, to_number, JsError, Value, ValueFunction};
use std::cell::RefCell;
use std::rc::Rc;

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
        Expression::Identifier(name) => eval_identifier(name, env, in_arrow_function),
        Expression::Object(props) => eval_object_literal(props, env, in_arrow_function),
        Expression::Array(elements) => eval_array_literal(elements, env, in_arrow_function),
        Expression::FunctionExpression { name, params, body } => {
            let closure = capture_env_for_closure(env);
            let mut func = ValueFunction::new(name.clone(), params.clone(), body.clone(), closure);
            func.strict = crate::interpreter::is_strict_mode();
            Ok(Value::Function(func))
        }
        Expression::ArrowFunction { params, body } => {
            let closure = capture_env_for_closure(env);
            let mut func = ValueFunction::new_arrow(params.clone(), body.clone(), closure);
            func.strict = crate::interpreter::is_strict_mode();
            // Per ES §14.2.1 step 5: arrow functions have name "" unless
            // assigned (e.g. `var x = () => {}`). The "" is stored as the
            // own property `name` so verifyProperty can read it.
            func.set_property("name", Value::String(String::new()));
            Ok(Value::Function(func))
        }
        Expression::Binary { op, left, right } => {
            let left_val = eval_expression(left, env, in_arrow_function)?;
            // Short-circuit logical operators before evaluating the right operand
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
            let identifier_scope = match left.as_ref() {
                Expression::Identifier(name) => env.borrow().binding_scope(name),
                _ => None,
            };
            let right_val = eval_expression(right, env, in_arrow_function)?;
            // Special case: identifier.prop = value - use set_property to preserve identity
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
                scope.borrow_mut().set(name.clone(), right_val.clone());
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

/// Evaluate logical compound assignment (||=, &&=, ??=)
fn eval_logical_compound_assign(
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
                let right_val = eval_expression(right, env, in_arrow_function)?;
                crate::eval::object::assign_to(left, &right_val, env)?;
                Ok(right_val)
            }
        }
        crate::ast::CompoundOp::LogicalAndAssign => {
            if !to_bool(left_val) {
                Ok(left_val.clone())
            } else {
                let right_val = eval_expression(right, env, in_arrow_function)?;
                crate::eval::object::assign_to(left, &right_val, env)?;
                Ok(right_val)
            }
        }
        crate::ast::CompoundOp::NullishCoalescingAssign => match left_val {
            Value::Null | Value::Undefined => {
                let right_val = eval_expression(right, env, in_arrow_function)?;
                crate::eval::object::assign_to(left, &right_val, env)?;
                Ok(right_val)
            }
            _ => Ok(left_val.clone()),
        },
        _ => Err(JsError("Invalid logical compound assignment".to_string())),
    }
}

/// Build the environment captured by a closure (function expression,
/// arrow function, getter/setter body, class method, function
/// declaration in a block, …). The captured environment holds the
/// SAME `Rc<RefCell<Scope>>` records as the active environment, so:
/// two closures created in the same block share the same block scope
/// and see each other's writes; a closure defined in a nested block
/// keeps the entire chain of block scopes down to the outer block;
/// a `let` initialized AFTER the closure was created is visible
/// because the closure shares storage with the live scope; scopes
/// that get popped after a block exits remain reachable to the
/// closure (via its `Rc`) but are skipped by lookups on the active
/// environment.
pub fn capture_env_for_closure(env: &Rc<RefCell<Environment>>) -> Rc<RefCell<Environment>> {
    let captured = env.borrow().capture_env();
    Rc::new(RefCell::new(captured))
}

/// Evaluate a unary expression
fn eval_unary_expr(
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
    let val = eval_expression(argument, env, in_arrow_function)?;
    eval_unary_op(op, &val)
}

/// Evaluate a delete expression
fn eval_delete(
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
            let obj_val = eval_expression(object, env, in_arrow_function)?;
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
                    // Per ES spec, ordinary functions have configurable length/name.
                    if matches!(prop_key.as_str(), "length" | "name") {
                        // Actually remove from the function's own properties
                        // so hasOwnProperty reflects the deletion.
                        let removed = f.remove_property(&prop_key);
                        Ok(Value::Boolean(removed))
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                Value::Class(c) => {
                    // Class name and prototype are configurable per spec
                    if prop_key == "name" || prop_key == "prototype" {
                        c.deleted_properties.borrow_mut().insert(prop_key.clone());
                        Ok(Value::Boolean(true))
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                _ => Ok(Value::Boolean(false)), // primitives etc.
            }
        }
        Expression::Identifier(name) => {
            // In strict mode, delete of unqualified identifier is SyntaxError
            if crate::interpreter::is_strict_mode() {
                return Err(JsError(format!(
                    "SyntaxError: cannot delete property '{}'",
                    name
                )));
            }
            // Sloppy mode: a var/let/const binding cannot be deleted (returns false).
            // A global property (one created by `x = ...` in sloppy mode and
            // stored on globalThis) CAN be deleted — remove it and return true.
            // A reference to nothing resolves silently and delete returns true.
            let kind = env.borrow().get_kind(name);
            if matches!(kind, Some(VarKind::Var | VarKind::Let | VarKind::Const)) {
                return Ok(Value::Boolean(false));
            }
            // Try to delete from globalThis object.
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
fn eval_update(
    op: UpdateOp,
    argument: &Expression,
    prefix: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let current = eval_expression(argument, env, in_arrow_function)?;
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
fn eval_sequence(
    exprs: &[Expression],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let mut last = Value::Undefined;
    for e in exprs {
        last = eval_expression(e, env, in_arrow_function)?;
    }
    Ok(last)
}

/// Evaluate a block expression
fn eval_block_expr(
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

#[cfg(test)]
mod tests {
    use crate::{Context, Value};

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        let mut ctx = Context::new().unwrap();
        ctx.eval(src)
    }

    #[test]
    fn test_logical_and_short_circuits() {
        assert_eq!(
            eval("false && (() => { throw 1; })()").unwrap(),
            Value::Boolean(false)
        );
        assert_eq!(
            eval("true || (() => { throw 1; })()").unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            eval("1 ?? (() => { throw 1; })()").unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn test_logical_compound_assign_targets_left() {
        assert_eq!(eval("let x = 0; x ||= 5; x").unwrap(), Value::Number(5.0));
        assert_eq!(eval("let y = 3; y &&= 7; y").unwrap(), Value::Number(7.0));
        assert_eq!(
            eval("let z = null; z ??= 9; z").unwrap(),
            Value::Number(9.0)
        );
        assert_eq!(eval("let w = 2; w ||= 5; w").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn test_class_instantiation() {
        assert_eq!(
            eval("class A { constructor(v) { this.v = v; } getV() { return this.v; } } let a = new A(42); a.getV()").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn test_do_while_desugaring() {
        assert_eq!(
            eval("let i = 0; do { i++; } while (i < 3); i").unwrap(),
            Value::Number(3.0)
        );
        // Body runs at least once even when condition is false
        assert_eq!(
            eval("let j = 0; do { j++; } while (false); j").unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn test_for_in_object_pattern_throws() {
        assert!(eval("for ({a} in {a: 1}) {}").is_err());
    }

    #[test]
    fn test_for_condition_error_propagates() {
        assert!(eval("for (let i = 0; (() => { throw 1; })(); i++) {}").is_err());
    }

    #[test]
    fn test_export_default_expr_lowers_to_assignment() {
        let program = crate::parser::parse_es_module("export default 42;").unwrap();
        let crate::ast::Program::Script(stmts) = program;
        let last = stmts.last().expect("expected lowered export statement");
        match last {
            crate::ast::Statement::Expression(expr) => match expr.as_ref() {
                crate::ast::Expression::Assignment { left, .. } => match left.as_ref() {
                    crate::ast::Expression::Member {
                        object, property, ..
                    } => {
                        assert!(matches!(
                            object.as_ref(),
                            crate::ast::Expression::Identifier(name) if name == "exports"
                        ));
                        assert!(matches!(
                            property,
                            crate::ast::PropertyKey::Ident(name) if name == "default"
                        ));
                    }
                    other => panic!("expected member assignment target, got {:?}", other),
                },
                other => panic!("expected assignment, got {:?}", other),
            },
            other => panic!("expected expression statement, got {:?}", other),
        }
    }
}
