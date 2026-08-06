//! Statement evaluation

use crate::ast::*;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::interpreter::{
    add_label, collect_for_head_lexical_names, collect_var_names_recursive, has_label,
    loop_handles_break, loop_handles_continue, pop_label_scope, predeclare_let_const,
    predeclare_var, push_for_body_iteration_scope, push_label_scope, set_control_flow,
    take_control_flow, ControlFlow,
};
use crate::value::function::ValueFunction;
use crate::value::{
    get_thrown_value, set_thrown_value, take_thrown_value, to_bool, to_js_string, to_object,
    JsError, Object, ObjectKind, Value,
};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

thread_local! {
    static ASYNC_DISPOSAL_BOUNDARY: Cell<bool> = const { Cell::new(false) };
    static ASYNC_DISPOSAL_EVALUATED: Cell<bool> = const { Cell::new(false) };
}

fn is_in_async_context() -> bool {
    crate::interpreter::is_in_async_function() || crate::interpreter::is_in_async_generator()
}

/// Returns true if expr is a Call expression eligible for tail-call optimization.
/// Direct `eval()` calls are excluded — they must not be tail-called.
pub(crate) fn is_tail_expr(expr: &Expression) -> bool {
    match expr {
        Expression::Call { .. } => true,
        Expression::Binary {
            op: BinaryOp::And | BinaryOp::Or | BinaryOp::NullishCoalescing,
            right,
            ..
        } => is_tail_expr(right),
        Expression::Conditional {
            consequent,
            alternate,
            ..
        } => is_tail_expr(consequent) || is_tail_expr(alternate),
        Expression::Sequence(expressions) => expressions.last().is_some_and(is_tail_expr),
        Expression::Parenthesized(inner) => is_tail_expr(inner),
        _ => false,
    }
}

/// Tail-call signal produced by `eval_function_body` and consumed by the
/// trampoline in `call_js_function_impl_with_strict`.
/// Stores the already-resolved `ValueFunction` + evaluated `Vec<Value>` args.
/// The accumulator chain is managed via the separate thread-local ACC_STACK.
#[derive(Debug, Clone)]
pub struct TailCallSignal {
    /// The resolved function to call (already extracted from Value::Function).
    pub function: ValueFunction,
    /// The evaluated arguments.
    pub arguments: Vec<Value>,
    /// `this` binding for member-expression tail calls; otherwise `Undefined`.
    pub this_val: Value,
}

impl TailCallSignal {
    pub fn new(function: ValueFunction, arguments: Vec<Value>, this_val: Value) -> Self {
        Self {
            function,
            arguments,
            this_val,
        }
    }
}

// Thread-local tail-call signal produced by `eval_function_body` and
// consumed by `call_js_function_impl_with_strict`'s trampoline.
thread_local! {
    static TAIL_CALL_SIGNAL: std::cell::RefCell<Option<TailCallSignal>> =
        const { std::cell::RefCell::new(None) };
    // Separate accumulator stack: survives across tail-call chains.
    // Each tail call pushes acc onto the stack; when the trampoline
    // gets a result back, it pops and combines with the returned value.
    static ACC_STACK: std::cell::RefCell<Vec<Value>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static EXPLICIT_RETURN_STACK: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
}

fn set_explicit_return_for_current_body() {
    EXPLICIT_RETURN_STACK.with(|stack| {
        if let Some(top) = stack.borrow_mut().last_mut() {
            *top = true;
        }
    });
}

/// Result of evaluating a function body, including whether it used an explicit `return`.
pub(crate) struct FunctionBodyResult {
    pub value: Value,
    pub explicit_return: bool,
}

/// Whether the most recent `eval_function_body` completed via an explicit `return`.
pub fn take_explicit_function_return() -> bool {
    EXPLICIT_RETURN_STACK.with(|stack| stack.borrow().last().copied().unwrap_or(false))
}

/// Set the tail-call signal for the trampoline to pick up.
pub(crate) fn set_tail_call_signal(signal: TailCallSignal) {
    TAIL_CALL_SIGNAL.with(|cell| *cell.borrow_mut() = Some(signal));
}

/// Take and clear the tail-call signal (consumed by the trampoline).
pub(crate) fn take_tail_call_signal() -> Option<TailCallSignal> {
    TAIL_CALL_SIGNAL.with(|cell| cell.borrow_mut().take())
}

fn has_tail_call_signal() -> bool {
    TAIL_CALL_SIGNAL.with(|cell| cell.borrow().is_some())
}

/// Push acc onto the thread-local accumulator stack (called before each tail call).
pub(crate) fn acc_stack_push(acc: Value) {
    ACC_STACK.with(|cell| cell.borrow_mut().push(acc));
}

/// Update the last (topmost) value on the acc stack. Used by the trampoline
/// to store the result from a returning function before looping.
pub(crate) fn acc_stack_update_last(val: Value) {
    ACC_STACK.with(|cell| {
        let mut stack = cell.borrow_mut();
        if let Some(last) = stack.last_mut() {
            *last = val;
        }
    });
}

/// Return the current length of the accumulator stack.
pub(crate) fn acc_stack_len() -> usize {
    ACC_STACK.with(|cell| cell.borrow().len())
}

/// Pop all entries down to a target length. Used by the trampoline to
/// restore the stack to a saved depth after a non-tail call returns.
pub(crate) fn acc_stack_pop_to(target_len: usize) {
    ACC_STACK.with(|cell| {
        let mut stack = cell.borrow_mut();
        stack.truncate(target_len);
    });
}

/// Return a clone of the topmost value on the stack, or None if empty.
/// Exists for test coverage of the accumulator stack.
#[allow(dead_code)]
pub(crate) fn acc_stack_top() -> Option<Value> {
    ACC_STACK.with(|cell| cell.borrow().last().cloned())
}

fn is_empty_completion(stmt: &Statement) -> bool {
    match stmt {
        Statement::VarDeclaration { .. }
        | Statement::FunctionDeclaration { .. }
        | Statement::ClassDeclaration { .. }
        | Statement::Dispose { .. }
        | Statement::RegisterDispose { .. }
        | Statement::Break(_)
        | Statement::Continue(_)
        | Statement::Empty => true,
        Statement::Block(stmts) => stmts.iter().all(is_empty_completion),
        Statement::SequenceDecls(stmts) => stmts.iter().all(is_empty_completion),
        Statement::Try { .. } => false,
        _ => false,
    }
}

/// Evaluate a list of statements
pub fn eval_statements(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    is_expr_body: bool,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let mut last_val = Value::Undefined;
    let last_idx = stmts.len().saturating_sub(1);
    for (i, stmt) in stmts.iter().enumerate() {
        let is_last_stmt = i == last_idx;
        let val = eval_statement(stmt, env, is_expr_body, in_arrow_function)?;
        // Per ES spec §8.3.2, empty completions (var/let/const/function declarations,
        // empty statements, empty blocks) should not replace the previous completion value.
        // Only update last_val when the statement produces a non-empty value.
        let is_empty_completion = is_empty_completion(stmt);
        if !is_empty_completion {
            last_val = val.clone();
        }
        let is_for_await = matches!(
            stmt,
            Statement::Expression(expr)
                if matches!(expr.as_ref(), Expression::ForOf { await_of: true, .. })
        );
        let pending_await =
            !is_last_stmt && crate::eval::r#await::take_last_pending_await().is_some();
        if (pending_await || is_in_async_context() && is_for_await)
            && crate::eval::r#await::is_promise(&val)
            && !is_last_stmt
        {
            let resumed = crate::eval::r#await::await_statement(
                val,
                stmts[i + 1..].to_vec(),
                Rc::clone(env),
                in_arrow_function,
            )?;
            take_control_flow();
            return Ok(resumed);
        }
        if crate::interpreter::peek_generator_yield() {
            return Ok(last_val);
        }
        // For the last statement, DON'T check ControlFlow::Return here.
        // The caller (eval_function_body) handles the final statement specially
        // so that `return g()` (non-tail call) evaluates the expression `g()`
        // before propagating the return. This prevents inner non-tail call
        // results from short-circuiting the rest of the function body.
        if is_last_stmt {
            continue;
        }
        match take_control_flow() {
            Some(ControlFlow::Return(value))
                if (pending_await || is_for_await)
                    && crate::eval::r#await::is_promise(&value)
                    && !is_last_stmt =>
            {
                return crate::eval::r#await::await_statement(
                    value,
                    stmts[i + 1..].to_vec(),
                    Rc::clone(env),
                    in_arrow_function,
                );
            }
            Some(ControlFlow::Return(val)) | Some(ControlFlow::Yield(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            // YieldDelegate: also propagate as Return (the generator handles it)
            Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            // Throw (from generator.throw()): surface as a real error at the
            // yield point so enclosing try/catch observes the thrown value.
            Some(ControlFlow::Throw(val)) => {
                set_thrown_value(val);
                return Err(JsError("Generator threw".to_string()));
            }
            // Propagate break/continue so enclosing loops can observe them.
            Some(cf @ (ControlFlow::Break(_) | ControlFlow::Continue(_))) => {
                set_control_flow(cf);
                return Ok(last_val);
            }
            None => {}
        }
    }
    Ok(last_val)
}

/// Evaluate a function body: return the completion value of the last
/// statement. Per ES spec, a function body returns the completion value of
/// its final statement when no explicit `return` is present.
///
/// When the last statement is `return callExpr` (at any nesting depth inside
/// a block), evaluates callee+args, resolves the target function, and sets a
/// thread-local signal for the trampoline in `call_js_function_impl_with_strict`.
pub fn eval_function_body(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    eval_function_body_with_meta(stmts, env, in_arrow_function).map(|r| r.value)
}

