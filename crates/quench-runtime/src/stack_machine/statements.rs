//! Statement evaluation for the stack machine.

use std::rc::Rc;
use std::cell::RefCell;

use crate::ast::*;
use crate::value::{Value, JsError, ValueFunction};
use crate::env::Environment;
use crate::interpreter as hir;
use crate::stack_machine::{Machine, Work, ForPhase};



/// Evaluate a single statement.
pub fn eval_stmt(machine: &mut Machine, stmt: Rc<Statement>, is_expr_body: bool) -> Result<(), JsError> {
    match &*stmt {
        Statement::VarDeclaration { kind, name, init } => {
            let env = Rc::clone(&machine.current_frame().env);
            let already_declared = *kind == VarKind::Var && env.borrow().has(name);
            if !already_declared {
                env.borrow_mut().declare_var(name.clone(), *kind);
            }
            if let Some(init_expr) = init {
                let frame = machine.current_frame();
                frame.work.push(Work::VarDecl { kind: *kind, name: name.clone() });
                frame.work.push(Work::EvalExpr(Rc::new(init_expr.clone())));
            } else {
                env.borrow_mut().initialize_declared(name, Value::Undefined);
                machine.current_frame().values.push(Value::Undefined);
            }
        }
        Statement::FunctionDeclaration { name, params, body } => {
            let func = ValueFunction::new(
                Some(name.clone()),
                params.clone(),
                body.clone(),
                Rc::clone(&machine.current_frame().env),
            );
            machine.current_frame().env.borrow_mut().define(name.clone(), Value::Function(func));
            machine.current_frame().values.push(Value::Undefined);
        }
        Statement::ClassDeclaration { name: _, class: _ } => {
            // Class declarations need recursive interpreter for full class semantics
            return Err(JsError("Class declarations must be evaluated with the recursive interpreter".to_string()));
        }
        Statement::If { condition, consequent, alternate } => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyIf { consequent: Rc::new((**consequent).clone()), alternate: alternate.as_deref().map(|s| Rc::new(s.clone())), is_expr_body });
            frame.work.push(Work::EvalExpr(Rc::new((**condition).clone())));
        }
        Statement::While { condition, body } => {
            machine.current_frame().work.push(Work::ApplyWhile { condition: Rc::new((**condition).clone()), body: Rc::new((**body).clone()), is_expr_body });
        }
        Statement::For { init, condition, update, body } => {
            machine.current_frame().work.push(Work::ApplyFor {
                condition: condition.as_deref().map(|e| Rc::new(e.clone())),
                update: update.as_deref().map(|e| Rc::new(e.clone())),
                body: Rc::new((**body).clone()),
                is_expr_body,
                phase: ForPhase::Init,
            });
            if let Some(for_init) = init {
                match for_init {
                    ForInit::Expression(expr) => {
                        machine.current_frame().work.push(Work::EvalExpr(Rc::new((**expr).clone())));
                        machine.current_frame().work.push(Work::Discard);
                    }
                    ForInit::VarDeclaration { kind, name, init: init_expr } => {
                        machine.current_frame().env.borrow_mut().declare_var(name.clone(), *kind);
                        if let Some(init_expr) = init_expr {
                            machine.current_frame().work.push(Work::ForInitVar { kind: *kind, name: name.clone() });
                            machine.current_frame().work.push(Work::EvalExpr(Rc::new(init_expr.clone())));
                        } else {
                            machine.current_frame().env.borrow_mut().initialize_declared(name, Value::Undefined);
                        }
                    }
                }
            }
        }
        Statement::Block(stmts) => {
            machine.current_frame().work.push(Work::ApplyBlock { stmts: Rc::new(stmts.clone()), index: 0, is_expr_body });
        }
        Statement::SequenceDecls(stmts) => {
            for stmt in stmts.iter().rev() {
                machine.current_frame().work.push(Work::EvalStmt(Rc::new(stmt.clone()), false));
            }
        }
        Statement::Return(expr) => {
            if let Some(e) = expr {
                machine.current_frame().work.push(Work::ApplyReturn);
                machine.current_frame().work.push(Work::EvalExpr(Rc::new((**e).clone())));
            } else {
                machine.current_frame().values.push(Value::Undefined);
                machine.current_frame().work.push(Work::ApplyReturn);
            }
        }
        Statement::Expression(expr) => {
            machine.current_frame().work.push(Work::EvalExpr(Rc::new((**expr).clone())));
        }
        Statement::Empty => {
            machine.current_frame().values.push(Value::Undefined);
        }
        Statement::Break(_) => {
            hir::set_control_flow(hir::ControlFlow::Break);
            machine.current_frame().values.push(Value::Undefined);
        }
        Statement::Continue(_) => {
            hir::set_control_flow(hir::ControlFlow::Continue);
            machine.current_frame().values.push(Value::Undefined);
        }
        Statement::TryCatch { body, param, handler } => {
            let frame_env = Rc::clone(&machine.current_frame().env);
            machine.current_frame().env.borrow_mut().push_scope();
            machine.current_frame().work.push(Work::PopScope);
            machine.current_frame().work.push(Work::PopCatch);
            let handler_rc = Rc::new(handler.as_ref().clone());
            machine.current_frame().work.push(Work::ApplyTryCatch { handler: Rc::clone(&handler_rc), param: param.clone(), is_expr_body });
            machine.current_frame().work.push(Work::EvalStmt(Rc::new(body.as_ref().clone()), is_expr_body));
            machine.current_frame().work.push(Work::PushCatch {
                handler: handler_rc,
                param: param.clone(),
                env: frame_env,
                is_expr_body,
            });
        }
        Statement::Throw(expr) => {
            machine.current_frame().work.push(Work::EvalExpr(Rc::new((**expr).clone())));
            machine.current_frame().work.push(Work::Throw);
        }
        Statement::Export(stmt) => {
            // Export statements just evaluate their wrapped statement
            machine.current_frame().work.push(Work::EvalStmt(Rc::new((**stmt).clone()), is_expr_body));
        }
    }
    Ok(())
}

