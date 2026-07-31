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
use crate::value::generator::{ForOfResume, ForOfSuspend};
use crate::value::object::enumerate_for_in_keys;
use crate::value::object::helpers::ObjData;
use crate::value::{JsError, Object, Value};

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
    Ok(crate::value::wtf8::wtf8_for_of_iterate(s))
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
    Yield(Value, bool),
}

type ForOfPending = Option<(Value, ForOfResume)>;

struct ForOfStep<'a> {
    variable: &'a Expression,
    item: &'a Value,
    body: &'a Statement,
    loop_binding: Option<VarKind>,
    dispose_async: Option<bool>,
    env: &'a Rc<RefCell<Environment>>,
    in_arrow_function: bool,
    resume: ForOfResume,
}

struct ForOfIteratorRun<'a> {
    iterator: Rc<RefCell<Object>>,
    variable: &'a Expression,
    body: &'a Statement,
    loop_binding: Option<VarKind>,
    dispose_async: Option<bool>,
    await_of: bool,
    env: &'a Rc<RefCell<Environment>>,
    in_arrow_function: bool,
    index: usize,
    pending: ForOfPending,
}

pub(crate) fn stage_stored_for_of_suspend(state: crate::value::generator::ForOfSuspend) {
    PENDING_FOR_OF.with(|cell| *cell.borrow_mut() = Some(state));
}

pub(crate) fn take_pending_for_of_suspend() -> Option<crate::value::generator::ForOfSuspend> {
    take_for_of_suspend()
}

pub(crate) fn stage_pending_destructuring_iterator(iterator: Rc<RefCell<Object>>) {
    PENDING_DESTRUCTURING_ITERATOR.with(|cell| *cell.borrow_mut() = Some(iterator));
}

pub(crate) fn take_pending_destructuring_iterator() -> Option<Rc<RefCell<Object>>> {
    PENDING_DESTRUCTURING_ITERATOR.with(|cell| cell.borrow_mut().take())
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
    static PENDING_DESTRUCTURING_ITERATOR: RefCell<Option<Rc<RefCell<Object>>>> =
        const { RefCell::new(None) };
    static PENDING_YIELD_DELEGATE: RefCell<Option<crate::value::generator::YieldDelegateSuspend>> =
        const { RefCell::new(None) };
}

pub(crate) fn stage_yield_delegate_suspend(state: crate::value::generator::YieldDelegateSuspend) {
    PENDING_YIELD_DELEGATE.with(|cell| *cell.borrow_mut() = Some(state));
}

pub(crate) fn take_pending_yield_delegate_suspend(
) -> Option<crate::value::generator::YieldDelegateSuspend> {
    PENDING_YIELD_DELEGATE.with(|cell| cell.borrow_mut().take())
}

/// Evaluate `yield*` with per-value generator suspension.
pub fn eval_yield_delegate(
    expr: &Expression,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    if let Some(state) = take_pending_yield_delegate_suspend() {
        return continue_yield_delegate(state, env);
    }
    let iterable = eval_expression(expr, env, in_arrow_function)?;
    let (iterator, await_values) = match iterable {
        Value::Generator(gen) => {
            let await_values = !gen.borrow().is_async;
            (
                crate::value::generator::generator_as_iterator_object(gen),
                await_values,
            )
        }
        Value::Object(o) if crate::interpreter::is_in_async_generator() => {
            obtain_async_iterator(&o, env)?
        }
        Value::Object(o) => (obtain_iterator(&o)?, false),
        _ => {
            return Err(JsError(
                "TypeError: delegated iterable is not iterable".to_string(),
            ))
        }
    };
    continue_yield_delegate(
        crate::value::generator::YieldDelegateSuspend {
            iterator,
            index: 0,
            await_values,
            abrupt_error: None,
            completion: None,
        },
        env,
    )
}

