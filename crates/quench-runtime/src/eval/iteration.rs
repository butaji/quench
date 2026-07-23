//! Iteration support for for-of/for-in loops

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::{Expression, Statement};
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::function::call_value;
use crate::eval::object::assign_to;
use crate::eval::statement::eval_statement;
use crate::interpreter::{set_control_flow, take_control_flow, ControlFlow};
use crate::value::{JsError, Object, ObjectKind, Value};

/// Get an iterator for for-of/for-in loops
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
    // Check if it's an array
    {
        let obj = o.borrow();
        if obj.kind == ObjectKind::Array {
            let mut result = Vec::new();
            for elem in &obj.elements {
                result.push(elem.clone());
            }
            return Ok(result);
        }
    }

    // Try Symbol.iterator
    {
        let obj = o.borrow();
        if let Some(Value::Object(symbol_rc)) = obj.get("Symbol") {
            if let Some(Value::Object(iter_fn)) = symbol_rc.borrow().get("iterator") {
                drop(obj);
                let result = call_value(Value::Object(Rc::clone(&iter_fn)), vec![])?;
                return get_iterator(&result);
            }
        }
    }

    // Fall back to numeric indices
    {
        let obj = o.borrow();
        let mut result = Vec::new();
        for elem in &obj.elements {
            result.push(elem.clone());
        }
        Ok(result)
    }
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