/// Evaluate a list of statements.
pub fn eval_stmts(
    machine: &mut Machine,
    stmts: &Rc<Vec<Statement>>,
    is_expr_body: bool,
    index: usize,
) -> Result<(), JsError> {
    let slice: &[Statement] = stmts;
    if index >= slice.len() {
        if machine.current_frame().values.is_empty() {
            machine.current_frame().values.push(Value::Undefined);
        }
        return Ok(());
    }
    if index + 1 == slice.len() {
        machine.current_frame().work.push(Work::EvalStmt(Rc::new(slice[index].clone()), is_expr_body));
    } else {
        machine.current_frame().work.push(Work::EvalStmts(Rc::clone(stmts), is_expr_body, index + 1));
        machine.current_frame().work.push(Work::Discard);
        machine.current_frame().work.push(Work::EvalStmt(Rc::new(slice[index].clone()), is_expr_body));
    }
    Ok(())
}

/// Apply an if statement.
pub fn apply_if(
    machine: &mut Machine,
    consequent: Rc<Statement>,
    alternate: Option<Rc<Statement>>,
    is_expr_body: bool,
) -> Result<(), JsError> {
    use crate::value::to_bool;

    let cond = machine.pop_value();
    if to_bool(&cond) {
        machine.current_frame().work.push(Work::EvalStmt(consequent, is_expr_body));
    } else if let Some(alt) = alternate {
        machine.current_frame().work.push(Work::EvalStmt(alt, is_expr_body));
    } else {
        machine.current_frame().values.push(Value::Undefined);
    }
    Ok(())
}

/// Apply a while loop.
pub fn apply_while(
    machine: &mut Machine,
    condition: Rc<Expression>,
    body: Rc<Statement>,
    is_expr_body: bool,
) -> Result<(), JsError> {
    let _ = hir::take_control_flow();
    let frame = machine.current_frame();
    frame.work.push(Work::ApplyWhileBody { condition: condition.clone(), body: body.clone(), is_expr_body });
    frame.work.push(Work::EvalExpr(condition));
    Ok(())
}

/// Apply the body of a while loop.
pub fn apply_while_body(
    machine: &mut Machine,
    condition: Rc<Expression>,
    body: Rc<Statement>,
    is_expr_body: bool,
) -> Result<(), JsError> {
    use crate::value::to_bool;

    let cond = machine.pop_value();
    if !to_bool(&cond) {
        machine.current_frame().values.push(Value::Undefined);
        return Ok(());
    }
    machine.current_frame().work.push(Work::ApplyWhile { condition: condition.clone(), body: body.clone(), is_expr_body });
    machine.current_frame().work.push(Work::EvalStmt(body, is_expr_body));
    Ok(())
}