fn obtain_async_iterator(
    object: &Rc<RefCell<Object>>,
    env: &Rc<RefCell<Environment>>,
) -> Result<(Rc<RefCell<Object>>, bool), JsError> {
    let Some(Value::Symbol(symbol)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("asyncIterator")
    else {
        return obtain_iterator(object).map(|iterator| (iterator, true));
    };
    let method =
        crate::eval::member::eval_object_member(object, &symbol.property_key(), Some(env))?;
    if matches!(method, Value::Undefined | Value::Null) {
        return obtain_iterator(object).map(|iterator| (iterator, true));
    }
    if !method.is_callable() {
        return Err(iterator_type_error("iterator method is not callable"));
    }
    let iterator = crate::eval::function::call_value_with_this(
        method,
        vec![],
        Value::Object(Rc::clone(object)),
    )?;
    match iterator {
        Value::Object(iterator) => Ok((iterator, false)),
        _ => Err(iterator_type_error("iterator is not an object")),
    }
}

fn iterator_type_error(message: &str) -> JsError {
    let (value, error) = crate::value::create_js_error_with_type(message, "TypeError");
    crate::value::set_thrown_value(value);
    error
}

fn continue_yield_delegate(
    mut state: crate::value::generator::YieldDelegateSuspend,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some((error, value)) = state.abrupt_error.take() {
        crate::value::set_thrown_value(value);
        return Err(error);
    }
    if let Some(value) = state.completion.take() {
        return Ok(value);
    }
    let resume_val = crate::interpreter::take_generator_resume_value();
    let next_value = if state.index == 0 {
        Value::Undefined
    } else {
        resume_val.clone()
    };
    let (value, done) = crate::eval::object::take_iterator_step_with_value(
        &state.iterator,
        &mut state.index,
        env,
        next_value,
    )?;
    if done {
        return Ok(value);
    }
    if crate::interpreter::peek_generator_yield() {
        return Ok(Value::Undefined);
    }
    crate::interpreter::set_generator_yield(value);
    crate::value::generator_replay::record_fresh_yield_resume(resume_val.clone());
    stage_yield_delegate_suspend(state);
    Ok(resume_val)
}

fn eval_for_of_body_tail(
    tail: &[Statement],
    resume_mid_delegate: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    if tail.is_empty() {
        return Ok(Value::Undefined);
    }
    if resume_mid_delegate {
        let stmt_result = eval_statement(&tail[0], env, false, in_arrow_function)?;
        if crate::interpreter::peek_generator_yield() {
            return Ok(stmt_result);
        }
        if tail.len() > 1 {
            return crate::eval::statement::eval_statements(
                &tail[1..],
                env,
                false,
                in_arrow_function,
            );
        }
        return Ok(stmt_result);
    }
    crate::eval::statement::eval_statements(tail, env, false, in_arrow_function)
}

fn eval_for_of_iterator(mut run: ForOfIteratorRun<'_>) -> Result<Value, JsError> {
    let per_iteration = run
        .loop_binding
        .is_some_and(|k| matches!(k, VarKind::Let | VarKind::Const));
    let mut completion = Value::Undefined;
    loop {
        let (item, resume) = if let Some((item, resume)) = run.pending.take() {
            (item, resume)
        } else {
            let (item, done) = take_iterator_step(&run.iterator, &mut run.index, run.env)?;
            if done {
                break;
            }
            (item, ForOfResume::default())
        };
        let item = if run.await_of {
            match await_for_await_of(item) {
                Ok(value) => value,
                Err(error) if crate::interpreter::is_in_async_generator() => {
                    return Err(error);
                }
                Err(error) => return deferred_for_await_rejection(error),
            }
        } else {
            item
        };
        let step = ForOfStep {
            variable: run.variable,
            item: &item,
            body: run.body,
            loop_binding: run.loop_binding,
            dispose_async: run.dispose_async,
            env: run.env,
            in_arrow_function: run.in_arrow_function,
            resume,
        };
        match run_for_of_iteration(step, per_iteration) {
            Ok(ForOfIterResult::Done(val)) => return abrupt_close(&run.iterator, Ok(val)),
            Ok(ForOfIterResult::Break(val)) => {
                let closed = abrupt_close(&run.iterator, Ok(val));
                if run.await_of {
                    match closed {
                        Ok(value) => return Ok(value),
                        Err(error) if crate::interpreter::is_in_async_generator() => {
                            return Err(error);
                        }
                        Err(error) if crate::interpreter::is_in_async_function() => {
                            return Err(error);
                        }
                        Err(error) => return deferred_for_await_rejection(error),
                    }
                }
                return closed;
            }
            Ok(ForOfIterResult::Yield(val, suspend_init)) => {
                // Compute body_tail before setting pending so it can be used in both.
                let body_tail = if suspend_init {
                    None
                } else {
                    crate::value::generator_replay::body_tail_after_yield(run.body, true)
                };
                // If we yielded during init/assign, run.pending is None (not yet set).
                // Capture the current item so on resume we use the same item, not a new iter.next().
                // For a body yield (suspend_init=false): body_only=true skips init on resume,
                // and the saved body_tail runs the post-yield tail. After the tail completes
                // normally, the loop calls take_iterator_step again for the next item.
                // For an init yield (suspend_init=true): pending keeps init=true so init re-runs.
                if run.pending.is_none() && !matches!(item, Value::Undefined) {
                    let resume = if suspend_init {
                        ForOfResume {
                            init: true,
                            ..Default::default()
                        }
                    } else {
                        ForOfResume {
                            body_only: true,
                            body_tail: body_tail.clone(),
                            mid_delegate: true,
                            init: false,
                        }
                    };
                    run.pending = Some((item.clone(), resume));
                }
                save_for_of_suspend(ForOfSuspend {
                    iterator: Rc::clone(&run.iterator),
                    index: run.index,
                    item: item.clone(),
                    resume_body: true,
                    body_tail,
                    resume_mid_delegate: !suspend_init,
                    resume_init: suspend_init,
                    variable: run.variable.clone(),
                    body: run.body.clone(),
                    loop_binding: run.loop_binding,
                    dispose_async: run.dispose_async,
                    await_of: run.await_of,
                    per_iteration,
                    in_arrow_function: run.in_arrow_function,
                    pending: run.pending.clone(),
                });
                return Ok(val);
            }
            Ok(ForOfIterResult::Step(body_val)) => completion = body_val,
            Err(e) => {
                let closed = abrupt_close(&run.iterator, Err(e));
                if run.await_of {
                    match closed {
                        Ok(value) => return Ok(value),
                        Err(error) if crate::interpreter::is_in_async_generator() => {
                            return Err(error);
                        }
                        Err(error) => return deferred_for_await_rejection(error),
                    }
                }
                return closed;
            }
        }
    }
    if let Some(ControlFlow::Return(val))
    | Some(ControlFlow::Throw(val))
    | Some(ControlFlow::Yield(val))
    | Some(ControlFlow::YieldDelegate(val)) = take_control_flow()
    {
        if run.await_of {
            set_control_flow(ControlFlow::Return(val.clone()));
        }
        Ok(val)
    } else {
        Ok(completion)
    }
}

fn deferred_for_await_rejection(error: JsError) -> Result<Value, JsError> {
    let reason = crate::value::take_thrown_value().unwrap_or_else(|| {
        let error_type = if error.0.contains("unresolvable") || error.0.contains("not defined") {
            "ReferenceError"
        } else {
            "TypeError"
        };
        let (value, _) =
            crate::value::error::create_js_error_with_type(&error.to_string(), error_type);
        crate::value::take_thrown_value();
        value
    });
    if error.0.contains("not defined") {
        crate::value::set_thrown_value(reason);
        return Err(error);
    }
    let proto = crate::builtins::promise::get_promise_proto();
    let mut object = Object::with_prototype(crate::value::ObjectKind::Promise, proto);
    object.promise_data = Some(crate::value::object::PromiseObjectData::new());
    let target = Rc::new(RefCell::new(object));
    let queued_target = Rc::clone(&target);
    let first = Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |_| {
        crate::builtins::promise::settle_reject(&queued_target, reason.clone());
        Ok(Value::Undefined)
    })));
    crate::builtins::promise::queue_microtask_impl(first);
    let promise = Value::Object(target);
    crate::interpreter::set_control_flow(ControlFlow::Return(promise.clone()));
    Ok(promise)
}