pub(crate) fn eval_function_body_with_meta(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<FunctionBodyResult, JsError> {
    EXPLICIT_RETURN_STACK.with(|stack| stack.borrow_mut().push(false));
    let value = eval_function_body_impl(stmts, env, in_arrow_function)?;
    let explicit_return =
        EXPLICIT_RETURN_STACK.with(|stack| stack.borrow_mut().pop().unwrap_or(false));
    Ok(FunctionBodyResult {
        value,
        explicit_return,
    })
}

fn eval_function_body_impl(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    {
        let mut names = Vec::new();
        collect_var_names_recursive(stmts, &mut names);
        let mut env_mut = env.borrow_mut();
        for name in names {
            env_mut.declare_var(name, VarKind::Var);
        }
    }
    let last_idx = stmts.len().saturating_sub(1);
    let mut _last_val = Value::Undefined;
    for (i, stmt) in stmts.iter().enumerate() {
        let is_last_stmt = i == last_idx;

        if is_in_async_context() {
            if let Statement::Expression(expr) = stmt {
                if let Expression::Await(arg) = expr.as_ref() {
                    let value = eval_expression(arg, env, in_arrow_function)?;
                    let awaited = crate::eval::r#await::await_statement(
                        value,
                        stmts[i + 1..].to_vec(),
                        Rc::clone(env),
                        in_arrow_function,
                    )?;
                    take_control_flow();
                    return Ok(awaited);
                }
            }
        }

        // Check for tail-call return at top level.
        if let Statement::Return(ref expr) = stmt {
            let handled_tail = is_last_stmt
                && expr.as_ref().is_some_and(|e| is_tail_expr(e))
                && acc_stack_len() > 0
                && try_handle_tail_call(expr, env, in_arrow_function)?;
            if handled_tail {
                break;
            }
            // Non-tail return (or tail return outside an active trampoline).
            let val = match expr {
                Some(e) => eval_expression(e, env, in_arrow_function)?,
                None => Value::Undefined,
            };
            set_control_flow(ControlFlow::Return(val.clone()));
            set_explicit_return_for_current_body();
            return Ok(val);
        }

        // Check for tail-call return inside a block or do-while at the last position.
        if is_last_stmt {
            if !matches!(stmt, Statement::Block(_) | Statement::If { .. })
                && acc_stack_len() > 0
                && has_tail_call_in_statement(stmt, env, in_arrow_function)?
            {
                break;
            }
            if let Statement::Block(inner_stmts) = stmt {
                if acc_stack_len() > 0
                    && handle_tail_call_in_block(inner_stmts, env, in_arrow_function, false)?
                        .is_some()
                {
                    // Tail call was set; break to let trampoline run.
                    break;
                }
            } else if let Statement::DoWhile { body, .. } = stmt {
                if acc_stack_len() > 0
                    && handle_tail_call_in_block(
                        std::slice::from_ref(body.as_ref()),
                        env,
                        in_arrow_function,
                        true,
                    )?
                    .is_some()
                {
                    break;
                }
            } else if let Statement::For { body, .. } = stmt {
                if acc_stack_len() > 0
                    && handle_tail_call_in_block(
                        std::slice::from_ref(body.as_ref()),
                        env,
                        in_arrow_function,
                        true,
                    )?
                    .is_some()
                {
                    break;
                }
            } else if let Statement::SequenceDecls(stmts) = stmt {
                if acc_stack_len() > 0
                    && handle_tail_call_in_block(stmts, env, in_arrow_function, true)?.is_some()
                {
                    break;
                }
            }
        }

        let stmt_val = eval_statement(stmt, env, false, in_arrow_function)?;
        if has_tail_call_signal() {
            return Ok(Value::Undefined);
        }
        let is_for_await = matches!(
            stmt,
            Statement::Expression(expr)
                if matches!(expr.as_ref(), Expression::ForOf { await_of: true, .. })
        );
        let pending_await = crate::eval::r#await::take_last_pending_await().is_some();
        let suspended_await = (pending_await || is_in_async_context() && is_for_await)
            && crate::eval::r#await::is_promise(&stmt_val);
        if suspended_await {
            let resumed = crate::eval::r#await::await_statement(
                stmt_val,
                stmts[i + 1..].to_vec(),
                Rc::clone(env),
                in_arrow_function,
            )?;
            take_control_flow();
            return Ok(resumed);
        }
        // Per ES §8.3.2, empty completions (var/let/const/function declarations,
        // empty statements, break/continue, empty blocks) should not replace the previous
        // completion value.
        let is_empty = is_empty_completion(stmt);
        if !is_empty {
            _last_val = stmt_val;
        }
        if take_async_disposal_boundary() && !is_last_stmt {
            let tail = &stmts[i + 1..];
            if !matches!(tail.first(), Some(Statement::Return(_))) {
                queue_async_function_tail(tail, env, in_arrow_function)?;
                return Ok(Value::Undefined);
            }
        }
        // For the last statement, DON'T check ControlFlow::Return here.
        // Let the final return statement be reached and evaluated properly.
        // This prevents inner non-tail call results from short-circuiting
        // the rest of the function body (e.g., `var x = g(); return x + 1`).
        if is_last_stmt {
            continue;
        }
        match take_control_flow() {
            Some(ControlFlow::Return(val)) => {
                set_explicit_return_for_current_body();
                return Ok(val);
            }
            Some(ControlFlow::Throw(val)) => {
                set_thrown_value(val);
                return Err(JsError("Generator threw".to_string()));
            }
            Some(
                cf @ (ControlFlow::Break(_)
                | ControlFlow::Continue(_)
                | ControlFlow::Yield(_)
                | ControlFlow::YieldDelegate(_)),
            ) => {
                set_control_flow(cf);
                return Ok(Value::Undefined);
            }
            None => {}
        }
    }
    // If we broke out of the loop, a tail-call signal was set.
    // Return the last completion value; the trampoline will extract acc from
    // the signal and combine with the completion if needed.
    // Also check for a pending Return from the last statement (e.g. `return g()` in
    // try/catch, or a bare return inside an if/else chain).
    if let Some(ControlFlow::Return(val)) = take_control_flow() {
        set_control_flow(ControlFlow::Return(val.clone()));
        set_explicit_return_for_current_body();
        return Ok(val);
    }
    // No explicit return — return undefined per ES spec
    Ok(Value::Undefined)
}

fn queue_async_function_tail(
    tail: &[Statement],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<(), JsError> {
    let tail = tail.to_vec();
    let env = Rc::clone(env);
    let callback = Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |_| {
        eval_statements(&tail, &env, false, in_arrow_function)
    })));
    crate::builtins::promise::queue_microtask_impl(callback);
    Ok(())
}

fn take_async_disposal_boundary() -> bool {
    ASYNC_DISPOSAL_BOUNDARY.with(|boundary| boundary.replace(false))
}

fn take_async_disposal_evaluated() -> bool {
    ASYNC_DISPOSAL_EVALUATED.with(|evaluated| evaluated.replace(false))
}

/// Handle a tail-call return expression when eligible. Returns true if a tail-call
/// signal was set (async/generator callees are excluded — they use Promise wrapping).
fn try_handle_tail_call(
    expr: &Option<Box<Expression>>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<bool, JsError> {
    let Some(e) = expr.as_ref() else {
        return Ok(false);
    };
    if let Expression::Binary { op, left, right } = e.as_ref() {
        let short_circuits = match op {
            BinaryOp::And => !to_bool(&eval_expression(left, env, in_arrow_function)?),
            BinaryOp::Or => to_bool(&eval_expression(left, env, in_arrow_function)?),
            BinaryOp::NullishCoalescing => {
                let value = eval_expression(left, env, in_arrow_function)?;
                !matches!(value, Value::Null | Value::Undefined)
            }
            _ => return Ok(false),
        };
        if short_circuits || !matches!(right.as_ref(), Expression::Call { .. }) {
            return Ok(false);
        }
        return try_handle_tail_call(&Some(right.clone()), env, in_arrow_function);
    }
    if let Expression::Conditional {
        condition,
        consequent,
        alternate,
    } = e.as_ref()
    {
        let branch = if to_bool(&eval_expression(condition, env, in_arrow_function)?) {
            consequent
        } else {
            alternate
        };
        return try_handle_tail_call(&Some(branch.clone()), env, in_arrow_function);
    }
    if let Expression::Sequence(expressions) = e.as_ref() {
        let Some(last) = expressions.last() else {
            return Ok(false);
        };
        for expression in &expressions[..expressions.len() - 1] {
            eval_expression(expression, env, in_arrow_function)?;
        }
        return try_handle_tail_call(&Some(Box::new(last.clone())), env, in_arrow_function);
    }
    if let Expression::Parenthesized(inner) = e.as_ref() {
        return try_handle_tail_call(&Some(inner.clone()), env, in_arrow_function);
    }
    let Expression::Call { callee, arguments } = e.as_ref() else {
        return Ok(false);
    };
    if matches!(callee.as_ref(), Expression::Member { .. }) {
        return Ok(false);
    }
    let callee_val = eval_expression(callee, env, in_arrow_function)?;
    let Value::Function(function) = callee_val else {
        return Ok(false);
    };
    if function.is_async || function.is_generator {
        return Ok(false);
    }
    let args = crate::eval::call::eval_call_arguments(arguments, env, in_arrow_function)?;
    set_tail_call_signal(TailCallSignal::new(function, args, Value::Undefined));
    Ok(true)
}

/// Recursively find a tail-call return inside a block at the last position.
/// Returns `Ok(Some(()))` when a tail call was set (caller should break).
/// Returns `Ok(None)` when no tail-call return was found (caller evaluates normally).
/// When `tail_calls_only` is true, plain `return` expressions are not handled here.
fn handle_tail_call_in_block(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
    tail_calls_only: bool,
) -> Result<Option<()>, JsError> {
    if stmts.is_empty() {
        return Ok(None);
    }
    // Push a new scope for this block (same as eval_block), so let/const
    // declarations are properly scoped and don't leak to the enclosing scope.
    env.borrow_mut().push_scope();
    predeclare_let_const(stmts, &mut env.borrow_mut());

    // Evaluate all statements except the last in the new scope.
    for stmt in &stmts[..stmts.len() - 1] {
        eval_statement(stmt, env, false, in_arrow_function)?;
    }
    let last_stmt = &stmts[stmts.len() - 1];

    // Last statement is a nested Block → recurse.
    if let Statement::Block(inner_stmts) | Statement::SequenceDecls(inner_stmts) = last_stmt {
        // Pop current scope before recursing — the recursive call pushes its own.
        env.borrow_mut().pop_scope();
        return handle_tail_call_in_block(inner_stmts, env, in_arrow_function, tail_calls_only);
    }

    if let Statement::If {
        condition,
        consequent,
        alternate,
    } = last_stmt
    {
        let branch = if to_bool(&eval_expression(condition, env, in_arrow_function)?) {
            consequent.as_ref()
        } else if let Some(alternate) = alternate {
            alternate.as_ref()
        } else {
            env.borrow_mut().pop_scope();
            return Ok(None);
        };
        let found = handle_tail_call_in_block(
            std::slice::from_ref(branch),
            env,
            in_arrow_function,
            tail_calls_only,
        )?;
        if found.is_some() {
            env.borrow_mut().pop_scope();
            return Ok(found);
        }
        if tail_calls_only {
            env.borrow_mut().pop_scope();
            return Ok(None);
        }
    }

    // Last statement can be the switch lowering loop wrapper used to convert
    // switch statements into a single-iteration loop plus a conditional chain.
    if let Statement::For {
        init:
            Some(ForInit::VarDeclaration {
                kind: VarKind::Var,
                name: loop_name,
                init: Some(Expression::Number(0.0)),
                ..
            }),
        condition: Some(cond),
        update: Some(upd),
        body,
    } = last_stmt
    {
        let is_switch_wrapper = {
            match (cond.as_ref(), upd.as_ref(), loop_name.as_str()) {
                (
                    Expression::Binary {
                        op: BinaryOp::Lt,
                        left,
                        right,
                    },
                    Expression::Update {
                        op: UpdateOp::Increment,
                        argument,
                        prefix: false,
                    },
                    loop_name,
                ) => {
                    let left_var = match left.as_ref() {
                        Expression::Identifier(id) => id.as_str(),
                        _ => "",
                    };
                    let right_is_one = matches!(right.as_ref(), Expression::Number(v) if *v == 1.0);
                    let update_var = match argument.as_ref() {
                        Expression::Identifier(id) => id.as_str(),
                        _ => "",
                    };
                    right_is_one && left_var == loop_name && update_var == loop_name
                }
                _ => false,
            }
        };

        if is_switch_wrapper
            && handle_tail_call_in_block(
                std::slice::from_ref(body.as_ref()),
                env,
                in_arrow_function,
                true,
            )?
            .is_some()
        {
            env.borrow_mut().pop_scope();
            return Ok(Some(()));
        }

        if tail_calls_only {
            env.borrow_mut().pop_scope();
            return Ok(None);
        }
    }

    // Last statement is a Return → check for tail call.
    if let Statement::Return(ref expr) = last_stmt {
        if expr.as_ref().is_some_and(|e| is_tail_expr(e))
            && try_handle_tail_call(expr, env, in_arrow_function)?
        {
            env.borrow_mut().pop_scope();
            return Ok(Some(()));
        }
        if tail_calls_only {
            env.borrow_mut().pop_scope();
            return Ok(None);
        }
        // Non-tail return inside block: evaluate it and propagate via control flow.
        let val = match expr.as_ref() {
            Some(e) => eval_expression(e, env, in_arrow_function)?,
            None => Value::Undefined,
        };
        set_control_flow(ControlFlow::Return(val));
        env.borrow_mut().pop_scope();
        return Ok(Some(()));
    }

    // No tail-call found; caller will evaluate the block normally.
    env.borrow_mut().pop_scope();
    Ok(None)
}

fn has_tail_call_in_statement(
    stmt: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<bool, JsError> {
    match stmt {
        Statement::Block(stmts) | Statement::SequenceDecls(stmts) => {
            if let Some(last) = stmts.last() {
                has_tail_call_in_statement(last, env, in_arrow_function)
            } else {
                Ok(false)
            }
        }
        Statement::Labeled { body, .. } => has_tail_call_in_statement(body, env, in_arrow_function),
        Statement::If {
            consequent,
            alternate,
            ..
        } => {
            if has_tail_call_in_statement(consequent, env, in_arrow_function)? {
                return Ok(true);
            }
            if let Some(alt) = alternate {
                has_tail_call_in_statement(alt, env, in_arrow_function)
            } else {
                Ok(false)
            }
        }
        Statement::DoWhile { body, .. }
        | Statement::ForIn { body, .. }
        | Statement::While { body, .. }
        | Statement::For { body, .. } => has_tail_call_in_statement(body, env, in_arrow_function),
        Statement::Try {
            body,
            handler,
            finalizer,
            ..
        } => {
            if has_tail_call_in_statement(body, env, in_arrow_function)? {
                return Ok(true);
            }
            if let Some(h) = handler {
                if has_tail_call_in_statement(h, env, in_arrow_function)? {
                    return Ok(true);
                }
            }
            if let Some(f) = finalizer {
                return has_tail_call_in_statement(f, env, in_arrow_function);
            }
            Ok(false)
        }
        Statement::Return(Some(expr)) if is_tail_expr(expr) => {
            try_handle_tail_call(&Some(expr.clone()), env, in_arrow_function)
        }
        Statement::Return(None) => Ok(false),
        _ => Ok(false),
    }
}

fn is_tail_candidate(stmt: &Statement) -> bool {
    match stmt {
        Statement::Block(stmts) | Statement::SequenceDecls(stmts) => {
            stmts.last().is_some_and(is_tail_candidate)
        }
        Statement::If {
            consequent,
            alternate,
            ..
        } => is_tail_candidate(consequent) || alternate.as_deref().is_some_and(is_tail_candidate),
        Statement::Return(Some(expr)) => is_tail_expr(expr),
        _ => false,
    }
}

/// Evaluate a single statement
pub fn eval_statement(
    stmt: &Statement,
    env: &Rc<RefCell<Environment>>,
    _is_expr_body: bool,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    match stmt {
        Statement::VarDeclaration { kind, name, init } => {
            eval_var_decl(kind, name, init, env, in_arrow_function)
        }
        Statement::PatternDeclaration {
            kind,
            pattern,
            init,
        } => eval_pattern_decl(kind, pattern, init, env, in_arrow_function),
        Statement::FunctionDeclaration {
            name,
            params,
            body,
            is_async,
            is_generator,
        } => eval_func_decl(name, params, body, env, *is_async, *is_generator),
        Statement::ClassDeclaration { name, class } => eval_class_decl(name, class, env),
        Statement::If {
            condition,
            consequent,
            alternate,
        } => {
            let cond_val = eval_expression(condition, env, in_arrow_function)?;
            let selected = if to_bool(&cond_val) {
                consequent.as_ref()
            } else if let Some(alt) = alternate {
                alt.as_ref()
            } else {
                return Ok(Value::Undefined);
            };
            if acc_stack_len() > 0 && is_tail_candidate(selected) {
                if let Statement::Block(stmts) = selected {
                    if handle_tail_call_in_block(stmts, env, in_arrow_function, false)?.is_some() {
                        return Ok(Value::Undefined);
                    }
                }
            }
            eval_statement(selected, env, _is_expr_body, in_arrow_function)
        }
        Statement::While { condition, body } => eval_while(condition, body, env, in_arrow_function),
        Statement::DoWhile {
            body,
            condition,
            labels,
        } => eval_do_while(body, condition, labels.clone(), env, in_arrow_function),
        Statement::For {
            init,
            condition,
            update,
            body,
        } => eval_for(
            init,
            condition,
            update,
            body,
            env,
            in_arrow_function,
            vec![],
        ),
        Statement::Block(stmts) => eval_block(stmts, env, in_arrow_function),
        Statement::SequenceDecls(stmts) => eval_statements(stmts, env, false, in_arrow_function),
        Statement::Return(expr) => {
            let val = match expr {
                Some(e) => eval_expression(e, env, in_arrow_function)?,
                None => Value::Undefined,
            };
            set_control_flow(ControlFlow::Return(val));
            Ok(Value::Undefined)
        }
        Statement::Expression(expr)
            if crate::interpreter::is_in_async_generator()
                && matches!(expr.as_ref(), Expression::Await(_)) =>
        {
            let Expression::Await(arg) = expr.as_ref() else {
                unreachable!()
            };
            let value = eval_expression(arg, env, in_arrow_function)?;
            let awaited = crate::builtins::promise::promise_resolve_impl_static(
                vec![value],
                crate::builtins::promise::get_promise_proto(),
            )?;
            set_control_flow(ControlFlow::Return(awaited.clone()));
            Ok(awaited)
        }
        Statement::Expression(expr) => eval_expression(expr, env, in_arrow_function),
        Statement::Empty => Ok(Value::Undefined),
        Statement::Labeled { label, body } => {
            push_label_scope();
            add_label(label);
            // Transfer this label (and any others already in scope) to a
            // DoWhile body so break/continue can find it. This is needed because
            // DoWhile is evaluated outside the Labeled statement's scope.
            if let Statement::DoWhile {
                body: inner_body,
                condition,
                labels,
            } = body.as_ref()
            {
                let mut all_labels = vec![label.clone()];
                all_labels.extend(labels.iter().cloned());
                let result =
                    eval_do_while(inner_body, condition, all_labels, env, in_arrow_function);
                pop_label_scope();
                return result;
            }
            if let Statement::For {
                init,
                condition,
                update,
                body: inner_body,
            } = body.as_ref()
            {
                let result = eval_for(
                    init,
                    condition,
                    update,
                    inner_body,
                    env,
                    in_arrow_function,
                    vec![label.clone()],
                );
                pop_label_scope();
                return result;
            }
            // For While loops, pass the label via the labels parameter.
            if let Statement::While {
                condition,
                body: inner_body,
            } = body.as_ref()
            {
                let result = eval_while_with_labels(
                    condition,
                    inner_body,
                    env,
                    in_arrow_function,
                    vec![label.clone()],
                );
                pop_label_scope();
                return result;
            }
            let result = eval_statement(body, env, false, in_arrow_function);
            if let Some(control) = take_control_flow() {
                let consumed = match &control {
                    ControlFlow::Break(None) => true,
                    ControlFlow::Break(Some(target)) => target == label,
                    _ => false,
                };
                if !consumed {
                    set_control_flow(control);
                }
            }
            pop_label_scope();
            result
        }
        Statement::Break(label) => {
            if let Some(name) = label {
                if !has_label(name) {
                    let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                        &format!("undefined label '{}'", name),
                        "SyntaxError",
                    );
                    crate::value::set_thrown_value(err_val);
                    return Err(js_err);
                }
            }
            set_control_flow(ControlFlow::Break(label.clone()));
            Ok(Value::Undefined)
        }
        Statement::Continue(label) => {
            if let Some(name) = label {
                if !has_label(name) {
                    let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                        &format!("undefined label '{}'", name),
                        "SyntaxError",
                    );
                    crate::value::set_thrown_value(err_val);
                    return Err(js_err);
                }
            }
            set_control_flow(ControlFlow::Continue(label.clone()));
            Ok(Value::Undefined)
        }
        Statement::Try {
            body,
            param,
            handler,
            finalizer,
        } => eval_try(body, param, handler, finalizer, env, in_arrow_function),
        Statement::Dispose { name, is_async } => eval_dispose(name, *is_async, env),
        Statement::RegisterDispose { name, is_async } => {
            eval_register_dispose(name, *is_async, env)
        }
        Statement::Throw(expr) => {
            let value = eval_expression(expr, env, in_arrow_function)?;
            let msg = to_js_string(&value);
            // Store the original thrown value for catch blocks to retrieve
            set_thrown_value(value);
            Err(JsError(msg))
        }
        Statement::With { object, body } => {
            // `with (obj) { body }` — push a scope onto the env whose
            // identifier lookup defers to obj's properties. We model this by
            // pushing a fresh scope and resolving names via that object at runtime.
            if crate::interpreter::is_strict_mode() {
                return Err(JsError(
                    "SyntaxError: 'with' statements are not allowed in strict mode".to_string(),
                ));
            }
            let obj_val = to_object(&eval_expression(object, env, in_arrow_function)?)?;
            let Value::Object(obj_rc) = obj_val else {
                return Err(JsError(
                    "TypeError: cannot use with on non-object".to_string(),
                ));
            };
            let with_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(env))));
            {
                let current_scope = with_env.borrow().current_scope();
                let mut scope = current_scope.borrow_mut();
                scope.set_with_object_binding(Rc::clone(&obj_rc));
            }
            let previous_eval_env = crate::interpreter::get_current_eval_env();
            crate::interpreter::set_current_eval_env(Some(Rc::clone(&with_env)));
            let result = eval_statement(body, &with_env, _is_expr_body, in_arrow_function);
            crate::interpreter::set_current_eval_env(previous_eval_env);
            with_env
                .borrow_mut()
                .current_scope()
                .borrow_mut()
                .clear_with_unscopables();
            result
        }
        Statement::Export(stmt) => {
            // Export statements wrap other statements (like assignments)
            eval_statement(stmt, env, _is_expr_body, in_arrow_function)
        }
        Statement::Import {
            default,
            named,
            namespace,
            source,
            deferred,
            import_type,
        } => eval_import(
            default,
            named,
            namespace,
            source,
            *deferred,
            import_type,
            env,
        ),
        Statement::ForIn {
            variable,
            object,
            body,
        } => eval_for_in_stmt(variable, object, body, env, in_arrow_function),
    }
}

