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
    member_target: Option<(&Value, &str)>,
) -> Result<Value, JsError> {
    match op {
        crate::ast::CompoundOp::LogicalOrAssign => {
            if to_bool(left_val) {
                Ok(left_val.clone())
            } else {
                let right_val =
                    crate::eval::expression::eval_expression(right, env, in_arrow_function)?;
                assign_logical_result(left, right, &right_val, env, member_target)?;
                Ok(right_val)
            }
        }
        crate::ast::CompoundOp::LogicalAndAssign => {
            if !to_bool(left_val) {
                Ok(left_val.clone())
            } else {
                let right_val =
                    crate::eval::expression::eval_expression(right, env, in_arrow_function)?;
                assign_logical_result(left, right, &right_val, env, member_target)?;
                Ok(right_val)
            }
        }
        crate::ast::CompoundOp::NullishCoalescingAssign => match left_val {
            Value::Null | Value::Undefined => {
                let right_val =
                    crate::eval::expression::eval_expression(right, env, in_arrow_function)?;
                assign_logical_result(left, right, &right_val, env, member_target)?;
                Ok(right_val)
            }
            _ => Ok(left_val.clone()),
        },
        _ => Err(JsError("Invalid logical compound assignment".to_string())),
    }
}

fn assign_logical_result(
    left: &Expression,
    right: &Expression,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
    member_target: Option<(&Value, &str)>,
) -> Result<(), JsError> {
    let mut assigned = value.clone();
    if let Expression::Identifier(name) = left {
        if crate::eval::object::is_anonymous_function_definition(right) {
            if let Value::Function(function) = &mut assigned {
                if function.name.is_none() {
                    function.name = Some(name.clone());
                    let _ = function.set_property("name", Value::String(name.clone()));
                }
            }
        }
    }
    if let Some((object, property)) = member_target {
        crate::eval::object::assign_to_member_value(object, property, &assigned, env)
    } else {
        crate::eval::object::assign_to(left, &assigned, env)
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
            if in_arrow_function && name == "arguments" && !env.borrow().has(name) {
                let msg = format!("ReferenceError: {} is not defined", name);
                let (err, js_err) =
                    crate::value::error::create_js_error_with_type(&msg, "ReferenceError");
                crate::value::set_thrown_value(err);
                return Err(js_err);
            }
            if name != "this" {
                let is_tdz = { env.borrow().is_tdz(name) };
                if is_tdz {
                    let msg = format!(
                        "ReferenceError: cannot access '{}' before initialization",
                        name
                    );
                    let (err, js_err) =
                        crate::value::error::create_js_error_with_type(&msg, "ReferenceError");
                    crate::value::set_thrown_value(err);
                    return Err(js_err);
                }
                let has_binding = env.borrow().has(name);
                let global = { env.borrow().get("globalThis") };
                let has_global_property = global
                    .as_ref()
                    .and_then(|value| match value {
                        crate::Value::Object(object) => {
                            crate::eval::object::proxy_has_property(object, name).ok()
                        }
                        _ => None,
                    })
                    .unwrap_or(false);
                if !has_binding && !has_global_property {
                    return Ok(Value::String("undefined".to_string()));
                }
                if !has_binding {
                    if let Some(crate::Value::Object(global)) = global {
                        let value = crate::eval::member::eval_object_member_value(
                            &global,
                            &crate::Value::String(name.to_string()),
                            None,
                        )?;
                        return crate::eval::operators::eval_unary_op(op, &value);
                    }
                }
                return match crate::eval::literal::eval_identifier(name, env, in_arrow_function) {
                    Ok(value) => crate::eval::operators::eval_unary_op(op, &value),
                    Err(error) if error.0.contains("is not defined") => {
                        Ok(Value::String("undefined".to_string()))
                    }
                    Err(error) => Err(error),
                };
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
            if matches!(object.as_ref(), Expression::Identifier(name) if name == "super") {
                let _ = crate::eval::get_super_value(env).ok_or_else(|| {
                    JsError("ReferenceError: super is only valid in class methods".to_string())
                })?;
                crate::eval::class::helpers::check_this_access_allowed(env)?;
                let _ = extract_property_name(property.clone(), *computed, env, in_arrow_function)?;
                let (thrown, error) = crate::value::error::create_js_error_with_type(
                    "Cannot delete super property",
                    "ReferenceError",
                );
                crate::value::error::set_thrown_value(thrown);
                return Err(error);
            }
            let obj_val = crate::eval::expression::eval_expression(object, env, in_arrow_function)?;
            let prop_key =
                extract_property_name(property.clone(), *computed, env, in_arrow_function)?;
            match obj_val {
                Value::Null | Value::Undefined => {
                    let msg = format!(
                        "TypeError: Cannot delete properties of {} (deleting)",
                        match &obj_val {
                            Value::Null => "null",
                            Value::Undefined => "undefined",
                            _ => unreachable!(),
                        },
                    );
                    let (err, js_err) =
                        crate::value::error::create_js_error_with_type(&msg, "TypeError");
                    crate::value::set_thrown_value(err);
                    Err(js_err)
                }
                Value::Object(obj_rc) => {
                    crate::eval::member::trigger_deferred_namespace(&obj_rc, &prop_key)?;
                    let deleted = obj_rc.borrow_mut().delete(&prop_key);
                    if !deleted && crate::interpreter::is_strict_mode() {
                        let (_, error) = crate::value::error::create_js_error_with_type(
                            "Cannot delete non-configurable property",
                            "TypeError",
                        );
                        return Err(error);
                    }
                    Ok(Value::Boolean(deleted))
                }
                Value::Function(f) => Ok(Value::Boolean(f.remove_property(&prop_key))),
                Value::Class(c) => {
                    if prop_key == "name" {
                        c.deleted_properties.borrow_mut().insert(prop_key.clone());
                        return Ok(Value::Boolean(true));
                    }
                    if prop_key == "prototype" {
                        return Ok(Value::Boolean(false));
                    }
                    if !c.has_static_own_property(&prop_key) {
                        return Ok(Value::Boolean(true));
                    }
                    c.deleted_properties.borrow_mut().insert(prop_key.clone());
                    Ok(Value::Boolean(true))
                }
                Value::NativeFunction(nf) => {
                    let flags = nf.get_property_flags(&prop_key);
                    let configurable = flags.map_or(true, |f| f.configurable);
                    if configurable {
                        nf.as_ref().remove_property(&prop_key);
                    } else if crate::interpreter::is_strict_mode() {
                        let (_, error) = crate::value::error::create_js_error_with_type(
                            "Cannot delete non-configurable property",
                            "TypeError",
                        );
                        return Err(error);
                    }
                    Ok(Value::Boolean(configurable))
                }
                Value::NativeConstructor(nc) => {
                    // Per spec: prototype is non-configurable (configurable: false)
                    // so delete returns false. name is also non-configurable.
                    // length IS configurable on most constructors.
                    if nc.delete_static_method(&prop_key) {
                        Ok(Value::Boolean(true))
                    } else if prop_key == "length" {
                        Ok(Value::Boolean(true))
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                Value::String(_)
                | Value::Number(_)
                | Value::Boolean(_)
                | Value::Symbol(_)
                | Value::BigInt(_) => Ok(Value::Boolean(true)),
                _ => Ok(Value::Boolean(false)),
            }
        }
        Expression::Identifier(name) => {
            if name == "this" {
                return Ok(Value::Boolean(true));
            }
            if matches!(name.as_str(), "NaN" | "undefined" | "Infinity") {
                return Ok(Value::Boolean(false));
            }
            if name == "new.target" {
                return Ok(Value::Boolean(true));
            }
            if crate::interpreter::is_strict_mode() {
                return Err(JsError(format!(
                    "SyntaxError: cannot delete property '{}'",
                    name
                )));
            }
            if env.borrow().is_deletable_binding(name) {
                return Ok(Value::Boolean(env.borrow_mut().delete_binding(name)));
            }
            if let Some(deleted) = env.borrow_mut().delete_from_object_env(name) {
                return Ok(Value::Boolean(deleted));
            }
            let kind = env.borrow().get_kind(name);
            if matches!(kind, Some(VarKind::Var | VarKind::Let | VarKind::Const))
                && !env.borrow().is_deletable_binding(name)
            {
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
        _ => {
            crate::eval::expression::eval_expression(expr, env, in_arrow_function)?;
            Ok(Value::Boolean(true))
        }
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
    if let Expression::Member {
        object,
        property,
        computed,
    } = argument
    {
        if matches!(object.as_ref(), Expression::Identifier(name) if name == "super") {
            let current =
                crate::eval::call::eval_super_member(property, *computed, env, in_arrow_function)?;
            let current_num = to_number(&current);
            let new_value = match op {
                UpdateOp::Increment => Value::Number(current_num + 1.0),
                UpdateOp::Decrement => Value::Number(current_num - 1.0),
            };
            crate::eval::call::set_super_property(
                property,
                *computed,
                new_value.clone(),
                env,
                in_arrow_function,
            )?;
            return if prefix {
                Ok(new_value)
            } else {
                Ok(Value::Number(current_num))
            };
        }
    }
    let identifier_scope = if let Expression::Identifier(name) = argument {
        env.borrow().binding_scope(name)
    } else {
        None
    };
    let member_target = if let Expression::Member {
        object,
        property,
        computed,
    } = argument
    {
        let object_value =
            crate::eval::expression::eval_expression(object, env, in_arrow_function)?;
        let property_name = crate::eval::call::extract_property_name(
            property.clone(),
            *computed,
            env,
            in_arrow_function,
        )?;
        let current = crate::eval::member::eval_member_access(&object_value, &property_name, env)?;
        Some((object_value, property_name, current))
    } else {
        None
    };
    let current = match member_target.as_ref() {
        Some((_, _, value)) => value.clone(),
        None => crate::eval::expression::eval_expression(argument, env, in_arrow_function)?,
    };
    let (new_value, old_value) = match &current {
        Value::BigInt(value) => {
            let unit = num_bigint::BigInt::from(1);
            let updated = match op {
                UpdateOp::Increment => value.as_ref() + &unit,
                UpdateOp::Decrement => value.as_ref() - &unit,
            };
            (
                Value::BigInt(Rc::new(updated)),
                Value::BigInt(Rc::clone(value)),
            )
        }
        _ => {
            let current_num = to_number(&current);
            if let Some(thrown) = crate::value::get_thrown_value() {
                return Err(JsError(crate::value::to_js_string(&thrown)));
            }
            let new_num = match op {
                UpdateOp::Increment => current_num + 1.0,
                UpdateOp::Decrement => current_num - 1.0,
            };
            (Value::Number(new_num), Value::Number(current_num))
        }
    };
    if let (Expression::Identifier(name), Some(scope)) = (argument, identifier_scope) {
        if !scope.borrow().is_global_object_binding()
            && !scope.borrow().is_with_environment()
            && matches!(
                scope.borrow().get_kind(name),
                Some(crate::ast::VarKind::Let | crate::ast::VarKind::Const)
            )
        {
            let updated = scope.borrow_mut().set(
                name.clone(),
                new_value.clone(),
                crate::interpreter::is_strict_mode(),
            );
            if !updated {
                let (thrown, error) = crate::value::error::create_js_error_with_type(
                    &format!("Assignment to constant variable '{name}'"),
                    "TypeError",
                );
                crate::value::set_thrown_value(thrown);
                return Err(error);
            }
            return if prefix { Ok(new_value) } else { Ok(old_value) };
        }
        let scope_ref = scope.borrow();
        if scope_ref.is_with_environment()
            && scope_ref.set_object_property_after_get(
                name,
                new_value.clone(),
                crate::interpreter::is_strict_mode(),
            ) == Some(true)
        {
            return if prefix {
                Ok(new_value.clone())
            } else {
                Ok(old_value.clone())
            };
        }
    }
    if let Some((object, property, _)) = member_target {
        crate::eval::object::assign_to_member_value(&object, &property, &new_value, env)?;
    } else {
        crate::eval::object::assign_to(argument, &new_value, env)?;
    }
    if prefix {
        Ok(new_value)
    } else {
        Ok(old_value)
    }
}

/// Evaluate a sequence expression (comma operator)
pub fn eval_sequence(
    exprs: &[Expression],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let mut last = Value::Undefined;
    let skip = if crate::value::generator_replay::is_resuming_pending_yield() {
        exprs
            .iter()
            .position(|expr| crate::value::generator_replay::count_yields_in_expr(expr) > 0)
            .unwrap_or(0)
    } else {
        0
    };
    for e in exprs.iter().skip(skip) {
        last = crate::eval::expression::eval_expression(e, env, in_arrow_function)?;
        if crate::interpreter::is_control_flow_set() || crate::interpreter::peek_generator_yield() {
            break;
        }
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

    #[test]
    fn update_uses_initial_with_binding_after_getter_deletes_property() {
        let r = eval(
            "function f() { var x = 0; var scope = { get x() { delete this.x; return 2; } }; \
             with (scope) { x++; } return scope.x === 3 && x === 0; } f()",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn update_preserves_bigint_type() {
        let r = eval("var x = 0n; x++; x === 1n").unwrap();
        assert_eq!(r, Value::Boolean(true));
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
    fn typeof_global_accessor_performs_get_value() {
        let r = eval("Object.defineProperty(this, 'value', { get() { return 1; } }); typeof value")
            .unwrap();
        assert_eq!(r, Value::String("number".into()));
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
    fn update_propagates_value_of_throw() {
        let r = eval(
            "var object = {valueOf: function() {throw 'error'}, toString: function() {return 1}}; try { ++object; 'none'; } catch (e) { e; }",
        )
        .unwrap();
        assert_eq!(r, Value::String("error".to_string()));
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
    fn delete_non_reference_expression_returns_true() {
        let r = eval("delete 1 && delete new Object()").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn delete_non_reference_expression_evaluates_side_effects() {
        let r =
            eval("var called = false; function f() { called = true; } delete f(); called").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn delete_primitive_member_returns_true() {
        let r = eval("delete 'Test262'[100]").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn delete_super_property_throws_reference_error() {
        let r = eval("class C extends Object { method(){ try { delete super.x; return false; } catch(e) { return e instanceof ReferenceError; } } } new C().method()").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn delete_function_property() {
        let r = eval("function f() {}; f.p = 42; delete f.p; f.p").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn delete_nonexistent_returns_false() {
        let r = eval("var o = {}; delete o.missing").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn compound_assignment_null_base_throws_before_property_key() {
        let err = eval(
            "var base = null; var prop = { toString: function() { throw new Error('key'); } }; base[prop] ^= 1;",
        )
        .unwrap_err();
        assert!(err.0.contains("TypeError"), "got {}", err.0);
        assert!(
            !err.0.contains("key"),
            "property key was evaluated: {}",
            err.0
        );
    }

    #[test]
    fn compound_assignment_evaluates_computed_property_once() {
        let result = eval(
            "var seen = 0; var base = {}; var prop = { toString: function() { seen++; return 'x'; } }; base[prop] ^= 1; seen",
        )
        .unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn delete_identifier_in_with_object_scope_updates_object_property() {
        let r = eval(
            "var myObj = { p1: 'a', del: false }; \
             eval('with(myObj){del = delete p1}'); \
             myObj.p1 === undefined && myObj.del === true",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
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
    fn block_expr_no_implicit_return() {
        let r = eval("var f = () => { 1; 2; 3 }; f()").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn block_expr_empty_returns_undefined() {
        let r = eval("var f = () => {}; f()").unwrap();
        assert_eq!(r, Value::Undefined);
    }
}
