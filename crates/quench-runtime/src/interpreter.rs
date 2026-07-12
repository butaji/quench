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

/// Control flow for break/continue/return statements
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ControlFlow {
    Break,
    Continue,
    Return(Value),
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
        // Restore so eval_statements / loops can consume it
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

/// Check if we're currently in strict mode
pub(crate) fn is_strict_mode() -> bool {
    STRICT_MODE.with(|cell| cell.get())
}

/// Set strict mode (used when evaluating "use strict"; directives)
pub(crate) fn set_strict_mode(strict: bool) {
    STRICT_MODE.with(|cell| cell.set(strict));
}

/// Get the current superclass
pub(crate) fn get_super_class() -> Option<Value> {
    SUPER_CLASS.with(|cell| cell.borrow().clone())
}

/// Get the super prototype for the current class
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
        // Restore the value for subsequent calls
        cell.set(val.clone());
        val
    })
}

pub(crate) fn take_native_this() {
    CURRENT_THIS.with(|cell| {
        cell.take();
    });
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
    CALL_THIS.with(|cell| {
        cell.take();
    });
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

/// Check the recursion depth, returning a guard that releases it on drop
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

/// Evaluate a complete program with hoisting
pub fn eval_program(
    program: &Program,
    env: &mut Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match program {
        Program::Script(statements) => {
            // Check for "use strict"; directive at the beginning of the script
            let script_is_strict = check_use_strict_directive(statements);
            let prev_strict = is_strict_mode();
            set_strict_mode(script_is_strict);

            hoist_functions(statements, env);
            hoist_classes(statements, env);
            predeclare_let_const(statements, &mut env.borrow_mut());
            let global_this = env.borrow().get("globalThis").unwrap_or(Value::Undefined);
            set_this_binding(env, global_this);
            let mut last_value = Value::Undefined;
            for stmt in statements {
                last_value = crate::eval::eval_statement(stmt, env, false, false)?;
            }

            // Restore previous strict mode
            set_strict_mode(prev_strict);

            // A top-level `return` is illegal JS; discard any stale control
            // flow so it cannot leak into the next eval call.
            let _ = take_control_flow();

            Ok(last_value)
        }
    }
}

/// Check if the first statement is "use strict"; directive
fn check_use_strict_directive(statements: &[crate::ast::Statement]) -> bool {
    if let Some(crate::ast::Statement::Expression(expr)) = statements.first() {
        if let crate::ast::Expression::String(s) = expr.as_ref() {
            return s.trim() == "use strict";
        }
    }
    false
}

pub(crate) fn set_this_binding(env: &Rc<RefCell<Environment>>, this_value: Value) {
    env.borrow_mut().current_scope_mut().set_this(this_value);
}

pub(crate) fn get_this_binding(env: &Rc<RefCell<Environment>>) -> Value {
    for scope in env.borrow().scopes.iter().rev() {
        if let Some(this_val) = scope.get_this() {
            // Sloppy mode: undefined/null this → globalThis (ESMA-262 12.2.1.1)
            if !is_strict_mode() && (this_val == Value::Undefined || this_val == Value::Null) {
                let global_this = env.borrow().get("globalThis").unwrap_or(Value::Undefined);
                return global_this;
            }
            return this_val;
        }
    }
    if !is_strict_mode() {
        let global_this = env.borrow().get("globalThis").unwrap_or(Value::Undefined);
        return global_this;
    }
    Value::Undefined
}

pub(crate) fn hoist_functions(statements: &[Statement], env: &Rc<RefCell<Environment>>) {
    for stmt in statements {
        match stmt {
            Statement::FunctionDeclaration { name, params, body } => {
                let mut func = crate::value::ValueFunction::new(
                    Some(name.clone()),
                    params.clone(),
                    body.clone(),
                    Rc::clone(env),
                );
                func.strict = crate::interpreter::is_strict_mode();
                env.borrow_mut().define(name.clone(), Value::Function(func));
            }
            Statement::Block(stmts) => hoist_functions(stmts, env),
            Statement::If {
                consequent,
                alternate,
                ..
            } => {
                hoist_functions(std::slice::from_ref(consequent.as_ref()), env);
                if let Some(alt) = alternate {
                    hoist_functions(std::slice::from_ref(alt.as_ref()), env);
                }
            }
            Statement::While { body, .. } => {
                hoist_functions(std::slice::from_ref(body.as_ref()), env)
            }
            Statement::For { body, .. } => {
                hoist_functions(std::slice::from_ref(body.as_ref()), env)
            }
            _ => {}
        }
    }
}

pub(crate) fn hoist_classes(statements: &[Statement], env: &Rc<RefCell<Environment>>) {
    for stmt in statements {
        match stmt {
            Statement::ClassDeclaration { name, class: _ } => {
                // Create class value placeholder for hoisting
                // The actual class is evaluated when the statement is executed
                env.borrow_mut().declare_var(name.clone(), VarKind::Let);
            }
            Statement::Block(stmts) => hoist_classes(stmts, env),
            Statement::If {
                consequent,
                alternate,
                ..
            } => {
                hoist_classes(std::slice::from_ref(consequent.as_ref()), env);
                if let Some(alt) = alternate {
                    hoist_classes(std::slice::from_ref(alt.as_ref()), env);
                }
            }
            Statement::While { body, .. } => {
                hoist_classes(std::slice::from_ref(body.as_ref()), env)
            }
            Statement::For { body, .. } => hoist_classes(std::slice::from_ref(body.as_ref()), env),
            _ => {}
        }
    }
}

pub(crate) fn collect_var_names(stmts: &[Statement]) -> Vec<String> {
    let mut names = Vec::new();
    collect_var_names_recursive(stmts, &mut names);
    names.sort();
    names.dedup();
    names
}

#[allow(clippy::complexity)]
pub(crate) fn collect_var_names_recursive(stmts: &[Statement], names: &mut Vec<String>) {
    for stmt in stmts {
        match stmt {
            Statement::VarDeclaration {
                kind: VarKind::Var,
                name,
                ..
            } => {
                names.push(name.clone());
            }
            Statement::Block(inner_stmts) => collect_var_names_recursive(inner_stmts, names),
            Statement::If {
                consequent,
                alternate,
                ..
            } => {
                collect_var_names_recursive(std::slice::from_ref(consequent.as_ref()), names);
                if let Some(alt) = alternate {
                    collect_var_names_recursive(std::slice::from_ref(alt.as_ref()), names);
                }
            }
            Statement::While { body, .. } => {
                collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names);
            }
            Statement::For { body, .. } => {
                collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names);
            }
            Statement::TryCatch { body, handler, .. } => {
                collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names);
                collect_var_names_recursive(std::slice::from_ref(handler.as_ref()), names);
            }
            Statement::SequenceDecls(inner) => {
                collect_var_names_recursive(inner, names);
            }
            _ => {}
        }
    }
}