pub(crate) fn eval_dispose(
    name: &str,
    is_async: bool,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let resource = env.borrow().get(name).unwrap_or(Value::Undefined);
    if matches!(resource, Value::Null | Value::Undefined) {
        return Ok(Value::Undefined);
    }
    let cache_name = dispose_cache_name(name);
    let method = match env.borrow().get(&cache_name) {
        Some(Value::Undefined) => return Ok(Value::Undefined),
        Some(method) => method,
        None => resolve_dispose_method(&resource, is_async, env)?,
    };
    let result = crate::eval::function::call_value_with_this(method, Vec::new(), resource)?;
    if is_async {
        crate::eval::r#await::eval_await_value(result)
    } else {
        Ok(result)
    }
}

pub(crate) fn eval_register_dispose(
    name: &str,
    is_async: bool,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if is_async {
        crate::eval::r#await::take_last_pending_await();
        ASYNC_DISPOSAL_EVALUATED.with(|evaluated| evaluated.set(true));
    }
    let resource = env.borrow().get(name).unwrap_or(Value::Undefined);
    let method = if matches!(resource, Value::Null | Value::Undefined) {
        Value::Undefined
    } else {
        match resolve_dispose_method(&resource, is_async, env) {
            Ok(method) => method,
            Err(error) => {
                env.borrow_mut()
                    .define(dispose_cache_name(name), Value::Undefined);
                return Err(error);
            }
        }
    };
    env.borrow_mut().define(dispose_cache_name(name), method);
    Ok(Value::Undefined)
}

fn dispose_cache_name(name: &str) -> String {
    format!("\0dispose:{}", name)
}

fn resolve_dispose_method(
    resource: &Value,
    is_async: bool,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let key_name = if is_async { "asyncDispose" } else { "dispose" };
    let symbol = env
        .borrow()
        .get("Symbol")
        .ok_or_else(|| JsError::new("ReferenceError: Symbol is not defined"))?;
    let key = match symbol {
        Value::NativeFunction(function) => function
            .get_property(key_name)
            .ok_or_else(|| JsError::new("TypeError: disposal symbol is not initialized"))?,
        Value::Function(function) => function
            .get_property(key_name)
            .ok_or_else(|| JsError::new("TypeError: disposal symbol is not initialized"))?,
        _ => {
            let msg = "TypeError: Symbol is not callable";
            let (err, js_err) = crate::value::error::create_js_error_with_type(&msg, "TypeError");
            crate::value::set_thrown_value(err);
            return Err(js_err);
        }
    };
    let mut method = get_resource_dispose_property(resource, &key, env)?;
    if is_async && matches!(method, Value::Undefined | Value::Null) {
        let fallback = match env.borrow().get("Symbol") {
            Some(Value::NativeFunction(function)) => function
                .get_property("dispose")
                .ok_or_else(|| JsError::new("TypeError: disposal symbol is not initialized"))?,
            Some(Value::Function(function)) => function
                .get_property("dispose")
                .ok_or_else(|| JsError::new("TypeError: disposal symbol is not initialized"))?,
            _ => {
                let msg = "TypeError: Symbol is not callable";
                let (err, js_err) =
                    crate::value::error::create_js_error_with_type(&msg, "TypeError");
                crate::value::set_thrown_value(err);
                return Err(js_err);
            }
        };
        method = get_resource_dispose_property(resource, &fallback, env)?;
    }
    if matches!(method, Value::Undefined | Value::Null) {
        return crate::throw!("TypeError", "object is not disposable");
    }
    if !method.is_callable() {
        return crate::throw!("TypeError", "disposal method is not callable");
    }
    Ok(method)
}

fn get_resource_dispose_property(
    resource: &Value,
    key: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match resource {
        Value::Object(object) => get_dispose_property(object, key, env),
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => {
            let name = match key {
                Value::Symbol(symbol) => symbol.property_key(),
                _ => crate::value::to_js_string(key),
            };
            crate::eval::member::eval_member_access(resource, &name, env)
        }
        Value::Class(_) => {
            let name = match key {
                Value::Symbol(symbol) => symbol.property_key(),
                _ => crate::value::to_js_string(key),
            };
            let value = crate::eval::member::eval_member_access(resource, &name, env)?;
            if !matches!(value, Value::Undefined) {
                return Ok(value);
            }
            let prototype = crate::builtins::function::get_function_prototype()
                .ok_or_else(|| JsError::new("TypeError: object is not disposable"))?;
            crate::eval::member::eval_object_member(&prototype, &name, Some(env))
        }
        _ => crate::throw!("TypeError", "object is not disposable"),
    }
}

