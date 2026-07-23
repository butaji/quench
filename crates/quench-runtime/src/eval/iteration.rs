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
    let saved_cf = take_control_flow();
    let close_err = call_iterator_return(iterator);
    if let Some(cf) = saved_cf {
        set_control_flow(cf);
    }
    match completion {
        Err(e) => Err(e),
        Ok(val) => {
            if let Some(close_err) = close_err {
                return Err(close_err);
            }
            Ok(val)
        }
    }
}

enum ForOfIterResult {
    Done(Value),
    Break(Value),
    Step(Value),
    Yield(Value),
}

pub(crate) fn stage_stored_for_of_suspend(state: crate::value::generator::ForOfSuspend) {
    PENDING_FOR_OF.with(|cell| *cell.borrow_mut() = Some(state));
}

pub(crate) fn take_pending_for_of_suspend() -> Option<crate::value::generator::ForOfSuspend> {
    take_for_of_suspend()
}

fn save_for_of_suspend(state: crate::value::generator::ForOfSuspend) {
    PENDING_FOR_OF.with(|cell| *cell.borrow_mut() = Some(state));
}

fn take_for_of_suspend() -> Option<crate::value::generator::ForOfSuspend> {
    PENDING_FOR_OF.with(|cell| cell.borrow_mut().take())
}

thread_local! {
    static PENDING_FOR_OF: RefCell<Option<crate::value::generator::ForOfSuspend>> =
        const { RefCell::new(None) };
}

fn stmt_index_after_first_yield(body: &Statement) -> Option<usize> {
    let Statement::Block(stmts) = body else {
        return None;
    };
    for (i, stmt) in stmts.iter().enumerate() {
        if crate::value::generator_replay::count_yields_in_stmt(stmt) > 0 {
            return Some(i + 1);
        }
    }
    None
}

