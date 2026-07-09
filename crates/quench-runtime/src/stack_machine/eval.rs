//! Expression evaluation for the stack machine.

#![allow(clippy::borrowed_box)] // Box<T> patterns are intentional for AST memory efficiency

use std::rc::Rc;
use std::cell::RefCell;

use crate::ast::*;
use crate::value::{Value, JsError, Object, ObjectKind, ValueFunction};

use crate::interpreter as hir;
use crate::stack_machine::{Machine, Work, AssignmentTarget, ObjectPropertyKind};

use super::property::property_key_static;

/// Evaluate an identifier.
pub fn eval_identifier(machine: &mut Machine, name: &str) -> Result<(), JsError> {
    let frame_env = &machine.current_frame().env;
    let result = if name == "this" {
        hir::get_this_binding(frame_env)
    } else {
        if frame_env.borrow().is_tdz(name) {
            return Err(JsError(format!(
                "ReferenceError: Cannot access '{}' before initialization",
                name
            )));
        }
        frame_env
            .borrow()
            .get(name)
            .ok_or_else(|| JsError(format!("ReferenceError: {} is not defined", name)))?
    };
    machine.current_frame().values.push(result);
    Ok(())
}

/// Evaluate an object literal.
pub fn eval_object(machine: &mut Machine, props: &[(PropertyKey, PropertyValue)]) -> Result<(), JsError> {
    let mut obj = Object::new(ObjectKind::Ordinary);
    if let Some(prototype) = crate::builtins::get_object_prototype() {
        obj.prototype = Some(prototype);
    }
    let obj_rc = Rc::new(RefCell::new(obj));
    machine.current_frame().values.push(Value::Object(Rc::clone(&obj_rc)));

    for (key, value) in props.iter().rev() {
        let key_str = property_key_static(key)?;
        match value {
            PropertyValue::Value(expr) => {
                let frame = machine.current_frame();
                frame.work.push(Work::ApplyObjectProperty {
                    key: key_str.to_string(),
                    kind: ObjectPropertyKind::Value,
                    obj: Rc::clone(&obj_rc),
                });
                frame.work.push(Work::EvalExpr(Rc::new(expr.clone())));
            }
            PropertyValue::Getter { body, .. } => {
                machine.current_frame().work.push(Work::ApplyObjectProperty {
                    key: key_str.to_string(),
                    kind: ObjectPropertyKind::Getter,
                    obj: Rc::clone(&obj_rc),
                });
                let getter_func = ValueFunction::new(
                    None,
                    Vec::new(),
                    body.clone(),
                    Rc::clone(&machine.current_frame().env),
                );
                machine.current_frame().values.push(Value::Function(getter_func));
            }
            PropertyValue::Setter { param, body } => {
                machine.current_frame().work.push(Work::ApplyObjectProperty {
                    key: key_str.to_string(),
                    kind: ObjectPropertyKind::Setter,
                    obj: Rc::clone(&obj_rc),
                });
                let setter_func = ValueFunction::new(
                    None,
                    vec![param.clone()],
                    body.clone(),
                    Rc::clone(&machine.current_frame().env),
                );
                machine.current_frame().values.push(Value::Function(setter_func));
            }
        }
    }
    Ok(())
}

/// Evaluate an array literal.
pub fn eval_array(machine: &mut Machine, elements: &[Expression]) -> Result<(), JsError> {
    let arr = Object::new_array(elements.len());
    let arr_rc = Rc::new(RefCell::new(arr));
    if let Some(prototype) = crate::builtins::get_array_prototype() {
        arr_rc.borrow_mut().prototype = Some(prototype);
    }
    machine.current_frame().values.push(Value::Object(Rc::clone(&arr_rc)));

    for (i, elem) in elements.iter().enumerate().rev() {
        let frame = machine.current_frame();
        frame.work.push(Work::ApplyObjectProperty {
            key: i.to_string(),
            kind: ObjectPropertyKind::Value,
            obj: Rc::clone(&arr_rc),
        });
        frame.work.push(Work::EvalExpr(Rc::new(elem.clone())));
    }
    Ok(())
}