fn get_dispose_property(
    object: &Rc<RefCell<Object>>,
    key: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let key_name = match key {
        Value::Symbol(symbol) => symbol.property_key(),
        _ => crate::value::to_js_string(key),
    };
    let has_getter = object.borrow().get_getter(&key_name).is_some();
    let value = crate::eval::member::eval_object_member_value(object, key, Some(env))?;
    if matches!(value, Value::Undefined) && !has_getter {
        crate::eval::member::eval_object_member(object, &key_name, Some(env))
    } else {
        Ok(value)
    }
}

/// Helper to set a property on globalThis if we're at the top level.
/// Helper to set a property on globalThis if we're at the top level.
pub(crate) fn set_on_global_this(env: &Rc<RefCell<Environment>>, name: &str, value: Value) {
    let is_top_level = {
        let env_ref = env.borrow();
        env_ref.get_parent().is_none()
    };
    if is_top_level {
        // Get globalThis outside the mutable borrow to avoid conflict
        let global_this = env.borrow().get("globalThis");
        if let Some(Value::Object(global_obj)) = global_this {
            global_obj.borrow_mut().set(name, value);
        }
    }
}

fn eval_pattern_decl(
    kind: &VarKind,
    pattern: &BindingElement,
    init: &Option<Expression>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    crate::eval::object::declare_pattern_bindings_with_kind(pattern, *kind, env);
    let value = if let Some(expr) = init {
        eval_expression(expr, env, in_arrow_function)?
    } else {
        Value::Undefined
    };
    let target = crate::eval::object::binding_pattern_expression(pattern.clone());
    // For const/let, use init_to (which initializes the binding from TDZ).
    // For var, use assign_to (var has no TDZ).
    if *kind == VarKind::Var {
        crate::eval::object::assign_to(&target, &value, env)?;
    } else {
        crate::eval::object::init_to(&target, &value, env)?;
    }
    Ok(Value::Undefined)
}

fn eval_var_decl(
    kind: &VarKind,
    name: &str,
    init: &Option<Expression>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    if *kind == VarKind::Var && init.is_none() {
        return Ok(Value::Undefined);
    }
    let existing_var = *kind == VarKind::Var && env.borrow().get_kind(name) == Some(VarKind::Var);
    let already_declared = env.borrow().current_kind(name).is_some();
    if !existing_var && !already_declared {
        env.borrow_mut().declare_var(name.to_string(), *kind);
    }
    let var_target = if *kind == VarKind::Var {
        env.borrow()
            .binding_scope(name)
            .filter(|scope| scope.borrow().is_with_environment())
    } else {
        None
    };
    let mut value = if let Some(expr) = init {
        if let Expression::Class(class) = expr {
            let inferred_name = if class.name.is_none() {
                Some(name)
            } else {
                None
            };
            crate::eval::class::eval_class_expr(class, env, inferred_name)?
        } else {
            eval_expression(expr, env, in_arrow_function)?
        }
    } else {
        Value::Undefined
    };
    // Per ES §13.3.3 SetFunctionName: only when IsAnonymousFunctionDefinition(Initializer).
    if let (Some(expr), Value::Function(ref mut f)) = (init, &mut value) {
        if f.name.is_none() && crate::eval::object::is_anonymous_function_definition(expr) {
            f.name = Some(name.to_string());
            let _ = f.set_property("name", Value::String(name.to_string()));
        }
    }
    let strict = crate::interpreter::is_strict_mode();
    if !init.is_some() && *kind == VarKind::Var && env.borrow().get_parent().is_none() {
        let existing_global = env.borrow().get_global_property(name).or_else(|| {
            crate::context::get_global_from_context("globalThis").and_then(|global| {
                let Value::Object(global_obj) = global else {
                    return None;
                };
                if global_obj.borrow().has(name) {
                    global_obj.borrow().get(name)
                } else {
                    None
                }
            })
        });

        if let Some(existing_global) = existing_global {
            if let Some(scope) = env.borrow().var_binding_scope(name) {
                scope
                    .borrow_mut()
                    .set(name.to_string(), existing_global.clone(), strict);
            } else {
                env.borrow_mut()
                    .initialize_declared(name, existing_global.clone());
            }
            if *kind == VarKind::Var && env.borrow().get_parent().is_none() {
                set_on_global_this(env, name, existing_global);
            }
            return Ok(Value::Undefined);
        }
    }

    if init.is_some() {
        if *kind == VarKind::Var {
            let target = var_target.or_else(|| env.borrow().binding_scope(name));
            if let Some(scope) = target {
                let mut scope = scope.borrow_mut();
                let set = if scope.is_with_environment() {
                    scope
                        .set_object_property_after_get(name, value.clone(), strict)
                        .is_some_and(|set| set)
                } else {
                    scope.set(name.to_string(), value.clone(), strict)
                };
                if !set {
                    scope.define(name.to_string(), value.clone());
                }
            } else {
                env.borrow_mut().set(name, value.clone());
            }
        } else {
            env.borrow_mut().initialize_declared(name, value.clone());
        }
    } else if !existing_var {
        env.borrow_mut().initialize_declared(name, value.clone());
    }
    Ok(Value::Undefined)
}

fn eval_func_decl(
    name: &str,
    params: &[Param],
    body: &[Statement],
    env: &Rc<RefCell<Environment>>,
    is_async: bool,
    is_generator: bool,
) -> Result<Value, JsError> {
    if matches!(env.borrow().get(name), Some(Value::Function(_))) {
        return Ok(Value::Undefined);
    }
    let scope_depth = env.borrow().scopes.len();
    let in_block = scope_depth > 1;
    let is_top_level = {
        let env_ref = env.borrow();
        env_ref.get_parent().is_none()
    };
    let existing_kind = env.borrow().current_kind(name);
    let mut func = crate::value::ValueFunction::new(
        Some(name.to_owned()),
        params.to_vec(),
        body.to_vec(),
        Rc::clone(env),
        is_async,
        is_generator,
    );
    func.strict = crate::interpreter::is_strict_mode()
        || crate::interpreter::helpers::check_use_strict_directive(body);
    func.name = Some(name.to_string()); // Set .name property per ES spec SetFunctionName
    let value = Value::Function(func);
    if let Some(existing_kind) = existing_kind {
        let mut env_mut = env.borrow_mut();
        match existing_kind {
            crate::ast::VarKind::Let | crate::ast::VarKind::Const => {
                env_mut.define(name.to_owned(), value.clone());
            }
            crate::ast::VarKind::Var => {
                env_mut.initialize_declared(name, value.clone());
                drop(env_mut);
                if is_top_level && !in_block {
                    set_on_global_this(env, name, value);
                }
                return Ok(Value::Undefined);
            }
        }
        drop(env_mut);
        if is_top_level && !in_block {
            set_on_global_this(env, name, value);
        }
        return Ok(Value::Undefined);
    }
    // ES2015+ strict mode: function declarations in blocks are block-scoped.
    // In sloppy mode, function declarations without prior lexical predeclaration
    // still default to var semantics for compatibility.
    let kind = if crate::interpreter::is_strict_mode() && in_block {
        VarKind::Let
    } else {
        VarKind::Var
    };
    env.borrow_mut().declare_var(name.to_owned(), kind);
    env.borrow_mut().define(name.to_owned(), value.clone());
    // Top-level function declarations are globals (same as var).
    if is_top_level && !in_block {
        set_on_global_this(env, name, value);
    }
    Ok(Value::Undefined)
}

fn eval_class_decl(
    name: &str,
    class: &Class,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Evaluate the class expression with the declared name so static field
    // initializers observe `this.name === "<name>"` per ES §14.6.13.
    let class_val = crate::eval::class::eval_class_expr(class, env, Some(name))?;
    if crate::value::generator_replay::yield_pending() {
        return Ok(Value::Undefined);
    }
    env.borrow_mut().define(name.to_owned(), class_val);
    Ok(Value::Undefined)
}

fn eval_while(
    condition: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    eval_while_with_labels(condition, body, env, in_arrow_function, vec![])
}

