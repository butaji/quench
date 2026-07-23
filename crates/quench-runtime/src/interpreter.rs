//! JavaScript interpreter - evaluates AST nodes
//!
//! This module provides the main interpreter entry points. The actual evaluation
//! logic is in the `eval` module.

use crate::ast::*;
use crate::env::Environment;
use crate::value::{JsError, Object, Value};
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub mod helpers;
#[cfg(test)]
mod tests;

// Re-export helpers for other modules
pub use helpers::*;

/// Control flow for break/continue/return statements
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant, dead_code)]
pub(crate) enum ControlFlow {
    Break,
    Continue(Option<String>),
    Return(Value),
    Yield(Value),
    YieldDelegate(Value),
}

thread_local! {
    static CONTROL_FLOW: Cell<Option<ControlFlow>> = const { Cell::new(None) };
}

pub(crate) fn set_control_flow(cf: ControlFlow) {
    CONTROL_FLOW.with(|cell| cell.set(Some(cf)));
}

pub(crate) fn take_control_flow() -> Option<ControlFlow> {
    CONTROL_FLOW.with(|cell| cell.take())
}

#[allow(dead_code)]
pub(crate) fn is_control_flow_set() -> bool {
    CONTROL_FLOW.with(|cell| {
        let val = cell.take();
        let is_set = val.is_some();
        cell.set(val);
        is_set
    })
}

const DEFAULT_MAX_RECURSION_DEPTH: usize = 10000;
static MAX_RECURSION_DEPTH_OVERRIDE: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_RECURSION_DEPTH);

fn get_max_depth() -> usize {
    MAX_RECURSION_DEPTH_OVERRIDE.load(Ordering::SeqCst)
}

#[allow(dead_code)]
pub fn set_max_call_depth(depth: usize) {
    MAX_RECURSION_DEPTH_OVERRIDE.store(depth, Ordering::SeqCst);
}

thread_local! {
    static CURRENT_EVAL_ENV: RefCell<Option<Rc<RefCell<Environment>>>> = const { RefCell::new(None) };
}

thread_local! {
    /// Value passed to generator.next(val) — becomes the yield expression's result
    static GENERATOR_RESUME_VALUE: RefCell<Value> = const { RefCell::new(Value::Undefined) };
}

thread_local! {
    /// Value yielded by a yield expression (for the generator to return)
    static GENERATOR_YIELD_VALUE: RefCell<Option<Value>> = const { RefCell::new(None) };
}

pub(crate) fn set_generator_resume_value(val: Value) {
    GENERATOR_RESUME_VALUE.with(|cell| *cell.borrow_mut() = val);
}

pub(crate) fn take_generator_resume_value() -> Value {
    GENERATOR_RESUME_VALUE.with(|cell| cell.replace(Value::Undefined))
}

pub(crate) fn set_generator_yield(val: Value) {
    GENERATOR_YIELD_VALUE.with(|cell| *cell.borrow_mut() = Some(val));
}

pub(crate) fn peek_generator_yield() -> bool {
    GENERATOR_YIELD_VALUE.with(|cell| cell.borrow().is_some())
}

pub(crate) fn take_generator_yield() -> Option<Value> {
    GENERATOR_YIELD_VALUE.with(|cell| cell.borrow_mut().take())
}

thread_local! {
    /// Value returned by a generator return statement
    static GENERATOR_RETURN_VALUE: RefCell<Option<Value>> = const { RefCell::new(None) };
}

#[allow(dead_code)]
pub(crate) fn set_generator_return(val: Value) {
    GENERATOR_RETURN_VALUE.with(|cell| *cell.borrow_mut() = Some(val));
}

pub(crate) fn take_generator_return() -> Option<Value> {
    GENERATOR_RETURN_VALUE.with(|cell| cell.borrow_mut().take())
}

// Thread-local label scope stack for validating break/continue targets.
// Each scope is a set of label names currently in scope.
// Push/pop for blocks/labels; save/restore for eval boundaries.
thread_local! {
    static LABEL_STACK: RefCell<Vec<std::collections::HashSet<String>>> =
        const { RefCell::new(Vec::new()) };
    // Records the label stack depth at the eval boundary. When > 0,
    // has_label only searches up to this depth, preventing eval code
    // from seeing labels defined outside the eval.
    static EVAL_BARRIER_DEPTH: RefCell<usize> = const { RefCell::new(0) };
}

/// Push a new empty label scope (enter a block or eval).
pub(crate) fn push_label_scope() {
    LABEL_STACK.with(|cell| cell.borrow_mut().push(std::collections::HashSet::new()));
}