/// Evaluate an assignment expression.
pub fn eval_assignment(machine: &mut Machine, left: &Expression, right: Rc<Expression>) -> Result<(), JsError> {
    match left {
        Expression::Identifier(name) => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyAssign { target: AssignmentTarget::Identifier(name.clone()) });
            frame.work.push(Work::EvalExpr(right));
        }
        Expression::Member { object, property, computed } => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyMemberAssign);
            if *computed {
                if let PropertyKey::Computed(key_expr) = property {
                    frame.work.push(Work::EvalExpr(Rc::new((**key_expr).clone())));
                } else {
                    return Err(JsError("Invalid computed property".to_string()));
                }
            } else {
                let key = property_key_static(property)?;
                frame.work.push(Work::PushValue(Value::String(key.to_string())));
            }
            frame.work.push(Work::EvalExpr(Rc::new((**object).clone())));
            frame.work.push(Work::EvalExpr(right));
        }
        _ => return Err(JsError("Invalid assignment target".to_string())),
    }
    Ok(())
}

/// Evaluate a full expression and push result to stack.
pub fn eval_expr(machine: &mut Machine, expr: Rc<Expression>) -> Result<(), JsError> {
    match &*expr {
        // Literals
        Expression::Number(n) => machine.current_frame().values.push(Value::Number(*n)),
        Expression::String(s) => machine.current_frame().values.push(Value::String(s.clone())),
        Expression::Boolean(b) => machine.current_frame().values.push(Value::Boolean(*b)),
        Expression::Null => machine.current_frame().values.push(Value::Null),
        Expression::Undefined => machine.current_frame().values.push(Value::Undefined),
        // Identifier, object, array
        Expression::Identifier(name) => eval_identifier(machine, name)?,
        Expression::Object(props) => eval_object(machine, props)?,
        Expression::Array(elements) => eval_array(machine, elements)?,
        // Function expressions
        Expression::FunctionExpression { name, params, body } => eval_function_expr(machine, name, params, body)?,
        Expression::ArrowFunction { params, body } => eval_arrow_function(machine, params, body)?,
        // Binary/unary operations
        Expression::Binary { op, left, right } => eval_binary_expr(machine, *op, left, right)?,
        Expression::Unary { op, argument } => eval_unary_expr(machine, *op, argument)?,
        // Assignment
        Expression::Assignment { left, right } => eval_assignment(machine, left, Rc::new((**right).clone()))?,
        Expression::CompoundAssignment { op, left, right } => eval_compound_assign(machine, op, left, right)?,
        // Call/member access
        Expression::Call { callee, arguments } => eval_call_expr(machine, callee, arguments)?,
        Expression::Member { object, property, computed } => eval_member_expr(machine, object, property, *computed)?,
        // Other expressions
        Expression::Conditional { condition, consequent, alternate } => eval_conditional_expr(machine, condition, consequent, alternate)?,
        Expression::Update { op, argument, prefix } => eval_update_expr(machine, *op, *prefix, argument)?,
        Expression::New { constructor, arguments } => eval_new_expr(machine, constructor, arguments)?,
        Expression::Sequence(exprs) => eval_sequence_expr(machine, exprs)?,
        Expression::BlockExpr(stmts) => eval_block_expr(machine, stmts)?,
        Expression::ForOf { variable, iterable, body } => eval_for_of_expr(machine, variable, iterable, body)?,
        Expression::ForIn { variable, object, body } => eval_for_in_expr(machine, variable, object, body)?,
        // Errors
        Expression::ArrayPattern(_) | Expression::ObjectPattern(_) => {
            return Err(JsError("Array/Object pattern must be used in assignment context".to_string()));
        }
        Expression::OptChain { .. } | Expression::OptChainCall { .. } => {
            return Err(JsError("Internal error: optional chaining not lowered".to_string()));
        }
        Expression::JsxElement { .. } => {
            return Err(JsError("JSX elements must be evaluated with the recursive interpreter".to_string()));
        }
        Expression::JsxFragment { .. } => {
            return Err(JsError("JSX fragments must be evaluated with the recursive interpreter".to_string()));
        }
        Expression::Class(_class) => {
            // Class expressions are evaluated using the recursive interpreter
            // Fall back to the recursive interpreter for class support
            return Err(JsError("Class expressions must be evaluated with the recursive interpreter".to_string()));
        }
        Expression::Spread(_) => {
            return Err(JsError("Spread must be used inside an array literal context".to_string()));
        }
    }
    Ok(())
}

