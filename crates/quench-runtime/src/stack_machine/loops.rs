//! For-of and for-in loop handling for the stack machine.

use std::rc::Rc;


use crate::ast::*;
use crate::value::{Value, JsError, to_js_string};

use crate::interpreter as hir;
use crate::eval::iteration::{get_iterator, get_enumerable_keys};
use crate::stack_machine::{Machine, Work};


/// Begin a for-of loop by getting the iterator.
pub fn begin_for_of(
    machine: &mut Machine,
    variable: Rc<Expression>,
    body: Rc<Statement>,
) -> Result<(), JsError> {
    let iterable = machine.pop_value();
    let items = get_iterator(&iterable)?;
    if items.is_empty() {
        machine.current_frame().values.push(Value::Undefined);
    } else {
        machine.current_frame().work.push(Work::ApplyForOf { variable: variable.clone(), body: body.clone(), items, index: 0 });
    }
    Ok(())
}

/// Apply a for-of loop iteration.
pub fn apply_for_of(
    machine: &mut Machine,
    variable: Rc<Expression>,
    body: Rc<Statement>,
    items: Vec<Value>,
    index: usize,
) -> Result<(), JsError> {
    let cf = hir::take_control_flow();
    if let Some(hir::ControlFlow::Break) = cf {
        machine.current_frame().values.push(Value::Undefined);
        return Ok(());
    }
    if index >= items.len() {
        machine.current_frame().values.push(Value::Undefined);
        return Ok(());
    }
    assign_value(machine, &variable, items[index].clone())?;
    machine.current_frame().work.push(Work::ApplyForOf { variable: variable.clone(), body: body.clone(), items, index: index + 1 });
    machine.current_frame().work.push(Work::EvalStmt(body, false));
    Ok(())
}

/// Begin a for-in loop by getting the enumerable keys.
pub fn begin_for_in(
    machine: &mut Machine,
    variable: Rc<Expression>,
    body: Rc<Statement>,
) -> Result<(), JsError> {
    let obj_value = machine.pop_value();
    let keys = get_enumerable_keys(&obj_value)?;
    if keys.is_empty() {
        machine.current_frame().values.push(Value::Undefined);
    } else {
        machine.current_frame().work.push(Work::ApplyForIn { variable: variable.clone(), body: body.clone(), keys, index: 0 });
    }
    Ok(())
}

/// Apply a for-in loop iteration.
pub fn apply_for_in(
    machine: &mut Machine,
    variable: Rc<Expression>,
    body: Rc<Statement>,
    keys: Vec<String>,
    index: usize,
) -> Result<(), JsError> {
    let cf = hir::take_control_flow();
    if let Some(hir::ControlFlow::Break) = cf {
        machine.current_frame().values.push(Value::Undefined);
        return Ok(());
    }
    if index >= keys.len() {
        machine.current_frame().values.push(Value::Undefined);
        return Ok(());
    }
    assign_value(machine, &variable, Value::String(keys[index].clone()))?;
    machine.current_frame().work.push(Work::ApplyForIn { variable: variable.clone(), body: body.clone(), keys, index: index + 1 });
    machine.current_frame().work.push(Work::EvalStmt(body, false));
    Ok(())
}

/// Assign a value to a target (identifier or member expression).
pub fn assign_value(
    machine: &mut Machine,
    target: &Expression,
    value: Value,
) -> Result<(), JsError> {
    match target {
        Expression::Identifier(name) => {
            let env = Rc::clone(&machine.current_frame().env);
            if env.borrow().has(name) {
                env.borrow_mut().set(name, value);
            } else {
                env.borrow_mut().define(name.clone(), value);
            }
            Ok(())
        }
        Expression::Member { object, property, computed } => {
            let obj_val = evaluate_once(machine, object)?;
            let key = if *computed {
                let key_val = evaluate_once(machine, property.as_computed_expr()?)?;
                to_js_string(&key_val)
            } else {
                super::property::property_key_static(property)?.to_string()
            };
            if let Value::Object(obj_rc) = obj_val {
                obj_rc.borrow_mut().set(&key, value);
                Ok(())
            } else {
                Err(JsError(format!("Cannot assign to property of non-object, got {:?}", obj_val)))
            }
        }
        _ => Err(JsError("Invalid assignment target".to_string())),
    }
}

/// Evaluate a single expression on a fresh machine sharing the current env.
/// Used for side-effect-free lvalue resolution.
pub fn evaluate_once(machine: &mut Machine, expr: &Expression) -> Result<Value, JsError> {
    let env = Rc::clone(&machine.current_frame().env);
    let mut temp = Machine::new(env);
    temp.current_frame().work.push(Work::EvalExpr(Rc::new(expr.clone())));
    temp.run()
}

/// Extension trait for PropertyKey to get computed expression.
pub trait ComputedProperty {
    fn as_computed_expr(&self) -> Result<&Expression, JsError>;
}

impl ComputedProperty for PropertyKey {
    fn as_computed_expr(&self) -> Result<&Expression, JsError> {
        match self {
            PropertyKey::Computed(expr) => Ok(expr),
            _ => Err(JsError("expected computed property key".to_string())),
        }
    }
}