/// Pop the current label scope (exit a block or eval).
pub(crate) fn pop_label_scope() {
    LABEL_STACK.with(|cell| {
        let mut stack = cell.borrow_mut();
        if !stack.is_empty() {
            stack.pop();
        }
    });
}

/// Add a label to the current scope.
pub(crate) fn add_label(name: &str) {
    LABEL_STACK.with(|cell| {
        let mut stack = cell.borrow_mut();
        if let Some(scope) = stack.last_mut() {
            scope.insert(name.to_string());
        }
    });
}

/// Check if a label is in scope. When inside eval (barrier > 0), searches
/// only scopes added after the eval boundary (indices >= barrier), preventing
/// eval code from seeing labels defined outside the eval. When outside eval
/// (barrier == 0), searches all scopes.
pub(crate) fn has_label(name: &str) -> bool {
    LABEL_STACK.with(|cell| {
        let stack = cell.borrow();
        let barrier = EVAL_BARRIER_DEPTH.with(|d| *d.borrow());
        if barrier > 0 {
            // Inside eval: search only scopes at/after the eval boundary
            stack[barrier..].iter().any(|scope| scope.contains(name))
        } else {
            // Outside eval: search all scopes
            stack.iter().any(|scope| scope.contains(name))
        }
    })
}

/// Set the eval barrier depth to the current label stack length.
/// Called by eval_impl to establish the boundary.
pub(crate) fn set_eval_barrier_depth(depth: usize) {
    EVAL_BARRIER_DEPTH.with(|d| *d.borrow_mut() = depth);
}

/// Clear the eval barrier depth (restore normal label visibility).
pub(crate) fn clear_eval_barrier_depth() {
    EVAL_BARRIER_DEPTH.with(|d| *d.borrow_mut() = 0);
}

/// Save the current label stack depth. Used to restore after eval.
pub(crate) fn label_stack_depth() -> usize {
    LABEL_STACK.with(|cell| cell.borrow().len())
}

pub(crate) fn set_current_eval_env(env: Option<Rc<RefCell<Environment>>>) {
    CURRENT_EVAL_ENV.with(|cell| *cell.borrow_mut() = env);
}

pub(crate) fn get_current_eval_env() -> Option<Rc<RefCell<Environment>>> {
    CURRENT_EVAL_ENV.with(|cell| cell.borrow().clone())
}