fn eval_while_with_labels(
    condition: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
    labels: Vec<String>,
) -> Result<Value, JsError> {
    let loop_labels = labels;
    let mut completion = Value::Undefined;
    while to_bool(&eval_expression(condition, env, in_arrow_function)?) {
        take_control_flow();
        let body_val = eval_statement(body, env, false, in_arrow_function)?;
        if crate::interpreter::peek_generator_yield() {
            return Ok(body_val);
        }
        // Per ES §13.7.3.6 step 2.g: if body's [[value]] is not empty, update V
        if !matches!(body_val, Value::Undefined) {
            completion = body_val;
        }
        match take_control_flow() {
            Some(cf @ ControlFlow::Break(_)) => {
                if loop_handles_break(&cf, &loop_labels) {
                    break;
                }
                set_control_flow(cf);
                break;
            }
            Some(cf @ ControlFlow::Continue(_)) => {
                if loop_handles_continue(&cf, &loop_labels) {
                    continue;
                }
                set_control_flow(cf);
                break;
            }
            Some(ControlFlow::Return(val)) | Some(ControlFlow::Yield(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            // YieldDelegate: also propagate as Return (the generator handles it)
            Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Throw(val)) => {
                set_thrown_value(val);
                return Err(JsError("Generator threw".to_string()));
            }
            None => {}
        }
    }
    Ok(completion)
}

fn eval_do_while(
    body: &Statement,
    condition: &Expression,
    labels: Vec<String>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    // Push label scope so break/continue inside body can find these labels
    push_label_scope();
    for lbl in &labels {
        add_label(lbl);
    }
    let result = eval_do_while_impl(body, condition, &labels, env, in_arrow_function);
    pop_label_scope();
    result
}

fn eval_do_while_impl(
    body: &Statement,
    condition: &Expression,
    loop_labels: &[String],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    predeclare_var(std::slice::from_ref(body), &mut env.borrow_mut());
    loop {
        take_control_flow();
        let body_val = eval_statement(body, env, false, in_arrow_function)?;
        match take_control_flow() {
            Some(cf @ ControlFlow::Break(_)) => {
                if loop_handles_break(&cf, loop_labels) {
                    return Ok(body_val);
                }
                set_control_flow(cf);
                return Ok(body_val);
            }
            Some(cf @ ControlFlow::Continue(_)) => {
                if !loop_handles_continue(&cf, loop_labels) {
                    set_control_flow(cf);
                    return Ok(body_val);
                }
            }
            Some(ControlFlow::Return(val)) | Some(ControlFlow::Yield(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Throw(val)) => {
                set_thrown_value(val);
                return Err(JsError("Generator threw".to_string()));
            }
            None => {}
        }
        if !to_bool(&eval_expression(condition, env, in_arrow_function)?) {
            return Ok(body_val);
        }
    }
}

fn eval_for_init_decl(
    kind: &VarKind,
    name: &str,
    init: &Option<Expression>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<(), JsError> {
    let effective_kind = if *kind == VarKind::Const {
        VarKind::Let
    } else {
        *kind
    };
    eval_var_decl(&effective_kind, name, init, env, in_arrow_function)?;
    Ok(())
}

fn eval_for_init(
    for_init: &ForInit,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<(), JsError> {
    match for_init {
        ForInit::Expression(expr) => {
            let _ = eval_expression(expr, env, in_arrow_function)?;
        }
        ForInit::VarDeclaration { kind, name, init } => {
            eval_for_init_decl(kind, name, init, env, in_arrow_function)?;
        }
        ForInit::PatternDeclaration {
            kind,
            pattern,
            init,
        } => {
            eval_pattern_decl(kind, pattern, init, env, in_arrow_function)?;
        }
        ForInit::DeclarationList { kind, decls } => {
            for decl in decls {
                if let Some(name) = &decl.name {
                    eval_for_init_decl(kind, name, &decl.init, env, in_arrow_function)?;
                } else if let Some(pattern) = &decl.pattern {
                    eval_pattern_decl(kind, pattern, &decl.init, env, in_arrow_function)?;
                }
            }
        }
    }
    Ok(())
}

fn eval_for_loop_condition(
    condition: &Option<Box<Expression>>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<bool, JsError> {
    if let Some(c) = condition.as_ref() {
        let val = eval_expression(c, env, in_arrow_function)?;
        Ok(to_bool(&val))
    } else {
        Ok(true)
    }
}

fn eval_for(
    init: &Option<ForInit>,
    condition: &Option<Box<Expression>>,
    update: &Option<Box<Expression>>,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
    loop_labels: Vec<String>,
) -> Result<Value, JsError> {
    let per_iter_names = collect_for_head_lexical_names(init.as_ref());
    let head_lexical = !per_iter_names.is_empty();
    if head_lexical {
        env.borrow_mut().push_scope();
    }
    if let Some(for_init) = init {
        eval_for_init(for_init, env, in_arrow_function)?;
    }
    if head_lexical {
        let initial_values = {
            let env_ref = env.borrow();
            per_iter_names
                .iter()
                .filter_map(|name| {
                    env_ref
                        .get_scope_from_bottom(0)?
                        .borrow()
                        .get(name)
                        .map(|value| (name.clone(), value))
                })
                .collect::<Vec<_>>()
        };
        env.borrow_mut().push_scope();
        for (name, value) in initial_values {
            env.borrow_mut().declare_var(name.clone(), VarKind::Let);
            env.borrow_mut().initialize_declared(&name, value);
        }
    }
    let mut completion = Value::Undefined;
    loop {
        // Push PI for THIS iteration (snapshot HEAD values). PI is on the chain
        // during condition check + body. The caller pops PI before the update
        // expression, so updates go to HEAD directly.
        if head_lexical {
            push_for_body_iteration_scope(&mut env.borrow_mut(), &per_iter_names);
        }
        // Condition check (PI is on chain — closures see per-iteration values)
        if !eval_for_loop_condition(condition, env, in_arrow_function)? {
            if head_lexical {
                env.borrow_mut().pop_scope();
            }
            break;
        }
        take_control_flow();
        let body_val = eval_statement(body, env, false, in_arrow_function)?;
        completion = body_val.clone();
        if crate::interpreter::peek_generator_yield() {
            if head_lexical {
                env.borrow_mut().pop_scope();
                env.borrow_mut().pop_scope();
                env.borrow_mut().pop_scope();
            }
            return Ok(body_val);
        }
        // Control flow handling — always pop PI before early exit
        match take_control_flow() {
            Some(cf @ ControlFlow::Break(_)) => {
                if loop_handles_break(&cf, &loop_labels) {
                    if head_lexical {
                        env.borrow_mut().pop_scope();
                        env.borrow_mut().pop_scope();
                        env.borrow_mut().pop_scope();
                    }
                    return Ok(body_val);
                }
                if head_lexical {
                    env.borrow_mut().pop_scope();
                    env.borrow_mut().pop_scope();
                    env.borrow_mut().pop_scope();
                }
                set_control_flow(cf);
                return Ok(Value::Undefined);
            }
            Some(cf @ ControlFlow::Continue(_)) => {
                if !loop_handles_continue(&cf, &loop_labels) {
                    if head_lexical {
                        env.borrow_mut().pop_scope();
                        env.borrow_mut().pop_scope();
                        env.borrow_mut().pop_scope();
                    }
                    set_control_flow(cf);
                    return Ok(Value::Undefined);
                }
                // Loop handles continue: fall through to pop PI and run update.
            }
            Some(ControlFlow::Return(val))
                if is_in_async_context() && crate::eval::r#await::is_promise(&val) =>
            {
                if head_lexical {
                    env.borrow_mut().pop_scope();
                }
                let for_stmt = Statement::For {
                    init: None,
                    condition: condition.clone(),
                    update: update.clone(),
                    body: Box::new((*body).clone()),
                };
                let mut continuation = for_stmt;
                for label in loop_labels.iter().rev() {
                    continuation = Statement::Labeled {
                        label: label.clone(),
                        body: Box::new(continuation),
                    };
                }
                let mut tail = Vec::new();
                if let Some(update) = update {
                    tail.push(Statement::Expression(update.clone()));
                }
                tail.push(continuation);
                let awaited = crate::eval::r#await::await_statement(
                    val,
                    tail,
                    Rc::clone(env),
                    in_arrow_function,
                )?;
                set_control_flow(ControlFlow::Return(awaited.clone()));
                return Ok(awaited);
            }
            Some(ControlFlow::Yield(val))
                if is_in_async_context() && crate::eval::r#await::is_promise(&val) =>
            {
                if head_lexical {
                    env.borrow_mut().pop_scope();
                }
                let for_stmt = Statement::For {
                    init: None,
                    condition: condition.clone(),
                    update: update.clone(),
                    body: Box::new((*body).clone()),
                };
                let mut continuation = for_stmt;
                for label in loop_labels.iter().rev() {
                    continuation = Statement::Labeled {
                        label: label.clone(),
                        body: Box::new(continuation),
                    };
                }
                let mut tail = Vec::new();
                if let Some(update) = update {
                    tail.push(Statement::Expression(update.clone()));
                }
                tail.push(continuation);
                let awaited = crate::eval::r#await::await_statement(
                    val,
                    tail,
                    Rc::clone(env),
                    in_arrow_function,
                )?;
                set_control_flow(ControlFlow::Return(awaited.clone()));
                return Ok(awaited);
            }
            Some(ControlFlow::Return(val)) | Some(ControlFlow::Yield(val)) => {
                if head_lexical {
                    env.borrow_mut().pop_scope();
                    env.borrow_mut().pop_scope();
                    env.borrow_mut().pop_scope();
                }
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::YieldDelegate(val)) => {
                if head_lexical {
                    env.borrow_mut().pop_scope();
                    env.borrow_mut().pop_scope();
                    env.borrow_mut().pop_scope();
                }
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Throw(val)) => {
                if head_lexical {
                    env.borrow_mut().pop_scope();
                    env.borrow_mut().pop_scope();
                    env.borrow_mut().pop_scope();
                }
                set_thrown_value(val);
                return Err(JsError("Generator threw".to_string()));
            }
            None => {}
        }
        if head_lexical {
            let pi = env.borrow().get_scope_from_bottom(0);
            let head = env.borrow().get_scope_from_bottom(1);
            if let (Some(pi), Some(head)) = (pi, head) {
                for name in &per_iter_names {
                    if let Some(value) = pi.borrow().get(name) {
                        head.borrow_mut().set(name.clone(), value, false);
                    }
                }
            }
            env.borrow_mut().pop_scope();
            push_for_body_iteration_scope(&mut env.borrow_mut(), &per_iter_names);
            env.borrow()
                .current_scope()
                .borrow_mut()
                .clear_per_iteration();
        }
        if let Some(update) = update {
            let const_name = match init {
                Some(ForInit::VarDeclaration {
                    kind: VarKind::Const,
                    name,
                    ..
                }) => Some(name.as_str()),
                _ => None,
            };
            let updates_const = const_name.is_some_and(|name| {
                matches!(
                    update.as_ref(),
                    Expression::Update { argument, .. }
                        if matches!(argument.as_ref(), Expression::Identifier(target) if target == name)
                )
            });
            if updates_const {
                let msg = "TypeError: Assignment to constant variable";
                let (err, js_err) =
                    crate::value::error::create_js_error_with_type(&msg, "TypeError");
                crate::value::set_thrown_value(err);
                return Err(js_err);
            }
            let _ = eval_expression(update, env, in_arrow_function)?;
        }
        if head_lexical {
            let pi = env.borrow().get_scope_from_bottom(0);
            let head = env.borrow().get_scope_from_bottom(1);
            if let (Some(pi), Some(head)) = (pi, head) {
                for name in &per_iter_names {
                    if let Some(value) = pi.borrow().get(name) {
                        head.borrow_mut().set(name.clone(), value, false);
                    }
                }
            }
        }
        if head_lexical {
            env.borrow_mut().pop_scope();
        }
    }
    // Pop the for-head scope (HEAD). PI was already popped inside the loop.
    if head_lexical {
        env.borrow_mut().pop_scope();
        env.borrow_mut().pop_scope();
    }
    Ok(completion)
}

fn eval_block(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    {
        let mut env_mut = env.borrow_mut();
        env_mut.push_scope();
        predeclare_let_const(stmts, &mut env_mut);
    }
    let result = eval_statements(stmts, env, false, in_arrow_function);
    env.borrow_mut().pop_scope();
    result
}

/// Evaluate a try-catch-finally statement
fn eval_try(
    body: &Statement,
    param: &Option<String>,
    handler: &Option<Box<Statement>>,
    finalizer: &Option<Box<Statement>>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    // Var declarations inside a try block are hoisted to the try's enclosing
    // scope, so they must be visible to the catch handler too. Predeclare them
    // in the parent env before evaluating the body.
    if let Statement::Block(stmts) = body {
        let mut names = Vec::new();
        collect_var_names_recursive(stmts, &mut names);
        for name in names {
            env.borrow_mut().declare_var(name, VarKind::Var);
        }
    }

    // Evaluate the try body
    let try_result = eval_statement(body, env, false, in_arrow_function).map(|value| {
        if matches!(body, Statement::Block(stmts) if stmts.is_empty()) {
            Value::Undefined
        } else {
            value
        }
    });

    // Handle the result
    match try_result {
        Ok(try_val) => {
            // If the generator is about to suspend, skip the finally for now.
            // The finally will run when the generator is resumed (via g.next(),
            // g.return(), or g.throw()) and the try body truly completes.
            if crate::interpreter::peek_generator_yield() {
                return Ok(try_val);
            }
            // Try succeeded - run finally if present, propagate control flow if needed
            if let Some(fin) = finalizer {
                if crate::interpreter::is_in_async_function()
                    && defer_disposal_until_pending_await(fin, &try_val, env, in_arrow_function)
                {
                    return Ok(try_val);
                }
                // Suspend pending control flow while finally runs.
                let pending_cf = take_control_flow();

                let fin_result = eval_statement(fin, env, false, in_arrow_function);
                if crate::interpreter::peek_generator_yield() {
                    crate::eval::generator::mark_yield_in_finally();
                }
                if fin_result.is_ok()
                    && crate::interpreter::is_in_async_function()
                    && finalizer_has_async_dispose(fin)
                    && take_async_disposal_evaluated()
                {
                    ASYNC_DISPOSAL_BOUNDARY.with(|boundary| boundary.set(true));
                }
                match fin_result {
                    Ok(fin_val) => {
                        // If finally has its own control flow (break/continue/return),
                        // it overrides the original. Per ES §14.15.4, finally's completion
                        // replaces the try's completion for [[Type]] break, continue, return.
                        if let Some(cf) = take_control_flow() {
                            let cf = cf.clone();
                            set_control_flow(cf.clone()); // Propagate finally's control flow
                            return match cf {
                                ControlFlow::Return(value) => Ok(value),
                                ControlFlow::Break(_) | ControlFlow::Continue(_) => Ok(fin_val),
                                ControlFlow::Yield(value) => Ok(value),
                                ControlFlow::YieldDelegate(value) => Ok(value),
                                ControlFlow::Throw(value) => {
                                    set_thrown_value(value.clone());
                                    let msg = to_js_string(&value);
                                    return Err(JsError(msg));
                                }
                            };
                        } else if crate::interpreter::is_in_async_function() {
                            if let Some(reason) = rejected_promise_reason(&fin_val) {
                                let _ = reason;
                                set_control_flow(ControlFlow::Return(fin_val.clone()));
                                return Ok(fin_val);
                            }
                            if let Some(cf) = pending_cf {
                                set_control_flow(cf);
                            }
                        } else if let Some(cf) = pending_cf {
                            set_control_flow(cf); // Restore original control flow
                        }
                        Ok(try_val)
                    }
                    Err(e) => {
                        let thrown =
                            take_thrown_value().unwrap_or_else(|| Value::String(e.0.clone()));
                        set_thrown_value(thrown);
                        Err(e)
                    }
                }
            } else {
                Ok(try_val)
            }
        }
        Err(_e) => {
            // Try threw - handle with catch if present
            let thrown_value = take_thrown_value().unwrap_or(Value::Undefined);
            let thrown_for_catch = thrown_value.clone();

            let has_catch_param = param.is_some();
            if has_catch_param {
                // Per ES §13.15.7: catch parameter creates a new lexical scope
                // so it doesn't shadow outer bindings.
                let name = param.as_ref().unwrap().clone();
                env.borrow_mut().push_scope();
                env.borrow_mut().declare_var(name.clone(), VarKind::Let);
                env.borrow_mut()
                    .initialize_declared(name.as_str(), thrown_for_catch);
            }

            if let Some(h) = handler {
                // Run catch block
                let catch_result = eval_statement(h, env, false, in_arrow_function);
                if has_catch_param {
                    env.borrow_mut().pop_scope();
                }
                let catch_thrown = if catch_result.is_err() {
                    take_thrown_value()
                } else {
                    None
                };

                // Run finally if present
                if let Some(fin) = finalizer {
                    let pending_cf = take_control_flow();
                    let fin_result = eval_statement(fin, env, false, in_arrow_function);
                    match fin_result {
                        Ok(fin_val) => {
                            // Finally's control flow overrides the catch's.
                            let fin_cf = take_control_flow();
                            if let Some(cf) = fin_cf {
                                let cf = cf.clone();
                                set_control_flow(cf.clone()); // Propagate finally's control flow
                                return match cf {
                                    ControlFlow::Return(value) => Ok(value),
                                    ControlFlow::Break(_) | ControlFlow::Continue(_) => Ok(fin_val),
                                    ControlFlow::Yield(value) => Ok(value),
                                    ControlFlow::YieldDelegate(value) => Ok(value),
                                    ControlFlow::Throw(value) => {
                                        set_thrown_value(value.clone());
                                        let msg = to_js_string(&value);
                                        Err(JsError(msg))
                                    }
                                };
                            } else if crate::interpreter::is_in_async_function() {
                                set_control_flow(ControlFlow::Return(fin_val.clone()));
                                return Ok(fin_val);
                            } else if let Some(cf) = pending_cf {
                                set_control_flow(cf); // Restore original
                            }
                            match catch_result {
                                Ok(v) => Ok(v),
                                Err(e) => {
                                    if let Some(thrown) = catch_thrown {
                                        set_thrown_value(thrown);
                                    }
                                    Err(e)
                                }
                            }
                        }
                        Err(e) => {
                            let thrown =
                                take_thrown_value().unwrap_or_else(|| Value::String(e.0.clone()));
                            set_thrown_value(thrown);
                            Err(e)
                        }
                    }
                } else {
                    match catch_result {
                        Ok(v) => Ok(v),
                        Err(e) => {
                            if let Some(thrown) = catch_thrown {
                                set_thrown_value(thrown);
                            }
                            Err(e)
                        }
                    }
                }
            } else {
                // No catch - run finally if present, then rethrow
                if let Some(fin) = finalizer {
                    let pending_cf = take_control_flow();
                    if finalizer_has_dispose(fin) {
                        eval_disposal_finalizer(fin, thrown_value, env, in_arrow_function)
                    } else {
                        match eval_statement(fin, env, false, in_arrow_function) {
                            Ok(fin_val) => {
                                if crate::interpreter::peek_generator_yield() {
                                    crate::eval::generator::mark_yield_in_finally();
                                }
                                if crate::interpreter::peek_generator_yield() {
                                    return Ok(fin_val);
                                }
                                if let Some(ControlFlow::Return(value)) = pending_cf {
                                    set_control_flow(ControlFlow::Return(value.clone()));
                                    return Ok(value);
                                }
                                if let Some(cf) = take_control_flow() {
                                    let cf = cf.clone();
                                    set_control_flow(cf.clone());
                                    return match cf {
                                        ControlFlow::Return(value) => Ok(value),
                                        ControlFlow::Break(_) | ControlFlow::Continue(_) => {
                                            Ok(fin_val)
                                        }
                                        ControlFlow::Yield(value)
                                        | ControlFlow::YieldDelegate(value) => Ok(value),
                                        ControlFlow::Throw(value) => {
                                            set_thrown_value(value.clone());
                                            Err(JsError(to_js_string(&value)))
                                        }
                                    };
                                }
                                let message = to_js_string(&thrown_value);
                                set_thrown_value(thrown_value);
                                Err(JsError(message))
                            }
                            Err(error) => Err(error),
                        }
                    }
                } else {
                    // No finally, no catch - rethrow
                    let msg = to_js_string(&thrown_value);
                    set_thrown_value(thrown_value);
                    Err(JsError(msg))
                }
            }
        }
    }
}

fn eval_disposal_finalizer(
    finalizer: &Statement,
    mut completion: Value,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let statements = match finalizer {
        Statement::Block(statements) | Statement::SequenceDecls(statements) => {
            statements.as_slice()
        }
        statement => std::slice::from_ref(statement),
    };
    for statement in statements {
        let result = eval_statement(statement, env, false, in_arrow_function);
        let error = match result {
            Ok(value) => rejected_promise_reason(&value),
            Err(_) => take_thrown_value(),
        };
        if let Some(error) = error {
            completion = create_suppressed_error(error, completion, env)?;
        }
    }
    let message = to_js_string(&completion);
    set_thrown_value(completion);
    Err(JsError(message))
}

fn create_suppressed_error(
    error: Value,
    suppressed: Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let constructor = env
        .borrow()
        .get("SuppressedError")
        .ok_or_else(|| JsError::new("ReferenceError: SuppressedError is not defined"))?;
    crate::eval::function::call_value_with_this(
        constructor,
        vec![error, suppressed],
        Value::Undefined,
    )
}

fn finalizer_has_async_dispose(finalizer: &Statement) -> bool {
    match finalizer {
        Statement::Block(statements) | Statement::SequenceDecls(statements) => statements
            .iter()
            .any(|statement| matches!(statement, Statement::Dispose { is_async: true, .. })),
        Statement::Dispose { is_async, .. } => *is_async,
        _ => false,
    }
}

fn finalizer_has_dispose(finalizer: &Statement) -> bool {
    match finalizer {
        Statement::Block(statements) | Statement::SequenceDecls(statements) => statements
            .iter()
            .any(|statement| matches!(statement, Statement::Dispose { .. })),
        Statement::Dispose { .. } => true,
        _ => false,
    }
}

fn defer_disposal_until_pending_await(
    finalizer: &Statement,
    pending_value: &Value,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> bool {
    let statements = match finalizer {
        Statement::Block(statements) | Statement::SequenceDecls(statements) => statements,
        _ => return false,
    };
    if !statements
        .iter()
        .all(|statement| matches!(statement, Statement::Dispose { .. }))
    {
        return false;
    }
    let promise = match pending_value {
        Value::Object(promise) if crate::eval::r#await::is_promise(pending_value) => {
            Some(Rc::clone(promise))
        }
        _ => None,
    }
    .or_else(crate::eval::r#await::take_last_pending_await);
    let Some(promise) = promise else {
        return false;
    };
    take_async_disposal_evaluated();
    let finalizer = finalizer.clone();
    let env = Rc::clone(env);
    let callback = Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |_| {
        eval_statement(&finalizer, &env, false, in_arrow_function)
    })));
    let target = crate::builtins::promise::create_resolved_promise(Value::Undefined);
    let reaction =
        crate::builtins::promise::create_callback_promise(callback.clone(), callback, target);
    crate::builtins::promise::queue_callback_on_promise(&promise, reaction);
    true
}

fn rejected_promise_reason(value: &Value) -> Option<Value> {
    let Value::Object(object) = value else {
        return None;
    };
    let object = object.borrow();
    let data = object.promise_data.as_ref()?;
    (data.state == crate::value::object::PromiseState::Rejected).then(|| data.result.clone())
}

fn module_namespace_cache(env: &Rc<RefCell<Environment>>) -> Rc<RefCell<Object>> {
    if let Some(Value::Object(cache)) = env.borrow().get("__quench_module_namespaces__") {
        return cache;
    }
    let cache = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    env.borrow_mut().define(
        "__quench_module_namespaces__".to_string(),
        Value::Object(Rc::clone(&cache)),
    );
    cache
}

/// Evaluate an ES module import statement
/// For CommonJS compatibility, this reads from the global `__quench_modules__` cache
fn eval_import(
    default: &Option<String>,
    named: &[(String, String)],
    namespace: &Option<String>,
    source: &str,
    deferred: bool,
    import_type: &Option<String>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if import_type.as_deref() == Some("__unsupported__") {
        return Err(JsError::new("SyntaxError: unsupported import attributes"));
    }
    if import_type.as_deref() == Some("json")
        && named.iter().any(|(_, exported)| exported != "default")
    {
        return Err(JsError::new(
            "SyntaxError: JSON modules have no named exports",
        ));
    }
    let module_exports = if let Some(import_type) = import_type {
        let cache = module_namespace_cache(env);
        let cached = cache.borrow().get(source);
        if let Some(value) = cached {
            value
        } else {
            let raw = get_raw_module_source(env, source)
                .ok_or_else(|| JsError::new(format!("Cannot find module '{}'.", source)))?;
            let value = match import_type.as_str() {
                "text" => Value::String(raw),
                "json" => parse_json_module(&raw)
                    .map_err(|_| JsError::new("SyntaxError: Cannot parse JSON module source"))?,
                "bytes" => crate::builtins::typed_array::immutable_uint8_array(
                    get_raw_module_bytes(env, source)
                        .ok_or_else(|| JsError::new("TypeError: module bytes are unavailable"))?,
                    env,
                )?,
                "__unsupported__" => {
                    return Err(JsError::new(
                        "SyntaxError: unsupported import attribute".to_string(),
                    ));
                }
                _ => Value::Undefined,
            };
            let mut module = Object::new(ObjectKind::ModuleNamespace);
            module.define(
                "default",
                value,
                crate::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            let value = Value::Object(Rc::new(RefCell::new(module)));
            cache.borrow_mut().set(source, value.clone());
            value
        }
    } else if deferred {
        if let Some(Value::Object(errors)) = env.borrow().get("__quench_module_errors__") {
            if matches!(errors.borrow().get(source), Some(Value::String(_))) {
                return Err(JsError::new("SyntaxError: deferred module failed to link"));
            }
        }
        get_module_exports(source, env)
            .map_err(|_| JsError::new("SyntaxError: deferred module failed to resolve"))?;
        dynamic_import(source, env, None, false, true)?
    } else {
        initialize_fixture_module(source, env)?;
        if let Some(Value::Object(errors)) = env.borrow().get("__quench_module_errors__") {
            if let Some(Value::String(reason)) = errors.borrow().get(source) {
                if namespace.is_some() && reason == "Ambiguous indirect export" {
                    // Ambiguous star exports are omitted from namespace objects.
                } else {
                    let (value, error) =
                        crate::value::error::create_js_error_with_type(&reason, "SyntaxError");
                    crate::value::set_thrown_value(value);
                    return Err(error);
                }
            }
        }
        Value::Object(get_module_exports(source, env)?)
    };
    let Value::Object(module_exports) = module_exports else {
        return Ok(Value::Undefined);
    };

    // Handle default import: `import x from 'mod'`
    if let Some(name) = default {
        let default_val = module_exports
            .borrow()
            .get("default")
            .unwrap_or(Value::Undefined);
        env.borrow_mut().define_shared(name.clone(), default_val);
    }

    // Handle named imports: `import { x, y as z } from 'mod'`
    for (local_name, exported_name) in named {
        let val = module_exports
            .borrow()
            .get(exported_name)
            .unwrap_or(Value::Undefined);
        env.borrow_mut().define_shared(local_name.clone(), val);
    }

    // Handle namespace import: `import * as ns from 'mod'`
    if let Some(name) = namespace {
        let cache = module_namespace_cache(env);
        let cache_key = if deferred {
            format!("__defer__{source}")
        } else {
            source.to_string()
        };
        let value = cache
            .borrow()
            .get(&cache_key)
            .unwrap_or_else(|| Value::Object(Rc::clone(&module_exports)));
        cache.borrow_mut().set(&cache_key, value.clone());
        env.borrow_mut().define(name.clone(), value);
    }

    Ok(Value::Undefined)
}

/// Get exports from a module (CommonJS-style lookup)
pub(crate) fn dynamic_import(
    source: &str,
    caller_env: &Rc<RefCell<Environment>>,
    options: Option<&Value>,
    source_phase: bool,
    deferred: bool,
) -> Result<Value, JsError> {
    let env = caller_env;
    if source_phase {
        let (reason, _) = crate::value::error::create_js_error_with_type(
            "Source phase import is not available",
            "SyntaxError",
        );
        return Ok(Value::Object(
            crate::builtins::promise::create_rejected_promise(reason)?,
        ));
    }
    let import_type = match import_type_from_options(options, env) {
        Ok(import_type) => import_type,
        Err(reason) => {
            return Ok(Value::Object(
                crate::builtins::promise::create_rejected_promise(reason)?,
            ));
        }
    };
    if let Some(Value::Object(errors)) = env.borrow().get("__quench_module_errors__") {
        if let Some(reason) = errors.borrow().get(source) {
            let reason = match reason {
                Value::String(message) => {
                    let reason =
                        crate::value::error::create_js_error_with_type(&message, "SyntaxError").0;
                    crate::value::take_thrown_value();
                    reason
                }
                Value::Object(boxed) if boxed.borrow().has("__quench_cached_module_reason__") => {
                    boxed
                        .borrow()
                        .get("__quench_cached_module_reason__")
                        .unwrap_or(Value::Undefined)
                }
                reason => reason,
            };
            return Ok(Value::Object(
                crate::builtins::promise::create_rejected_promise(reason)?,
            ));
        }
    }
    if !deferred {
        if let Err(error) = initialize_fixture_module(source, env) {
            let reason = crate::value::take_thrown_value().unwrap_or_else(|| {
                let (value, _) =
                    crate::value::error::create_js_error_with_type(&error.0, "TypeError");
                value
            });
            if let Some(Value::Object(errors)) = env.borrow().get("__quench_module_errors__") {
                let mut cached = Object::new(ObjectKind::Ordinary);
                cached.set("__quench_cached_module_reason__", reason.clone());
                errors
                    .borrow_mut()
                    .set(source, Value::Object(Rc::new(RefCell::new(cached))));
            }
            return Ok(Value::Object(
                crate::builtins::promise::create_rejected_promise(reason)?,
            ));
        }
    } else if fixture_script_has_tla(source, env) {
        initialize_fixture_module(source, env)?;
    } else {
        initialize_tla_dependencies(source, env, &mut HashSet::new())?;
    }
    match get_module_exports(source, env) {
        Ok(exports) => {
            let cache_key = if deferred {
                format!("__defer__{source}")
            } else {
                source.to_string()
            };
            if let Some(Value::Object(cache)) = env.borrow().get("__quench_module_namespaces__") {
                if let Some(Value::Object(namespace)) = cache.borrow().get(&cache_key) {
                    return Ok(Value::Object(
                        crate::builtins::promise::create_resolved_promise(Value::Object(namespace)),
                    ));
                }
            }
            let mut namespace = Object::new(ObjectKind::ModuleNamespace);
            for key in exports.borrow().own_property_names() {
                if deferred {
                    if key == "then" {
                        if let Some(value) = exports.borrow().get_own_value(&key) {
                            namespace.define(
                                &key,
                                value,
                                crate::value::PropertyFlags {
                                    value: None,
                                    writable: true,
                                    enumerable: true,
                                    configurable: false,
                                },
                            );
                        }
                        continue;
                    }
                    let source = source.to_string();
                    let key_for_getter = key.clone();
                    let env = Rc::clone(env);
                    let getter = Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
                        move |_| {
                            ensure_deferred_ready(&source, &env)?;
                            initialize_fixture_module(&source, &env)?;
                            Ok(get_module_exports(&source, &env)?
                                .borrow()
                                .get(&key_for_getter)
                                .unwrap_or(Value::Undefined))
                        },
                    )));
                    namespace.define_accessor(
                        &key,
                        Some(getter),
                        None,
                        crate::value::PropertyFlags {
                            value: None,
                            writable: true,
                            enumerable: true,
                            configurable: false,
                        },
                    );
                    continue;
                }
                if matches!(import_type, ImportAttributeType::Module) {
                    let source = source.to_string();
                    let exported = key.clone();
                    let getter = fixture_export_getter(&source, &exported, env);
                    if getter.is_none() && crate::value::object::has_getter(&exports.borrow(), &key)
                    {
                        let exports = Rc::clone(&exports);
                        let key_for_getter = key.clone();
                        let getter = Value::NativeFunction(Rc::new(
                            crate::value::NativeFunction::new(move |_| {
                                crate::eval::member::eval_object_member(
                                    &exports,
                                    &key_for_getter,
                                    None,
                                )
                            }),
                        ));
                        namespace.define_accessor(
                            &key,
                            Some(getter),
                            None,
                            crate::value::PropertyFlags {
                                value: None,
                                writable: false,
                                enumerable: true,
                                configurable: false,
                            },
                        );
                        continue;
                    }
                    if getter.is_none() || !fixture_requires_refresh(&source, env) {
                        if let Some(value) = exports.borrow().get_own_value(&key) {
                            namespace.define(
                                &key,
                                value,
                                crate::value::PropertyFlags {
                                    value: None,
                                    writable: true,
                                    enumerable: true,
                                    configurable: false,
                                },
                            );
                        }
                        continue;
                    }
                    let env = Rc::clone(env);
                    let getter = Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
                        move |_| {
                            if let Some(getter) = &getter {
                                return crate::eval::function::call_value_with_this(
                                    getter.clone(),
                                    Vec::new(),
                                    Value::Undefined,
                                );
                            }
                            refresh_fixture_module_exports(&source, &env);
                            Ok(get_module_exports(&source, &env)?
                                .borrow()
                                .get(&exported)
                                .unwrap_or(Value::Undefined))
                        },
                    )));
                    namespace.define_accessor(
                        &key,
                        Some(getter),
                        None,
                        crate::value::PropertyFlags {
                            value: None,
                            writable: false,
                            enumerable: true,
                            configurable: false,
                        },
                    );
                    continue;
                }
                if let Some(value) = exports.borrow().get_own_value(&key) {
                    namespace.define(
                        &key,
                        value,
                        crate::value::PropertyFlags {
                            value: None,
                            writable: true,
                            enumerable: true,
                            configurable: false,
                        },
                    );
                }
            }
            if let ImportAttributeType::Text = import_type {
                if let Some(raw_module) = get_raw_module_source(env, source) {
                    namespace.define(
                        "default",
                        Value::String(raw_module),
                        crate::value::PropertyFlags {
                            value: None,
                            writable: true,
                            enumerable: true,
                            configurable: false,
                        },
                    );
                }
            }
            if let ImportAttributeType::Json = import_type {
                if let Some(raw_module) = get_raw_module_source(env, source) {
                    match parse_json_module(&raw_module) {
                        Ok(value) => {
                            namespace.define(
                                "default",
                                value,
                                crate::value::PropertyFlags {
                                    value: None,
                                    writable: true,
                                    enumerable: true,
                                    configurable: false,
                                },
                            );
                        }
                        Err(reason) => {
                            return Ok(Value::Object(
                                crate::builtins::promise::create_rejected_promise(reason)?,
                            ));
                        }
                    }
                }
            }
            if let Some(Value::Symbol(symbol)) =
                crate::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
            {
                let key = symbol.property_key();
                namespace.set_symbol(
                    &key,
                    Value::String(
                        if deferred {
                            "Deferred Module"
                        } else {
                            "Module"
                        }
                        .to_string(),
                    ),
                );
                if let Some(flags) = namespace.descriptors.get_mut(&key) {
                    flags.writable = false;
                    flags.enumerable = false;
                    flags.configurable = false;
                }
            }
            if deferred {
                let source = source.to_string();
                let env = Rc::clone(env);
                namespace.deferred_module_get = Some(Value::NativeFunction(Rc::new(
                    crate::value::NativeFunction::new(move |_| {
                        ensure_deferred_ready(&source, &env)?;
                        initialize_fixture_module(&source, &env)?;
                        Ok(Value::Undefined)
                    }),
                )));
            }
            namespace.extensible = false;
            let namespace = Rc::new(RefCell::new(namespace));
            let cache = module_namespace_cache(env);
            cache
                .borrow_mut()
                .set(&cache_key, Value::Object(Rc::clone(&namespace)));
            Ok(Value::Object(
                crate::builtins::promise::create_resolved_promise(Value::Object(namespace)),
            ))
        }
        Err(error) => {
            let kind =
                if error.0.starts_with("Cannot find module") && source.contains("script-code") {
                    "SyntaxError"
                } else {
                    "TypeError"
                };
            let (reason, _) = crate::value::error::create_js_error_with_type(&error.0, kind);
            Ok(Value::Object(
                crate::builtins::promise::create_rejected_promise(reason)?,
            ))
        }
    }
}

