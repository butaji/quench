//! Statement evaluation

use crate::ast::*;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::interpreter::{
    add_label, collect_var_names_recursive, has_label, pop_label_scope, predeclare_let_const,
    push_label_scope, set_control_flow, take_control_flow, ControlFlow,
};
use crate::value::function::ValueFunction;
use crate::value::{
    set_thrown_value, take_thrown_value, to_bool, to_js_string, JsError, Object, ObjectKind, Value,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Returns true if expr is a Call expression — the only expression type
/// that can appear in a proper tail position per ES §14.2.1.
pub(crate) fn is_tail_expr(expr: &Expression) -> bool {
    matches!(expr, Expression::Call { .. })
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
}

impl TailCallSignal {
    pub fn new(function: ValueFunction, arguments: Vec<Value>) -> Self {
        Self {
            function,
            arguments,
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
}

/// Set the tail-call signal for the trampoline to pick up.
pub(crate) fn set_tail_call_signal(signal: TailCallSignal) {
    TAIL_CALL_SIGNAL.with(|cell| *cell.borrow_mut() = Some(signal));
}

/// Take and clear the tail-call signal (consumed by the trampoline).
pub(crate) fn take_tail_call_signal() -> Option<TailCallSignal> {
    TAIL_CALL_SIGNAL.with(|cell| cell.borrow_mut().take())
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
        // Per ES spec §8.3.2, empty completions (var/let/const/function declarations)
        // should not replace the previous completion value. Only update last_val
        // when the statement produces a non-empty value (like an expression).
        if !matches!(
            stmt,
            Statement::VarDeclaration { .. }
                | Statement::FunctionDeclaration { .. }
                | Statement::ClassDeclaration { .. }
                | Statement::SequenceDecls(_)
        ) {
            last_val = val;
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
            Some(ControlFlow::Return(val)) | Some(ControlFlow::Yield(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            // YieldDelegate: also propagate as Return (the generator handles it)
            Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            // Propagate break/continue so enclosing loops can observe them.
            Some(cf @ (ControlFlow::Break | ControlFlow::Continue)) => {
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
    let last_idx = stmts.len().saturating_sub(1);
    let mut last_val = Value::Undefined;
    for (i, stmt) in stmts.iter().enumerate() {
        let is_last_stmt = i == last_idx;

        // Check for tail-call return at top level.
        if let Statement::Return(ref expr) = stmt {
            if is_last_stmt && expr.as_ref().is_some_and(|e| is_tail_expr(e)) {
                // Set tail-call signal, then break to let the trampoline extract
                // the accumulator from the acc_stack.
                handle_tail_call(expr, env, in_arrow_function)?;
                break;
            }
            // Non-tail return.
            let val = match expr {
                Some(e) => eval_expression(e, env, in_arrow_function)?,
                None => Value::Undefined,
            };
            set_control_flow(ControlFlow::Return(val.clone()));
            return Ok(val);
        }

        // Check for tail-call return inside a block at the last position.
        // Per ES §14.2.1, the block's body is in tail position.
        if is_last_stmt {
            if let Statement::Block(inner_stmts) = stmt {
                if let Some(()) = handle_tail_call_in_block(inner_stmts, env, in_arrow_function)? {
                    // Tail call was set; break to let trampoline run.
                    break;
                }
            }
        }

        let stmt_val = eval_statement(stmt, env, false, in_arrow_function)?;
        // Per ES §8.3.2, empty completions (var/let/const/function declarations)
        // should not replace the previous completion value.
        if !matches!(
            stmt,
            Statement::VarDeclaration { .. }
                | Statement::FunctionDeclaration { .. }
                | Statement::ClassDeclaration { .. }
                | Statement::SequenceDecls(_)
        ) {
            last_val = stmt_val;
        }
        // For the last statement, DON'T check ControlFlow::Return here.
        // Let the final return statement be reached and evaluated properly.
        // This prevents inner non-tail call results from short-circuiting
        // the rest of the function body (e.g., `var x = g(); return x + 1`).
        if is_last_stmt {
            continue;
        }
        match take_control_flow() {
            Some(ControlFlow::Return(val)) => return Ok(val),
            Some(
                cf @ (ControlFlow::Break
                | ControlFlow::Continue
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
        return Ok(val);
    }
    Ok(last_val)
}

/// Handle a tail-call return expression: resolve callee and args, then
/// set the thread-local signal for the trampoline to pick up.
fn handle_tail_call(
    expr: &Option<Box<Expression>>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<(), JsError> {
    if let Some(e) = expr.as_ref() {
        if let Expression::Call { callee, arguments } = e.as_ref() {
            let callee_val = eval_expression(callee, env, in_arrow_function)?;
            let args: Vec<Value> = arguments
                .iter()
                .map(|arg| eval_expression(arg, env, in_arrow_function))
                .collect::<Result<Vec<_>, _>>()?;
            let function = resolve_callee_to_function(callee_val)?;
            set_tail_call_signal(TailCallSignal::new(function, args));
        }
    }
    Ok(())
}

/// Recursively find a tail-call return inside a block at the last position.
/// Returns `Ok(Some(()))` when a tail call was set (caller should break).
/// Returns `Ok(None)` when no tail-call return was found (caller evaluates normally).
fn handle_tail_call_in_block(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Option<()>, JsError> {
    if stmts.is_empty() {
        return Ok(None);
    }
    let last_stmt = &stmts[stmts.len() - 1];

    // Last statement is a Return → check for tail call.
    if let Statement::Return(ref expr) = last_stmt {
        if expr.as_ref().is_some_and(|e| is_tail_expr(e)) {
            handle_tail_call(expr, env, in_arrow_function)?;
            return Ok(Some(()));
        }
        // Non-tail return inside block: evaluate it and propagate via control flow.
        let val = match expr.as_ref() {
            Some(e) => eval_expression(e, env, in_arrow_function)?,
            None => Value::Undefined,
        };
        set_control_flow(ControlFlow::Return(val));
        return Ok(Some(()));
    }

    // Last statement is a nested Block → recurse.
    if let Statement::Block(inner_stmts) = last_stmt {
        return handle_tail_call_in_block(inner_stmts, env, in_arrow_function);
    }

    // No tail-call found; caller will evaluate the block normally.
    Ok(None)
}

/// Resolve a Value to a ValueFunction, used for tail-call resolution.
fn resolve_callee_to_function(callee_val: Value) -> Result<ValueFunction, JsError> {
    match callee_val {
        Value::Function(f) => Ok(f),
        Value::Symbol(_) => Err(JsError("Symbol is not a function".into())),
        _ => Err(JsError(format!(
            "{} is not a function",
            value_type_name(&callee_val)
        ))),
    }
}

/// Return a human-readable type name for a Value.
fn value_type_name(v: &Value) -> &str {
    match v {
        Value::Undefined => "undefined",
        Value::Null => "null",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::BigInt(_) => "bigint",
        Value::String(_) => "string",
        Value::Symbol(_) => "symbol",
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Class(_)
        | Value::Generator(_) => "object",
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
            if to_bool(&cond_val) {
                eval_statement(consequent.as_ref(), env, _is_expr_body, in_arrow_function)
            } else if let Some(alt) = alternate {
                eval_statement(alt.as_ref(), env, _is_expr_body, in_arrow_function)
            } else {
                Ok(Value::Undefined)
            }
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
        } => eval_for(init, condition, update, body, env, in_arrow_function),
        Statement::Block(stmts) => eval_block(stmts, env, in_arrow_function),
        Statement::SequenceDecls(stmts) => {
            // Evaluate var declarations in sequence without creating a new scope
            let mut result = Value::Undefined;
            for stmt in stmts {
                result = eval_statement(stmt, env, false, in_arrow_function)?;
            }
            Ok(result)
        }
        Statement::Return(expr) => {
            let val = match expr {
                Some(e) => eval_expression(e, env, in_arrow_function)?,
                None => Value::Undefined,
            };
            set_control_flow(ControlFlow::Return(val));
            Ok(Value::Undefined)
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
            let result = eval_statement(body, env, false, in_arrow_function);
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
            set_control_flow(ControlFlow::Break);
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
            set_control_flow(ControlFlow::Continue);
            Ok(Value::Undefined)
        }
        Statement::Try {
            body,
            param,
            handler,
            finalizer,
        } => eval_try(body, param, handler, finalizer, env, in_arrow_function),
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
            // pushing a fresh scope and pre-populating its bindings with a
            // snapshot of the object's own enumerable properties. A more
            // faithful implementation would track the object and defer each
            // get to it; this approximation is enough for the tests in
            // scope (variable captures of with-scoped names).
            if crate::interpreter::is_strict_mode() {
                return Err(JsError(
                    "SyntaxError: 'with' statements are not allowed in strict mode".to_string(),
                ));
            }
            let obj_val = eval_expression(object, env, in_arrow_function)?;
            let Value::Object(obj_rc) = obj_val else {
                return eval_statement(body, env, _is_expr_body, in_arrow_function);
            };
            env.borrow_mut().push_scope();
            env.borrow()
                .current_scope()
                .borrow_mut()
                .set_object_binding(Rc::clone(&obj_rc));
            {
                let obj_borrowed = obj_rc.borrow();
                // Check Symbol.unscopables (§13.11.7): properties blocked by
                // unscopables are not added to the with-scope.
                let unscopables_val = obj_borrowed
                    .symbol_properties
                    .iter()
                    .find_map(|(k, v)| {
                        if k.starts_with("Symbol(") && k.contains("unscopables") {
                            Some(v.clone())
                        } else {
                            None
                        }
                    })
                    // Also check regular properties (the key might be the
                    // Symbol description string like "Symbol.unscopables")
                    .or_else(|| obj_borrowed.properties.get("Symbol.unscopables").cloned());
                let blocked: std::collections::HashSet<String> =
                    if let Some(Value::Object(u_obj)) = unscopables_val {
                        let u = u_obj.borrow();
                        u.properties
                            .iter()
                            .filter(|(_, v)| crate::value::to_bool(v))
                            .map(|(k, _)| k.clone())
                            .collect()
                    } else {
                        std::collections::HashSet::new()
                    };
                for (key, value) in &obj_borrowed.properties {
                    if !blocked.contains(key) {
                        env.borrow_mut()
                            .current_scope()
                            .borrow_mut()
                            .define(key.clone(), value.clone());
                    }
                }
            }
            let result = eval_statement(body, env, _is_expr_body, in_arrow_function);
            env.borrow_mut().pop_scope();
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
        } => eval_import(default, named, namespace, source, env),
        Statement::ForIn {
            variable,
            object,
            body,
        } => eval_for_in_stmt(variable, object, body, env, in_arrow_function),
    }
}

/// Helper to set a property on globalThis if we're at the top level.
fn set_on_global_this(env: &Rc<RefCell<Environment>>, name: &str, value: Value) {
    // Only set on globalThis if this is the top-level environment
    let is_top_level = env.borrow().get_parent().is_none();
    if is_top_level {
        // Get globalThis outside the mutable borrow to avoid conflict
        let global_this = env.borrow().get("globalThis");
        if let Some(Value::Object(global_obj)) = global_this {
            global_obj.borrow_mut().set(name, value);
        }
    }
}

fn eval_var_decl(
    kind: &VarKind,
    name: &str,
    init: &Option<Expression>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let already_declared = env.borrow().current_scope().borrow().has(name);
    if !already_declared {
        env.borrow_mut().declare_var(name.to_string(), *kind);
    }
    let mut value = if let Some(expr) = init {
        eval_expression(expr, env, in_arrow_function)?
    } else {
        Value::Undefined
    };
    // Per ES §13.3.3 SetFunctionName: when a VariableDeclaration's
    // initializer evaluates to a function expression that has no name,
    // bind the variable's name as the function's `name`.
    if let Value::Function(ref mut f) = value {
        if f.name.is_none() {
            f.name = Some(name.to_string());
            let _ = f.set_property("name", Value::String(name.to_string()));
        }
    }
    env.borrow_mut().initialize_declared(name, value.clone());
    // For top-level var declarations, also set on globalThis
    if *kind == VarKind::Var && env.borrow().get_parent().is_none() {
        set_on_global_this(env, name, value);
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
    let mut func = crate::value::ValueFunction::new(
        Some(name.to_owned()),
        params.to_vec(),
        body.to_vec(),
        Rc::clone(env),
        is_async,
        is_generator,
    );
    func.strict = crate::interpreter::is_strict_mode();
    func.name = Some(name.to_string()); // Set .name property per ES spec SetFunctionName
    let value = Value::Function(func);
    env.borrow_mut().define(name.to_owned(), value.clone());
    // Top-level function declarations are globals (same as var).
    if env.borrow().get_parent().is_none() {
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
    env.borrow_mut().define(name.to_owned(), class_val);
    Ok(Value::Undefined)
}

fn eval_while(
    condition: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    while to_bool(&eval_expression(condition, env, in_arrow_function)?) {
        take_control_flow();
        let _ = eval_statement(body, env, false, in_arrow_function)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Return(val)) | Some(ControlFlow::Yield(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            // YieldDelegate: also propagate as Return (the generator handles it)
            Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    Ok(Value::Undefined)
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
    let result = eval_do_while_impl(body, condition, env, in_arrow_function);
    pop_label_scope();
    result
}

fn eval_do_while_impl(
    body: &Statement,
    condition: &Expression,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    loop {
        take_control_flow();
        let body_val = eval_statement(body, env, false, in_arrow_function)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Return(val)) | Some(ControlFlow::Yield(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Continue) => {}
            None => {}
        }
        // Check condition; if false, return the body completion value
        if !to_bool(&eval_expression(condition, env, in_arrow_function)?) {
            return Ok(body_val);
        }
    }
    Ok(Value::Undefined)
}

fn eval_for(
    init: &Option<ForInit>,
    condition: &Option<Box<Expression>>,
    update: &Option<Box<Expression>>,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    if let Some(for_init) = init {
        match for_init {
            ForInit::Expression(expr) => {
                let _ = eval_expression(expr, env, in_arrow_function)?;
            }
            ForInit::VarDeclaration { kind, name, init } => {
                env.borrow_mut().declare_var(name.to_string(), *kind);
                let value = init
                    .as_ref()
                    .map(|e| eval_expression(e, env, in_arrow_function))
                    .unwrap_or(Ok(Value::Undefined))?;
                env.borrow_mut().initialize_declared(name, value);
            }
        }
    }
    let check_condition = || -> Result<bool, JsError> {
        if let Some(c) = condition.as_ref() {
            Ok(to_bool(&eval_expression(c, env, in_arrow_function)?))
        } else {
            Ok(true)
        }
    };
    while check_condition()? {
        take_control_flow();
        let _ = eval_statement(body, env, false, in_arrow_function)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Return(val)) | Some(ControlFlow::Yield(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            // YieldDelegate: also propagate as Return (the generator handles it)
            Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Continue) => {}
            None => {}
        }
        if let Some(update) = update {
            let _ = eval_expression(update, env, in_arrow_function)?;
        }
    }
    Ok(Value::Undefined)
}

fn eval_block(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    env.borrow_mut().push_scope();
    predeclare_let_const(stmts, &mut env.borrow_mut());
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
    let try_result = eval_statement(body, env, false, in_arrow_function);

    // Handle the result
    match try_result {
        Ok(try_val) => {
            // Try succeeded - run finally if present, propagate control flow if needed
            if let Some(fin) = finalizer {
                // Check for pending control flow before finally
                let pending_cf = take_control_flow();
                drop(pending_cf);

                let fin_result = eval_statement(fin, env, false, in_arrow_function);
                match fin_result {
                    Ok(_) => {
                        // Finally completed normally - check if it set a new control flow
                        if let Some(cf) = take_control_flow() {
                            match cf {
                                ControlFlow::Return(val) => {
                                    set_control_flow(ControlFlow::Return(val));
                                }
                                ControlFlow::Break => {
                                    set_control_flow(ControlFlow::Break);
                                }
                                ControlFlow::Continue => {
                                    set_control_flow(ControlFlow::Continue);
                                }
                                _ => {}
                            }
                        }
                        // Return the try value if no control flow override
                        Ok(try_val)
                    }
                    Err(e) => Err(e), // Finally threw - propagate
                }
            } else {
                Ok(try_val)
            }
        }
        Err(_e) => {
            // Try threw - handle with catch if present
            let thrown_value = take_thrown_value().unwrap_or(Value::Undefined);
            let thrown_for_catch = thrown_value.clone();

            if let Some(name) = param {
                env.borrow_mut().define(name.to_string(), thrown_for_catch);
            }

            if let Some(h) = handler {
                // Run catch block
                let catch_result = eval_statement(h, env, false, in_arrow_function);

                // Run finally if present
                if let Some(fin) = finalizer {
                    let pending_cf = take_control_flow();
                    drop(pending_cf);

                    let fin_result = eval_statement(fin, env, false, in_arrow_function);
                    match fin_result {
                        Ok(_) => {
                            // Propagate control flow from catch or finally
                            if let Some(cf) = take_control_flow() {
                                match cf {
                                    ControlFlow::Return(val) => {
                                        set_control_flow(ControlFlow::Return(val));
                                    }
                                    ControlFlow::Break => {
                                        set_control_flow(ControlFlow::Break);
                                    }
                                    ControlFlow::Continue => {
                                        set_control_flow(ControlFlow::Continue);
                                    }
                                    _ => {}
                                }
                            }
                            // Return catch result if no control flow override
                            catch_result
                        }
                        Err(e) => Err(e), // Finally threw
                    }
                } else {
                    catch_result
                }
            } else {
                // No catch - run finally if present, then rethrow
                if let Some(fin) = finalizer {
                    let pending_cf = take_control_flow();
                    drop(pending_cf);

                    let fin_result = eval_statement(fin, env, false, in_arrow_function);
                    match fin_result {
                        Ok(_) => {
                            // Finally completed normally - rethrow
                            let msg = to_js_string(&thrown_value);
                            set_thrown_value(thrown_value);
                            Err(JsError(msg))
                        }
                        Err(e) => Err(e), // Finally threw - propagate that instead
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

/// Evaluate an ES module import statement
/// For CommonJS compatibility, this reads from the global `__quench_modules__` cache
fn eval_import(
    default: &Option<String>,
    named: &[(String, String)],
    namespace: &Option<String>,
    source: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Get the module's exports from our module cache
    let module_exports = get_module_exports(source, env)?;

    // Handle default import: `import x from 'mod'`
    if let Some(name) = default {
        let default_val = module_exports
            .borrow()
            .get("default")
            .unwrap_or(Value::Undefined);
        env.borrow_mut().define(name.clone(), default_val);
    }

    // Handle named imports: `import { x, y as z } from 'mod'`
    for (local_name, exported_name) in named {
        let val = module_exports
            .borrow()
            .get(exported_name)
            .unwrap_or(Value::Undefined);
        env.borrow_mut().define(local_name.clone(), val);
    }

    // Handle namespace import: `import * as ns from 'mod'`
    if let Some(name) = namespace {
        env.borrow_mut()
            .define(name.clone(), Value::Object(module_exports.clone()));
    }

    Ok(Value::Undefined)
}

/// Get exports from a module (CommonJS-style lookup)
fn get_module_exports(
    source: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    // Check if we have a cached module in the global __quench_modules__
    let cache = env.borrow().get("__quench_modules__");

    if let Some(Value::Object(cache_obj)) = &cache {
        let key = normalize_module_path(source);
        if let Some(Value::Object(exports_obj)) = cache_obj.borrow().get(&key) {
            return Ok(exports_obj.clone());
        }
    }

    // Check globalThis.__quench_modules__
    let global = env.borrow().get("globalThis");
    if let Some(Value::Object(global_obj)) = &global {
        if let Some(Value::Object(modules_obj)) = global_obj.borrow().get("__quench_modules__") {
            let key = normalize_module_path(source);
            if let Some(Value::Object(exports_obj)) = modules_obj.borrow().get(&key) {
                return Ok(exports_obj.clone());
            }
        }
    }

    // Create a new empty exports object for this module
    let exports = Object::new(ObjectKind::Ordinary);
    let exports_rc = Rc::new(RefCell::new(exports));

    // Cache it for future imports
    let key = normalize_module_path(source);
    if let Some(Value::Object(cache_obj)) = cache {
        cache_obj
            .borrow_mut()
            .set(&key, Value::Object(exports_rc.clone()));
    }

    Ok(exports_rc)
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
    use crate::eval::iteration::get_enumerable_keys;
    use crate::eval::object::assign_to;

    let obj_value = eval_expression(object, env, in_arrow_function)?;
    let keys = get_enumerable_keys(&obj_value)?;
    if matches!(variable, Expression::ObjectPattern(_)) {
        return Err(JsError("unsupported pattern in for-in loop".to_string()));
    }
    for key in keys {
        assign_to(variable, &Value::String(key), env)?;
        let _ = eval_statement(body, env, false, in_arrow_function)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Return(val)) | Some(ControlFlow::Yield(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            // YieldDelegate: also propagate as Return (the generator handles it)
            Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    Ok(Value::Undefined)
}

#[cfg(test)]
mod tests;