/// Apply a for loop with a specific phase.
pub fn apply_for(
    machine: &mut Machine,
    condition: Option<Rc<Expression>>,
    update: Option<Rc<Expression>>,
    body: Rc<Statement>,
    is_expr_body: bool,
    phase: ForPhase,
) -> Result<(), JsError> {
    match phase {
        ForPhase::Init => {
            machine.current_frame().work.push(Work::ApplyFor {
                condition: condition.clone(),
                update: update.clone(),
                body: body.clone(),
                is_expr_body,
                phase: ForPhase::Check,
            });
        }
        ForPhase::Check => {
            let _ = hir::take_control_flow();
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyForBody { condition: condition.clone(), update: update.clone(), body: body.clone(), is_expr_body });
            if let Some(c) = &condition {
                frame.work.push(Work::EvalExpr(c.clone()));
            } else {
                frame.values.push(Value::Boolean(true));
            }
        }
        ForPhase::Update => {
            let cf = hir::take_control_flow();
            match cf {
                Some(hir::ControlFlow::Break) => {
                    machine.current_frame().values.push(Value::Undefined);
                }
                _ => {
                    if let Some(u) = &update {
                        machine.current_frame().work.push(Work::Discard);
                        machine.current_frame().work.push(Work::EvalExpr(u.clone()));
                    }
                    machine.current_frame().work.push(Work::ApplyFor {
                        condition: condition.clone(),
                        update: update.clone(),
                        body: body.clone(),
                        is_expr_body,
                        phase: ForPhase::Check,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Apply the body of a for loop.
pub fn apply_for_body(
    machine: &mut Machine,
    condition: Option<Rc<Expression>>,
    update: Option<Rc<Expression>>,
    body: Rc<Statement>,
    is_expr_body: bool,
) -> Result<(), JsError> {
    use crate::value::to_bool;

    let cond = machine.pop_value();
    if !to_bool(&cond) {
        machine.current_frame().values.push(Value::Undefined);
        return Ok(());
    }
    machine.current_frame().work.push(Work::ApplyFor {
        condition: condition.clone(),
        update: update.clone(),
        body: body.clone(),
        is_expr_body,
        phase: ForPhase::Update,
    });
    machine.current_frame().work.push(Work::EvalStmt(body, is_expr_body));
    Ok(())
}

/// Apply a block statement.
pub fn apply_block(
    machine: &mut Machine,
    stmts: &Rc<Vec<Statement>>,
    index: usize,
    is_expr_body: bool,
) -> Result<(), JsError> {
    let slice: &[Statement] = stmts;
    if index == 0 {
        machine.current_frame().env.borrow_mut().push_scope();
        hir::predeclare_let_const(slice, &mut machine.current_frame().env.borrow_mut());
        machine.current_frame().work.push(Work::PopScope);
    }
    if index >= slice.len() {
        if machine.current_frame().values.is_empty() {
            machine.current_frame().values.push(Value::Undefined);
        }
        return Ok(());
    }
    if index + 1 == slice.len() {
        machine.current_frame().work.push(Work::EvalStmt(Rc::new(slice[index].clone()), is_expr_body));
    } else {
        machine.current_frame().work.push(Work::ApplyBlock {
            stmts: stmts.clone(),
            index: index + 1,
            is_expr_body,
        });
        machine.current_frame().work.push(Work::Discard);
        machine.current_frame().work.push(Work::EvalStmt(Rc::new(slice[index].clone()), is_expr_body));
    }
    Ok(())
}

/// Apply a try-catch block.
pub fn apply_try_catch(
    _machine: &mut Machine,
    _handler: Rc<Statement>,
    _param: Option<String>,
    _is_expr_body: bool,
) -> Result<(), JsError> {
    Ok(())
}

/// Apply a return statement.
pub fn apply_return(machine: &mut Machine) -> Result<(), JsError> {
    let value = machine.pop_value();
    machine.frames.pop();
    if let Some(caller) = machine.frames.last_mut() {
        caller.values.push(value);
    } else {
        machine.frames.push(Machine::new(Rc::new(RefCell::new(Environment::new()))).frames.pop().unwrap());
        if let Some(caller) = machine.frames.last_mut() {
            caller.values.push(value);
        }
    }
    Ok(())
}
