//! Helper functions for expression evaluation in the stack machine.

use crate::ast::*;
use crate::value::{Value, JsError};

/// Apply a conditional expression.
pub fn apply_conditional(
    machine: &mut super::Machine,
    consequent: std::rc::Rc<Expression>,
    alternate: std::rc::Rc<Expression>,
) -> Result<(), JsError> {
    let cond = machine.pop_value();
    if crate::value::to_bool(&cond) {
        machine.current_frame().work.push(super::Work::EvalExpr(consequent));
    } else {
        machine.current_frame().work.push(super::Work::EvalExpr(alternate));
    }
    Ok(())
}

/// Apply an update expression (++/--).
pub fn apply_update(
    machine: &mut super::Machine,
    op: UpdateOp,
    prefix: bool,
    _target: super::AssignmentTarget,
) -> Result<(), JsError> {
    let current = machine.pop_value();
    let current_num = crate::value::to_number(&current);
    let new_val = match op {
        UpdateOp::Increment => current_num + 1.0,
        UpdateOp::Decrement => current_num - 1.0,
    };
    machine.current_frame().values.push(if prefix { Value::Number(new_val) } else { Value::Number(current_num) });
    Ok(())
}

/// Apply a sequence expression.
pub fn apply_sequence(
    machine: &mut super::Machine,
    exprs: &std::rc::Rc<Vec<Expression>>,
    index: usize,
) -> Result<(), JsError> {
    let slice: &[Expression] = exprs;
    if index + 1 >= slice.len() {
        machine.current_frame().work.push(super::Work::EvalExpr(std::rc::Rc::new(slice[index].clone())));
    } else {
        machine.current_frame().work.push(super::Work::ApplySequence { exprs: exprs.clone(), index: index + 1 });
        machine.current_frame().work.push(super::Work::Discard);
        machine.current_frame().work.push(super::Work::EvalExpr(std::rc::Rc::new(slice[index].clone())));
    }
    Ok(())
}

/// Apply a block expression.
pub fn apply_block_expr(
    machine: &mut super::Machine,
    stmts: &std::rc::Rc<Vec<Statement>>,
    index: usize,
) -> Result<(), JsError> {
    let slice: &[Statement] = stmts;
    if index + 1 >= slice.len() {
        machine.current_frame().work.push(super::Work::EvalStmt(std::rc::Rc::new(slice[index].clone()), false));
    } else {
        machine.current_frame().work.push(super::Work::ApplyBlockExpr { stmts: stmts.clone(), index: index + 1 });
        machine.current_frame().work.push(super::Work::Discard);
        machine.current_frame().work.push(super::Work::EvalStmt(std::rc::Rc::new(slice[index].clone()), false));
    }
    Ok(())
}
