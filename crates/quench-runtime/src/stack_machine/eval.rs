//! Expression evaluation for the stack machine.

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
        Expression::Number(n) => machine.current_frame().values.push(Value::Number(*n)),
        Expression::String(s) => machine.current_frame().values.push(Value::String(s.clone())),
        Expression::Boolean(b) => machine.current_frame().values.push(Value::Boolean(*b)),
        Expression::Null => machine.current_frame().values.push(Value::Null),
        Expression::Undefined => machine.current_frame().values.push(Value::Undefined),
        Expression::Identifier(name) => eval_identifier(machine, name)?,
        Expression::Object(props) => eval_object(machine, props)?,
        Expression::Array(elements) => eval_array(machine, elements)?,
        Expression::FunctionExpression { name, params, body } => {
            let func = ValueFunction::new(
                name.clone(),
                params.clone(),
                body.clone(),
                Rc::clone(&machine.current_frame().env),
            );
            machine.current_frame().values.push(Value::Function(func));
        }
        Expression::ArrowFunction { params, body } => {
            let func = ValueFunction::new_arrow(
                params.clone(),
                body.clone(),
                Rc::clone(&machine.current_frame().env),
            );
            machine.current_frame().values.push(Value::Function(func));
        }
        Expression::Binary { op, left, right } => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyBinary(*op));
            frame.work.push(Work::EvalExpr(Rc::new((**right).clone())));
            frame.work.push(Work::EvalExpr(Rc::new((**left).clone())));
        }
        Expression::Unary { op, argument } => {
            // typeof on an undeclared identifier must not throw.
            if *op == UnaryOp::Typeof {
                if let Expression::Identifier(name) = argument.as_ref() {
                    if name != "this" && !machine.current_frame().env.borrow().has(name) {
                        machine.current_frame().values.push(Value::String("undefined".to_string()));
                        return Ok(());
                    }
                }
            }
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyUnary(*op));
            frame.work.push(Work::EvalExpr(Rc::new((**argument).clone())));
        }
        Expression::Assignment { left, right } => eval_assignment(machine, left, Rc::new((**right).clone()))?,
        Expression::CompoundAssignment { op, left, right } => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyCompoundAssign { op: op.to_binary(), target: AssignmentTarget::Identifier(String::new()) });
            frame.work.push(Work::EvalExpr(Rc::new((**left).clone())));
            frame.work.push(Work::EvalExpr(Rc::new((**right).clone())));
        }
        Expression::Call { callee, arguments } => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyCall { argc: arguments.len() });
            for arg in arguments.iter().rev() {
                frame.work.push(Work::EvalExpr(Rc::new(arg.clone())));
            }
            frame.work.push(Work::EvalCallee(Rc::new((**callee).clone())));
        }
        Expression::Member { object, property, computed } => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyMember { property: property.clone(), computed: *computed, callee_mode: false });
            if *computed {
                if let PropertyKey::Computed(key_expr) = property {
                    frame.work.push(Work::EvalExpr(Rc::new((**key_expr).clone())));
                } else {
                    return Err(JsError("Invalid computed property".to_string()));
                }
            }
            frame.work.push(Work::EvalExpr(Rc::new((**object).clone())));
        }
        Expression::Conditional { condition, consequent, alternate } => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyConditional { consequent: Rc::new((**consequent).clone()), alternate: Rc::new((**alternate).clone()) });
            frame.work.push(Work::EvalExpr(Rc::new((**condition).clone())));
        }
        Expression::Update { op, argument, prefix } => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyUpdate { op: *op, prefix: *prefix, target: AssignmentTarget::Identifier(String::new()) });
            frame.work.push(Work::EvalExpr(Rc::new((**argument).clone())));
        }
        Expression::New { constructor, arguments } => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyNew { argc: arguments.len() });
            for arg in arguments.iter().rev() {
                frame.work.push(Work::EvalExpr(Rc::new(arg.clone())));
            }
            frame.work.push(Work::EvalExpr(Rc::new((**constructor).clone())));
        }
        Expression::Sequence(exprs) => {
            if exprs.is_empty() {
                machine.current_frame().values.push(Value::Undefined);
            } else {
                machine.current_frame().work.push(Work::ApplySequence { exprs: Rc::new(exprs.clone()), index: 0 });
            }
        }
        Expression::BlockExpr(stmts) => {
            if stmts.is_empty() {
                machine.current_frame().values.push(Value::Undefined);
            } else {
                machine.current_frame().work.push(Work::ApplyBlockExpr { stmts: Rc::new(stmts.clone()), index: 0 });
            }
        }
        Expression::ForOf { variable, iterable, body } => {
            let frame = machine.current_frame();
            frame.work.push(Work::BeginForOf { variable: Rc::new((**variable).clone()), body: Rc::new((**body).clone()) });
            frame.work.push(Work::EvalExpr(Rc::new((**iterable).clone())));
        }
        Expression::ForIn { variable, object, body } => {
            let frame = machine.current_frame();
            frame.work.push(Work::BeginForIn { variable: Rc::new((**variable).clone()), body: Rc::new((**body).clone()) });
            frame.work.push(Work::EvalExpr(Rc::new((**object).clone())));
        }
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
    }
    Ok(())
}