fn eval_for_of_iterator(
    iterator: Rc<RefCell<Object>>,
    variable: &Expression,
    body: &Statement,
    loop_binding: Option<VarKind>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
    mut index: usize,
    mut pending_resume: Option<(Value, bool, Option<usize>)>,
) -> Result<Value, JsError> {
    let per_iteration = loop_binding.is_some_and(|k| matches!(k, VarKind::Let | VarKind::Const));
    let mut completion = Value::Undefined;
    loop {
        let (item, body_only, body_stmt_resume) = if let Some(resume) = pending_resume.take() {
            resume
        } else {
            let (item, done) = take_iterator_step(&iterator, &mut index, env)?;
            if done {
                break;
            }
            (item, false, None)
        };
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
    body_only: bool,
    body_stmt_resume: Option<usize>,
) -> Result<ForOfIterResult, JsError> {
    if per_iteration && !body_only {
        env.borrow_mut().push_scope();
    }
    let result = (|| {
        if !body_only {
            if let Some(kind) = loop_binding {
                declare_for_of_binding(variable, kind, env)?;
            }
            if loop_binding.is_some() {
                init_to(variable, item, env)?;
            } else {
                assign_to(variable, item, env)?;
            }
        }
        if body_only {
            if let (Some(start), Statement::Block(stmts)) = (body_stmt_resume, body) {
                return crate::eval::statement::eval_statements(
                    &stmts[start..],
                    env,
                    false,
                    in_arrow_function,
                );
            }
        }
        eval_statement(body, env, false, in_arrow_function)
    })();
    let yielding = crate::interpreter::peek_generator_yield();
    if per_iteration && !yielding {
        env.borrow_mut().pop_scope();
    }
    match result {
        Ok(body_val) => {
            if yielding {
                return Ok(ForOfIterResult::Yield(body_val));
            }
            match take_control_flow() {
                Some(cf @ ControlFlow::Break(_)) => {
                    set_control_flow(cf);
                    Ok(ForOfIterResult::Break(body_val))
                }
                Some(cf @ ControlFlow::Continue(_)) => {
                    if loop_handles_continue(&cf, &[]) {
                        Ok(ForOfIterResult::Step(body_val))
                    } else {
                        set_control_flow(cf);
                        Ok(ForOfIterResult::Break(body_val))
                    }
                }
                Some(ControlFlow::Return(val))
                | Some(ControlFlow::Yield(val))
                | Some(ControlFlow::YieldDelegate(val)) => {
                    set_control_flow(ControlFlow::Return(val.clone()));
                    Ok(ForOfIterResult::Done(val))
                }
                None => Ok(ForOfIterResult::Step(body_val)),
            }
        }
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
    if let Some(suspend) = take_for_of_suspend() {
        return eval_for_of_iterator(
            suspend.iterator,
            &suspend.variable,
            &suspend.body,
            suspend.loop_binding,
            env,
            suspend.in_arrow_function,
            suspend.index,
            Some((suspend.item, true, suspend.body_stmt_resume)),
        );
    }

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
        0,
        None,
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
    fn for_of_sloppy_arguments_object() {
        let mut ctx = new_ctx();
        let count = ctx
            .eval(
                "(function() { \
                   var i = 0; \
                   for (var v of arguments) { i++; } \
                   return i; \
                 }(1, 2, 3))",
            )
            .unwrap();
        assert_eq!(count, Value::Number(3.0));
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
    fn generator_first_next_yields_before_unreachable_throw() {
        let mut ctx = new_ctx();
        let done = ctx
            .eval(
                "function* values() { try { yield; throw new Error('x'); } finally {} } \
                 var g = values(); \
                 g.next().done",
            )
            .unwrap();
        assert_eq!(done, Value::Boolean(false));
    }

    #[test]
    fn for_of_generator_unreachable_throw_after_yield() {
        let mut ctx = new_ctx();
        let iteration_count = ctx
            .eval(
                "var iterationCount = 0; \
                 function* values() { try { yield; throw new Error('unreachable'); } finally {} } \
                 try { for (var x of values()) { iterationCount += 1; } } catch (e) {} \
                 iterationCount",
            )
            .unwrap();
        assert_eq!(iteration_count, Value::Number(1.0));
    }

    #[test]
    fn for_of_generator_with_pre_yield_side_effect() {
        let mut ctx = new_ctx();
        let iteration_count = ctx
            .eval(
                "var startedCount = 0; var iterationCount = 0; \
                 function* values() { startedCount += 1; try { yield; } finally {} } \
                 try { for (var x of values()) { iterationCount += 1; throw 0; } } catch (e) {} \
                 iterationCount",
            )
            .unwrap();
        assert_eq!(iteration_count, Value::Number(1.0));
    }

    #[test]
    fn for_of_generator_throw_closes_like_test262() {
        let mut ctx = new_ctx();
        let iteration_count = ctx
            .eval(
                "var startedCount = 0; var finallyCount = 0; var iterationCount = 0; \
                 function* values() { \
                   startedCount += 1; \
                   try { yield; throw new Error('unreachable'); } \
                   finally { finallyCount += 1; } \
                 } \
                 var iterable = values(); \
                 try { \
                   for (var x of iterable) { \
                     if (startedCount !== 1) throw new Error('started'); \
                     if (finallyCount !== 0) throw new Error('finally early'); \
                     iterationCount += 1; \
                     throw 0; \
                   } \
                 } catch (e) {} \
                 iterationCount",
            )
            .unwrap();
        assert_eq!(iteration_count, Value::Number(1.0));
        let finally_count = ctx.eval("finallyCount").unwrap();
        assert_eq!(finally_count, Value::Number(1.0));
    }

    #[test]
    fn for_of_generator_throw_runs_finally_on_close() {
        let mut ctx = new_ctx();
        let finally = ctx
            .eval(
                "var finallyCount = 0; \
                 function* values() { \
                   try { yield; } finally { finallyCount += 1; } \
                 } \
                 try { for (var x of values()) { throw 0; } } catch (e) {} \
                 finallyCount",
            )
            .unwrap();
        assert_eq!(finally, Value::Number(1.0));
    }

    #[test]
    fn for_of_unlabeled_break_exits_loop() {
        let mut ctx = new_ctx();
        let count = ctx
            .eval(
                "var count = 0; \
                 for (var x of [1, 2, 3]) { count++; break; } \
                 count",
            )
            .unwrap();
        assert_eq!(count, Value::Number(1.0));
    }

    #[test]
    fn for_of_break_outer_label_without_try() {
        let mut ctx = new_ctx();
        let i = ctx
            .eval(
                "var i = 0; \
                 outer: while (true) { \
                   for (var x of [1]) { i++; break outer; } \
                   throw new Error('after for-of'); \
                 } \
                 i",
            )
            .unwrap();
        assert_eq!(i, Value::Number(1.0));
    }

    #[test]
    fn for_of_break_outer_from_try_block() {
        let mut ctx = new_ctx();
        let i = ctx
            .eval(
                "var i = 0; \
                 outer: while (true) { \
                   for (var x of [1]) { \
                     try { i++; break outer; } catch (e) {} \
                     throw new Error('after try'); \
                   } \
                   throw new Error('after for-of'); \
                 } \
                 i",
            )
            .unwrap();
        assert_eq!(i, Value::Number(1.0));
    }

    #[test]
    fn for_of_break_outer_with_generator_no_try() {
        let mut ctx = new_ctx();
        let i = ctx
            .eval(
                "function* values() { yield 1; throw new Error('after yield'); } \
                 var iterator = values(); var i = 0; \
                 outer: while (true) { \
                   for (var x of iterator) { i++; break outer; } \
                   throw new Error('after for-of'); \
                 } \
                 i",
            )
            .unwrap();
        assert_eq!(i, Value::Number(1.0));
    }

    #[test]
    fn for_of_break_outer_label_closes_generator() {
        let mut ctx = new_ctx();
        let i = ctx
            .eval(
                "function* values() { yield 1; throw new Error('after yield'); } \
                 var iterator = values(); var i = 0; \
                 outer: while (true) { \
                   for (var x of iterator) { \
                     try { i++; break outer; } catch (e) {} \
                     throw new Error('after try'); \
                   } \
                   throw new Error('after for-of'); \
                 } \
                 i",
            )
            .unwrap();
        assert_eq!(i, Value::Number(1.0));
    }

    #[test]
    fn for_of_break_from_finally_exits_loop() {
        let mut ctx = new_ctx();
        let i = ctx
            .eval(
                "function* values() { yield 1; throw new Error('after yield'); } \
                 var iterator = values(); var i = 0; \
                 for (var x of iterator) { \
                   try {} finally { i++; break; throw new Error('after break'); } \
                   throw new Error('after try'); \
                 } \
                 i",
            )
            .unwrap();
        assert_eq!(i, Value::Number(1.0));
    }

    #[test]
    fn for_of_return_from_try_in_iife() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval(
                "function* values() { yield 1; throw new Error('after yield'); } \
                 var iterator = values(); \
                 (function() { \
                   for (var x of iterator) { \
                     try { return 34; } catch (e) {} \
                     throw new Error('after try'); \
                   } \
                   throw new Error('after for-of'); \
                 })()",
            )
            .unwrap();
        assert_eq!(result, Value::Number(34.0));
    }

    #[test]
    fn for_of_destructure_assign_error_closes_iterator() {
        let mut ctx = new_ctx();
        let counts = ctx
            .eval(
                "var callCount = 0; var iterationCount = 0; \
                 var iterable = {}; var x = { set attr(_) { throw new Error('Test262'); } }; \
                 iterable[Symbol.iterator] = function() { \
                   return { \
                     next: function() { return { done: false, value: [0] }; }, \
                     return: function() { callCount += 1; } \
                   }; \
                 }; \
                 var errName = ''; \
                 try { for ([x.attr] of iterable) { iterationCount += 1; } } \
                 catch (e) { errName = e.name; } \
                 JSON.stringify([callCount, iterationCount, errName])",
            )
            .unwrap();
        assert_eq!(counts, Value::String("[1,0,\"Error\"]".to_string()));
    }

    #[test]
    fn for_of_body_throw_wins_over_non_callable_iterator_return() {
        let mut ctx = new_ctx();
        let err = ctx
            .eval(
                "var msg = ''; \
                 var iterable = {}; \
                 iterable[Symbol.iterator] = function() { \
                   return { \
                     next: function() { return { done: false, value: null }; }, \
                     return: 'str' \
                   }; \
                 }; \
                 try { \
                   for (var x of iterable) { throw new Error('body'); } \
                 } catch (e) { msg = e.message; } \
                 msg",
            )
            .unwrap();
        assert_eq!(err, Value::String("body".to_string()));
    }

    #[test]
    fn for_of_string_bmp_visits_all_characters() {
        let mut ctx = new_ctx();
        let count = ctx
            .eval(
                "var iterationCount = 0; \
                 for (var value of 'abc') { iterationCount++; } \
                 iterationCount",
            )
            .unwrap();
        assert_eq!(count, Value::Number(3.0));
    }

    #[test]
    fn for_of_array_mutation_visible_during_iteration() {
        let mut ctx = new_ctx();
        let count = ctx
            .eval(
                "var array = [0, 1]; var iterationCount = 0; \
                 for (var x of array) { \
                   if (x !== 0) throw 0; \
                   array.pop(); \
                   iterationCount++; \
                 } \
                 iterationCount",
            )
            .unwrap();
        assert_eq!(count, Value::Number(1.0));
    }

    #[test]
    fn for_of_break_closes_iterator() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval(
                "var returnCount = 0; var iterable = {}; \
                 iterable[Symbol.iterator] = function() { \
                   return { \
                     next: function() { return { done: false, value: 1 }; }, \
                     return: function() { returnCount += 1; return {}; } \
                   }; \
                 }; \
                 for (var x of iterable) { break; } \
                 returnCount",
            )
            .unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn for_of_throw_closes_iterator() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval(
                "var returnCount = 0; var iterable = {}; \
                 iterable[Symbol.iterator] = function() { \
                   return { \
                     next: function() { return { done: false, value: 1 }; }, \
                     return: function() { returnCount += 1; return {}; } \
                   }; \
                 }; \
                 try { for (var x of iterable) { throw 0; } } catch (e) {} \
                 returnCount",
            )
            .unwrap();
        assert_eq!(result, Value::Number(1.0));
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