pub(crate) fn collect_let_const_declarations(stmts: &[Statement]) -> Vec<(String, VarKind)> {
    let mut decls = Vec::new();
    collect_let_const_recursive(stmts, &mut decls);
    decls.sort_by(|a, b| a.0.cmp(&b.0));
    decls.dedup_by(|a, b| a.0 == b.0);
    decls
}

pub(crate) fn collect_let_const_recursive(stmts: &[Statement], decls: &mut Vec<(String, VarKind)>) {
    for stmt in stmts {
        match stmt {
            Statement::VarDeclaration {
                kind: VarKind::Let,
                name,
                ..
            } => {
                decls.push((name.clone(), VarKind::Let));
            }
            Statement::VarDeclaration {
                kind: VarKind::Const,
                name,
                ..
            } => {
                decls.push((name.clone(), VarKind::Const));
            }
            Statement::SequenceDecls(inner) => {
                collect_let_const_recursive(inner, decls);
            }
            Statement::Block(inner) => {
                collect_let_const_recursive(inner, decls);
            }
            _ => {}
        }
    }
}

pub(crate) fn predeclare_var(stmts: &[Statement], env: &mut Environment) {
    let names = collect_var_names(stmts);
    for name in names {
        env.declare_var(name, VarKind::Var);
    }
}

pub(crate) fn predeclare_let_const(stmts: &[Statement], env: &mut Environment) {
    let decls = collect_let_const_declarations(stmts);
    for (name, kind) in decls {
        // Skip if already declared in any outer scope (let/const cannot be shadowed
        // by redeclaring in an inner block - they share the same binding)
        if !env.has(&name) {
            env.declare_var(name, kind);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reset_depth() {
        reset_depth();
    }
}