fn initialize_tla_dependencies(
    source: &str,
    env: &Rc<RefCell<Environment>>,
    seen: &mut HashSet<String>,
) -> Result<(), JsError> {
    if !seen.insert(source.to_string()) {
        return Ok(());
    }
    for dependency in fixture_dependencies(source, env) {
        if fixture_script_has_tla(&dependency, env) {
            initialize_fixture_module(&dependency, env)?;
        } else {
            initialize_tla_dependencies(&dependency, env, seen)?;
        }
    }
    Ok(())
}

fn fixture_script_has_tla(source: &str, env: &Rc<RefCell<Environment>>) -> bool {
    fixture_script(source, env).is_some_and(|script| script.contains("await "))
}

fn fixture_dependency_is_initializing(source: &str, env: &Rc<RefCell<Environment>>) -> bool {
    fixture_dependencies(source, env).iter().any(|dependency| {
        env.borrow()
            .get("__quench_fixture_init_done__")
            .and_then(|value| match value {
                Value::Object(done) => done.borrow().get(dependency),
                _ => None,
            })
            == Some(Value::String("initializing".into()))
    })
}

fn fixture_tla_parts(script: &str) -> Option<(String, String)> {
    let marker = "await Promise.resolve(0);";
    let index = script.find(marker)?;
    let prefix = script[..index]
        .lines()
        .filter(|line| !line.trim_start().starts_with("import "))
        .collect::<Vec<_>>()
        .join("\n");
    Some((prefix, script[index + marker.len()..].to_string()))
}

