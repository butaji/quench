//! Expression evaluation
//!
//! This module contains the main expression evaluator and helper functions
//! for evaluating different expression types.

use crate::ast::*;
use crate::builtins;
use crate::env::Environment;
use crate::eval::class::{
    eval_class_expr, get_constructor_prototype, instantiate_class_from_ast_with_env,
};
use crate::eval::function::call_value_with_this;
use crate::eval::iteration::{get_enumerable_keys, get_iterator};
use crate::eval::jsx::{eval_jsx_element, eval_jsx_fragment};
use crate::eval::member::eval_member_access;
use crate::eval::object::{assign_to, eval_callee_with_this};
use crate::eval::operators::{eval_binary_op, eval_unary_op};
use crate::eval::statement::eval_statement;
use crate::interpreter::{
    get_this_binding, predeclare_let_const, take_control_flow, ControlFlow,
};
use crate::value::{
    to_js_string, to_number, to_bool, JsError, Object, ObjectKind, Value, ValueFunction,
};
use std::cell::RefCell;
use std::rc::Rc;

pub use crate::eval::statement::eval_statements;

/// Evaluate an expression
pub fn eval_expression(
    expr: &Expression,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match expr {
        Expression::Number(n) => Ok(Value::Number(*n)),
        Expression::String(s) => Ok(Value::String(s.clone())),
        Expression::Boolean(b) => Ok(Value::Boolean(*b)),
        Expression::Null => Ok(Value::Null),
        Expression::Undefined => Ok(Value::Undefined),
        Expression::Identifier(name) => eval_identifier(name, env),
        Expression::Object(props) => eval_object_literal(props, env),
        Expression::Array(elements) => eval_array_literal(elements, env),
        Expression::FunctionExpression { name, params, body } => {
            Ok(Value::Function(ValueFunction::new(
                name.clone(),
                params.clone(),
                body.clone(),
                Rc::clone(env),
            )))
        }
        Expression::ArrowFunction { params, body } => {
            Ok(Value::Function(ValueFunction::new_arrow(
                params.clone(),
                body.clone(),
                Rc::clone(env),
            )))
        }
        Expression::Binary { op, left, right } => {
            let left_val = eval_expression(left, env)?;
            let right_val = eval_expression(right, env)?;
            eval_binary_op(*op, &left_val, &right_val)
        }
        Expression::Unary { op, argument } => eval_unary_expr(*op, argument, env),
        Expression::Assignment { left, right } => {
            let right_val = eval_expression(right, env)?;
            assign_to(left, &right_val, env)?;
            Ok(right_val)
        }
        Expression::CompoundAssignment { op, left, right } => {
            let left_val = eval_expression(left, env)?;
            let right_val = eval_expression(right, env)?;
            let result = eval_binary_op(op.to_binary(), &left_val, &right_val)?;
            assign_to(left, &result, env)?;
            Ok(result)
        }
        Expression::Call { callee, arguments } => eval_call(callee, arguments, env),
        Expression::Member { object, property, computed } => {
            eval_member(object, property, *computed, env)
        }
        Expression::Conditional { condition, consequent, alternate } => {
            if to_bool(&eval_expression(condition, env)?) {
                eval_expression(consequent, env)
            } else {
                eval_expression(alternate, env)
            }
        }
        Expression::Update { op, argument, prefix } => {
            eval_update(*op, argument, *prefix, env)
        }
        Expression::New { constructor, arguments } => {
            eval_new(constructor, arguments, env)
        }
        Expression::Sequence(exprs) => eval_sequence(exprs, env),
        Expression::BlockExpr(stmts) => eval_block_expr(stmts, env),
        Expression::ArrayPattern(_) => {
            Err(JsError("Array pattern must be used in assignment context".to_string()))
        }
        Expression::ObjectPattern(_) => {
            Err(JsError("Object pattern must be used in assignment context".to_string()))
        }
        Expression::ForOf { variable, iterable, body } => {
            eval_for_of(variable, iterable, body, env)
        }
        Expression::ForIn { variable, object, body } => {
            eval_for_in(variable, object, body, env)
        }
        Expression::OptChain { .. } | Expression::OptChainCall { .. } => {
            Err(JsError("Internal error: optional chaining not lowered".to_string()))
        }
        Expression::JsxElement { tag, props, children } => {
            eval_jsx_element(tag, props, children, env)
        }
        Expression::JsxFragment { children } => {
            eval_jsx_fragment(children, env)
        }
        Expression::Class(class) => {
            eval_class_expr(class, env)
        }
        Expression::Spread(_) => {
            Err(JsError("Spread must be used inside an array literal context".to_string()))
        }
    }
}