/// Evaluate a function expression.
fn eval_function_expr(machine: &mut Machine, name: &Option<String>, params: &[String], body: &[Statement]) -> Result<(), JsError> {
    let func = ValueFunction::new(
        name.clone(),
        params.to_vec(),
        body.to_vec(),
        Rc::clone(&machine.current_frame().env),
    );
    machine.current_frame().values.push(Value::Function(func));
    Ok(())
}

/// Evaluate an arrow function expression.
fn eval_arrow_function(machine: &mut Machine, params: &[String], body: &Box<ArrowBody>) -> Result<(), JsError> {
    let func = ValueFunction::new_arrow(
        params.to_vec(),
        body.clone(),
        Rc::clone(&machine.current_frame().env),
    );
    machine.current_frame().values.push(Value::Function(func));
    Ok(())
}

/// Evaluate a binary expression.
fn eval_binary_expr(machine: &mut Machine, op: BinaryOp, left: &Box<Expression>, right: &Box<Expression>) -> Result<(), JsError> {
    let frame = machine.current_frame();
    frame.work.push(Work::ApplyBinary(op));
    frame.work.push(Work::EvalExpr(Rc::new((**right).clone())));
    frame.work.push(Work::EvalExpr(Rc::new((**left).clone())));
    Ok(())
}

/// Evaluate a unary expression (handles special typeof behavior).
fn eval_unary_expr(machine: &mut Machine, op: UnaryOp, argument: &Box<Expression>) -> Result<(), JsError> {
    // typeof on an undeclared identifier must not throw.
    if op == UnaryOp::Typeof {
        if let Expression::Identifier(name) = argument.as_ref() {
            if name != "this" && !machine.current_frame().env.borrow().has(name) {
                machine.current_frame().values.push(Value::String("undefined".to_string()));
                return Ok(());
            }
        }
    }
    // Handle delete specially - needs object reference and property name
    if op == UnaryOp::Delete {
        return eval_delete_expr(machine, argument);
    }
    let frame = machine.current_frame();
    frame.work.push(Work::ApplyUnary(op));
    frame.work.push(Work::EvalExpr(Rc::new((**argument).clone())));
    Ok(())
}

/// Evaluate a delete expression in the stack machine.
fn eval_delete_expr(machine: &mut Machine, argument: &Box<Expression>) -> Result<(), JsError> {
    match argument.as_ref() {
        Expression::Member { object, property, computed } => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyDeleteMember {
                property: property.clone(),
                computed: *computed,
            });
            // For computed property, we need to evaluate it and push the result
            // For non-computed, we just use the static property name
            if *computed {
                match property {
                    PropertyKey::Computed(expr) => {
                        frame.work.push(Work::EvalExpr(Rc::new((**expr).clone())));
                    }
                    _ => { /* should not happen */ }
                }
            }
            // Evaluate object expression
            frame.work.push(Work::EvalExpr(Rc::new((**object).clone())));
            Ok(())
        }
        Expression::Identifier(name) => {
            // In JavaScript, delete on unqualified identifiers only works for var declarations
            // For simplicity, we just return true without actually removing
            if machine.current_frame().env.borrow().has(name) {
                // In a real implementation, we'd need to handle this properly
                // For now, just acknowledge the variable exists
            }
            machine.current_frame().values.push(Value::Boolean(true));
            Ok(())
        }
        _ => {
            machine.current_frame().values.push(Value::Boolean(false));
            Ok(())
        }
    }
}

/// Evaluate a compound assignment expression (e.g., +=, -=).
fn eval_compound_assign(machine: &mut Machine, op: &CompoundOp, left: &Box<Expression>, right: &Box<Expression>) -> Result<(), JsError> {
    let frame = machine.current_frame();
    frame.work.push(Work::ApplyCompoundAssign { op: op.to_binary(), target: AssignmentTarget::Identifier(String::new()) });
    frame.work.push(Work::EvalExpr(Rc::new((**left).clone())));
    frame.work.push(Work::EvalExpr(Rc::new((**right).clone())));
    Ok(())
}

