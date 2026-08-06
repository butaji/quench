//! Expression evaluation
//!
//! Main expression evaluator that dispatches to specialized modules
//! based on expression type.

use crate::ast::*;
use crate::env::Environment;
use crate::eval::call::{eval_call, eval_member, eval_new, set_super_property};
use crate::eval::class::eval_class_expr;
use crate::eval::iteration::{eval_for_in, eval_for_of};
use crate::eval::literal::{
    eval_array_literal, eval_identifier, eval_object_literal, eval_regexp_literal,
};
pub use crate::eval::literal::{eval_property_key, get_super_value};
use crate::eval::operators::eval_binary_op;
pub use crate::eval::statement::eval_statements;
use crate::eval::statement::set_on_global_this;
use crate::value::{to_bool, JsError, Value, ValueFunction};
use num_bigint::BigInt;
use std::cell::RefCell;
use std::rc::Rc;

pub mod helpers;
pub use helpers::*;

#[cfg(test)]
mod tests;

fn replay_pending_yield() -> Result<Option<Value>, JsError> {
    if !crate::value::generator_replay::is_resuming_pending_yield() {
        return Ok(None);
    }
    let Some(value) = crate::value::generator_replay::try_replay_yield() else {
        return Ok(None);
    };
    match crate::interpreter::take_control_flow() {
        Some(crate::interpreter::ControlFlow::Return(returned)) => {
            crate::interpreter::set_control_flow(crate::interpreter::ControlFlow::Return(returned));
        }
        Some(crate::interpreter::ControlFlow::Throw(thrown)) => {
            crate::value::set_thrown_value(thrown);
            return Err(JsError("Generator threw".to_string()));
        }
        Some(flow) => crate::interpreter::set_control_flow(flow),
        None => {}
    }
    Ok(Some(value))
}

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
        Expression::Yield(expr) => {
            if expr.as_ref().is_some_and(|operand| {
                crate::value::generator_replay::count_yields_in_expr(operand) == 0
            }) {
                if let Some(value) = replay_pending_yield()? {
                    return Ok(value);
                }
            }
            let value = match expr {
                Some(e) => crate::eval::expression::eval_expression(e, env, in_arrow_function)?,
                None => {
                    if let Some(replayed) = crate::value::generator_replay::try_replay_yield() {
                        // After replay, check for pending Return/Throw control flow
                        // (from generator.return() / generator.throw()).
                        // try_replay_yield already consumed the resume value,
                        // so do NOT call take_generator_resume_value() here.
                        let maybe_cf = crate::interpreter::take_control_flow();
                        if let Some(cf) = maybe_cf {
                            match cf {
                                crate::interpreter::ControlFlow::Return(val) => {
                                    crate::interpreter::set_control_flow(
                                        crate::interpreter::ControlFlow::Return(val),
                                    );
                                    return Ok(replayed);
                                }
                                crate::interpreter::ControlFlow::Throw(val) => {
                                    crate::value::set_thrown_value(val);
                                    return Err(crate::value::JsError(
                                        "Generator threw".to_string(),
                                    ));
                                }
                                other => {
                                    crate::interpreter::set_control_flow(other);
                                }
                            }
                        }
                        return Ok(replayed);
                    }
                    Value::Undefined
                }
            };
            if crate::interpreter::peek_generator_yield() {
                return Ok(Value::Undefined);
            }
            if crate::value::generator_replay::is_resuming_pending_yield() {
                if let Some(cf) = crate::interpreter::take_control_flow() {
                    match cf {
                        crate::interpreter::ControlFlow::Return(val) => {
                            crate::interpreter::set_control_flow(
                                crate::interpreter::ControlFlow::Return(val),
                            );
                            return Ok(crate::interpreter::take_generator_resume_value());
                        }
                        crate::interpreter::ControlFlow::Throw(val) => {
                            crate::value::set_thrown_value(val);
                            return Err(crate::value::JsError("Generator threw".to_string()));
                        }
                        other => crate::interpreter::set_control_flow(other),
                    }
                }
                if let Some(replayed) = crate::value::generator_replay::try_replay_yield() {
                    return Ok(replayed);
                }
            }
            let resume_val = crate::interpreter::take_generator_resume_value();
            // When generator.return() or generator.throw() resumes the generator,
            // ControlFlow::Return or ControlFlow::Throw is pending. Check this
            // BEFORE the replay path so throw/return are not masked by replayed
            // yield values.
            {
                let maybe_cf = crate::interpreter::take_control_flow();
                let is_return_or_throw = maybe_cf.is_some();
                if let Some(cf) = maybe_cf {
                    crate::interpreter::set_control_flow(cf);
                }
                if is_return_or_throw {
                    let cf = crate::interpreter::take_control_flow();
                    match cf {
                        Some(crate::interpreter::ControlFlow::Return(val)) => {
                            // Restore the Return completion so it propagates
                            // through the generator body to the return statement.
                            crate::interpreter::set_control_flow(
                                crate::interpreter::ControlFlow::Return(val.clone()),
                            );
                            crate::value::generator_replay::record_fresh_yield_resume(
                                resume_val.clone(),
                            );
                            return Ok(resume_val);
                        }
                        Some(crate::interpreter::ControlFlow::Throw(val)) => {
                            // Use the thrown value directly — don't round-trip through
                            // create_js_error_with_type which may have side effects.
                            crate::value::set_thrown_value(val);
                            return Err(crate::value::JsError("Generator threw".to_string()));
                        }
                        _ => {}
                    }
                }
            }
            // Replay path for class field computed keys (normal .next() only)
            let in_class_field = crate::interpreter::is_eval_in_class_field()
                || env.borrow().is_in_class_field_initializer();
            if expr.is_some() && in_class_field {
                if let Some(replayed) = crate::value::generator_replay::try_replay_yield() {
                    return Ok(replayed);
                }
            }

            crate::interpreter::set_generator_yield(value.clone());
            crate::eval::generator::mark_assignment_yield();
            crate::value::generator_replay::record_fresh_yield_resume(resume_val.clone());
            Ok(resume_val)
        }
        Expression::YieldDelegate(expr) => {
            crate::eval::iteration::eval_yield_delegate(expr, env, in_arrow_function)
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
            let func = Value::Function({
                let mut f = ValueFunction::new(
                    name.clone(),
                    params.clone(),
                    body.clone(),
                    Rc::clone(&closure),
                    *is_async,
                    *is_generator,
                );
                f.strict = crate::interpreter::is_strict_mode()
                    || crate::interpreter::helpers::check_use_strict_directive(body);
                f
            });
            // Per ES spec §12.4.1.3: a named FunctionExpression creates an
            // immutable binding for its own name inside the function's environment.
            // Create a fresh scope for this binding so it doesn't leak into the
            // enclosing scope via shared Rc<RefCell<Scope>> from live_scopes_snapshot.
            if let Some(ref name) = name {
                let func_clone = func.clone();
                closure.borrow_mut().push_scope();
                closure
                    .borrow_mut()
                    .declare_var(name.clone(), crate::ast::VarKind::Const);
                closure
                    .borrow_mut()
                    .current_scope()
                    .borrow_mut()
                    .mark_function_name(name.clone());
                closure.borrow_mut().initialize_declared(name, func_clone);
            }
            Ok(func)
        }
        Expression::ArrowFunction {
            params,
            body,
            is_async,
            is_generator,
        } => {
            let closure = capture_env_for_closure(env);
            let mut func = ValueFunction::new_arrow(params.clone(), body.clone(), closure);
            func.strict = crate::interpreter::is_strict_mode();
            func.is_async = *is_async;
            func.is_generator = *is_generator;
            Ok(Value::Function(func))
        }
        Expression::PrivateIn { name, right } => {
            crate::eval::generator::begin_assignment_rhs();
            let value = eval_expression(right, env, in_arrow_function)?;
            if crate::eval::generator::take_assignment_yield() {
                return Ok(Value::Undefined);
            }
            let object = match value {
                Value::Object(object) => object,
                _ => {
                    let (_, error) = crate::value::error::create_js_error_with_type(
                        "right-hand side is not an object",
                        "TypeError",
                    );
                    return Err(error);
                }
            };
            let object = object.borrow();
            Ok(Value::Boolean(
                object.properties.contains_key(name)
                    || object.getters.contains_key(name)
                    || object.setters.contains_key(name),
            ))
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
            {
                let await_arg = match right.as_ref() {
                    Expression::Await(await_arg) => Some(await_arg.as_ref()),
                    Expression::Parenthesized(inner)
                        if matches!(inner.as_ref(), Expression::Await(_)) =>
                    {
                        match inner.as_ref() {
                            Expression::Await(await_arg) => Some(await_arg.as_ref()),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(await_arg) = await_arg {
                    let awaited = eval_expression(await_arg, env, in_arrow_function)?;
                    let left_for_continuation = left_val.clone();
                    let op = *op;
                    return crate::eval::r#await::await_with_continuation(
                        awaited,
                        Rc::new(move |right_value| {
                            eval_binary_op(op, &left_for_continuation, &right_value)
                        }),
                    );
                }
            }
            let right_val = if let (Expression::Identifier(name), Expression::Class(class)) =
                (left.as_ref(), right.as_ref())
            {
                let inferred_name = if class.name.is_none() {
                    Some(name.as_str())
                } else {
                    None
                };
                crate::eval::class::eval_class_expr(class, env, inferred_name)?
            } else {
                eval_expression(right, env, in_arrow_function)?
            };
            if crate::interpreter::peek_generator_yield() {
                return Ok(right_val);
            }
            eval_binary_op(*op, &left_val, &right_val)
        }
        Expression::Unary { op, argument } => {
            eval_unary_expr(*op, argument, env, in_arrow_function)
        }
        Expression::Assignment { left, right } => {
            let identifier_scope = match left.as_ref() {
                Expression::Identifier(name) => env.borrow().binding_scope(name),
                Expression::Parenthesized(inner) => match inner.as_ref() {
                    Expression::Identifier(name) => env.borrow().binding_scope(name),
                    _ => None,
                },
                _ => None,
            };
            let assignment_target = match left.as_ref() {
                Expression::Parenthesized(inner) => inner.as_ref(),
                other => other,
            };
            if identifier_scope.is_none()
                && crate::interpreter::is_strict_mode()
                && matches!(assignment_target, Expression::Identifier(_))
            {
                let Expression::Identifier(name) = assignment_target else {
                    unreachable!();
                };
                let (_, error) = crate::value::error::create_js_error_with_type(
                    &format!("{} is not defined", name),
                    "ReferenceError",
                );
                return Err(error);
            }
            if matches!(assignment_target, Expression::Member { .. }) {
                crate::eval::object::touch_assignment_target(assignment_target, env)?;
            }
            let mut right_val = if let (Expression::Identifier(name), Expression::Class(class)) =
                (left.as_ref(), right.as_ref())
            {
                let inferred_name = if class.name.is_none() {
                    Some(name.as_str())
                } else {
                    None
                };
                eval_class_expr(class, env, inferred_name)?
            } else {
                crate::eval::generator::begin_assignment_rhs();
                eval_expression(right, env, in_arrow_function)?
            };
            if crate::eval::generator::take_assignment_yield() {
                return Ok(right_val);
            }
            if crate::eval::generator::take_suspended_assignment()
                && crate::eval::generator::take_pending_return()
            {
                if let Some(control) = crate::interpreter::take_control_flow() {
                    crate::interpreter::set_control_flow(control);
                    return Ok(right_val);
                }
            }
            if matches!(
                right.as_ref(),
                Expression::Yield(_) | Expression::YieldDelegate(_)
            ) && crate::eval::generator::take_pending_return()
            {
                if let Some(control) = crate::interpreter::take_control_flow() {
                    crate::interpreter::set_control_flow(control);
                    return Ok(right_val);
                }
            }
            if let (Expression::Identifier(name), Value::Function(function)) =
                (left.as_ref(), &right_val)
            {
                if function.name.is_none()
                    && crate::eval::object::is_anonymous_function_definition(right)
                {
                    let mut named = function.clone();
                    named.name = Some(name.clone());
                    let _ = named.set_property("name", Value::String(name.clone()));
                    right_val = Value::Function(named);
                }
            }
            // Handle super.property = value — uses super [[Set]] semantics.
            if let Expression::Member {
                object,
                property,
                computed,
            } = assignment_target
            {
                if let Expression::Identifier(name) = object.as_ref() {
                    if name == "super" {
                        return set_super_property(
                            property,
                            *computed,
                            right_val,
                            env,
                            in_arrow_function,
                        );
                    }
                }
            }
            if let Expression::Member {
                object,
                property,
                computed,
            } = assignment_target
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
            if let (Expression::Identifier(name), Some(scope)) =
                (assignment_target, identifier_scope)
            {
                if scope.borrow().is_global_object_binding()
                    && !crate::interpreter::is_strict_mode()
                    && matches!(name.as_str(), "undefined" | "NaN" | "Infinity")
                {
                    return Ok(right_val);
                }
                // Per ES spec §12.4.5.1, `let` and `const` at global scope do NOT
                // create properties on the global object. The object_binding_has
                // check below is meant for `var` bindings whose global property was
                // deleted. Skip it for `let`/`const` to avoid false ReferenceErrors.
                let is_var_like = matches!(
                    scope.borrow().get_kind(name),
                    Some(crate::ast::VarKind::Var) | None
                );
                if crate::interpreter::is_strict_mode()
                    && scope.borrow().is_global_object_binding()
                    && matches!(name.as_str(), "undefined" | "NaN" | "Infinity")
                {
                    let (_, error) = crate::value::error::create_js_error_with_type(
                        &format!("Cannot assign to read-only property '{}'", name),
                        "TypeError",
                    );
                    return Err(error);
                }
                if is_var_like
                    && crate::interpreter::is_strict_mode()
                    && scope.borrow().object_binding_has(name) == Some(false)
                    && !matches!(name.as_str(), "undefined" | "NaN" | "Infinity")
                {
                    let (_, error) = crate::value::error::create_js_error_with_type(
                        &format!("{} is not defined", name),
                        "ReferenceError",
                    );
                    return Err(error);
                }
                let object_property_result = if scope.borrow().is_with_environment() {
                    scope.borrow().set_object_property_after_get(
                        name,
                        right_val.clone(),
                        crate::interpreter::is_strict_mode(),
                    )
                } else {
                    scope.borrow().set_object_property(
                        name,
                        right_val.clone(),
                        crate::interpreter::is_strict_mode(),
                    )
                };
                if object_property_result == Some(true) {
                    if scope.borrow().is_global_object_binding() {
                        let strict = crate::interpreter::is_strict_mode();
                        if let Some(var_scope) = env.borrow().var_binding_scope(name) {
                            if !var_scope
                                .borrow_mut()
                                .set(name.clone(), right_val.clone(), strict)
                            {
                                scope
                                    .borrow_mut()
                                    .set(name.clone(), right_val.clone(), strict);
                            }
                        } else {
                            scope
                                .borrow_mut()
                                .set(name.clone(), right_val.clone(), strict);
                        }
                    }
                    return Ok(right_val);
                }
                if object_property_result == Some(false) && crate::interpreter::is_strict_mode() {
                    if scope.borrow().is_with_environment()
                        && scope.borrow().object_binding_has(name) != Some(true)
                    {
                        let (_, error) = crate::value::error::create_js_error_with_type(
                            &format!("{} is not defined", name),
                            "ReferenceError",
                        );
                        return Err(error);
                    }
                    let (_, error) = crate::value::error::create_js_error_with_type(
                        &format!("Cannot assign to read-only property '{}'", name),
                        "TypeError",
                    );
                    return Err(error);
                }
                if let Some(thrown) = crate::value::get_thrown_value() {
                    return Err(JsError(crate::value::to_js_string(&thrown)));
                }
                if object_property_result.is_none() && scope.borrow().is_with_environment() {
                    if crate::interpreter::is_strict_mode()
                        && scope.borrow().object_binding_has(name) != Some(true)
                    {
                        let (_, error) = crate::value::error::create_js_error_with_type(
                            &format!("{} is not defined", name),
                            "ReferenceError",
                        );
                        return Err(error);
                    }
                    return crate::eval::object::assign_to(left, &right_val, env)
                        .map(|_| right_val);
                }
                if !scope.borrow_mut().set(
                    name.clone(),
                    right_val.clone(),
                    crate::interpreter::is_strict_mode(),
                ) {
                    if scope.borrow().is_function_name(name)
                        && !crate::interpreter::is_strict_mode()
                    {
                        return Ok(right_val);
                    }
                    if scope.borrow().get_kind(name) == Some(VarKind::Const)
                        && !(scope.borrow().is_function_name(name)
                            && !crate::interpreter::is_strict_mode())
                    {
                        let (_, error) = crate::value::error::create_js_error_with_type(
                            &format!("Assignment to constant variable '{}'", name),
                            "TypeError",
                        );
                        return Err(error);
                    }
                    crate::eval::object::assign_to(left, &right_val, env)?;
                }
                if matches!(scope.borrow().get_kind(name), Some(VarKind::Var) | None)
                    && scope.borrow().is_object_binding()
                {
                    set_on_global_this(env, name, right_val.clone());
                }
                return Ok(right_val);
            }
            // No binding scope: identifier not found in env chain.
            if let Expression::Identifier(name) = assignment_target {
                let name = name.clone();
                if crate::interpreter::is_strict_mode()
                    && matches!(name.as_str(), "undefined" | "NaN" | "Infinity")
                {
                    let (_, error) = crate::value::error::create_js_error_with_type(
                        &format!("Cannot assign to read-only property '{}'", name),
                        "TypeError",
                    );
                    return Err(error);
                }
                if let Some(result) = env.borrow().set_in_object_env(
                    &name,
                    right_val.clone(),
                    crate::interpreter::is_strict_mode(),
                ) {
                    if let Some(thrown) = crate::value::get_thrown_value() {
                        return Err(JsError(crate::value::to_js_string(&thrown)));
                    }
                    if !result && crate::interpreter::is_strict_mode() {
                        let (_, error) = crate::value::error::create_js_error_with_type(
                            &format!("Cannot assign to read-only property '{}'", name),
                            "TypeError",
                        );
                        return Err(error);
                    }
                    return Ok(right_val);
                }
                if crate::interpreter::is_strict_mode() {
                    let (_, error) = crate::value::error::create_js_error_with_type(
                        &format!("{} is not defined", name),
                        "ReferenceError",
                    );
                    return Err(error);
                }
                env.borrow_mut().set(&name, right_val.clone());
                return Ok(right_val);
            }
            crate::eval::object::assign_to(left, &right_val, env)?;
            Ok(right_val)
        }
        Expression::CompoundAssignment { op, left, right } => {
            if let Expression::Member {
                object,
                property,
                computed,
            } = left.as_ref()
            {
                if matches!(object.as_ref(), Expression::Identifier(name) if name == "super") {
                    let left_value = crate::eval::call::eval_super_member(
                        property,
                        *computed,
                        env,
                        in_arrow_function,
                    )?;
                    let right_value = eval_expression(right, env, in_arrow_function)?;
                    let result = eval_binary_op(op.to_binary(), &left_value, &right_value)?;
                    crate::eval::call::set_super_property(
                        property,
                        *computed,
                        result.clone(),
                        env,
                        in_arrow_function,
                    )?;
                    return Ok(result);
                }
            }
            if let Expression::Member {
                object,
                property,
                computed,
            } = left.as_ref()
            {
                if matches!(object.as_ref(), Expression::Identifier(name) if name == "super") {
                    crate::eval::class::helpers::check_this_access_allowed(env)?;
                }
                let object_value = eval_expression(object, env, in_arrow_function)?;
                if matches!(object_value, Value::Null | Value::Undefined) {
                    if let PropertyKey::Computed(expression) = property {
                        // Per ES §13.15.2 step 1.a: evaluate the key first
                        // (so a throwing toString propagates) before the
                        // base check. Errors must be a proper TypeError
                        // object so the harness's assert.throws sees it.
                        eval_expression(expression, env, in_arrow_function)?;
                    }
                    let msg = format!(
                        "TypeError: Cannot read properties of {} (reading)",
                        match &object_value {
                            Value::Null => "null",
                            Value::Undefined => "undefined",
                            _ => "non-object",
                        },
                    );
                    let (err, js_err) =
                        crate::value::error::create_js_error_with_type(&msg, "TypeError");
                    crate::value::set_thrown_value(err);
                    return Err(js_err);
                }
                let property_name = crate::eval::call::extract_property_name(
                    property.clone(),
                    *computed,
                    env,
                    in_arrow_function,
                )?;
                let left_value =
                    crate::eval::member::eval_member_access(&object_value, &property_name, env)?;
                let right_value = eval_expression(right, env, in_arrow_function)?;
                let result = eval_binary_op(op.to_binary(), &left_value, &right_value)?;
                crate::eval::object::assign_to_member_value(
                    &object_value,
                    &property_name,
                    &result,
                    env,
                )?;
                return Ok(result);
            }
            // Evaluate left first (needed for binary op value).
            let mut identifier_scope = if let Expression::Identifier(name) = left.as_ref() {
                env.borrow().binding_scope(name)
            } else {
                None
            };
            let left_result = if let (Expression::Identifier(name), Some(scope)) =
                (left.as_ref(), identifier_scope.as_ref())
            {
                if scope.borrow().is_with_environment() {
                    scope
                        .borrow()
                        .get_object_binding_value_once(name)
                        .ok_or_else(|| JsError(format!("ReferenceError: {} is not defined", name)))
                } else {
                    eval_expression(left, env, in_arrow_function)
                }
            } else {
                eval_expression(left, env, in_arrow_function)
            };
            let left_val = left_result?;
            // Extract identifier info before dropping borrow.
            let ident_name = if let Expression::Identifier(name) = left.as_ref() {
                Some(name.clone())
            } else {
                None
            };
            drop(env.borrow());
            // Evaluate right side after borrow is dropped.
            let right_val = eval_expression(right, env, in_arrow_function)?;
            let result = eval_binary_op(op.to_binary(), &left_val, &right_val)?;
            // Identifier with known scope: update binding directly (avoids nested borrow).
            let mut rebind_identifier = false;
            if ident_name.is_some() {
                if let Some(name) = ident_name.as_ref() {
                    if let Some(ref scope) = identifier_scope {
                        let scope_ref = scope.borrow();
                        if scope_ref.is_with_environment() {
                            let set_result = scope_ref.set_object_property_after_get(
                                name,
                                result.clone(),
                                crate::interpreter::is_strict_mode(),
                            );
                            if set_result == Some(true) {
                                return Ok(result);
                            }
                            if set_result.is_none() && !crate::interpreter::is_strict_mode() {
                                rebind_identifier = true;
                            }
                            if crate::interpreter::is_strict_mode()
                                && scope_ref.object_binding_has(name) != Some(true)
                            {
                                let (err_val, err) = crate::value::error::create_js_error_with_type(
                                    &format!("{} is not defined", name),
                                    "ReferenceError",
                                );
                                crate::value::set_thrown_value(err_val);
                                return Err(err);
                            }
                        }
                    }
                }
                if rebind_identifier {
                    identifier_scope = None;
                }
                if let Some(scope) = identifier_scope {
                    if let Some(name) = ident_name.as_ref() {
                        crate::eval::object::cache_destructuring_identifier_reference(
                            name,
                            Some(scope),
                        );
                        crate::eval::object::assign_to_identifier(name, &result, env, None)?;
                    }
                } else {
                    crate::eval::object::assign_to(left, &result, env)?;
                }
                return Ok(result);
            }
            // Member expression or other: re-evaluate left (borrow now dropped).
            let left_val2 = eval_expression(left, env, in_arrow_function)?;
            let result2 = eval_binary_op(op.to_binary(), &left_val2, &right_val)?;
            crate::eval::object::assign_to(left, &result2, env)?;
            Ok(result2)
        }
        Expression::LogicalCompoundAssignment { op, left, right } => {
            let member_target = if let Expression::Member {
                object,
                property,
                computed,
            } = left.as_ref()
            {
                if matches!(object.as_ref(), Expression::Identifier(name) if name == "super") {
                    crate::eval::class::helpers::check_this_access_allowed(env)?;
                }
                let object_value = eval_expression(object, env, in_arrow_function)?;
                if matches!(object_value, Value::Null | Value::Undefined) {
                    if let PropertyKey::Computed(expression) = property {
                        // Per ES §13.15.2 step 1.a: evaluate the key first
                        // (so a throwing toString propagates) before the
                        // base check. Errors must be a proper TypeError
                        // object so the harness's assert.throws sees it.
                        eval_expression(expression, env, in_arrow_function)?;
                    }
                    let msg = format!(
                        "TypeError: Cannot read properties of {} (reading)",
                        match &object_value {
                            Value::Null => "null",
                            Value::Undefined => "undefined",
                            _ => "non-object",
                        },
                    );
                    let (err, js_err) =
                        crate::value::error::create_js_error_with_type(&msg, "TypeError");
                    crate::value::set_thrown_value(err);
                    return Err(js_err);
                }
                let property_name = crate::eval::call::extract_property_name(
                    property.clone(),
                    *computed,
                    env,
                    in_arrow_function,
                )?;
                let left_val =
                    crate::eval::member::eval_member_access(&object_value, &property_name, env)?;
                let result = eval_logical_compound_assign(
                    op,
                    left,
                    &left_val,
                    right,
                    env,
                    in_arrow_function,
                    Some((&object_value, property_name.as_str())),
                )?;
                return Ok(result);
            } else {
                None
            };
            let left_val = eval_expression(left, env, in_arrow_function)?;
            let result = eval_logical_compound_assign(
                op,
                left,
                &left_val,
                right,
                env,
                in_arrow_function,
                member_target,
            )?;
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
        Expression::Parenthesized(expression) => {
            eval_expression(expression, env, in_arrow_function)
        }
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
            await_of,
            loop_binding,
            dispose_async,
        } => eval_for_of(
            variable,
            iterable,
            body,
            *loop_binding,
            *dispose_async,
            *await_of,
            env,
            in_arrow_function,
        ),
        Expression::Await(arg) => {
            if let Expression::Binary { op, left, right } = arg.as_ref() {
                let left_value = eval_expression(left, env, in_arrow_function)?;
                let right_value = eval_expression(right, env, in_arrow_function)?;
                let operation = *op;
                let left_value = Rc::new(left_value);
                return crate::eval::r#await::await_with_continuation(
                    right_value,
                    Rc::new(move |value| eval_binary_op(operation, left_value.as_ref(), &value)),
                );
            }
            eval_await(arg, env, in_arrow_function)
        }
        Expression::ForIn {
            variable,
            object,
            body,
            loop_binding,
        } => eval_for_in(
            variable,
            object,
            body,
            *loop_binding,
            env,
            in_arrow_function,
        ),
        Expression::Class(class) => eval_class_expr(class, env, None),
        Expression::Spread(_) => Err(JsError(
            "Spread must be used inside an array literal context".to_string(),
        )),
        Expression::Elision => Err(JsError(
            "Array elision must be used inside an array literal context".to_string(),
        )),
    }
}

fn eval_await(
    arg: &Expression,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let value = eval_expression(arg, env, in_arrow_function)?;
    crate::eval::r#await::eval_await_value(value)
}

/// Build the environment captured by a closure.
pub fn capture_env_for_closure(env: &Rc<RefCell<Environment>>) -> Rc<RefCell<Environment>> {
    let mut captured = env.borrow().capture_env();
    if crate::interpreter::is_eval_in_class_field() || env.borrow().is_in_class_field_initializer()
    {
        captured.set_in_class_field_initializer(true);
    }
    Rc::new(RefCell::new(captured))
}
