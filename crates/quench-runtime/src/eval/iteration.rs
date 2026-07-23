//! Iteration support for for-of/for-in loops

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::{Expression, Statement};
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::object::{assign_to, call_iterator_return, obtain_iterator, take_iterator_step};
use crate::eval::statement::eval_statement;
use crate::interpreter::{set_control_flow, take_control_flow, ControlFlow};
use crate::value::{JsError, Object, ObjectKind, Value};

/// Get an iterator for for-of/for-in loops (materialized; spread/destructuring).
pub fn get_iterator(value: &Value) -> Result<Vec<Value>, JsError> {
    match value {
        Value::Object(o) => get_object_iterator(o),
        Value::String(s) => get_string_iterator(s),
        Value::Generator(gen) => get_generator_values(gen),
        _ => Err(JsError("TypeError: Value is not iterable".to_string())),
    }
}

fn get_generator_values(
    gen: &Rc<RefCell<crate::value::GeneratorObject>>,
) -> Result<Vec<Value>, JsError> {
    let mut values = Vec::new();
    let mut g = gen.borrow_mut();
    loop {
        let result = g.next(Value::Undefined)?;
        if result.done {
            break;
        }
        values.push(result.value);
    }
    Ok(values)
}

fn get_object_iterator(o: &Rc<RefCell<Object>>) -> Result<Vec<Value>, JsError> {
    if o.borrow().kind == ObjectKind::Array {
        return Ok(o.borrow().elements.clone());
    }
    let env = Rc::new(RefCell::new(Environment::new()));
    let iterator = obtain_iterator(o)?;
    let mut index = 0usize;
    let mut items = Vec::new();
    loop {
        let (item, done) = take_iterator_step(&iterator, &mut index, &env)?;
        if done {
            break;
        }
        items.push(item);
    }
    Ok(items)
}

fn get_string_iterator(s: &str) -> Result<Vec<Value>, JsError> {
    Ok(s.chars().map(|c| Value::String(c.to_string())).collect())
}

/// Get enumerable property keys for for-in loop
pub fn get_enumerable_keys(value: &Value) -> Result<Vec<String>, JsError> {
    match value {
        Value::Object(o) => get_object_keys(o),
        Value::String(s) => Ok((0..s.len()).map(|i| i.to_string()).collect()),
        _ => Ok(vec![]),
    }
}

fn get_object_keys(o: &Rc<RefCell<Object>>) -> Result<Vec<String>, JsError> {
    let obj = o.borrow();
    let mut keys = obj.own_keys();
    for i in 0..obj.elements.len() {
        let key = i.to_string();
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    Ok(keys)
}

fn abrupt_close(
    iterator: &Rc<RefCell<Object>>,
    completion: Result<Value, JsError>,
) -> Result<Value, JsError> {
    if let Some(close_err) = call_iterator_return(iterator) {
        return Err(close_err);
    }
    completion
}

fn eval_for_of_iterator(
    iterator: Rc<RefCell<Object>>,
    variable: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let mut index = 0usize;
    loop {
        let (item, done) = take_iterator_step(&iterator, &mut index, env)?;
        if done {
            break;
        }
        assign_to(variable, &item, env)?;
        let body_result = eval_statement(body, env, false, in_arrow_function);
        if let Err(e) = body_result {
            return abrupt_close(&iterator, Err(e));
        }
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Return(val))
            | Some(ControlFlow::Yield(val))
            | Some(ControlFlow::YieldDelegate(val)) => {
                return abrupt_close(&iterator, {
                    set_control_flow(ControlFlow::Return(val.clone()));
                    Ok(val)
                });
            }
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    if let Some(ControlFlow::Return(val))
    | Some(ControlFlow::Yield(val))
    | Some(ControlFlow::YieldDelegate(val)) = take_control_flow()
    {
        Ok(val)
    } else {
        Ok(Value::Undefined)
    }
}

/// Evaluate a for-of loop
pub fn eval_for_of(
    variable: &Expression,
    iterable: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let iter_value = eval_expression(iterable, env, in_arrow_function)?;
    let iterator = match &iter_value {
        Value::String(s) => {
            let items: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
            let arr = Object::new_array_from(items);
            Rc::new(RefCell::new(arr))
        }
        Value::Generator(gen) => {
            crate::value::generator::generator_as_iterator_object(Rc::clone(gen))
        }
        Value::Object(o) => obtain_iterator(o)?,
        _ => return Err(JsError("TypeError: Value is not iterable".to_string())),
    };
    eval_for_of_iterator(iterator, variable, body, env, in_arrow_function)
}

/// Evaluate a for-in loop
pub fn eval_for_in(
    variable: &Expression,
    object: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let obj_value = eval_expression(object, env, in_arrow_function)?;
    let keys = get_enumerable_keys(&obj_value)?;
    for key in keys {
        assign_to(variable, &Value::String(key), env)?;
        let _ = eval_statement(body, env, false, in_arrow_function)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Return(val))
            | Some(ControlFlow::Yield(val))
            | Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    if let Some(ControlFlow::Return(val))
    | Some(ControlFlow::Yield(val))
    | Some(ControlFlow::YieldDelegate(val)) = take_control_flow()
    {
        Ok(val)
    } else {
        Ok(Value::Undefined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins;
    use crate::context::Context;
    use crate::value::Value;

    fn new_ctx() -> Context {
        let mut ctx = Context::new().unwrap();
        builtins::register_builtins(&mut ctx);
        ctx
    }

    #[test]
    fn test_get_iterator_array() {
        let mut ctx = new_ctx();
        let arr = ctx.eval("[10, 20, 30]").unwrap();
        let items = get_iterator(&arr).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], Value::Number(10.0));
    }

    #[test]
    fn test_for_of_array_sum() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let s = 0; for (let x of [1, 2, 3]) { s += x; } s")
            .unwrap();
        assert_eq!(result, Value::Number(6.0));
    }

    #[test]
    fn test_for_of_return_closes_iterator() {
        let mut ctx = new_ctx();
        let result = ctx.eval(
            "class E extends Error {} \
             var error = new E(); \
             var iter = { \
               [Symbol.iterator]() { return this; }, \
               next() { return { done: false }; }, \
               return() { throw error; } \
             }; \
             class C extends class {} { \
               constructor() { \
                 super(); \
                 for (var k of iter) { return 0; } \
               } \
             }; \
             var threw = false; \
             try { new C(); } catch (e) { threw = (e instanceof E); } \
             threw",
        );
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_for_of_non_iterable_throws() {
        let mut ctx = new_ctx();
        let result = ctx.eval("let s = 0; for (let x of 42) { s += x; }");
        assert!(result.is_err());
    }
}