fn await_for_await_of(value: Value) -> Result<Value, JsError> {
    let promise = crate::builtins::promise::promise_resolve_impl_static(
        vec![value],
        crate::builtins::promise::get_promise_proto(),
    )?;
    let Value::Object(promise) = promise else {
        return Ok(Value::Undefined);
    };
    crate::builtins::promise::execute_pending_microtasks()?;
    let data = promise.borrow().promise_data.clone();
    match data.map(|data| (data.state, data.result)) {
        Some((crate::value::object::PromiseState::Fulfilled, value)) => Ok(value),
        Some((crate::value::object::PromiseState::Rejected, reason)) => {
            crate::value::set_thrown_value(reason);
            Err(JsError("for-await value rejected".to_string()))
        }
        _ => Ok(Value::Undefined),
    }
}

fn run_for_of_iteration(
    step: ForOfStep<'_>,
    per_iteration: bool,
) -> Result<ForOfIterResult, JsError> {
    let ForOfStep {
        variable,
        item,
        body,
        loop_binding,
        dispose_async,
        env,
        in_arrow_function,
        resume,
    } = step;
    let ForOfResume {
        body_only,
        body_tail,
        mid_delegate: resume_mid_delegate,
        init: resume_init,
    } = resume;
    if per_iteration && !body_only {
        env.borrow_mut().push_scope();
    }
    let mut suspend_init = false;
    let result = (|| {
        let need_init = !body_only || resume_init;
        if need_init {
            if !body_only {
                if let Some(kind) = loop_binding {
                    declare_for_of_binding(variable, kind, env)?;
                }
            }
            if loop_binding.is_some() {
                init_to(variable, item, env)?;
            } else {
                assign_to(variable, item, env)?;
            }
            if let (Some(is_async), Expression::Identifier(name)) = (dispose_async, variable) {
                crate::eval::statement::eval_register_dispose(name, is_async, env)?;
            }
            if crate::interpreter::peek_generator_yield() {
                suspend_init = true;
                return Ok(Value::Undefined);
            }
        }
        if body_only && !resume_init {
            if let Some(tail) = body_tail {
                return eval_for_of_body_tail(&tail, resume_mid_delegate, env, in_arrow_function);
            }
        }
        let result = eval_statement(body, env, false, in_arrow_function);
        if let (Some(is_async), Expression::Identifier(name)) = (dispose_async, variable) {
            crate::eval::statement::eval_dispose(name, is_async, env)?;
        }
        result
    })();
    let yielding = crate::interpreter::peek_generator_yield();
    if per_iteration && !yielding {
        env.borrow_mut().pop_scope();
    }
    match result {
        Ok(body_val) => {
            if yielding {
                return Ok(ForOfIterResult::Yield(body_val, suspend_init));
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
                Some(ControlFlow::Throw(val)) => {
                    // throw in for-of body: propagate as error so the
                    // iterator is properly closed via abrupt_close.
                    let msg = crate::value::to_js_string(&val);
                    Err(crate::value::JsError::from(msg))
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
        Err(e) => {
            // When array_destructuring_impl detects ControlFlow::Return pending from
            // generator.return(), it returns Err("Return completion") to signal:
            // "close the iterator, exit the for-of without running the body".
            // The return value is stored in the thrown value. Extract it and
            // propagate as ForOfIterResult::Done — NOT as an error (which would
            // re-close the iterator in eval_for_of_iterator's abrupt_close).
            if e.to_string() == "Return completion" {
                let val = crate::value::take_thrown_value().unwrap_or(Value::Undefined);
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(ForOfIterResult::Done(val));
            }
            Err(e)
        }
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
    dispose_async: Option<bool>,
    await_of: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    if let Some(suspend) = take_for_of_suspend() {
        return eval_for_of_iterator(ForOfIteratorRun {
            iterator: suspend.iterator,
            variable: &suspend.variable,
            body: &suspend.body,
            loop_binding: suspend.loop_binding,
            dispose_async: suspend.dispose_async,
            await_of: suspend.await_of,
            env,
            in_arrow_function: suspend.in_arrow_function,
            index: suspend.index,
            pending: suspend.pending,
        });
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
            let items: Vec<Value> = crate::value::wtf8::wtf8_for_of_iterate(s);
            let arr = Object::new_array_from(items);
            Rc::new(RefCell::new(arr))
        }
        Value::Generator(gen) => {
            crate::value::generator::generator_as_iterator_object(Rc::clone(gen))
        }
        Value::Object(o) if await_of => {
            let (iterator, _) = obtain_async_iterator(o, env)?;
            iterator
        }
        Value::Object(o) => obtain_iterator(o)?,
        _ => return Err(JsError("TypeError: Value is not iterable".to_string())),
    };
    eval_for_of_iterator(ForOfIteratorRun {
        iterator,
        variable,
        body,
        loop_binding,
        dispose_async,
        await_of,
        env,
        in_arrow_function,
        index: 0,
        pending: None,
    })
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
                    // Break label doesn't match this loop — exit and let
                    // the enclosing scope handle it. The control flow is
                    // already set.
                    Ok(ForInIterResult::Break(body_val))
                }
            }
            Some(cf @ ControlFlow::Continue(_)) => {
                if loop_handles_continue(&cf, &[]) {
                    Ok(ForInIterResult::Step(body_val))
                } else {
                    // Continue label doesn't match this loop — exit and let
                    // the enclosing scope handle it. The control flow is
                    // already set.
                    Ok(ForInIterResult::Break(body_val))
                }
            }
            Some(ControlFlow::Return(val))
            | Some(ControlFlow::Throw(val))
            | Some(ControlFlow::Yield(val))
            | Some(ControlFlow::YieldDelegate(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                Ok(ForInIterResult::Done(val))
            }
            None => Ok(ForInIterResult::Step(body_val)),
        },
        Err(e) => Err(e),
    }
}

