//! Statement evaluation

use crate::ast::*;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::value::{to_js_string, to_bool, JsError, Value};
use crate::interpreter::{
    predeclare_let_const, take_control_flow, is_control_flow_set, set_control_flow, ControlFlow,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate a list of statements
pub fn eval_statements(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    is_expr_body: bool,
) -> Result<Value, JsError> {
    let mut last_value = Value::Undefined;
    for stmt in stmts {
        last_value = eval_statement(stmt, env, is_expr_body)?;
        if is_control_flow_set() {
            break;
        }
    }
    Ok(last_value)
}

/// Evaluate a single statement
pub fn eval_statement(
    stmt: &Statement,
    env: &Rc<RefCell<Environment>>,
    _is_expr_body: bool,
) -> Result<Value, JsError> {
    match stmt {
        Statement::VarDeclaration { kind, name, init } => {
            eval_var_decl(kind, name, init, env)
        }
        Statement::FunctionDeclaration { name, params, body } => {
            eval_func_decl(name, params, body, env)
        }
        Statement::ClassDeclaration { name, class } => {
            eval_class_decl(name, class, env)
        }
        Statement::If { condition, consequent, alternate } => {
            let cond_val = eval_expression(condition, env)?;
            if to_bool(&cond_val) {
                eval_statement(consequent.as_ref(), env, _is_expr_body)
            } else if let Some(alt) = alternate {
                eval_statement(alt.as_ref(), env, _is_expr_body)
            } else {
                Ok(Value::Undefined)
            }
        }
        Statement::While { condition, body } => {
            eval_while(condition, body, env)
        }
        Statement::For { init, condition, update, body } => {
            eval_for(init, condition, update, body, env)
        }
        Statement::Block(stmts) => {
            eval_block(stmts, env)
        }
        Statement::SequenceDecls(stmts) => {
            // Evaluate var declarations in sequence without creating a new scope
            let mut result = Value::Undefined;
            for stmt in stmts {
                result = eval_statement(stmt, env, false)?;
            }
            Ok(result)
        }
        Statement::Return(expr) => {
            if let Some(e) = expr {
                eval_expression(e, env)
            } else {
                Ok(Value::Undefined)
            }
        }
        Statement::Expression(expr) => {
            eval_expression(expr, env)
        }
        Statement::Empty => Ok(Value::Undefined),
        Statement::Break(_) => {
            set_control_flow(ControlFlow::Break);
            Ok(Value::Undefined)
        }
        Statement::Continue(_) => {
            set_control_flow(ControlFlow::Continue);
            Ok(Value::Undefined)
        }
        Statement::TryCatch { body, param, handler } => {
            eval_try_catch(body, param, handler, env)
        }
        Statement::Throw(expr) => {
            let msg = to_js_string(&eval_expression(expr, env)?);
            Err(JsError(msg))
        }
        Statement::Export(stmt) => {
            // Export statements wrap other statements (like assignments)
            eval_statement(stmt, env, _is_expr_body)
        }
    }
}

fn eval_var_decl(
    kind: &VarKind,
    name: &str,
    init: &Option<Expression>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let already_declared = *kind == VarKind::Var && env.borrow().has(name);
    if !already_declared {
        env.borrow_mut().declare_var(name.to_string(), *kind);
    }
    let value = if let Some(expr) = init {
        eval_expression(expr, env)?
    } else {
        Value::Undefined
    };
    env.borrow_mut().initialize_declared(name, value);
    Ok(Value::Undefined)
}

fn eval_func_decl(
    name: &str,
    params: &[String],
    body: &[Statement],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let func = crate::value::ValueFunction::new(
        Some(name.to_owned()),
        params.to_vec(),
        body.to_vec(),
        Rc::clone(env),
    );
    env.borrow_mut()
        .define(name.to_owned(), Value::Function(func));
    Ok(Value::Undefined)
}

fn eval_class_decl(
    name: &str,
    class: &Class,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Evaluate the class expression
    let class_val = eval_expression(&Expression::Class(class.clone()), env)?;
    env.borrow_mut().define(name.to_owned(), class_val);
    Ok(Value::Undefined)
}

fn eval_while(
    condition: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let mut last = Value::Undefined;
    let mut iter_count = 0;
    while to_bool(&eval_expression(condition, env)?) {
        iter_count += 1;
        if iter_count > 10 {
            return Err(JsError("while loop ran too many times".to_string()));
        }
        let _ = take_control_flow();
        last = eval_statement(body, env, false)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    Ok(last)
}

fn eval_for(
    init: &Option<ForInit>,
    condition: &Option<Box<Expression>>,
    update: &Option<Box<Expression>>,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(for_init) = init {
        match for_init {
            ForInit::Expression(expr) => {
                let _ = eval_expression(expr, env)?;
            }
            ForInit::VarDeclaration { kind, name, init } => {
                env.borrow_mut().declare_var(name.to_string(), *kind);
                let value = init
                    .as_ref()
                    .map(|e| eval_expression(e, env))
                    .unwrap_or(Ok(Value::Undefined))?;
                env.borrow_mut().initialize_declared(name, value);
            }
        }
    }
    let check_condition = || -> bool {
        if let Some(c) = condition.as_ref() {
            to_bool(&eval_expression(c, env).unwrap_or(Value::Undefined))
        } else {
            true
        }
    };
    while check_condition() {
        take_control_flow();
        let _ = eval_statement(body, env, false)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => {}
            None => {}
        }
        if let Some(update) = update {
            let _ = eval_expression(update, env)?;
        }
    }
    Ok(Value::Undefined)
}

fn eval_block(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    env.borrow_mut().push_scope();
    predeclare_let_const(stmts, &mut env.borrow_mut());
    let result = eval_statements(stmts, env, false);
    env.borrow_mut().pop_scope();
    result
}

fn eval_try_catch(
    body: &Statement,
    param: &Option<String>,
    handler: &Statement,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match eval_statement(body, env, false) {
        Ok(v) => Ok(v),
        Err(e) => {
            if let Some(name) = param {
                env.borrow_mut()
                    .define(name.to_string(), Value::String(e.to_string()));
            }
            eval_statement(handler, env, false)
        }
    }
}