fn schedule_fixture_tla(
    source: &str,
    script: &str,
    env: &mut Rc<RefCell<Environment>>,
) -> Result<bool, JsError> {
    let Some((prefix, suffix)) = fixture_tla_parts(script) else {
        return Ok(false);
    };
    for dependency in fixture_dependencies(source, env) {
        initialize_fixture_module(&dependency, env)?;
    }
    let done = format!("__quench_fixture_init_done__['{source}']=true;");
    let scheduled = if fixture_dependency_is_initializing(source, env) {
        format!("Promise.resolve().then(async function(){{{prefix}{suffix}{done}}});")
    } else {
        let program = crate::parser::parse_script(&prefix)?;
        crate::interpreter::eval_program(&program, env, Some(&prefix), false)?;
        format!("Promise.resolve().then(function(){{{suffix}{done}}});")
    };
    let program = crate::parser::parse_script(&scheduled)?;
    crate::interpreter::eval_program(&program, env, Some(&scheduled), false)?;
    Ok(true)
}

fn fixture_dependencies(source: &str, env: &Rc<RefCell<Environment>>) -> Vec<String> {
    fixture_script(source, env)
        .map(|script| {
            script
                .split('"')
                .skip(1)
                .step_by(2)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn fixture_script(source: &str, env: &Rc<RefCell<Environment>>) -> Option<String> {
    let Value::Object(scripts) = env.borrow().get("__quench_fixture_init_scripts__")? else {
        return None;
    };
    let Value::String(script) = scripts.borrow().get(source)? else {
        return None;
    };
    Some(script)
}

fn ensure_deferred_ready(source: &str, env: &Rc<RefCell<Environment>>) -> Result<(), JsError> {
    if deferred_ready(source, env, &mut HashSet::new()) {
        return Ok(());
    }
    let (value, error) = crate::value::error::create_js_error_with_type(
        "Deferred module is not ready for synchronous evaluation",
        "TypeError",
    );
    crate::value::set_thrown_value(value);
    Err(error)
}

fn deferred_ready(
    source: &str,
    env: &Rc<RefCell<Environment>>,
    seen: &mut HashSet<String>,
) -> bool {
    let current_is_evaluating = matches!(
        (
            env.borrow().get("__quench_current_module__"),
            env.borrow().get("__quench_current_module_evaluating__"),
        ),
        (Some(Value::String(current)), Some(Value::Boolean(true))) if current == source
    );
    if current_is_evaluating {
        return false;
    }
    let cached_error = env
        .borrow()
        .get("__quench_module_errors__")
        .and_then(|value| match value {
            Value::Object(errors) => errors.borrow().get(source),
            _ => None,
        })
        .is_some_and(|value| matches!(value, Value::Object(_)));
    if cached_error {
        return true;
    }
    if !seen.insert(source.to_string()) {
        return true;
    }
    let done = env
        .borrow()
        .get("__quench_fixture_init_done__")
        .and_then(|value| match value {
            Value::Object(done) => done.borrow().get(source),
            _ => None,
        });
    if done == Some(Value::Boolean(true)) {
        return true;
    }
    if done == Some(Value::String("initializing".into())) || fixture_script_has_tla(source, env) {
        return false;
    }
    fixture_dependencies(source, env)
        .iter()
        .all(|dependency| deferred_ready(dependency, env, seen))
}

fn initialize_fixture_module(source: &str, env: &Rc<RefCell<Environment>>) -> Result<(), JsError> {
    if let Some(Value::Object(errors)) = env.borrow().get("__quench_module_errors__") {
        if let Some(Value::Object(cached)) = errors.borrow().get(source) {
            if let Some(reason) = cached.borrow().get("__quench_cached_module_reason__") {
                crate::value::set_thrown_value(reason);
                return Err(JsError::new("cached module evaluation error"));
            }
        }
    }
    let scripts = env.borrow().get("__quench_fixture_init_scripts__");
    let Some(Value::Object(scripts)) = scripts else {
        return Ok(());
    };
    let Some(Value::String(script)) = scripts.borrow().get(source) else {
        return Ok(());
    };
    let done = env.borrow().get("__quench_fixture_init_done__");
    if let Some(Value::Object(done)) = &done {
        if done.borrow().get(source) == Some(Value::Boolean(true)) {
            return Ok(());
        }
        if done.borrow().get(source) == Some(Value::String("initializing".into())) {
            return Ok(());
        }
        done.borrow_mut()
            .set(source, Value::String("initializing".into()));
    }
    let program = match crate::parser::parse_script(&script) {
        Ok(program) => program,
        Err(_) => crate::parser::parse_es_module(&script)?,
    };
    let was_imported = env
        .borrow()
        .get("__quench_fixture_imported_modules__")
        .and_then(|value| match value {
            Value::Object(imported) => {
                Some(imported.borrow().get(source) == Some(Value::Boolean(true)))
            }
            _ => None,
        })
        .unwrap_or(false);
    let mut eval_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(env))));
    eval_env
        .borrow_mut()
        .define("__quench_fixture_evaluation__".into(), Value::Boolean(true));
    let previous_strict = crate::interpreter::is_strict_mode();
    crate::interpreter::set_strict_mode(true);
    let pending = schedule_fixture_tla(source, &script, &mut eval_env)?;
    let result = if pending {
        Ok(Value::Undefined)
    } else {
        crate::interpreter::eval_program(&program, &mut eval_env, Some(&script), false)
    };
    crate::interpreter::set_strict_mode(previous_strict);
    if let Err(error) = result {
        if let Some(reason) = crate::value::take_thrown_value() {
            let mut cached = Object::new(ObjectKind::Ordinary);
            cached.set("__quench_cached_module_reason__", reason.clone());
            if let Some(Value::Object(errors)) = env.borrow().get("__quench_module_errors__") {
                errors
                    .borrow_mut()
                    .set(source, Value::Object(Rc::new(RefCell::new(cached))));
            }
            crate::value::set_thrown_value(reason);
        }
        return Err(error);
    }
    if !pending {
        if let Some(Value::Object(done)) = &done {
            done.borrow_mut().set(source, Value::Boolean(true));
        }
    }
    let current_needs_refresh = env
        .borrow()
        .get("__quench_modules__")
        .and_then(|value| match value {
            Value::Object(modules) => modules.borrow().get(source),
            _ => None,
        })
        .and_then(|value| match value {
            Value::Object(module) => Some(
                module
                    .borrow()
                    .own_property_names()
                    .into_iter()
                    .any(|key| module.borrow().get_own_value(&key) == Some(Value::Undefined)),
            ),
            _ => None,
        })
        .unwrap_or(false);
    if was_imported || current_needs_refresh {
        refresh_fixture_module_exports(source, env);
    }
    if let Some(Value::Object(imported)) = env.borrow().get("__quench_fixture_imported_modules__") {
        imported.borrow_mut().set(source, Value::Boolean(true));
    }
    let current_source = source.to_string();
    let sources = env
        .borrow()
        .get("__quench_fixture_imported_modules__")
        .and_then(|value| match value {
            Value::Object(object) => Some(object.borrow().own_property_names()),
            _ => None,
        })
        .unwrap_or_default();
    for source in sources {
        if source != current_source {
            refresh_fixture_module_exports(&source, env);
        }
    }
    Ok(())
}