/// Evaluate a for-of loop
pub fn eval_for_of(
    variable: &Expression,
    iterable: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let iter_value = eval_expression(iterable, env, in_arrow_function)?;
    let items = get_iterator(&iter_value)?;
    for item in items {
        assign_to(variable, &item, env)?;
        let _ = eval_statement(body, env, false, in_arrow_function)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Return(val))
            | Some(ControlFlow::Yield(val))
            | Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Continue(None)) => {}
            Some(cf @ ControlFlow::Continue(Some(_))) => {
                set_control_flow(cf);
                return Ok(Value::Undefined);
            }
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
            Some(ControlFlow::Continue(None)) => {}
            Some(cf @ ControlFlow::Continue(Some(_))) => {
                set_control_flow(cf);
                return Ok(Value::Undefined);
            }
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
    use crate::value::{Object, ObjectKind, Value};

    fn new_ctx() -> Context {
        let mut ctx = Context::new().unwrap();
        builtins::register_builtins(&mut ctx);
        ctx
    }

    // get_iterator

    #[test]
    fn test_get_iterator_array() {
        let mut ctx = new_ctx();
        let arr = ctx.eval("[10, 20, 30]").unwrap();
        let items = get_iterator(&arr).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], Value::Number(10.0));
        assert_eq!(items[1], Value::Number(20.0));
        assert_eq!(items[2], Value::Number(30.0));
    }

    #[test]
    fn test_get_iterator_empty_array() {
        let mut ctx = new_ctx();
        let arr = ctx.eval("[]").unwrap();
        let items = get_iterator(&arr).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_get_iterator_generator() {
        let mut ctx = new_ctx();
        let gen = ctx.eval("(function*(){ yield 1; yield 2; })()").unwrap();
        let items = get_iterator(&gen).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], Value::Number(1.0));
        assert_eq!(items[1], Value::Number(2.0));
    }

    #[test]
    fn test_get_iterator_empty_generator() {
        let mut ctx = new_ctx();
        let gen = ctx.eval("(function*(){})()").unwrap();
        let items = get_iterator(&gen).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_get_iterator_string() {
        let items = get_iterator(&Value::String("abc".to_string())).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], Value::String("a".to_string()));
        assert_eq!(items[1], Value::String("b".to_string()));
        assert_eq!(items[2], Value::String("c".to_string()));
    }

    #[test]
    fn test_get_iterator_empty_string() {
        let items = get_iterator(&Value::String("".to_string())).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_get_iterator_unicode_string() {
        let items = get_iterator(&Value::String("aé世".to_string())).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], Value::String("a".to_string()));
        assert_eq!(items[1], Value::String("é".to_string()));
        assert_eq!(items[2], Value::String("世".to_string()));
    }

    #[test]
    fn test_get_iterator_non_iterable_number() {
        let err = get_iterator(&Value::Number(42.0)).unwrap_err();
        assert!(
            err.0.contains("TypeError"),
            "expected TypeError, got: {0:?}",
            err.0
        );
    }

    #[test]
    fn test_get_iterator_non_iterable_boolean() {
        let err = get_iterator(&Value::Boolean(true)).unwrap_err();
        assert!(err.0.contains("TypeError"));
    }

    #[test]
    fn test_get_iterator_non_iterable_null() {
        let err = get_iterator(&Value::Null).unwrap_err();
        assert!(err.0.contains("TypeError"));
    }

    #[test]
    fn test_get_iterator_non_iterable_undefined() {
        let err = get_iterator(&Value::Undefined).unwrap_err();
        assert!(err.0.contains("TypeError"));
    }

    #[test]
    fn test_get_iterator_object_with_elements() {
        let mut obj = Object::new(ObjectKind::Ordinary);
        obj.elements.push(Value::Number(1.0));
        obj.elements.push(Value::Number(2.0));
        obj.elements.push(Value::Number(3.0));
        let val = Value::Object(Rc::new(RefCell::new(obj)));
        let items = get_iterator(&val).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], Value::Number(1.0));
        assert_eq!(items[2], Value::Number(3.0));
    }

    #[test]
    fn test_get_iterator_object_empty_elements() {
        let obj = Object::new(ObjectKind::Ordinary);
        let val = Value::Object(Rc::new(RefCell::new(obj)));
        let items = get_iterator(&val).unwrap();
        assert!(items.is_empty());
    }

    // get_enumerable_keys

    #[test]
    fn test_get_enumerable_keys_object() {
        let mut obj = Object::new(ObjectKind::Ordinary);
        obj.set("x", Value::Number(1.0));
        obj.set("y", Value::Number(2.0));
        let val = Value::Object(Rc::new(RefCell::new(obj)));
        let keys = get_enumerable_keys(&val).unwrap();
        assert_eq!(keys, vec!["x", "y"]);
    }

    #[test]
    fn test_get_enumerable_keys_object_with_elements() {
        let mut obj = Object::new(ObjectKind::Ordinary);
        obj.set("a", Value::Number(10.0));
        obj.elements.push(Value::Number(1.0));
        obj.elements.push(Value::Number(2.0));
        let val = Value::Object(Rc::new(RefCell::new(obj)));
        let keys = get_enumerable_keys(&val).unwrap();
        assert!(keys.contains(&"a".to_string()));
        assert!(keys.contains(&"0".to_string()));
        assert!(keys.contains(&"1".to_string()));
    }

    #[test]
    fn test_get_enumerable_keys_string() {
        let keys = get_enumerable_keys(&Value::String("ab".to_string())).unwrap();
        assert_eq!(keys, vec!["0", "1"]);
    }

    #[test]
    fn test_get_enumerable_keys_empty_string() {
        let keys = get_enumerable_keys(&Value::String("".to_string())).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_get_enumerable_keys_number() {
        let keys = get_enumerable_keys(&Value::Number(42.0)).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_get_enumerable_keys_boolean() {
        let keys = get_enumerable_keys(&Value::Boolean(false)).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_get_enumerable_keys_undefined() {
        let keys = get_enumerable_keys(&Value::Undefined).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_get_enumerable_keys_null() {
        let keys = get_enumerable_keys(&Value::Null).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_get_enumerable_keys_empty_object() {
        let obj = Object::new(ObjectKind::Ordinary);
        let val = Value::Object(Rc::new(RefCell::new(obj)));
        let keys = get_enumerable_keys(&val).unwrap();
        assert!(keys.is_empty());
    }

    // for-of loops (via Context::eval -> eval_for_of)

    #[test]
    fn test_for_of_array_sum() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let s = 0; for (let x of [1, 2, 3, 4, 5]) { s += x; } s")
            .unwrap();
        assert_eq!(result, Value::Number(15.0));
    }

    #[test]
    fn test_for_of_string_concat() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let s = ''; for (let ch of 'abc') { s += ch; } s")
            .unwrap();
        assert_eq!(result, Value::String("abc".to_string()));
    }

    #[test]
    fn test_for_of_empty_array() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let s = 0; for (let x of []) { s += x; } s")
            .unwrap();
        assert_eq!(result, Value::Number(0.0));
    }

    #[test]
    fn test_for_of_empty_string() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let s = ''; for (let ch of '') { s += ch; } s")
            .unwrap();
        assert_eq!(result, Value::String("".to_string()));
    }

    #[test]
    fn test_for_of_break() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let s = 0; for (let x of [1, 2, 3, 4]) { s += x; if (x === 3) break; } s")
            .unwrap();
        assert_eq!(result, Value::Number(6.0));
    }

    #[test]
    fn test_for_of_continue() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let s = ''; for (let ch of 'abcde') { if (ch === 'c') continue; s += ch; } s")
            .unwrap();
        assert_eq!(result, Value::String("abde".to_string()));
    }

    #[test]
    fn test_for_of_break_immediately() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let s = 0; for (let x of [1, 2, 3]) { break; s += x; } s")
            .unwrap();
        assert_eq!(result, Value::Number(0.0));
    }

    #[test]
    fn test_for_of_single_element_array() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let s = 0; for (let x of [42]) { s += x; } s")
            .unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    // for-in loops (via Context::eval -> eval_for_in)

    #[test]
    fn test_for_in_object_keys() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let keys = ''; for (let k in {a: 1, b: 2}) { keys += k; } keys")
            .unwrap();
        assert_eq!(result, Value::String("ab".to_string()));
    }

    #[test]
    fn test_for_in_empty_object() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let count = 0; for (let k in {}) { count++; } count")
            .unwrap();
        assert_eq!(result, Value::Number(0.0));
    }

    #[test]
    fn test_for_in_break() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval(
                "let keys = ''; for (let k in {a: 1, b: 2, c: 3}) { keys += k; if (k === 'b') break; } keys",
            )
            .unwrap();
        assert_eq!(result, Value::String("ab".to_string()));
    }

    #[test]
    fn test_for_in_continue() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval(
                "let keys = ''; for (let k in {a: 1, b: 2, c: 3}) { if (k === 'b') continue; keys += k; } keys",
            )
            .unwrap();
        assert_eq!(result, Value::String("ac".to_string()));
    }

    #[test]
    fn test_for_in_on_number_does_not_iterate() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let count = 0; for (let k in 42) { count++; } count")
            .unwrap();
        assert_eq!(result, Value::Number(0.0));
    }

    #[test]
    fn test_for_in_on_string() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let keys = []; for (let k in 'ab') { keys.push(k); } keys.length")
            .unwrap();
        assert_eq!(result, Value::Number(2.0));
    }

    #[test]
    fn test_for_in_on_undefined_does_not_iterate() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let count = 0; for (let k in undefined) { count++; } count")
            .unwrap();
        assert_eq!(result, Value::Number(0.0));
    }

    #[test]
    fn test_for_in_on_null_does_not_iterate() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval("let count = 0; for (let k in null) { count++; } count")
            .unwrap();
        assert_eq!(result, Value::Number(0.0));
    }

    // Edge cases: error propagation

    #[test]
    fn test_for_of_non_iterable_throws() {
        let mut ctx = new_ctx();
        let result = ctx.eval("let s = 0; for (let x of 42) { s += x; }");
        assert!(result.is_err());
    }

    #[test]
    fn test_for_of_throw_in_body_propagates() {
        let mut ctx = new_ctx();
        let result = ctx.eval(
            "let s = 0; for (let x of [1, 2]) { if (x === 2) { throw new Error('boom'); } s += x; }",
        );
        assert!(result.is_err());
    }
}