thread_local! {
    static DIRECT_EVAL: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn set_direct_eval(is_direct: bool) {
    DIRECT_EVAL.with(|cell| cell.set(is_direct));
}

pub(crate) fn is_direct_eval() -> bool {
    DIRECT_EVAL.with(|cell| cell.get())
}

thread_local! {
    static EVAL_IN_CLASS_FIELD: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn set_eval_in_class_field(in_field: bool) {
    EVAL_IN_CLASS_FIELD.with(|cell| cell.set(in_field));
}

pub(crate) fn is_eval_in_class_field() -> bool {
    EVAL_IN_CLASS_FIELD.with(|cell| cell.get())
}

thread_local! {
    static INSIDE_SUPER_CALL: Cell<usize> = const { Cell::new(0) };
}

pub(crate) fn push_inside_super_call() {
    INSIDE_SUPER_CALL.with(|cell| cell.set(cell.get().saturating_add(1)));
}

pub(crate) fn pop_inside_super_call() {
    INSIDE_SUPER_CALL.with(|cell| cell.set(cell.get().saturating_sub(1)));
}

pub(crate) fn is_inside_super_call() -> bool {
    INSIDE_SUPER_CALL.with(|cell| cell.get() > 0)
}

thread_local! {
    static CURRENT_THIS: Cell<Option<Value>> = const { Cell::new(None) };
}

thread_local! {
    static CALL_THIS: Cell<Option<Value>> = const { Cell::new(None) };
}

thread_local! {
    static CURRENT_DEPTH: Cell<usize> = const { Cell::new(0) };
}

thread_local! {
    static SUPER_CLASS: RefCell<Option<Value>> = const { RefCell::new(None) };
}

thread_local! {
    static STRICT_MODE: Cell<bool> = const { Cell::new(false) };
}

thread_local! {
    static NEW_TARGET: std::cell::RefCell<Option<Value>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn set_new_target(target: Option<Value>) {
    NEW_TARGET.with(|cell| *cell.borrow_mut() = target);
}

pub(crate) fn get_new_target() -> Option<Value> {
    NEW_TARGET.with(|cell| cell.borrow().clone())
}

pub(crate) fn is_strict_mode() -> bool {
    STRICT_MODE.with(|cell| cell.get())
}

pub(crate) fn set_strict_mode(strict: bool) {
    STRICT_MODE.with(|cell| cell.set(strict));
}

pub(crate) fn get_super_class() -> Option<Value> {
    SUPER_CLASS.with(|cell| cell.borrow().clone())
}

pub fn get_super_prototype() -> Option<Rc<RefCell<Object>>> {
    get_super_class().and_then(|v| {
        if let Value::Function(ref f) = v {
            Some(f.get_prototype())
        } else if let Value::Object(ref o) = v {
            o.borrow().get("prototype").and_then(|p| {
                if let Value::Object(ref proto) = p {
                    Some(proto.clone())
                } else {
                    None
                }
            })
        } else {
            None
        }
    })
}

pub(crate) fn set_native_this(this_val: Value) {
    CURRENT_THIS.with(|cell| cell.set(Some(this_val)));
}

pub(crate) fn get_native_this() -> Option<Value> {
    CURRENT_THIS.with(|cell| {
        let val = cell.take();
        cell.set(val.clone());
        val
    })
}

pub(crate) fn take_native_this() {
    CURRENT_THIS.with(|cell| cell.take());
}

pub(crate) fn set_this_value(this_val: Value) {
    CALL_THIS.with(|cell| cell.set(Some(this_val)));
}

pub(crate) fn get_this_value() -> Option<Value> {
    CALL_THIS.with(|cell| {
        let val = cell.take();
        cell.set(val.clone());
        val
    })
}

pub(crate) fn take_this_value() {
    CALL_THIS.with(|cell| cell.take());
}

pub(crate) fn check_depth() -> Result<(), JsError> {
    let depth = CURRENT_DEPTH.with(|cell| {
        let d = cell.get();
        cell.set(d + 1);
        d
    });
    if depth >= get_max_depth() {
        CURRENT_DEPTH.with(|cell| cell.set(cell.get().saturating_sub(1)));
        Err(JsError("Maximum call stack size exceeded".to_string()))
    } else {
        Ok(())
    }
}

pub(crate) fn release_depth() {
    CURRENT_DEPTH.with(|cell| cell.set(cell.get().saturating_sub(1)));
}

/// RAII guard that releases the recursion depth counter when dropped
pub(crate) struct DepthGuard;

pub(crate) fn check_depth_guard() -> Result<DepthGuard, JsError> {
    check_depth()?;
    Ok(DepthGuard)
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        release_depth();
    }
}

pub fn reset_depth() {
    CURRENT_DEPTH.with(|cell| cell.set(0));
}

/// Evaluate a complete program with hoisting.
pub fn eval_program(
    program: &Program,
    env: &mut Rc<RefCell<Environment>>,
    _source: Option<&str>,
    set_this: bool,
) -> Result<Value, JsError> {
    match program {
        Program::Script(statements) => {
            let prev_strict = is_strict_mode();
            let script_is_strict = check_use_strict_directive(statements);
            let eval_is_strict = script_is_strict || is_strict_mode();
            set_strict_mode(eval_is_strict);

            hoist_functions(statements, env);
            hoist_classes(statements, env);
            predeclare_var(statements, &mut env.borrow_mut());
            predeclare_let_const(statements, &mut env.borrow_mut());

            if set_this {
                let this_value = env.borrow().get("globalThis").unwrap_or(Value::Undefined);
                set_this_binding(env, this_value);
            }

            let mut last_value = Value::Undefined;
            for stmt in statements {
                let val = crate::eval::eval_statement(stmt, env, false, false)?;
                // Empty completions (var/let/const/function/class declarations,
                // empty statements, empty blocks) should not replace the previous
                // completion value (ES §8.3.2).
                let is_empty_completion = matches!(
                    stmt,
                    crate::ast::Statement::VarDeclaration { .. }
                        | crate::ast::Statement::FunctionDeclaration { .. }
                        | crate::ast::Statement::ClassDeclaration { .. }
                        | crate::ast::Statement::SequenceDecls(_)
                        | crate::ast::Statement::Empty
                ) || matches!(stmt, crate::ast::Statement::Block(s) if s.is_empty());
                if !is_empty_completion {
                    last_value = val;
                }
            }

            set_strict_mode(prev_strict);
            let _ = take_control_flow();
            Ok(last_value)
        }
    }
}