fn eval_identifier(name: &str, env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    if name == "this" {
        return Ok(get_this_binding(env));
    }
    if name == "super" {
        return eval_super(env);
    }
    if env.borrow().is_tdz(name) {
        return Err(JsError(format!(
            "ReferenceError: Cannot access '{}' before initialization",
            name
        )));
    }
    env.borrow()
        .get(name)
        .ok_or_else(|| JsError(format!("ReferenceError: {} is not defined", name)))
}

/// Get the super class value from the environment chain
fn get_super_from_env(env: &Rc<RefCell<Environment>>) -> Option<Value> {
    let mut current = Some(env.clone());
    while let Some(e) = current {
        if let Some(super_class) = e.borrow().get_super_class() {
            return Some(super_class);
        }
        current = e.borrow().get_parent();
    }
    None
}

fn eval_super(env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    get_super_from_env(env)
        .ok_or_else(|| JsError("ReferenceError: super is only valid in class methods".to_string()))
}

fn eval_object_literal(
    props: &[(PropertyKey, PropertyValue)],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let mut obj = Object::new(ObjectKind::Ordinary);
    if let Some(prototype) = builtins::get_object_prototype() {
        obj.prototype = Some(prototype);
    }
    for (key, value) in props {
        let key_str = eval_property_key(key, env)?;
        match value {
            PropertyValue::Value(expr) => {
                let val = eval_expression(expr, env)?;
                obj.set(&key_str, val);
            }
            PropertyValue::Getter { params: _, body } => {
                obj.set_getter(&key_str, Rc::new(body.clone()), Rc::clone(env));
            }
            PropertyValue::Setter { param, body } => {
                obj.set_setter(&key_str, param.clone(), Rc::new(body.clone()), Rc::clone(env));
            }
        }
    }
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

fn eval_property_key(key: &PropertyKey, env: &Rc<RefCell<Environment>>) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(s) => Ok(s.clone()),
        PropertyKey::String(s) => Ok(s.clone()),
        PropertyKey::Number(n) => Ok(n.to_string()),
        PropertyKey::Computed(e) => Ok(to_js_string(&eval_expression(e, env)?)),
    }
}

fn eval_array_literal(
    elements: &[Expression],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let mut arr = Object::new_array(0);
    for elem_expr in elements.iter() {
        match elem_expr {
            Expression::Spread(spread_expr) => {
                let spread_val = eval_expression(spread_expr, env)?;
                let items = get_iterator(&spread_val)?;
                for item in items {
                    let idx = arr.elements.len();
                    arr.set(&idx.to_string(), item);
                }
            }
            _ => {
                let value = eval_expression(elem_expr, env)?;
                let idx = arr.elements.len();
                arr.set(&idx.to_string(), value);
            }
        }
    }
    if let Some(prototype) = builtins::get_array_prototype() {
        arr.prototype = Some(prototype);
    }
    Ok(Value::Object(Rc::new(RefCell::new(arr))))
}

fn eval_unary_expr(
    op: UnaryOp,
    argument: &Expression,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if op == UnaryOp::Typeof {
        if let Expression::Identifier(name) = argument {
            if name != "this" && !env.borrow().has(name) {
                return Ok(Value::String("undefined".to_string()));
            }
        }
    }
    let val = eval_expression(argument, env)?;
    eval_unary_op(op, &val)
}

fn eval_call(
    callee: &Expression,
    arguments: &[Expression],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Handle super() calls specially
    if let Expression::Identifier(name) = callee {
        if name == "super" {
            return eval_super_call(arguments, env);
        }
    }
    let (func, this_val) = eval_callee_with_this(callee, env)?;
    let args: Result<Vec<Value>, _> = arguments.iter().map(|a| eval_expression(a, env)).collect();
    call_value_with_this(func, args?, this_val)
}

