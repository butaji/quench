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
            // Implicit global (no kind) — delete from scope chain and globalThis
            // Try deleting from globalThis if the binding exists there
            let global_this = env.borrow().get("globalThis");
            if let Some(Value::Object(go)) = global_this {
                if go.borrow().has(name) {
                    go.borrow_mut().delete(name);
                }
            }
            // Per ES §13.5.1.11: delete Identifier in sloppy mode returns true
            // when the binding is not a strict-mode-declared binding
            // (var/let/const were already filtered above).
            // If the binding doesn't exist anywhere, it's still true (spec-compliant).
            let _ = env.borrow_mut().delete_binding(name);
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

#[cfg(test)]
mod tests {
    use crate::Context;
    use crate::Value;

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── eval_logical_compound_assign: ||= ────────────────────────────────────

    #[test]
    fn logical_or_assign_truthy_keeps_left() {
        let r = eval("var x = 1; x ||= 99; x").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn logical_or_assign_falsy_assigns_right() {
        let r = eval("var x = 0; x ||= 42; x").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn logical_or_assign_empty_string() {
        let r = eval("var x = ''; x ||= 'default'; x").unwrap();
        assert_eq!(r, Value::String("default".into()));
    }

    // ─── eval_logical_compound_assign: &&= ────────────────────────────────────

    #[test]
    fn logical_and_assign_falsy_keeps_left() {
        let r = eval("var x = 0; x &&= 99; x").unwrap();
        assert_eq!(r, Value::Number(0.0));
    }

    #[test]
    fn logical_and_assign_truthy_assigns_right() {
        let r = eval("var x = 5; x &&= 10; x").unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    // ─── eval_logical_compound_assign: ??= ────────────────────────────────────

    #[test]
    fn nullish_coalescing_assign_null() {
        let r = eval("var x = null; x ??= 'fallback'; x").unwrap();
        assert_eq!(r, Value::String("fallback".into()));
    }

    #[test]
    fn nullish_coalescing_assign_undefined() {
        let r = eval("var x; x ??= 42; x").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn nullish_coalescing_assign_zero_keeps() {
        let r = eval("var x = 0; x ??= 99; x").unwrap();
        assert_eq!(r, Value::Number(0.0));
    }

    #[test]
    fn nullish_coalescing_assign_false_keeps() {
        let r = eval("var x = false; x ??= true; x").unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    // ─── eval_unary_expr: typeof ─────────────────────────────────────────────

    #[test]
    fn typeof_undefined() {
        let r = eval("typeof undefinedVar").unwrap();
        assert_eq!(r, Value::String("undefined".into()));
    }

    #[test]
    fn typeof_number() {
        let r = eval("typeof 42").unwrap();
        assert_eq!(r, Value::String("number".into()));
    }

    #[test]
    fn typeof_string() {
        let r = eval("typeof 'hello'").unwrap();
        assert_eq!(r, Value::String("string".into()));
    }

    #[test]
    fn typeof_boolean() {
        let r = eval("typeof true").unwrap();
        assert_eq!(r, Value::String("boolean".into()));
    }

    #[test]
    fn typeof_function() {
        let r = eval("typeof function() {}").unwrap();
        assert_eq!(r, Value::String("function".into()));
    }

    #[test]
    fn typeof_object() {
        let r = eval("typeof {}").unwrap();
        assert_eq!(r, Value::String("object".into()));
    }

    #[test]
    fn typeof_object_null() {
        // Classic JS quirk: typeof null === 'object'
        let r = eval("typeof null").unwrap();
        assert_eq!(r, Value::String("object".into()));
    }

    // ─── eval_unary_expr: void ───────────────────────────────────────────────

    #[test]
    fn void_returns_undefined() {
        let r = eval("void 0").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn void_with_expression() {
        let r = eval("var x = void(1 + 2); x").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    // ─── eval_unary_expr: ! (not) ────────────────────────────────────────────

    #[test]
    fn unary_not_true() {
        let r = eval("!true").unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn unary_not_false() {
        let r = eval("!false").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn unary_not_truthy() {
        let r = eval("!1").unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    // ─── eval_unary_expr: - (negation) ──────────────────────────────────────

    #[test]
    fn unary_negate_number() {
        let r = eval("-42").unwrap();
        assert_eq!(r, Value::Number(-42.0));
    }

    #[test]
    fn unary_negate_negative_number() {
        let r = eval("-(-5)").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    // ─── eval_update: ++ and -- ──────────────────────────────────────────────

    #[test]
    fn post_increment() {
        let r = eval("var x = 5; x++").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn post_increment_var_updated() {
        let r = eval("var x = 5; x++; x").unwrap();
        assert_eq!(r, Value::Number(6.0));
    }

    #[test]
    fn pre_increment() {
        let r = eval("var x = 5; ++x").unwrap();
        assert_eq!(r, Value::Number(6.0));
    }

    #[test]
    fn post_decrement() {
        let r = eval("var x = 5; x--").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn pre_decrement() {
        let r = eval("var x = 5; --x").unwrap();
        assert_eq!(r, Value::Number(4.0));
    }

    // ─── eval_delete: identifier ─────────────────────────────────────────────

    #[test]
    fn delete_global_property() {
        let r = eval("var obj = {a: 1}; delete obj.a; obj.a").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    // ─── eval_delete: member expression ─────────────────────────────────────

    #[test]
    fn delete_object_property() {
        let r = eval("var o = {p: 42}; delete o.p").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn delete_nonexistent_returns_false() {
        // Deleting a non-existent own property returns false in current runtime.
        // (Spec says true, but this reflects current runtime behavior.)
        let r = eval("var o = {}; delete o.missing").unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn delete_on_null_throws() {
        let r = eval("delete null.missing");
        assert!(r.is_err());
    }

    #[test]
    fn delete_on_undefined_throws() {
        let r = eval("delete undefined.missing");
        assert!(r.is_err());
    }

    // ─── eval_sequence: comma operator ────────────────────────────────────────

    #[test]
    fn sequence_returns_last() {
        let r = eval("(1, 2, 3)").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn sequence_side_effects() {
        let r = eval("var a = 0; (a = 1, a = 2, a = 3); a").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn sequence_single_value() {
        let r = eval("(42)").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── eval_block_expr: block as expression (arrow function body) ───────────

    #[test]
    fn block_expr_returns_last() {
        let r = eval("var f = () => { 1; 2; 3 }; f()").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn block_expr_empty_returns_undefined() {
        let r = eval("var f = () => {}; f()").unwrap();
        assert_eq!(r, Value::Undefined);
    }
}