/// Evaluate a for-in loop
pub fn eval_for_in(
    variable: &Expression,
    object: &Expression,
    body: &Statement,
    loop_binding: Option<VarKind>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let head_lexical = loop_binding.is_some_and(|k| matches!(k, VarKind::Let | VarKind::Const));
    if head_lexical {
        env.borrow_mut().push_scope();
        declare_for_in_head_bindings(variable, loop_binding.unwrap(), env)?;
    }

    let obj_value = eval_expression(object, env, in_arrow_function)?;

    if head_lexical {
        env.borrow_mut().pop_scope();
    }

    let per_iteration = head_lexical;
    let mut completion = Value::Undefined;
    let key_queue = get_enumerable_keys(&obj_value)?;
    let mut index = 0usize;

    while index < key_queue.len() {
        let key = key_queue[index].clone();
        index += 1;
        if !key_still_enumerable(&obj_value, &key) {
            continue;
        }

        match run_for_in_iteration(
            variable,
            &key,
            body,
            loop_binding,
            per_iteration,
            env,
            in_arrow_function,
        ) {
            Ok(ForInIterResult::Done(val)) => return Ok(val),
            Ok(ForInIterResult::Break(val)) => return Ok(val),
            Ok(ForInIterResult::Step(body_val)) => completion = body_val,
            Err(e) => return Err(e),
        }
    }

    if let Some(ControlFlow::Return(val))
    | Some(ControlFlow::Throw(val))
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
    fn for_await_of_awaits_values_from_sync_iterables() {
        let mut ctx = new_ctx();
        ctx.eval("var result = 0; async function f() { for await (var value of [Promise.resolve(7)]) { result = value; } } f();")
            .unwrap();
        assert_eq!(ctx.eval("result").unwrap(), Value::Number(7.0));
    }

    #[test]
    fn for_await_of_destructuring_error_rejects_async_function() {
        let mut ctx = new_ctx();
        ctx.eval(
            "let _; var reason; async function fn() { for await ([[ _ ]] of [[null]]) {} } let promise = fn(); promise.catch(function(error) { reason = error; });",
        )
        .unwrap();
        let promise = ctx.eval("promise").unwrap();
        assert_eq!(
            ctx.eval("promise.constructor === Promise").unwrap(),
            Value::Boolean(true)
        );
        let state = match promise {
            Value::Object(object) => object
                .borrow()
                .promise_data
                .as_ref()
                .map(|data| data.state.clone()),
            _ => None,
        };
        assert_eq!(state, Some(crate::value::object::PromiseState::Rejected));
        assert_eq!(
            ctx.eval("reason.constructor === TypeError").unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn for_await_of_rejection_preserves_microtask_order() {
        let mut ctx = new_ctx();
        ctx.eval("var actual = []; var p = Promise.resolve(0); Object.defineProperty(p, 'constructor', { get() { throw new Error(); } }); async function f() { actual.push('start'); for await (var x of [p]); actual.push('never'); } Promise.resolve().then(() => actual.push('tick 1')).then(() => actual.push('tick 2')).then(() => actual.push('after')); f().catch(() => actual.push('catch'));")
            .unwrap();
        assert_eq!(
            ctx.eval("actual.join(',')").unwrap(),
            Value::String("start,tick 1,tick 2,catch,after".into())
        );
    }

    #[test]
    fn async_generator_nested_destructuring_clears_stale_thrown_value() {
        let mut ctx = new_ctx();
        let result = ctx.eval(
            "async function* fn() { for await ([[ _ ]] of [[null]]) {} } var p = fn().next(); typeof p",
        );
        assert_eq!(result.unwrap(), Value::String("object".into()));
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
    fn for_of_assignment_updates_outer_var() {
        let mut ctx = new_ctx();
        ctx.eval("var result; function f() { for (result of [7]); } f();")
            .unwrap();
        assert_eq!(ctx.eval("result").unwrap(), Value::Number(7.0));
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
    fn for_of_yield_star_runs_post_delegate_statements() {
        let mut ctx = new_ctx();
        let j = ctx
            .eval(
                "function* values() { yield 1; yield 1; } \
                 var dataIterator = values(); \
                 var controlIterator = (function*() { \
                   for (var x of dataIterator) { \
                     i++; \
                     yield * values(); \
                     j++; \
                   } \
                 })(); \
                 var i = 0; var j = 0; \
                 controlIterator.next(); \
                 controlIterator.next(); \
                 controlIterator.next(); \
                 j",
            )
            .unwrap();
        assert_eq!(j, Value::Number(1.0));
    }

    #[test]
    fn for_of_destructure_default_tdz() {
        let mut ctx = new_ctx();
        let err = ctx
            .eval(
                "var x; var threw = false; \
                 try { for ({ x = y } of [{}]) { } } \
                 catch (e) { threw = e.name === 'ReferenceError'; } \
                 let y; \
                 threw",
            )
            .unwrap();
        assert_eq!(err, Value::Boolean(true));
    }

    #[test]
    fn for_of_destructure_default_tdz_simple_identifier() {
        let mut ctx = new_ctx();
        let err = ctx
            .eval(
                "var threw = false; \
                 try { y; } \
                 catch (e) { threw = e.name === 'ReferenceError'; } \
                 let y; \
                 threw",
            )
            .unwrap();
        assert_eq!(err, Value::Boolean(true));
    }

    #[test]
    fn tdz_destructure_assign_same_scope() {
        // Same scope: `y` is in TDZ in the same block as the destructuring
        let mut ctx = new_ctx();
        let r1 = ctx.eval("{ let y; ({ x = y } = {}); x }");
        // `let y;` at the top of the block initializes `y` (exits TDZ)
        // So `x = y` should resolve `y` to `undefined`
        assert_eq!(r1.unwrap(), Value::Undefined);
    }

    #[test]
    fn tdz_destructure_assign_before_let() {
        let mut ctx = new_ctx();
        let r1 = ctx.eval(
            "{ \
               ({ x = y } = {}); \
               let y; \
             }",
        );
        // `let y` is hoisted to top of block, so `y` is in TDZ when destructuring runs
        assert!(r1.is_err(), "expected TDZ error, got: {:?}", r1);
    }

    #[test]
    fn tdz_destructure_let_decl_before_let_in_block() {
        let mut ctx = new_ctx();
        let r1 = ctx.eval(
            "{ \
               let { x = y } = {}; \
               let y; \
             }",
        );
        assert!(
            r1.is_err(),
            "expected TDZ error for let decl pattern, got: {:?}",
            r1
        );
    }

    #[test]
    fn tdz_destructure_let_decl_at_script_level() {
        let mut ctx = new_ctx();
        let r1 = ctx.eval(
            "let { x = y } = {}; \
             let y; \
             'ok'",
        );
        assert!(
            r1.is_err(),
            "expected TDZ error at script level (let decl), got: {:?}",
            r1
        );
    }

    #[test]
    fn tdz_destructure_assign_at_script_level() {
        let mut ctx = new_ctx();
        // This matches the test262 pattern: assignment destructuring, let at bottom
        let r1 = ctx.eval(
            "var x; \
             ({ x = y } = {}); \
             let y; \
             'ok'",
        );
        assert!(
            r1.is_err(),
            "expected TDZ error at script level (assign), got: {:?}",
            r1
        );
    }

    #[test]
    fn tdz_for_of_at_script_level() {
        let mut ctx = new_ctx();
        // This matches the test262 for-of pattern
        let r1 = ctx.eval(
            "var x; \
             ({ x = y } = {}); \
             let y; \
             'ok'",
        );
        assert!(r1.is_err(), "expected TDZ error for for-of, got: {:?}", r1);
    }

    #[test]
    fn tdz_in_destructure_default_works_in_standalone() {
        let mut ctx = new_ctx();
        let err = ctx
            .eval(
                "var x; var threw = false; \
                 try { ({ x = y } = {}); } \
                 catch (e) { threw = e.name === 'ReferenceError'; } \
                 let y; \
                 threw",
            )
            .unwrap();
        assert_eq!(err, Value::Boolean(true));
    }

    #[test]
    fn tdz_in_destructure_default_works_in_standalone_let_decl() {
        let mut ctx = new_ctx();
        let err = ctx
            .eval(
                "var threw = false; \
                 try { let { x = y } = {}; } \
                 catch (e) { threw = e.name === 'ReferenceError'; } \
                 let y; \
                 threw",
            )
            .unwrap();
        assert_eq!(err, Value::Boolean(true));
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

    /// Reproduces test262: for-of/dstr/array-elem-iter-rtrn-close.js
    /// When generator.return() is called on a generator with a destructuring
    /// for-of body, IteratorClose must be called BEFORE the body executes.
    #[test]
    fn for_of_destructuring_generator_return_closes_iterator_before_body() {
        let mut ctx = new_ctx();
        let result = ctx.eval(
            "var nextCount = 0; \
             var returnCount = 0; \
             var unreachable = 0; \
             var iterator = { \
               next: function() { nextCount += 1; return {done: false, value: undefined}; }, \
               return: function() { returnCount += 1; return {}; } \
             }; \
             var iterable = {}; \
             iterable[Symbol.iterator] = function() { return iterator; }; \
             function* g() { \
               for ([ {} = yield ] of [iterable]) { unreachable += 1; } \
             } \
             var iter = g(); \
             iter.next(); \
             iter.return(777); \
             JSON.stringify([returnCount, unreachable])",
        );
        match result {
            Ok(Value::String(s)) => {
                assert_eq!(
                    s.as_str(),
                    "[1,0]",
                    "expected IteratorClose before body, got {s}"
                );
            }
            Ok(v) => panic!("expected string [1,0], got {v:?}"),
            Err(e) => panic!("expected [1,0], got error: {e}"),
        }
    }

    /// Reproduces generator-close-via-return.js
    #[test]
    fn for_of_generator_return_closes_via_return() {
        let mut ctx = new_ctx();
        let result = ctx.eval(
            "var finallyCount = 0; \
             function* values() { \
               try { yield; } finally { finallyCount += 1; } \
             } \
             var iterable = values(); \
             iterable.next(); \
             iterable.return(0); \
             finallyCount",
        );
        assert_eq!(result.unwrap(), Value::Number(1.0));
    }

    /// Reproduces test262: for-of/dstr/array-elem-nested-array-yield-expr.js
    /// When `yield` appears in a computed property key within a destructuring
    /// pattern inside a for-of loop, the generator must suspend and resume
    /// correctly — the destructuring must complete with the resumed value
    /// as the property key, and the loop body must execute.
    #[test]
    fn for_of_destructure_yield_in_computed_key() {
        let mut ctx = new_ctx();
        // This reproduces the test262 case: `for ([[x[yield]]] of [value])`
        // with a generator, testing that the body runs after the yield in the
        // computed property key is resolved via iter.next('prop').
        let result = ctx.eval(
            "var value = [[22]]; \
             var x = {}; \
             var iter = (function*() { \
               var counter = 0; \
               for ([[x[yield]]] of [value]) { \
                 counter += 1; \
               } \
               return counter; \
             }()); \
             iter.next(); \
             var r2 = iter.next('prop'); \
             JSON.stringify([r2.done, r2.value, x.prop])",
        );
        match result {
            Ok(Value::String(s)) => {
                assert_eq!(s.as_str(), "[true,1,22]", "expected [true,1,22], got {s}");
            }
            Ok(v) => panic!("expected string result, got {v:?}"),
            Err(e) => panic!("expected success, got error: {e}"),
        }
    }

    /// Simpler: yield directly in for-of destructuring target (no nesting).
    #[test]
    fn for_of_destructure_yield_in_computed_key_simple() {
        let mut ctx = new_ctx();
        let result = ctx.eval(
            "var x = {}; \
             var iter = (function*() { \
               var count = 0; \
               for ([x[yield]] of [[42]]) { \
                 count += 1; \
               } \
               return count; \
             }()); \
             iter.next(); \
             var r2 = iter.next('key'); \
             JSON.stringify([r2.done, r2.value, x.key])",
        );
        match result {
            Ok(Value::String(s)) => {
                assert_eq!(s.as_str(), "[true,1,42]", "expected [true,1,42], got {s}");
            }
            Ok(v) => panic!("expected string result, got {v:?}"),
            Err(e) => panic!("expected success, got error: {e}"),
        }
    }

    /// Reproduces test262:
    /// test/language/statements/for-of/dstr/array-elem-iter-thrw-close-err
    /// When destructuring evaluation of a computed property key throws
    /// (e.g. `{}[thrower()]`), IteratorClose must happen BEFORE any next()
    /// step, the for-of body must NOT execute, and the original error
    /// takes precedence over the return() error.
    #[test]
    fn for_of_destructure_computed_key_throw_closes_iterator_before_next() {
        let mut ctx = new_ctx();
        let result = ctx.eval(
            "var nextCount = 0; \
             var returnCount = 0; \
             function ReturnError() {} \
             var iterator = { \
               next: function() { nextCount += 1; return {done: true}; }, \
               return: function() { returnCount += 1; throw new ReturnError(); } \
             }; \
             var iterable = {}; \
             var thrower = function() { throw new Test262Error(); }; \
             iterable[Symbol.iterator] = function() { return iterator; }; \
             var counter = 0; \
             try { \
               for ([{}[thrower()]] of [iterable]) { counter += 1; } \
             } catch (e) {} \
             JSON.stringify([nextCount, returnCount, counter])",
        );
        match result {
            Ok(Value::String(s)) => {
                // Per ES spec: computed key evaluation happens BEFORE IteratorStep,
                // so next() = 0, return() = 1 (from IteratorClose), counter = 0.
                assert_eq!(s.as_str(), "[0,1,0]", "expected [0,1,0], got {s}");
            }
            Ok(v) => panic!("expected string result, got {v:?}"),
            Err(e) => panic!("expected success, got error: {e}"),
        }
    }

    /// Reproduces test262:
    /// test/language/statements/for-of/dstr/array-elem-trlg-iter-rest-rtrn-close
    /// Iteration is limited to 1 next() call — yield in rest computed key
    /// suspends generator BEFORE remaining elements are collected.
    #[test]
    fn for_of_destructure_rest_yield_iterator_close() {
        let mut ctx = new_ctx();
        let result = ctx.eval(
            "var nextCount = 0; \
             var returnCount = 0; \
             var iterator = { \
               next: function() { \
                 nextCount += 1; \
                 return { done: nextCount > 10, value: nextCount }; \
               }, \
               return: function() { \
                 returnCount += 1; \
                 return {}; \
               } \
             }; \
             var iterable = {}; \
             iterable[Symbol.iterator] = function() { return iterator; }; \
             var gen = (function*() { \
               var counter = 0; \
               for ([x, ...{}[yield]] of [iterable]) { \
                 counter += 1; \
               } \
               return counter; \
             }()); \
             gen.next(); \
             var r2 = gen.return(999); \
             JSON.stringify([nextCount, returnCount, r2.done, r2.value])",
        );
        match result {
            Ok(Value::String(s)) => {
                // After gen.next(): 1 next call (for 'x'), generator suspended at yield.
                // After gen.return(999): iterator.return() called, generator returns 999.
                assert_eq!(
                    s.as_str(),
                    "[1,1,true,999]",
                    "expected [1,1,true,999], got {s}"
                );
            }
            Ok(v) => panic!("expected string result, got {v:?}"),
            Err(e) => panic!("expected success, got error: {e}"),
        }
    }
}