/// Evaluate a super() call - invokes the parent constructor with the given arguments
fn eval_super_call(
    arguments: &[Expression],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let super_val = get_super_from_env(env)
        .ok_or_else(|| JsError("ReferenceError: super is only valid in class methods".to_string()))?;
    
    let args: Vec<Value> = arguments.iter().map(|a| eval_expression(a, env)).collect::<Result<_, _>>()?;
    let this_val = get_this_binding(env);
    
    // Call the super constructor with the current 'this' binding
    match super_val {
        Value::Class(super_class) => {
            // For AST-based classes, we need to call the constructor
            // but ensure 'this' is bound correctly
            crate::eval::class::call_super_constructor(super_class, args, this_val, env)
        }
        Value::Object(o) => {
            if let Some(Value::Function(constructor)) = o.borrow().get("constructor") {
                crate::eval::function::call_value_with_this(
                    Value::Function(constructor.clone()),
                    args,
                    this_val,
                )
            } else {
                Ok(Value::Undefined)
            }
        }
        Value::NativeConstructor(nc) => {
            // For native constructors, we need special handling
            // Call with 'new' semantics but with provided 'this'
            crate::eval::function::call_value_with_this(
                Value::NativeConstructor(nc.clone()),
                args,
                this_val,
            )
        }
        _ => Err(JsError("TypeError: super is not a constructor".to_string())),
    }
}

fn eval_member(
    object: &Expression,
    property: &PropertyKey,
    _computed: bool,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let obj_val = eval_expression(object, env)?;
    let prop_name = eval_property_key(property, env)?;
    eval_member_access(&obj_val, &prop_name, env)
}

fn eval_update(
    op: UpdateOp,
    argument: &Expression,
    prefix: bool,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let current = eval_expression(argument, env)?;
    let current_num = to_number(&current);
    let new_val = match op {
        UpdateOp::Increment => current_num + 1.0,
        UpdateOp::Decrement => current_num - 1.0,
    };
    assign_to(argument, &Value::Number(new_val), env)?;
    if prefix {
        Ok(Value::Number(new_val))
    } else {
        Ok(Value::Number(current_num))
    }
}

fn eval_new(
    constructor: &Expression,
    arguments: &[Expression],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let constructor_val = eval_expression(constructor, env)?;
    let args: Result<Vec<Value>, _> = arguments.iter().map(|a| eval_expression(a, env)).collect();
    let args = args?;

    // Handle class instantiation
    if let Value::Class(class) = &constructor_val {
        return instantiate_class_from_ast_with_env(class.clone(), args, env);
    }

    let actual_constructor = match &constructor_val {
        Value::Object(o) => {
            let obj = o.borrow();
            if let Some(constructor) = obj.get("constructor") {
                constructor.clone()
            } else {
                return Err(JsError("Object is not a constructor".to_string()));
            }
        }
        other => other.clone(),
    };

    let prototype = get_constructor_prototype(&constructor_val)?;
    let new_obj = if let Some(proto) = prototype {
        Object::with_prototype(ObjectKind::Ordinary, proto)
    } else {
        Object::new(ObjectKind::Ordinary)
    };
    let new_obj_rc = Rc::new(RefCell::new(new_obj));

    let result = call_value_with_this(
        actual_constructor,
        args,
        Value::Object(Rc::clone(&new_obj_rc)),
    )?;

    let use_constructor_result = match &constructor_val {
        Value::NativeConstructor(_) => true,
        Value::Function(f) => f.body.iter().any(Statement::has_explicit_return),
        _ => false,
    };

    if use_constructor_result && matches!(result, Value::Object(_)) {
        Ok(result)
    } else {
        Ok(Value::Object(new_obj_rc))
    }
}

fn eval_sequence(exprs: &[Expression], env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    let mut last = Value::Undefined;
    for e in exprs {
        last = eval_expression(e, env)?;
    }
    Ok(last)
}

fn eval_block_expr(stmts: &[Statement], env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    let mut last = Value::Undefined;
    for stmt in stmts {
        last = eval_statement(stmt, env, false)?;
    }
    Ok(last)
}

fn eval_for_of(
    variable: &Expression,
    iterable: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let iter_value = eval_expression(iterable, env)?;
    let items = get_iterator(&iter_value)?;
    let mut last = Value::Undefined;
    for item in items {
        assign_to(variable, &item, env)?;
        last = eval_statement(body, env, false)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    Ok(last)
}

fn eval_for_in(
    variable: &Expression,
    object: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let obj_value = eval_expression(object, env)?;
    let keys = get_enumerable_keys(&obj_value)?;
    let mut last = Value::Undefined;
    for key in keys {
        assign_to(variable, &Value::String(key), env)?;
        last = eval_statement(body, env, false)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    Ok(last)
}
