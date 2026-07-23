//! Iteration support for for-of/for-in loops

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::{Expression, Statement, VarKind};
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::object::{
    assign_to, call_iterator_return, declare_pattern_bindings_with_kind, init_to, obtain_iterator,
    take_iterator_step,
};
use crate::eval::statement::eval_statement;
use crate::interpreter::{
    loop_handles_break, loop_handles_continue, set_control_flow, take_control_flow, ControlFlow,
};
use crate::value::object::enumerate_for_in_keys;
use crate::value::object::helpers::ObjData;
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
        Value::Object(o) => Ok(enumerate_for_in_keys(o)),
        Value::String(s) => Ok((0..s.len()).map(|i| i.to_string()).collect()),
        _ => Ok(vec![]),
    }
}

fn declare_for_in_head_bindings(
    variable: &Expression,
    kind: VarKind,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    declare_for_of_binding(variable, kind, env)
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

enum ForOfIterResult {
    Done(Value),
    Break(Value),
    Step(Value),
}

fn eval_for_of_iterator(
    iterator: Rc<RefCell<Object>>,
    variable: &Expression,
    body: &Statement,
    loop_binding: Option<VarKind>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let per_iteration = loop_binding.is_some_and(|k| matches!(k, VarKind::Let | VarKind::Const));
    let mut index = 0usize;
    let mut completion = Value::Undefined;
    loop {
        let (item, done) = take_iterator_step(&iterator, &mut index, env)?;
        if done {
            break;
        }
        match run_for_of_iteration(
            variable,
            &item,
            body,
            loop_binding,
            per_iteration,
            env,
            in_arrow_function,
        );
        if let Err(e) = iteration {
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
        Ok(completion)
    }
}

fn run_for_of_iteration(
    variable: &Expression,
    item: &Value,
    body: &Statement,
    loop_binding: Option<VarKind>,
    per_iteration: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<ForOfIterResult, JsError> {
    if per_iteration {
        env.borrow_mut().push_scope();
    }
    let result = (|| {
        if let Some(kind) = loop_binding {
            declare_for_of_binding(variable, kind, env)?;
        }
        if loop_binding.is_some() {
            init_to(variable, item, env)?;
        } else {
            assign_to(variable, item, env)?;
        }
        eval_statement(body, env, false, in_arrow_function)
    })();
    if per_iteration {
        env.borrow_mut().pop_scope();
    }
    match result {
        Ok(body_val) => match take_control_flow() {
            Some(cf @ ControlFlow::Break(_)) => {
                if loop_handles_break(&cf, &[]) {
                    Ok(ForOfIterResult::Break(body_val))
                } else {
                    set_control_flow(cf);
                    Ok(ForOfIterResult::Step(body_val))
                }
            }
            Some(cf @ ControlFlow::Continue(_)) => {
                if loop_handles_continue(&cf, &[]) {
                    Ok(ForOfIterResult::Step(body_val))
                } else {
                    set_control_flow(cf);
                    Ok(ForOfIterResult::Step(body_val))
                }
            }
            Some(ControlFlow::Return(val))
            | Some(ControlFlow::Yield(val))
            | Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                Ok(ForOfIterResult::Done(val))
            }
            None => Ok(ForOfIterResult::Step(body_val)),
        },
        Err(e) => Err(e),
    }
}

fn declare_for_of_binding(
    variable: &Expression,
    kind: VarKind,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    match variable {
        Expression::Identifier(name) => {
            env.borrow_mut().declare_var(name.clone(), kind);
            Ok(())
        }
        Expression::ArrayPattern(bindings) => {
            for binding in bindings {
                declare_pattern_bindings_with_kind(binding, kind, env);
            }
            Ok(())
        }
        Expression::ObjectPattern(props) => {
            for (_, binding) in props {
                declare_pattern_bindings_with_kind(binding, kind, env);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Evaluate a for-of loop
pub fn eval_for_of(
    variable: &Expression,
    iterable: &Expression,
    body: &Statement,
    loop_binding: Option<crate::ast::VarKind>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let head_lexical = loop_binding.is_some_and(|k| matches!(k, VarKind::Let | VarKind::Const));
    if head_lexical {
        env.borrow_mut().push_scope();
        declare_for_in_head_bindings(variable, loop_binding.unwrap(), env)?;
    }

    let iter_value = eval_expression(iterable, env, in_arrow_function)?;

    if head_lexical {
        env.borrow_mut().pop_scope();
    }

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
    eval_for_of_iterator(
        iterator,
        variable,
        body,
        loop_binding,
        env,
        in_arrow_function,
    )
}

fn key_still_enumerable(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(o) => {
            let obj = o.borrow();
            if let ObjData::Idx { length, .. } = obj.data {
                return key
                    .parse::<usize>()
                    .ok()
                    .is_some_and(|i| (i as u64) < length);
            }
            drop(obj);
            let mut current: Option<Rc<RefCell<Object>>> = Some(Rc::clone(o));
            while let Some(cur_rc) = current {
                let cur = cur_rc.borrow();
                if cur.has_own(key) {
                    return cur.is_enumerable(key);
                }
                current = cur.prototype.clone();
            }
            false
        }
        Value::String(s) => key.parse::<usize>().ok().is_some_and(|i| i < s.len()),
        _ => false,
    }
}

enum ForInIterResult {
    Done(Value),
    Break(Value),
    Step(Value),
}

fn run_for_in_iteration(
    variable: &Expression,
    key: &str,
    body: &Statement,
    loop_binding: Option<VarKind>,
    per_iteration: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<ForInIterResult, JsError> {
    if per_iteration {
        env.borrow_mut().push_scope();
    }
    let result = (|| {
        if let Some(kind) = loop_binding {
            declare_for_of_binding(variable, kind, env)?;
        }
        if loop_binding.is_some() {
            init_to(variable, &Value::String(key.to_string()), env)?;
        } else {
            assign_to(variable, &Value::String(key.to_string()), env)?;
        }
        eval_statement(body, env, false, in_arrow_function)
    })();
    if per_iteration {
        env.borrow_mut().pop_scope();
    }
    match result {
        Ok(body_val) => match take_control_flow() {
            Some(cf @ ControlFlow::Break(_)) => {
                if loop_handles_break(&cf, &[]) {
                    Ok(ForInIterResult::Break(body_val))
                } else {
                    set_control_flow(cf);
                    Ok(ForInIterResult::Step(body_val))
                }
            }
            Some(cf @ ControlFlow::Continue(_)) => {
                if loop_handles_continue(&cf, &[]) {
                    Ok(ForInIterResult::Step(body_val))
                } else {
                    set_control_flow(cf);
                    Ok(ForInIterResult::Step(body_val))
                }
            }
            Some(ControlFlow::Return(val))
            | Some(ControlFlow::Yield(val))
            | Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                Ok(ForInIterResult::Done(val))
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
        Ok(completion)
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

    #[test]
    fn for_in_destructures_key_string() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval(
                "var obj = Object.create(null); obj.key = 1; var value; \
                 for (let [x] in obj) { value = x; } value",
            )
            .unwrap();
        assert_eq!(result, Value::String("k".to_string()));
    }

    #[test]
    fn for_in_typed_array_indices() {
        let mut ctx = new_ctx();
        let count = ctx
            .eval(
                "var rab = new ArrayBuffer(8); var ta = new Uint8Array(rab, 0, 3); \
                 var keys = []; for (var k in ta) keys.push(k); keys.length",
            )
            .unwrap();
        assert_eq!(count, Value::Number(3.0));
    }

    #[test]
    fn get_enumerable_keys_after_set_prototype_of() {
        let mut ctx = new_ctx();
        ctx.eval(
            "var proto = { p4: 1 }; var o = { p1: 1, p2: 2, p3: 3 }; \
             Object.setPrototypeOf(o, proto); globalThis.__o = o;",
        )
        .unwrap();
        let o = ctx.get_global("__o").expect("__o");
        let keys = get_enumerable_keys(&o).unwrap();
        assert_eq!(keys, vec!["p1", "p2", "p3", "p4"]);
    }

    #[test]
    fn for_in_set_prototype_enumerates_inherited_keys() {
        let mut ctx = new_ctx();
        assert_eq!(
            ctx.eval(
                "var proto = { p4: 1 }; var o = { p1: 1, p2: 2, p3: 3 }; \
                 Object.setPrototypeOf(o, proto); Object.getPrototypeOf(o) === proto",
            )
            .unwrap(),
            Value::Boolean(true)
        );
        let result = ctx
            .eval(
                "var proto = { p4: 1 }; var o = { p1: 1, p2: 2, p3: 3 }; \
                 Object.setPrototypeOf(o, proto); var keys = []; \
                 for (var k in o) keys.push(k); keys.join(',')",
            )
            .unwrap();
        assert_eq!(result, Value::String("p1,p2,p3,p4".to_string()));
    }

    #[test]
    fn for_in_prototype_enumeration() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval(
                "var proto = { p4: 1 }; var o = { p1: 1, p2: 2, p3: 3 }; \
                 Object.setPrototypeOf(o, proto); var keys = []; \
                 for (var k in o) { keys.push(k); } keys.join(',')",
            )
            .unwrap();
        assert_eq!(result, Value::String("p1,p2,p3,p4".to_string()));
    }

    #[test]
    fn for_in_completion_value_from_body() {
        let mut ctx = new_ctx();
        let result = ctx.eval("var b; for (b in { x: 0 }) { 3; }").unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn for_in_let_fresh_binding_per_iteration() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval(
                "var fns = {}; var obj = Object.create(null); \
                 obj.a = 1; obj.b = 1; obj.c = 1; \
                 for (let x in obj) { fns[x] = function() { return x; }; } \
                 fns.a() + fns.b() + fns.c()",
            )
            .unwrap();
        assert_eq!(result, Value::String("abc".to_string()));
    }

    #[test]
    fn for_in_head_tdz_before_object_expr() {
        let mut ctx = new_ctx();
        let err = ctx
            .eval("let x = 1; for (const x in { x }) {}")
            .unwrap_err();
        assert!(
            err.to_string().contains("ReferenceError"),
            "expected ReferenceError, got {err}"
        );
    }

    #[test]
    fn for_of_completion_value_from_body() {
        let mut ctx = new_ctx();
        let result = ctx.eval("var b; for (b of [0]) { 3; }").unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn for_of_head_tdz_before_iterable_expr() {
        let mut ctx = new_ctx();
        let err = ctx.eval("let x = 1; for (const x of [x]) {}").unwrap_err();
        assert!(
            err.to_string().contains("ReferenceError"),
            "expected ReferenceError, got {err}"
        );
    }

    #[test]
    fn for_of_nested_generators() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval(
                "function* values() { yield 3; yield 7; } \
                 var i = 0; for (var x of values()) { \
                   if (x === 3) { i++; for (var y of values()) { if (y === 3) i++; } } \
                 } i",
            )
            .unwrap();
        assert_eq!(result, Value::Number(2.0));
    }
}