/// Evaluate a call expression.
fn eval_call_expr(machine: &mut Machine, callee: &Box<Expression>, arguments: &[Expression]) -> Result<(), JsError> {
    let frame = machine.current_frame();
    frame.work.push(Work::ApplyCall { argc: arguments.len() });
    for arg in arguments.iter().rev() {
        frame.work.push(Work::EvalExpr(Rc::new(arg.clone())));
    }
    frame.work.push(Work::EvalCallee(Rc::new((**callee).clone())));
    Ok(())
}

/// Evaluate a member access expression.
fn eval_member_expr(machine: &mut Machine, object: &Box<Expression>, property: &PropertyKey, computed: bool) -> Result<(), JsError> {
    let frame = machine.current_frame();
    frame.work.push(Work::ApplyMember { property: property.clone(), computed, callee_mode: false });
    if computed {
        if let PropertyKey::Computed(key_expr) = property {
            frame.work.push(Work::EvalExpr(Rc::new((**key_expr).clone())));
        } else {
            return Err(JsError("Invalid computed property".to_string()));
        }
    }
    frame.work.push(Work::EvalExpr(Rc::new((**object).clone())));
    Ok(())
}

/// Evaluate a conditional expression.
fn eval_conditional_expr(machine: &mut Machine, condition: &Box<Expression>, consequent: &Box<Expression>, alternate: &Box<Expression>) -> Result<(), JsError> {
    let frame = machine.current_frame();
    frame.work.push(Work::ApplyConditional { consequent: Rc::new((**consequent).clone()), alternate: Rc::new((**alternate).clone()) });
    frame.work.push(Work::EvalExpr(Rc::new((**condition).clone())));
    Ok(())
}

/// Evaluate an update expression (++ or --).
fn eval_update_expr(machine: &mut Machine, op: UpdateOp, prefix: bool, argument: &Box<Expression>) -> Result<(), JsError> {
    let frame = machine.current_frame();
    frame.work.push(Work::ApplyUpdate { op, prefix, target: AssignmentTarget::Identifier(String::new()) });
    frame.work.push(Work::EvalExpr(Rc::new((**argument).clone())));
    Ok(())
}

/// Evaluate a new expression.
fn eval_new_expr(machine: &mut Machine, constructor: &Box<Expression>, arguments: &[Expression]) -> Result<(), JsError> {
    let frame = machine.current_frame();
    frame.work.push(Work::ApplyNew { argc: arguments.len() });
    for arg in arguments.iter().rev() {
        frame.work.push(Work::EvalExpr(Rc::new(arg.clone())));
    }
    frame.work.push(Work::EvalExpr(Rc::new((**constructor).clone())));
    Ok(())
}

/// Evaluate a sequence expression.
fn eval_sequence_expr(machine: &mut Machine, exprs: &[Expression]) -> Result<(), JsError> {
    if exprs.is_empty() {
        machine.current_frame().values.push(Value::Undefined);
    } else {
        machine.current_frame().work.push(Work::ApplySequence { exprs: Rc::new(exprs.to_vec()), index: 0 });
    }
    Ok(())
}

/// Evaluate a block expression.
fn eval_block_expr(machine: &mut Machine, stmts: &[Statement]) -> Result<(), JsError> {
    if stmts.is_empty() {
        machine.current_frame().values.push(Value::Undefined);
    } else {
        machine.current_frame().work.push(Work::ApplyBlockExpr { stmts: Rc::new(stmts.to_vec()), index: 0 });
    }
    Ok(())
}

/// Evaluate a for-of expression.
fn eval_for_of_expr(machine: &mut Machine, variable: &Box<Expression>, iterable: &Box<Expression>, body: &Box<Statement>) -> Result<(), JsError> {
    let frame = machine.current_frame();
    frame.work.push(Work::BeginForOf { variable: Rc::new((**variable).clone()), body: Rc::new((**body).clone()) });
    frame.work.push(Work::EvalExpr(Rc::new((**iterable).clone())));
    Ok(())
}

/// Evaluate a for-in expression.
fn eval_for_in_expr(machine: &mut Machine, variable: &Box<Expression>, object: &Box<Expression>, body: &Box<Statement>) -> Result<(), JsError> {
    let frame = machine.current_frame();
    frame.work.push(Work::BeginForIn { variable: Rc::new((**variable).clone()), body: Rc::new((**body).clone()) });
    frame.work.push(Work::EvalExpr(Rc::new((**object).clone())));
    Ok(())
}