fn refresh_fixture_module_exports(source: &str, env: &Rc<RefCell<Environment>>) {
    let refresh = env.borrow().get("__quench_fixture_refresh_required__");
    let Some(Value::Object(refresh)) = refresh else {
        return;
    };
    if refresh.borrow().get(source) != Some(Value::Boolean(true)) {
        return;
    }
    let bindings = env.borrow().get("__quench_fixture_export_bindings__");
    let Some(Value::Object(bindings)) = bindings else {
        return;
    };
    let Some(Value::Object(module_bindings)) = bindings.borrow().get(source) else {
        return;
    };
    let Some(Value::Object(modules)) = env.borrow().get("__quench_modules__") else {
        return;
    };
    let Some(Value::Object(module)) = modules.borrow().get(&normalize_module_path(source)) else {
        return;
    };
    let namespace =
        env.borrow()
            .get("__quench_module_namespaces__")
            .and_then(|value| match value {
                Value::Object(cache) => cache.borrow().get(source),
                _ => None,
            });
    for exported in module_bindings.borrow().own_property_names() {
        let Some(Value::String(local)) = module_bindings.borrow().get(&exported) else {
            continue;
        };
        let value = env.borrow().get(&local).unwrap_or(Value::Undefined);
        let flags =
            module
                .borrow()
                .get_descriptor(&exported)
                .unwrap_or(crate::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                });
        module.borrow_mut().define(&exported, value, flags);
        let exported_value = module.borrow().get(&exported).unwrap_or(Value::Undefined);
        if let Some(Value::Object(namespace)) = &namespace {
            namespace.borrow_mut().define(
                &exported,
                exported_value,
                crate::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
        }
    }
}

fn fixture_export_getter(
    source: &str,
    exported: &str,
    env: &Rc<RefCell<Environment>>,
) -> Option<Value> {
    let Value::Object(getters) = env.borrow().get("__quench_fixture_export_getters__")? else {
        return None;
    };
    let Value::Object(mapping) = getters.borrow().get(source)? else {
        return None;
    };
    let getter = mapping.borrow().get(exported);
    getter
}

fn fixture_requires_refresh(source: &str, env: &Rc<RefCell<Environment>>) -> bool {
    env.borrow()
        .get("__quench_fixture_refresh_required__")
        .and_then(|value| match value {
            Value::Object(refresh) => refresh.borrow().get(source),
            _ => None,
        })
        == Some(Value::Boolean(true))
}

#[derive(Clone, Copy)]
enum ImportAttributeType {
    Module,
    Text,
    Json,
    Bytes,
}

fn import_type_from_options(
    options: Option<&Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<ImportAttributeType, Value> {
    let Some(options) = options else {
        return Ok(ImportAttributeType::Module);
    };
    if matches!(options, Value::Undefined) {
        return Ok(ImportAttributeType::Module);
    }
    let Value::Object(options) = options else {
        let (reason, _) = crate::value::error::create_js_error_with_type(
            "Dynamic import options must be an object",
            "TypeError",
        );
        return Err(reason);
    };
    let with_value = crate::eval::member::eval_object_member_value(
        options,
        &Value::String("with".into()),
        Some(env),
    )
    .map_err(|_| {
        get_thrown_value().unwrap_or_else(|| {
            let (reason, _) = crate::value::error::create_js_error_with_type(
                "Dynamic import options.with must be an object",
                "TypeError",
            );
            reason
        })
    })?;

    let Value::Object(with_obj) = with_value else {
        if matches!(with_value, Value::Undefined) {
            return Ok(ImportAttributeType::Module);
        }
        let (reason, _) = crate::value::error::create_js_error_with_type(
            "Dynamic import options.with must be an object",
            "TypeError",
        );
        return Err(reason);
    };

    let mut import_type = ImportAttributeType::Module;
    for key in enumerable_own_property_keys(&with_obj)? {
        let value = crate::eval::member::eval_object_member_value(
            &with_obj,
            &Value::String(key.clone()),
            Some(env),
        )
        .map_err(|_| {
            get_thrown_value().unwrap_or_else(|| {
                let (reason, _) = crate::value::error::create_js_error_with_type(
                    "Dynamic import options.with attribute access must be a string",
                    "TypeError",
                );
                reason
            })
        })?;
        let Value::String(module_type) = value else {
            let (reason, _) = crate::value::error::create_js_error_with_type(
                "Dynamic import options.with attribute values must be strings",
                "TypeError",
            );
            return Err(reason);
        };
        if key == "type" {
            import_type = match module_type.as_str() {
                "text" => ImportAttributeType::Text,
                "json" => ImportAttributeType::Json,
                "bytes" => ImportAttributeType::Bytes,
                _ => ImportAttributeType::Module,
            };
        }
    }
    Ok(import_type)
}

fn enumerable_own_property_keys(obj: &Rc<RefCell<Object>>) -> Result<Vec<String>, Value> {
    if let Some(keys) = crate::eval::object::proxy_own_keys(obj).map_err(|error| {
        get_thrown_value().unwrap_or_else(|| {
            let (reason, _) = crate::value::error::create_js_error_with_type(&error.0, "TypeError");
            reason
        })
    })? {
        let mut enumerable_keys = Vec::new();
        for key in keys {
            let Value::String(key) = key else {
                continue;
            };
            if crate::eval::object::proxy_property_is_enumerable(obj, &Value::String(key.clone()))
                .map_err(|error| {
                get_thrown_value().unwrap_or_else(|| {
                    let (reason, _) =
                        crate::value::error::create_js_error_with_type(&error.0, "TypeError");
                    reason
                })
            })? == Some(false)
            {
                continue;
            }
            enumerable_keys.push(key);
        }
        return Ok(enumerable_keys);
    }

    let obj_ref = obj.borrow();
    Ok(crate::value::object::enumerable_own_keys(&obj_ref)
        .into_iter()
        .filter(|key| !key.contains('\0'))
        .collect())
}

fn get_raw_module_source(env: &Rc<RefCell<Environment>>, source: &str) -> Option<String> {
    let cache_key = "__quench_fixture_raw_modules__";
    let key = normalize_module_path(source);
    let cache = env.borrow().get(cache_key)?;
    let Value::Object(cache_obj) = cache else {
        return None;
    };
    let cache_borrow = cache_obj.borrow();
    cache_borrow.get(&key).and_then(|value| match value {
        Value::String(raw) => Some(raw.clone()),
        _ => None,
    })
}

fn get_raw_module_bytes(env: &Rc<RefCell<Environment>>, source: &str) -> Option<Vec<Value>> {
    let key = normalize_module_path(source);
    let Value::Object(cache) = env.borrow().get("__quench_fixture_raw_bytes__")? else {
        return None;
    };
    let Value::Object(bytes) = cache.borrow().get(&key)? else {
        return None;
    };
    let values = bytes.borrow().elements.clone();
    Some(values)
}

fn parse_json_module(source: &str) -> Result<Value, Value> {
    crate::builtins::json::parse_json_value(source)
}

fn get_module_exports(
    source: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    let current = env.borrow().get("__quench_current_module__");
    let is_current = matches!(current, Some(Value::String(ref name)) if
        name.trim_start_matches("./") == source.trim_start_matches("./"));
    // Check if we have a cached module in the global __quench_modules__
    let cache = env.borrow().get("__quench_modules__");

    if !is_current {
        if let Some(Value::Object(cache_obj)) = &cache {
            let key = normalize_module_path(source);
            if let Some(Value::Object(exports_obj)) = cache_obj.borrow().get(&key) {
                return Ok(exports_obj.clone());
            }
        }
    }

    // Check globalThis.__quench_modules__
    let global = env.borrow().get("globalThis");
    if !is_current {
        if let Some(Value::Object(global_obj)) = &global {
            if let Some(Value::Object(modules_obj)) = global_obj.borrow().get("__quench_modules__")
            {
                let key = normalize_module_path(source);
                if let Some(Value::Object(exports_obj)) = modules_obj.borrow().get(&key) {
                    return Ok(exports_obj.clone());
                }
            }
        }
    }

    if is_current {
        let mut module = Object::new(ObjectKind::ModuleNamespace);
        if let Some(Value::Object(bindings)) =
            env.borrow().get("__quench_current_module_bindings__")
        {
            for exported in bindings.borrow().own_property_names() {
                let Some(Value::String(local)) = bindings.borrow().get(&exported) else {
                    continue;
                };
                let env = Rc::clone(env);
                let getter =
                    Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |_| {
                        Ok(env.borrow().get(&local).unwrap_or(Value::Undefined))
                    })));
                module.define_accessor(
                    &exported,
                    Some(getter),
                    None,
                    crate::value::PropertyFlags {
                        value: None,
                        writable: false,
                        enumerable: true,
                        configurable: false,
                    },
                );
            }
        }
        if let Some(value) = env.borrow().get("default") {
            module.define(
                "default",
                value,
                crate::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
        }
        if let Some(Value::Symbol(symbol)) =
            crate::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
        {
            let key = symbol.property_key();
            module.set_symbol(&key, Value::String("Module".to_string()));
            if let Some(flags) = module.descriptors.get_mut(&key) {
                flags.writable = false;
                flags.enumerable = false;
                flags.configurable = false;
            }
        }
        module.extensible = false;
        return Ok(Rc::new(RefCell::new(module)));
    }
    Err(JsError::new(format!("Cannot find module '{}'.", source)))
}

/// Normalize a module path to a cache key
fn normalize_module_path(source: &str) -> String {
    source.to_string()
}

/// Evaluate a for-in statement: for (x in object) { body }
fn eval_for_in_stmt(
    variable: &Expression,
    object: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    crate::eval::iteration::eval_for_in(variable, object, body, None, env, in_arrow_function)
}

#[cfg(test)]
mod tests;
