//! Generator function support — function* and yield.
//!
//! Generators are implemented as objects with a `GeneratorState` that tracks
//! the function body, environment, and current position.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::{Expression, Statement};
use crate::env::Environment;
use crate::value::{Object, ObjectKind, Value};
use crate::JsError;

/// Saved for-of loop state when a generator `yield` suspends mid-iteration.
#[derive(Debug, Clone)]
pub struct ForOfSuspend {
    pub iterator: Rc<RefCell<Object>>,
    pub index: usize,
    pub item: Value,
    pub resume_body: bool,
    pub body_tail: Option<Vec<Statement>>,
    pub resume_mid_delegate: bool,
    pub resume_init: bool,
    pub variable: crate::ast::Expression,
    pub body: Statement,
    pub loop_binding: Option<crate::ast::VarKind>,
    pub dispose_async: Option<bool>,
    pub await_of: bool,
    pub await_values: bool,
    pub per_iteration: bool,
    pub in_arrow_function: bool,
    /// The pending (item, resume) from the current iteration — must be restored
    /// on resume so `take_iterator_step` is NOT called again.
    pub pending: Option<(Value, ForOfResume)>,
}

/// Minimal snapshot of the per-iteration resume state.
#[derive(Debug, Clone, Default)]
pub struct ForOfResume {
    pub body_only: bool,
    pub body_tail: Option<Vec<Statement>>,
    pub mid_delegate: bool,
    pub init: bool,
}

/// Generator state
#[derive(Debug, Clone, PartialEq)]
pub enum GeneratorState {
    Suspended,
    Running,
    Completed,
}

/// A generator object created by calling a generator function.
#[derive(Debug, Clone)]
pub struct GeneratorObject {
    pub body: std::rc::Rc<Vec<Statement>>,
    pub params: Vec<crate::ast::Param>,
    pub closure: Rc<RefCell<Environment>>,
    pub strict: bool,
    pub state: GeneratorState,
    pub yield_index: usize,
    pub yielded_value: Value,
    pub next_value: Value,
    pub is_async: bool,
    pub prototype: Option<Rc<RefCell<Object>>>,
    /// Pre-evaluated arguments for async generators.
    /// When set, params are bound eagerly before the generator is returned.
    pub args: Option<Vec<Value>>,
    /// Statement index suspended mid-evaluation (nested yields in class, etc.)
    pub pending_stmt: Option<usize>,
    /// Yields in `pending_stmt` already completed across prior `.next()` calls.
    pub yields_to_replay: usize,
    /// Resume values for completed yields in the pending statement.
    pub stored_resumes: Vec<Value>,
    /// Execution environment persisted across `.next()` calls.
    pub call_env: Option<Rc<RefCell<Environment>>>,
    /// Mid-for-of suspension when `yield` runs in the loop body.
    pub for_of_suspend: Option<ForOfSuspend>,
    /// Mid-yield* delegation state.
    pub yield_delegate_suspend: Option<YieldDelegateSuspend>,
    pub yielded_done_present: bool,
    pub await_completion: bool,
    pub await_resume: bool,
}

/// Saved yield* delegation iterator position across `.next()` calls.
#[derive(Debug, Clone)]
pub struct YieldDelegateSuspend {
    pub iterator: Rc<RefCell<Object>>,
    pub yielded_result: Option<Rc<RefCell<Object>>>,
    pub index: usize,
    pub await_values: bool,
    pub abrupt_error: Option<(JsError, Value)>,
    pub completion: Option<Value>,
    pub done_present: bool,
}

impl GeneratorObject {
    pub fn new(
        body: std::rc::Rc<Vec<Statement>>,
        params: Vec<crate::ast::Param>,
        closure: Rc<RefCell<Environment>>,
        strict: bool,
    ) -> Self {
        GeneratorObject {
            body,
            params,
            closure,
            strict,
            state: GeneratorState::Suspended,
            yield_index: 0,
            yielded_value: Value::Undefined,
            next_value: Value::Undefined,
            is_async: false,
            prototype: None,
            args: None,
            pending_stmt: None,
            yields_to_replay: 0,
            stored_resumes: Vec::new(),
            call_env: None,
            for_of_suspend: None,
            yield_delegate_suspend: None,
            yielded_done_present: true,
            await_completion: false,
            await_resume: false,
        }
    }

    fn call_env(&mut self) -> Result<Rc<RefCell<Environment>>, JsError> {
        if let Some(ref env) = self.call_env {
            return Ok(Rc::clone(env));
        }
        let call_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(
            &self.closure,
        ))));
        let global_this = self
            .closure
            .borrow()
            .get("globalThis")
            .unwrap_or(Value::Undefined);
        call_env
            .borrow_mut()
            .current_scope()
            .borrow_mut()
            .set_this(global_this.clone());
        if let Some(ref args) = self.args {
            let args_obj =
                crate::eval::class::helpers::create_arguments_object_simple(args.clone());
            call_env
                .borrow_mut()
                .define("arguments".to_string(), args_obj);
            let stub = crate::value::ValueFunction::new(
                None,
                self.params.clone(),
                (*self.body).clone(),
                Rc::clone(&self.closure),
                self.is_async,
                true,
            );
            crate::eval::function::bind_params(&stub, &self.params, args, &call_env)?;
            let body_env_rc = crate::eval::function::function_body_env(
                &call_env,
                &stub,
                &global_this,
                &self.params,
            );
            body_env_rc.borrow_mut().push_scope();
            crate::interpreter::predeclare_var(&self.body, &mut body_env_rc.borrow_mut());
            crate::interpreter::predeclare_let_const(&self.body, &mut body_env_rc.borrow_mut());
            self.call_env = Some(body_env_rc);
            return Ok(Rc::clone(self.call_env.as_ref().unwrap()));
        }
        self.call_env = Some(Rc::clone(&call_env));
        Ok(call_env)
    }

    /// Advance the generator by one step.
    pub fn next(&mut self, value: Value) -> Result<IteratorResult, JsError> {
        let _generator_guard = crate::eval::generator::enter_value_generator();
        if self.pending_stmt.is_none() {
            crate::eval::generator::reset_assignment_state();
        }
        if self.state == GeneratorState::Completed {
            return Ok(IteratorResult {
                value: Value::Undefined,
                done: true,
            });
        }
        self.state = GeneratorState::Running;
        self.await_completion = false;
        let initial_resume = self.yield_index == 0 && self.pending_stmt.is_none();
        self.next_value = if initial_resume {
            Value::Undefined
        } else {
            value
        };

        // Store the resume value so yield expressions can find it
        crate::interpreter::set_generator_resume_value(self.next_value.clone());
        // Save any pending ControlFlow (e.g. from generator.return() / generator.throw())
        // so the yield expression handler (eval/expression.rs) can detect it.
        // Only Yield/YieldDelegate variants are stale carry-over from a prior yield;
        // Return/Throw are fresh completions that must be passed through.
        let pending_cf = crate::interpreter::take_control_flow();
        if let Some(s) = self.for_of_suspend.take() {
            crate::eval::iteration::stage_stored_for_of_suspend(s);
        }
        if let Some(s) = self.yield_delegate_suspend.take() {
            crate::eval::iteration::stage_yield_delegate_suspend(s);
        }

        let call_env = self.call_env()?;

        let prev_strict = crate::interpreter::is_strict_mode();
        crate::interpreter::set_strict_mode(self.strict);
        let previous_eval_env = crate::interpreter::get_current_eval_env();
        crate::interpreter::set_current_eval_env(Some(Rc::clone(&call_env)));

        // Re-set Return/Throw control flow so eval_yield can detect it.
        // Yield/YieldDelegate variants are stale and NOT re-set (they were
        // either handled by the loop body or are left over from a prior suspend).
        if let Some(cf) = &pending_cf {
            match cf {
                crate::interpreter::ControlFlow::Return(_)
                | crate::interpreter::ControlFlow::Throw(_) => {
                    if matches!(cf, crate::interpreter::ControlFlow::Return(_)) {
                        crate::eval::generator::mark_pending_return();
                    }
                    crate::interpreter::set_control_flow(cf.clone());
                }
                _ => {}
            }
        }

        if crate::eval::generator::take_yield_in_finally() {
            if let Some(crate::interpreter::ControlFlow::Return(value)) = pending_cf {
                self.state = GeneratorState::Completed;
                self.pending_stmt = None;
                self.call_env = None;
                crate::interpreter::set_current_eval_env(previous_eval_env);
                crate::interpreter::set_strict_mode(prev_strict);
                return Ok(IteratorResult { value, done: true });
            }
        }

        let start = self.pending_stmt.unwrap_or(0);
        let mut completion = Value::Undefined;
        for (i, stmt) in self.body.iter().enumerate().skip(start) {
            if self.is_async
                && matches!(stmt, Statement::Expression(expr) if matches!(expr.as_ref(), Expression::Await(_)))
            {
                let arg = match stmt {
                    Statement::Expression(expr) => match expr.as_ref() {
                        Expression::Await(arg) => arg,
                        _ => unreachable!(),
                    },
                    _ => unreachable!(),
                };
                let value = crate::eval::eval_expression(arg, &call_env, false)?;
                let awaited = crate::builtins::promise::promise_resolve_impl_static(
                    vec![value],
                    crate::builtins::promise::get_promise_proto(),
                )?;
                self.pending_stmt = Some(i + 1);
                self.await_completion = true;
                self.await_resume = true;
                crate::value::generator_replay::set_resuming_pending_yield(false);
                crate::interpreter::set_current_eval_env(previous_eval_env);
                crate::interpreter::set_strict_mode(prev_strict);
                return Ok(IteratorResult {
                    value: awaited,
                    done: false,
                });
            }
            crate::value::generator_replay::set_resuming_pending_yield(self.pending_stmt.is_some());
            crate::value::generator_replay::begin_stmt_run(
                self.yields_to_replay,
                &self.stored_resumes,
            );
            if let Statement::Return(expr) = stmt {
                self.await_completion = expr.is_some() && self.is_async;
                let return_val = match expr {
                    Some(e) => crate::eval::eval_expression(e, &call_env, false)?,
                    None => Value::Undefined,
                };
                if let Some(yield_val) = crate::interpreter::take_generator_yield() {
                    crate::value::generator_replay::commit_suspend(&mut self.stored_resumes);
                    self.yields_to_replay = self.stored_resumes.len();
                    self.pending_stmt = Some(i);
                    self.yielded_value = yield_val;
                    self.yield_index += 1;
                    self.state = GeneratorState::Suspended;
                    if let Some(s) = crate::eval::iteration::take_pending_for_of_suspend() {
                        self.for_of_suspend = Some(s);
                    }
                    if let Some(s) = crate::eval::iteration::take_pending_yield_delegate_suspend() {
                        self.yielded_done_present = s.done_present;
                        self.yield_delegate_suspend = Some(s);
                    }
                    self.await_completion = self.is_async
                        && self
                            .yield_delegate_suspend
                            .as_ref()
                            .is_none_or(|state| state.await_values);
                    crate::value::generator_replay::set_resuming_pending_yield(false);
                    crate::interpreter::set_current_eval_env(previous_eval_env);
                    crate::interpreter::set_strict_mode(prev_strict);
                    return Ok(IteratorResult {
                        value: self.yielded_value.clone(),
                        done: false,
                    });
                }
                completion = return_val;
                self.state = GeneratorState::Completed;
                self.pending_stmt = None;
                self.yields_to_replay = 0;
                self.stored_resumes.clear();
                self.for_of_suspend = None;
                self.yield_delegate_suspend = None;
                let _ = crate::eval::iteration::take_pending_for_of_suspend();
                let _ = crate::eval::iteration::take_pending_yield_delegate_suspend();
                self.call_env = None;
                crate::value::generator_replay::set_resuming_pending_yield(false);
                crate::interpreter::set_current_eval_env(previous_eval_env);
                crate::interpreter::set_strict_mode(prev_strict);
                return Ok(IteratorResult {
                    value: completion,
                    done: true,
                });
            }
            match crate::eval::eval_statement(stmt, &call_env, false, false) {
                Ok(_val) => {
                    // Check yield FIRST. When generator.return() resumes a generator,
                    // the yield expression handler (eval/expression.rs) detects the
                    // pending ControlFlow::Return and does NOT set the generator yield
                    // flag. So if the yield flag IS set, it's a normal yield or a
                    // `return yield` pattern — suspend the generator.
                    if let Some(yield_val) = crate::interpreter::take_generator_yield() {
                        crate::value::generator_replay::commit_suspend(&mut self.stored_resumes);
                        self.yields_to_replay = self.stored_resumes.len();
                        self.pending_stmt = Some(i);
                        self.yielded_value = yield_val;
                        self.yield_index += 1;
                        self.state = GeneratorState::Suspended;
                        if let Some(s) = crate::eval::iteration::take_pending_for_of_suspend() {
                            self.for_of_suspend = Some(s);
                        }
                        if let Some(s) =
                            crate::eval::iteration::take_pending_yield_delegate_suspend()
                        {
                            self.yielded_done_present = s.done_present;
                            self.yield_delegate_suspend = Some(s);
                        }
                        self.await_completion = self.is_async
                            && self
                                .yield_delegate_suspend
                                .as_ref()
                                .is_none_or(|state| state.await_values);
                        crate::value::generator_replay::set_resuming_pending_yield(false);
                        crate::interpreter::set_current_eval_env(previous_eval_env);
                        crate::interpreter::set_strict_mode(prev_strict);
                        return Ok(IteratorResult {
                            value: self.yielded_value.clone(),
                            done: false,
                        });
                    }
                    if let Some(crate::interpreter::ControlFlow::Return(ret)) =
                        crate::interpreter::take_control_flow()
                    {
                        completion = ret;
                        if self.is_async && crate::eval::r#await::is_promise(&completion) {
                            self.await_completion = true;
                            self.state = GeneratorState::Suspended;
                            crate::value::generator_replay::set_resuming_pending_yield(false);
                            crate::interpreter::set_current_eval_env(previous_eval_env);
                            crate::interpreter::set_strict_mode(prev_strict);
                            return Ok(IteratorResult {
                                value: completion,
                                done: false,
                            });
                        }
                        break;
                    }
                    crate::value::generator_replay::commit_completed_yields(
                        &mut self.stored_resumes,
                    );
                    self.pending_stmt = None;
                    self.yields_to_replay = 0;
                    self.stored_resumes.clear();
                    if let Some(return_val) = crate::interpreter::take_generator_return() {
                        completion = return_val;
                        break;
                    }
                }
                Err(e) => {
                    self.state = GeneratorState::Completed;
                    self.call_env = None;
                    crate::interpreter::set_current_eval_env(previous_eval_env);
                    crate::interpreter::set_strict_mode(prev_strict);
                    return Err(e);
                }
            }
        }

        self.state = GeneratorState::Completed;
        self.pending_stmt = None;
        self.yields_to_replay = 0;
        self.stored_resumes.clear();
        self.for_of_suspend = None;
        self.yield_delegate_suspend = None;
        let _ = crate::eval::iteration::take_pending_for_of_suspend();
        let _ = crate::eval::iteration::take_pending_yield_delegate_suspend();
        self.call_env = None;
        crate::value::generator_replay::set_resuming_pending_yield(false);
        crate::interpreter::set_current_eval_env(previous_eval_env);
        crate::interpreter::set_strict_mode(prev_strict);
        Ok(IteratorResult {
            value: completion,
            done: true,
        })
    }
}

/// Result of a generator step
#[derive(Debug, Clone)]
pub struct IteratorResult {
    pub value: Value,
    pub done: bool,
}

impl IteratorResult {
    pub fn to_object(&self) -> Value {
        let mut obj = Object::new(ObjectKind::Ordinary);
        obj.prototype = crate::builtins::get_object_prototype();
        obj.set("value", self.value.clone());
        obj.set("done", Value::Boolean(self.done));
        Value::Object(Rc::new(RefCell::new(obj)))
    }

    fn to_object_with_done(&self, done_present: bool) -> Value {
        if done_present || self.done {
            return self.to_object();
        }
        let mut obj = Object::new(ObjectKind::Ordinary);
        obj.prototype = crate::builtins::get_object_prototype();
        obj.set("value", self.value.clone());
        Value::Object(Rc::new(RefCell::new(obj)))
    }
}

/// Wrap a generator as an iterator object ({ next, return }) for destructuring.
pub fn generator_as_iterator_object(gen: Rc<RefCell<GeneratorObject>>) -> Rc<RefCell<Object>> {
    let mut obj = Object::new(ObjectKind::Ordinary);
    if gen.borrow().is_async {
        obj.set("next", async_generator_next_fn(Rc::clone(&gen)));
        obj.set("return", async_generator_return_fn(Rc::clone(&gen)));
        obj.set("throw", async_generator_throw_fn(gen));
    } else {
        obj.set("next", generator_next_fn(Rc::clone(&gen)));
        obj.set("return", generator_return_fn(Rc::clone(&gen)));
        obj.set("throw", generator_throw_fn(Rc::clone(&gen)));
    }
    Rc::new(RefCell::new(obj))
}

/// Create a NativeFunction that calls GeneratorObject::next().
pub fn generator_next_fn(gen: Rc<RefCell<GeneratorObject>>) -> Value {
    Value::NativeFunction(std::rc::Rc::new(crate::value::NativeFunction::new(
        move |args| {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            let result = gen
                .try_borrow_mut()
                .map_err(|_| JsError("TypeError: generator is already executing".to_string()))?
                .next(arg)?;
            if let Some(result) = gen
                .borrow_mut()
                .yield_delegate_suspend
                .as_mut()
                .and_then(|state| state.yielded_result.take())
            {
                return Ok(Value::Object(result));
            }
            let done_present = gen.borrow().yielded_done_present;
            Ok(result.to_object_with_done(done_present))
        },
    )))
}

/// Create a NativeFunction that calls GeneratorObject::return().
pub fn generator_return_fn(gen: Rc<RefCell<GeneratorObject>>) -> Value {
    Value::NativeFunction(std::rc::Rc::new(crate::value::NativeFunction::new(
        move |args| {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            let pending_destructuring =
                crate::eval::iteration::take_pending_destructuring_iterator();
            let mut g = gen
                .try_borrow_mut()
                .map_err(|_| JsError("TypeError: generator is already executing".to_string()))?;
            if g.state == GeneratorState::Completed {
                return Ok(IteratorResult {
                    value: arg,
                    done: true,
                }
                .to_object());
            }
            let suspended_start = g.state == GeneratorState::Suspended
                && g.yield_index == 0
                && g.pending_stmt.is_none();
            if suspended_start {
                g.state = GeneratorState::Completed;
                g.call_env = None;
                return Ok(IteratorResult {
                    value: arg,
                    done: true,
                }
                .to_object());
            }
            // If generator is suspended mid for-of, close the inner iterator
            // before resuming. Per ES §25.4.3.7 GeneratorResumeAbrupt:
            // "If generatorKind is async, return ? AsyncGeneratorResolve(generator,
            // value, true)." — but for sync generators, IteratorClose must run
            // so that for-of closes its inner iterator before the return
            // completion propagates through the generator body.
            if let Some(ref suspend) = g.for_of_suspend {
                if let Some(close_err) =
                    crate::eval::object::call_iterator_return(&suspend.iterator)
                {
                    return Err(close_err);
                }
            }
            if let Some(suspend) = g.yield_delegate_suspend.as_ref() {
                match crate::eval::object::call_iterator_return_done(&suspend.iterator, arg.clone())
                {
                    Err(error) => {
                        let reason = crate::value::take_thrown_value()
                            .unwrap_or_else(|| Value::String(error.to_string()));
                        let mut suspend = suspend.clone();
                        suspend.abrupt_error = Some((error, reason));
                        g.yield_delegate_suspend = Some(suspend);
                        let result = g.next(Value::Undefined)?;
                        return Ok(result.to_object());
                    }
                    Ok(Some(false)) => {
                        return Ok(IteratorResult {
                            value: Value::Undefined,
                            done: false,
                        }
                        .to_object());
                    }
                    Ok(Some(true)) | Ok(None) => {}
                }
            }
            if let Some(suspend) = g.yield_delegate_suspend.as_mut() {
                suspend.completion = Some(arg.clone());
            }
            if let Some(iterator) = pending_destructuring {
                if let Some(close_err) = crate::eval::object::call_iterator_return(&iterator) {
                    return Err(close_err);
                }
                g.state = GeneratorState::Completed;
                g.call_env = None;
                return Ok(IteratorResult {
                    value: arg,
                    done: true,
                }
                .to_object());
            }
            crate::interpreter::set_control_flow(crate::interpreter::ControlFlow::Return(
                arg.clone(),
            ));
            crate::eval::generator::mark_pending_return();
            let result = g.next(Value::Undefined);
            crate::eval::generator::take_pending_return();
            let result = result?;
            Ok(result.to_object())
        },
    )))
}

/// Create a NativeFunction that calls GeneratorObject::throw().
pub fn generator_throw_fn(gen: Rc<RefCell<GeneratorObject>>) -> Value {
    Value::NativeFunction(std::rc::Rc::new(crate::value::NativeFunction::new(
        move |args| {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            if gen.borrow().yield_delegate_suspend.is_some() {
                let result = delegate_abrupt(&gen, "throw", arg)?;
                if let Some(result) = gen
                    .borrow_mut()
                    .yield_delegate_suspend
                    .as_mut()
                    .and_then(|state| state.yielded_result.take())
                {
                    return Ok(Value::Object(result));
                }
                return Ok(result.to_object());
            }
            let mut g = gen
                .try_borrow_mut()
                .map_err(|_| JsError("TypeError: generator is already executing".to_string()))?;
            if g.state == GeneratorState::Completed {
                crate::value::set_thrown_value(arg);
                return Err(JsError("Generator threw".to_string()));
            }
            let suspended_start = g.state == GeneratorState::Suspended
                && g.yield_index == 0
                && g.pending_stmt.is_none();
            if suspended_start {
                g.state = GeneratorState::Completed;
                g.call_env = None;
                crate::value::set_thrown_value(arg.clone());
                return Err(JsError(format!(
                    "Generator threw: {}",
                    crate::value::to_js_string(&arg)
                )));
            }
            // If generator is suspended mid for-of, close the inner iterator
            // before resuming — same as generator_return_fn.
            if let Some(ref suspend) = g.for_of_suspend {
                if let Some(close_err) =
                    crate::eval::object::call_iterator_return(&suspend.iterator)
                {
                    return Err(close_err);
                }
            }
            crate::interpreter::set_control_flow(crate::interpreter::ControlFlow::Throw(
                arg.clone(),
            ));
            let result = g.next(Value::Undefined);
            match result {
                Ok(ir) => Ok(ir.to_object()),
                Err(e) => Err(e),
            }
        },
    )))
}

/// Async generator next: wraps result in a Promise.
pub fn async_generator_next_fn(gen: Rc<RefCell<GeneratorObject>>) -> Value {
    Value::NativeFunction(std::rc::Rc::new(crate::value::NativeFunction::new(
        move |args| {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            let proto = crate::builtins::promise::get_promise_proto();
            crate::interpreter::enter_async_generator();
            let (result, await_completion) = {
                let mut generator = gen.borrow_mut();
                let result = generator.next(arg);
                (result, generator.await_completion)
            };
            crate::interpreter::leave_async_generator();
            match result {
                Ok(ir) if await_completion => {
                    resolve_async_result_later(ir, proto, Rc::clone(&gen))
                }
                Ok(ir) => crate::builtins::promise::promise_resolve_impl_static(
                    vec![ir.to_object()],
                    proto,
                ),
                Err(error) => {
                    {
                        let mut generator = gen.borrow_mut();
                        generator.state = GeneratorState::Completed;
                        generator.call_env = None;
                    }
                    let reason = match crate::value::take_thrown_value() {
                        Some(value) => value,
                        None => {
                            crate::value::error::create_js_error_with_type(
                                &error.to_string(),
                                "TypeError",
                            )
                            .0
                        }
                    };
                    crate::builtins::promise::promise_reject_impl_static(vec![reason], proto)
                }
            }
        },
    )))
}

fn queue_async_generator_fulfilled(callback: Value, value: Value) {
    let job = Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |_| {
        crate::eval::function::call_value_with_this(
            callback.clone(),
            vec![value.clone()],
            Value::Undefined,
        )?;
        Ok(Value::Undefined)
    })));
    crate::builtins::promise::queue_microtask_impl(job);
}

fn resolve_async_result_later(
    result: IteratorResult,
    proto: Rc<RefCell<Object>>,
    generator: Rc<RefCell<GeneratorObject>>,
) -> Result<Value, JsError> {
    let source = crate::builtins::promise::promise_resolve_impl_static(
        vec![result.value],
        Rc::clone(&proto),
    )?;
    let mut promise = Object::with_prototype(ObjectKind::Promise, Rc::clone(&proto));
    promise.promise_data = Some(crate::value::object::PromiseObjectData::new());
    let promise = Rc::new(RefCell::new(promise));
    let fulfilled_target = Rc::clone(&promise);
    let rejected_target = Rc::clone(&promise);
    let done = result.done;
    let generator_for_fulfilled = Rc::clone(&generator);
    let fulfilled_slot = Rc::new(RefCell::new(None));
    let fulfilled_slot_for_callback = Rc::clone(&fulfilled_slot);
    let rejected_slot = Rc::new(RefCell::new(None));
    let rejected_slot_for_callback = Rc::clone(&rejected_slot);
    let fulfilled =
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |args| {
            let value = args.first().cloned().unwrap_or(Value::Undefined);
            let should_resume = {
                let Ok(mut generator_ref) = generator_for_fulfilled.try_borrow_mut() else {
                    if let Some(callback) = fulfilled_slot_for_callback.borrow().clone() {
                        queue_async_generator_fulfilled(callback, value);
                    }
                    return Ok(Value::Undefined);
                };
                let should_resume = generator_ref.await_resume;
                generator_ref.await_resume = false;
                should_resume
            };
            if should_resume {
                let (next_result, await_completion) = {
                    let Ok(mut generator_ref) = generator_for_fulfilled.try_borrow_mut() else {
                        if let Some(callback) = fulfilled_slot_for_callback.borrow().clone() {
                            queue_async_generator_fulfilled(callback, value);
                        }
                        return Ok(Value::Undefined);
                    };
                    crate::interpreter::enter_async_generator();
                    let next_result = generator_ref.next(value);
                    let await_completion = generator_ref.await_completion;
                    crate::interpreter::leave_async_generator();
                    (next_result, await_completion)
                };
                let next_value = match next_result {
                    Ok(next) if await_completion => resolve_async_result_later(
                        next,
                        Rc::clone(&proto),
                        Rc::clone(&generator_for_fulfilled),
                    )?,
                    Ok(next) => crate::builtins::promise::promise_resolve_impl_static(
                        vec![next.to_object()],
                        Rc::clone(&proto),
                    )?,
                    Err(error) => crate::builtins::promise::promise_reject_impl_static(
                        vec![crate::value::take_thrown_value()
                            .unwrap_or_else(|| Value::String(error.to_string()))],
                        Rc::clone(&proto),
                    )?,
                };
                crate::builtins::promise::settle_resolve(&fulfilled_target, next_value);
                return Ok(Value::Undefined);
            }
            let result = IteratorResult { value, done }.to_object();
            crate::builtins::promise::settle_resolve(&fulfilled_target, result);
            Ok(Value::Undefined)
        })));
    *fulfilled_slot.borrow_mut() = Some(fulfilled.clone());
    let rejected = Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |args| {
        let reason = args.first().cloned().unwrap_or(Value::Undefined);
        let Ok(mut generator) = generator.try_borrow_mut() else {
            if let Some(callback) = rejected_slot_for_callback.borrow().clone() {
                queue_async_generator_fulfilled(callback, reason);
            }
            return Ok(Value::Undefined);
        };
        generator.state = GeneratorState::Completed;
        generator.call_env = None;
        crate::builtins::promise::settle_reject(&rejected_target, reason);
        Ok(Value::Undefined)
    })));
    *rejected_slot.borrow_mut() = Some(rejected.clone());
    let Value::Object(source) = source else {
        return Ok(Value::Object(promise));
    };
    let then = crate::eval::member::eval_object_member(&source, "then", None)?;
    crate::eval::function::call_value_with_this(
        then,
        vec![fulfilled, rejected],
        Value::Object(source),
    )?;
    Ok(Value::Object(promise))
}

/// Async generator return: wraps result in a Promise.
pub fn async_generator_return_fn(gen: Rc<RefCell<GeneratorObject>>) -> Value {
    Value::NativeFunction(std::rc::Rc::new(crate::value::NativeFunction::new(
        move |args| {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            let proto = crate::builtins::promise::get_promise_proto();
            let pending_suspend = crate::eval::iteration::take_pending_for_of_suspend();
            let pending_destructuring =
                crate::eval::iteration::take_pending_destructuring_iterator();
            if gen.borrow().yield_delegate_suspend.is_some()
                && gen.borrow().for_of_suspend.is_none()
                && pending_suspend.is_none()
            {
                return async_delegate_return_queued(Rc::clone(&gen), arg, proto);
            }
            let close_error = gen
                .borrow()
                .for_of_suspend
                .as_ref()
                .and_then(|suspend| crate::eval::object::call_iterator_return(&suspend.iterator))
                .or_else(|| {
                    pending_suspend.as_ref().and_then(|suspend| {
                        crate::eval::object::call_iterator_return(&suspend.iterator)
                    })
                })
                .or_else(|| {
                    pending_destructuring
                        .as_ref()
                        .and_then(crate::eval::object::call_iterator_return)
                });
            if let Some(error) = close_error {
                let reason = crate::value::take_thrown_value()
                    .unwrap_or_else(|| Value::String(error.to_string()));
                gen.borrow_mut().state = GeneratorState::Completed;
                gen.borrow_mut().for_of_suspend = None;
                return crate::builtins::promise::promise_reject_impl_static(vec![reason], proto);
            }
            gen.borrow_mut().state = GeneratorState::Completed;
            gen.borrow_mut().for_of_suspend = None;
            resolve_async_result_queued(
                IteratorResult {
                    value: arg,
                    done: true,
                },
                proto,
                Rc::clone(&gen),
            )
        },
    )))
}

fn async_delegate_return_queued(
    generator: Rc<RefCell<GeneratorObject>>,
    argument: Value,
    proto: Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
    let mut promise = Object::with_prototype(ObjectKind::Promise, Rc::clone(&proto));
    promise.promise_data = Some(crate::value::object::PromiseObjectData::new());
    let promise = Rc::new(RefCell::new(promise));
    let source =
        crate::builtins::promise::promise_resolve_impl_static(vec![argument], proto.clone())?;
    chain_delegate_argument(source, promise.clone(), generator, proto)?;
    Ok(Value::Object(promise))
}

fn chain_delegate_argument(
    source: Value,
    target: Rc<RefCell<Object>>,
    generator: Rc<RefCell<GeneratorObject>>,
    proto: Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
    let Value::Object(source) = source else {
        return Ok(Value::Undefined);
    };
    let fulfilled_target = Rc::clone(&target);
    let fulfilled =
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |args| {
            let value = args.first().cloned().unwrap_or(Value::Undefined);
            let result = async_delegate_abrupt(&generator, "return", value, Rc::clone(&proto))?;
            chain_async_result(result, Rc::clone(&fulfilled_target))?;
            Ok(Value::Undefined)
        })));
    let rejected = Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |args| {
        let reason = args.first().cloned().unwrap_or(Value::Undefined);
        crate::builtins::promise::settle_reject(&target, reason);
        Ok(Value::Undefined)
    })));
    let then = crate::eval::member::eval_object_member(&source, "then", None)?;
    crate::eval::function::call_value_with_this(
        then,
        vec![fulfilled, rejected],
        Value::Object(source),
    )
}

fn async_delegate_abrupt(
    generator: &Rc<RefCell<GeneratorObject>>,
    method_name: &str,
    argument: Value,
    proto: Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
    let Some(state) = generator.borrow().yield_delegate_suspend.clone() else {
        return resolve_async_result_later(
            IteratorResult {
                value: argument,
                done: true,
            },
            proto,
            Rc::clone(generator),
        );
    };
    crate::interpreter::enter_async_generator();
    let result = delegate_abrupt(generator, method_name, argument);
    crate::interpreter::leave_async_generator();
    match result {
        Ok(result) if result.done && method_name == "throw" => {
            resume_delegate_completion(generator, state, result.value, proto)
        }
        Ok(result) => resolve_async_result_later(result, proto, Rc::clone(generator)),
        Err(error) => resume_async_delegate_error(generator, state, error, proto),
    }
}

fn resume_delegate_completion(
    generator: &Rc<RefCell<GeneratorObject>>,
    mut state: YieldDelegateSuspend,
    value: Value,
    proto: Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
    state.completion = Some(value);
    let mut generator_ref = generator.borrow_mut();
    generator_ref.yield_delegate_suspend = Some(state);
    generator_ref.state = GeneratorState::Suspended;
    crate::interpreter::enter_async_generator();
    let result = generator_ref.next(Value::Undefined);
    let await_completion = generator_ref.await_completion;
    crate::interpreter::leave_async_generator();
    drop(generator_ref);
    match result {
        Ok(result) if await_completion => {
            resolve_async_result_later(result, proto, Rc::clone(generator))
        }
        Ok(result) => {
            crate::builtins::promise::promise_resolve_impl_static(vec![result.to_object()], proto)
        }
        Err(error) => {
            let reason = crate::value::take_thrown_value()
                .unwrap_or_else(|| Value::String(error.to_string()));
            crate::builtins::promise::promise_reject_impl_static(vec![reason], proto)
        }
    }
}

fn resume_async_delegate_error(
    generator: &Rc<RefCell<GeneratorObject>>,
    mut state: YieldDelegateSuspend,
    error: JsError,
    proto: Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
    let reason =
        crate::value::take_thrown_value().unwrap_or_else(|| Value::String(error.to_string()));
    state.abrupt_error = Some((error, reason));
    let mut generator_ref = generator.borrow_mut();
    generator_ref.yield_delegate_suspend = Some(state);
    generator_ref.state = GeneratorState::Suspended;
    crate::interpreter::enter_async_generator();
    let result = generator_ref.next(Value::Undefined);
    let await_completion = generator_ref.await_completion;
    crate::interpreter::leave_async_generator();
    drop(generator_ref);
    match result {
        Ok(result) if await_completion => {
            resolve_async_result_later(result, proto, Rc::clone(generator))
        }
        Ok(result) => {
            crate::builtins::promise::promise_resolve_impl_static(vec![result.to_object()], proto)
        }
        Err(error) => {
            let reason = crate::value::take_thrown_value()
                .unwrap_or_else(|| Value::String(error.to_string()));
            crate::builtins::promise::promise_reject_impl_static(vec![reason], proto)
        }
    }
}

fn delegate_abrupt(
    generator: &Rc<RefCell<GeneratorObject>>,
    method_name: &str,
    argument: Value,
) -> Result<IteratorResult, JsError> {
    let state = generator
        .borrow_mut()
        .yield_delegate_suspend
        .take()
        .unwrap();
    let method = match crate::eval::member::eval_object_member(&state.iterator, method_name, None) {
        Ok(method) => method,
        Err(error) => return resume_delegate_error(generator, state, error),
    };
    if matches!(method, Value::Undefined | Value::Null) {
        if method_name == "throw" {
            let close =
                match crate::eval::member::eval_object_member(&state.iterator, "return", None) {
                    Ok(close) => close,
                    Err(error) => return resume_delegate_error(generator, state, error),
                };
            if !matches!(close, Value::Undefined | Value::Null) {
                if !close.is_callable() {
                    return resume_delegate_error(
                        generator,
                        state,
                        generator_type_error("iterator return is not callable"),
                    );
                }
                let result = match crate::eval::function::call_value_with_this(
                    close,
                    vec![],
                    Value::Object(Rc::clone(&state.iterator)),
                ) {
                    Ok(result) => result,
                    Err(error) => return resume_delegate_error(generator, state, error),
                };
                let _ = crate::eval::object::await_async_iterator_result(result)?;
            }
            return resume_delegate_error(
                generator,
                state,
                generator_type_error("iterator does not provide a throw method"),
            );
        }
        generator.borrow_mut().state = GeneratorState::Completed;
        return Ok(IteratorResult {
            value: argument,
            done: true,
        });
    }
    if !method.is_callable() {
        return resume_delegate_error(
            generator,
            state,
            generator_type_error("iterator method is not callable"),
        );
    }
    let result = match crate::eval::function::call_value_with_this(
        method,
        vec![argument],
        Value::Object(Rc::clone(&state.iterator)),
    ) {
        Ok(result) => result,
        Err(error) => return resume_delegate_error(generator, state, error),
    };
    let result = crate::eval::object::await_async_iterator_result(result)?;
    let Value::Object(result) = result else {
        return resume_delegate_error(
            generator,
            state,
            generator_type_error("iterator result is not an object"),
        );
    };
    match finish_delegate_abrupt(generator, state.clone(), result) {
        Ok(result) => Ok(result),
        Err(error) if generator.borrow().is_async => Err(error),
        Err(error) => resume_delegate_error(generator, state, error),
    }
}

fn resume_delegate_error(
    generator: &Rc<RefCell<GeneratorObject>>,
    mut state: YieldDelegateSuspend,
    error: JsError,
) -> Result<IteratorResult, JsError> {
    let reason =
        crate::value::take_thrown_value().unwrap_or_else(|| Value::String(error.to_string()));
    state.abrupt_error = Some((error, reason));
    generator.borrow_mut().yield_delegate_suspend = Some(state);
    Ok(generator.borrow_mut().next(Value::Undefined)?)
}

fn finish_delegate_abrupt(
    generator: &Rc<RefCell<GeneratorObject>>,
    mut state: YieldDelegateSuspend,
    result: Rc<RefCell<Object>>,
) -> Result<IteratorResult, JsError> {
    let done = crate::eval::member::eval_object_member(&result, "done", None)?;
    let done = crate::value::to_bool(&done);
    let is_async = generator.borrow().is_async;
    let value = if done || is_async {
        crate::eval::member::eval_object_member(&result, "value", None)?
    } else {
        state.yielded_result = Some(result);
        Value::Undefined
    };
    if done && !is_async {
        state.completion = Some(value);
        let mut generator = generator.borrow_mut();
        generator.yield_delegate_suspend = Some(state);
        generator.state = GeneratorState::Suspended;
        return generator.next(Value::Undefined);
    }
    let mut generator = generator.borrow_mut();
    generator.state = if done {
        GeneratorState::Completed
    } else {
        generator.yield_delegate_suspend = Some(state);
        GeneratorState::Suspended
    };
    Ok(IteratorResult { value, done })
}

fn generator_type_error(message: &str) -> JsError {
    let (value, error) = crate::value::create_js_error_with_type(message, "TypeError");
    crate::value::set_thrown_value(value);
    error
}

fn resolve_async_result_queued(
    result: IteratorResult,
    proto: Rc<RefCell<Object>>,
    generator: Rc<RefCell<GeneratorObject>>,
) -> Result<Value, JsError> {
    let mut promise = Object::with_prototype(ObjectKind::Promise, Rc::clone(&proto));
    promise.promise_data = Some(crate::value::object::PromiseObjectData::new());
    let promise = Rc::new(RefCell::new(promise));
    let target = Rc::clone(&promise);
    let job = Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |_| {
        match resolve_async_result_later(result.clone(), Rc::clone(&proto), Rc::clone(&generator)) {
            Ok(source) => chain_async_result(source, Rc::clone(&target))?,
            Err(error) => {
                crate::builtins::promise::settle_reject(&target, Value::String(error.to_string()))
            }
        }
        Ok(Value::Undefined)
    })));
    crate::builtins::promise::queue_microtask_impl(job);
    Ok(Value::Object(promise))
}

fn chain_async_result(source: Value, target: Rc<RefCell<Object>>) -> Result<(), JsError> {
    let Value::Object(source) = source else {
        return Ok(());
    };
    let fulfilled_target = Rc::clone(&target);
    let fulfilled =
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |args| {
            let value = args.first().cloned().unwrap_or(Value::Undefined);
            crate::builtins::promise::settle_resolve(&fulfilled_target, value);
            Ok(Value::Undefined)
        })));
    let rejected = Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |args| {
        let reason = args.first().cloned().unwrap_or(Value::Undefined);
        crate::builtins::promise::settle_reject(&target, reason);
        Ok(Value::Undefined)
    })));
    let then = crate::eval::member::eval_object_member(&source, "then", None)?;
    crate::eval::function::call_value_with_this(
        then,
        vec![fulfilled, rejected],
        Value::Object(source),
    )?;
    Ok(())
}

/// Async generator throw: returns a rejected Promise.
pub fn async_generator_throw_fn(gen: Rc<RefCell<GeneratorObject>>) -> Value {
    Value::NativeFunction(std::rc::Rc::new(crate::value::NativeFunction::new(
        move |args| {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            let proto = crate::builtins::promise::get_promise_proto();
            if gen.borrow().yield_delegate_suspend.is_some() {
                return async_delegate_abrupt(&gen, "throw", arg, proto);
            }
            gen.borrow_mut().state = GeneratorState::Completed;
            crate::builtins::promise::promise_reject_impl_static(vec![arg], proto)
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    #[test]
    fn test_generator_state_eq() {
        assert_eq!(GeneratorState::Suspended, GeneratorState::Suspended);
        assert_eq!(GeneratorState::Running, GeneratorState::Running);
        assert_eq!(GeneratorState::Completed, GeneratorState::Completed);
    }

    #[test]
    fn test_generator_state_neq() {
        assert_ne!(GeneratorState::Suspended, GeneratorState::Running);
        assert_ne!(GeneratorState::Suspended, GeneratorState::Completed);
        assert_ne!(GeneratorState::Running, GeneratorState::Completed);
    }

    #[test]
    fn test_generator_new_defaults() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let body = Rc::new(vec![Statement::Expression(Box::new(Expression::Number(
            1.0,
        )))]);
        let gen = GeneratorObject::new(body, vec![], env, true);
        assert_eq!(gen.state, GeneratorState::Suspended);
        assert_eq!(gen.yield_index, 0);
        assert_eq!(gen.yielded_value, Value::Undefined);
        assert_eq!(gen.next_value, Value::Undefined);
        assert!(gen.strict);
        assert!(!gen.is_async);
        assert!(gen.prototype.is_none());
    }

    #[test]
    fn test_generator_next_empty_body() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let body = Rc::new(vec![]);
        let mut gen = GeneratorObject::new(body, vec![], env, false);
        let result = gen.next(Value::Undefined).unwrap();
        assert!(result.done);
        assert_eq!(result.value, Value::Undefined);
        assert_eq!(gen.state, GeneratorState::Completed);
    }

    #[test]
    fn test_generator_next_already_completed() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let body = Rc::new(vec![]);
        let mut gen = GeneratorObject::new(body, vec![], env, false);
        gen.state = GeneratorState::Completed;
        let result = gen.next(Value::Number(99.0)).unwrap();
        assert!(result.done);
        assert_eq!(result.value, Value::Undefined);
        // Still completed
        assert_eq!(gen.state, GeneratorState::Completed);
    }

    #[test]
    fn test_iterator_result_undone() {
        let ir = IteratorResult {
            value: Value::Number(42.0),
            done: false,
        };
        let obj_val = ir.to_object();
        let obj = match obj_val {
            Value::Object(ref o) => o,
            _ => panic!("Expected Object"),
        };
        assert_eq!(obj.borrow().get("value"), Some(Value::Number(42.0)));
        assert_eq!(obj.borrow().get("done"), Some(Value::Boolean(false)));
    }

    #[test]
    fn test_iterator_result_done() {
        let ir = IteratorResult {
            value: Value::String("fin".into()),
            done: true,
        };
        let obj_val = ir.to_object();
        let obj = match obj_val {
            Value::Object(ref o) => o,
            _ => panic!("Expected Object"),
        };
        assert_eq!(obj.borrow().get("value"), Some(Value::String("fin".into())));
        assert_eq!(obj.borrow().get("done"), Some(Value::Boolean(true)));
    }

    #[test]
    fn test_generator_next_fn_returns_native_fn() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let gen = Rc::new(RefCell::new(GeneratorObject::new(
            Rc::new(vec![]),
            vec![],
            env,
            false,
        )));
        assert!(matches!(generator_next_fn(gen), Value::NativeFunction(_)));
    }

    #[test]
    fn test_generator_return_fn_returns_native_fn() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let gen = Rc::new(RefCell::new(GeneratorObject::new(
            Rc::new(vec![]),
            vec![],
            env,
            false,
        )));
        assert!(matches!(generator_return_fn(gen), Value::NativeFunction(_)));
    }

    #[test]
    fn test_generator_throw_fn_returns_native_fn() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let gen = Rc::new(RefCell::new(GeneratorObject::new(
            Rc::new(vec![]),
            vec![],
            env,
            false,
        )));
        assert!(matches!(generator_throw_fn(gen), Value::NativeFunction(_)));
    }

    #[test]
    fn test_count_yields_in_expr() {
        use crate::value::generator_replay::count_yields_in_expr;
        assert_eq!(count_yields_in_expr(&Expression::Yield(None)), 1);
        assert_eq!(
            count_yields_in_expr(&Expression::Yield(Some(Box::new(Expression::Number(1.0))))),
            1,
        );
        assert_eq!(
            count_yields_in_expr(&Expression::YieldDelegate(Box::new(
                Expression::Identifier("x".into())
            ))),
            1,
        );
        assert_eq!(count_yields_in_expr(&Expression::Number(42.0)), 0);
        assert_eq!(count_yields_in_expr(&Expression::Boolean(true)), 0);
    }

    #[test]
    fn test_count_yields_in_stmt() {
        use crate::value::generator_replay::count_yields_in_stmt;
        assert_eq!(
            count_yields_in_stmt(&Statement::Expression(Box::new(Expression::Yield(None)))),
            1,
        );
        assert_eq!(
            count_yields_in_stmt(&Statement::Return(Some(Box::new(Expression::Yield(None))))),
            1,
        );
        assert_eq!(
            count_yields_in_stmt(&Statement::VarDeclaration {
                kind: crate::ast::VarKind::Let,
                name: "x".into(),
                init: None,
            }),
            0,
        );
    }

    #[test]
    fn test_generator_clone() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let body = Rc::new(vec![]);
        let gen = GeneratorObject::new(body, vec![], env, true);
        let gen_clone = gen.clone();
        assert_eq!(gen.state, gen_clone.state);
        assert_eq!(gen.yield_index, gen_clone.yield_index);
        assert_eq!(gen.strict, gen_clone.strict);
    }

    #[test]
    fn test_generator_debug_output() {
        let gen_str = format!("{:?}", GeneratorState::Suspended);
        assert!(!gen_str.is_empty());
    }

    #[test]
    fn test_generator_bindings_persist_across_yield_in_later_statement() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "function* g() { yield 1; let c = 2; return yield c; } \
                 var iter = g(); iter.next(); iter.next(1); iter.next(3).value",
            )
            .unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    /// Test that a generator with a simple yield body returns properly.
    /// This tests via JS eval to verify the full stack works.
    #[test]
    fn test_generator_via_eval_create() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval("function* g() { yield 1; } typeof g").unwrap();
        assert_eq!(result, Value::String("function".into()));
    }

    #[test]
    fn test_generator_via_eval_call_returns_generator_object() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval("function* g() { yield 1; } let gen = g(); typeof gen")
            .unwrap();
        assert_eq!(result, Value::String("object".into()));
    }

    #[test]
    fn test_generator_via_eval_next_method_exists() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval("function* g() { yield 1; } let gen = g(); typeof gen.next")
            .unwrap();
        assert_eq!(result, Value::String("function".into()));
    }

    #[test]
    fn test_generator_via_eval_next_returns_object() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval("function* g() { yield 1; } let gen = g(); typeof gen.next()")
            .unwrap();
        assert_eq!(result, Value::String("object".into()));
    }

    #[test]
    fn test_generator_via_eval_next_value() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval("function* g() { yield 1; } let gen = g(); gen.next().value")
            .unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn test_generator_via_eval_next_done() {
        let mut ctx = crate::Context::new().unwrap();
        // A generator with one yield: first next() returns {value: 1, done: false}
        let done = ctx
            .eval("function* g() { yield 1; } let gen = g(); gen.next().done")
            .unwrap();
        assert_eq!(done, Value::Boolean(false));

        // Second next() should return {value: undefined, done: true}
        let done2 = ctx
            .eval("function* g() { yield 1; } let gen = g(); gen.next(); gen.next().done")
            .unwrap();
        assert_eq!(done2, Value::Boolean(true));
    }

    #[test]
    fn test_generator_via_eval_multiple_yields() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "function* g() { yield 1; yield 2; yield 3; } \
             let gen = g(); \
             let a = gen.next().value; \
             let b = gen.next().value; \
             let c = gen.next().value; \
             [a, b, c]",
            )
            .unwrap();
        // Check array result
        match result {
            Value::Object(ref obj) => {
                let arr = obj.borrow();
                assert_eq!(arr.elements.first(), Some(&Value::Number(1.0)));
                assert_eq!(arr.elements.get(1), Some(&Value::Number(2.0)));
                assert_eq!(arr.elements.get(2), Some(&Value::Number(3.0)));
            }
            _ => panic!("Expected array object"),
        }
    }

    #[test]
    fn generator_return_runs_finally_once() {
        let mut ctx = crate::Context::new().unwrap();
        let count = ctx
            .eval(
                "var finallyCount = 0; \
                 function* g() { try { yield; } finally { finallyCount += 1; } } \
                 var gen = g(); gen.next(); gen.return(0); finallyCount",
            )
            .unwrap();
        assert_eq!(count, Value::Number(1.0));
    }

    #[test]
    fn test_generator_return_method() {
        let mut ctx = crate::Context::new().unwrap();
        let done = ctx
            .eval(
                "function* g() { yield 1; yield 2; } \
             let gen = g(); \
             gen.next(); \
             gen.return(99).value",
            )
            .unwrap();
        assert_eq!(done, Value::Number(99.0));
    }

    #[test]
    fn test_generator_throw_method() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "function* g() { yield 1; } \
             let gen = g(); \
             gen.next(); \
             try { gen.throw(new Error('test')); 'no_error' } catch(e) { 'error' }",
            )
            .unwrap();
        assert_eq!(result, Value::String("error".into()));
    }

    #[test]
    fn test_async_generator_returns_promise_from_next() {
        let mut ctx = crate::Context::new().unwrap();
        // Calling an async generator returns an async generator object
        let result = ctx
            .eval("async function* ag() { yield 1; } let gen = ag(); typeof gen.next")
            .unwrap();
        assert_eq!(result, Value::String("function".into()));
        // Calling .next() on an async generator should return a Promise (check via .then)
        let result = ctx
            .eval("async function* ag() { yield 1; } let gen = ag(); let p = gen.next(); typeof p")
            .unwrap();
        assert_eq!(result, Value::String("object".into()));
    }

    #[test]
    fn test_async_generator_next_returns_pending_promise() {
        let mut ctx = crate::Context::new().unwrap();
        // Verify Promise works first
        let result = ctx.eval("typeof Promise.resolve().then").unwrap();
        assert_eq!(result, Value::String("function".into()));
        // Check if the async generator's next method returns a Promise
        // by looking at what typeof gen.next()() returns (the function call result)
        let _result = ctx
            .eval(
                r#"
            async function* ag() { yield 1; }
            let gen = ag();
            // gen.next is a function
            let nextFn = gen.next;
            // Call it - should return a Promise
            let p = nextFn();
            String([typeof p, typeof p.then])
        "#,
            )
            .unwrap();
        // p should be {done: false, value: <promise>}
        let result = ctx.eval("async function* ag() { yield 1; } let gen = ag(); let p = gen.next(); typeof p.then").unwrap();
        assert_eq!(result, Value::String("function".into()));
    }

    #[test]
    fn async_yield_star_null_throw_closes_iterator_and_rejects() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var throwGets = 0, returnGets = 0, result; \
             var source = { [Symbol.asyncIterator]() { return this; }, \
               next() { return { value: 1, done: false }; }, \
               get throw() { throwGets++; return null; }, \
               get return() { returnGets++; } }; \
             async function* g() { yield* source; } var iterator = g(); \
             iterator.next().then(function() { return iterator.throw(); }, function(e) { result = e; }) \
             .then(function(e) { result = e; }, function(e) { result = e; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("throwGets"), Some(crate::Value::Number(1.0)));
        assert_eq!(
            ctx.get_global("returnGets"),
            Some(crate::Value::Number(1.0))
        );
        assert!(matches!(
            ctx.get_global("result"),
            Some(crate::Value::Object(_))
        ));
    }

    #[test]
    fn async_generator_yield_thenable_exposes_reject_function() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var result; var thenable = { then(resolve, reject) { resolve(reject); } }; \
             var iter = (async function*() { yield thenable; }()); \
             iter.next().then(function(value) { result = [typeof value.value, value.value.length, value.value.name]; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert!(matches!(
            ctx.get_global("result"),
            Some(crate::Value::Object(_))
        ));
        assert_eq!(
            ctx.eval("result[0] + ',' + result[1] + ',' + result[2]")
                .unwrap(),
            crate::Value::String("function,1,".into())
        );
    }

    #[test]
    fn test_async_generator_is_async_flag() {
        // Test that async generators have is_async = true
        let mut ctx = crate::Context::new().unwrap();
        // Verify we can call async generator and get a result
        let result = ctx
            .eval("async function* ag() { yield 1; } let gen = ag(); typeof gen.next()")
            .unwrap();
        // .next() should return something callable (a Promise)
        assert_eq!(result, Value::String("object".into()));
    }

    #[test]
    fn test_async_generator_call_returns_generator_object() {
        let mut ctx = crate::Context::new().unwrap();
        // Calling an async generator function returns an object with next method
        let result = ctx
            .eval("async function* ag() { yield 1; } let gen = ag(); typeof gen")
            .unwrap();
        assert_eq!(result, Value::String("object".into()));
    }

    #[test]
    fn test_async_generator_with_default_params() {
        // Reproduces test262: async-gen-method/dflt-params-arg-val-not-undefined.js
        // When called with explicit args, defaults should NOT be evaluated.
        let mut ctx = crate::Context::new().unwrap();
        // Simple case first - async generator with default param
        let result = ctx
            .eval(
                r#"
            async function* gen(a = 42) {
                return a;
            }
            let g = gen();
            typeof g.next
        "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("function".into()));
    }

    #[test]
    fn test_async_generator_with_explicit_args_no_default_eval() {
        // Test262: default params should NOT be evaluated when explicit args are passed
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                r#"
            var evaluated = false;
            async function* gen(a = (evaluated = true, 1)) {
                return a;
            }
            let g = gen(99);
            // At this point default was NOT evaluated
            typeof g.next
        "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("function".into()));
    }

    #[test]
    fn async_generator_nested_array_param_runs_default() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "var count = 0; \
                 async function* f([[] = function() { count += 1; return []; }()]) {} \
                 f([]).next(); count",
            )
            .unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn async_generator_yield_delegate_prefers_async_iterator() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "var used = ''; \
                 var obj = { \
                   get [Symbol.asyncIterator]() { used = 'async'; return function() { \
                     return { next: function() { return { done: true }; } }; \
                   }; }, \
                   get [Symbol.iterator]() { used = 'sync'; throw 0; } \
                 }; \
                 async function* f() { yield* obj; } \
                 f().next(); used",
            )
            .unwrap();
        assert_eq!(result, Value::String("async".into()));
    }

    #[test]
    fn async_generator_next_rejects_with_thrown_value() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var reason = { marker: 1 }; var caught; \
             async function* f() { throw reason; } \
             f().next().catch(function(error) { caught = error; });",
        )
        .unwrap();
        let result = ctx.eval("caught.marker").unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn async_generator_next_rejects_with_yielded_rejected_promise() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var error = new Error(); \
             var firstValue; var firstReason; var secondResult; \
             var iter = (async function*() { yield Promise.reject(error); yield 'unreachable'; })(); \
             iter.next().then(() => { firstValue = true; }, (reason) => { \
               firstReason = reason; \
               iter.next().then((result) => { secondResult = result; }); \
             });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(
            ctx.eval("firstValue === undefined").unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            ctx.eval("firstReason === error").unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            ctx.eval("secondResult.done && secondResult.value === undefined")
                .unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn async_generator_yield_in_destructuring_keeps_iterator_suspend() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var iterable = { [Symbol.iterator]() { return { next() { return { done: false, value: undefined }; }, return() { return null; } }; } }; \
             async function* fn() { for await ([{} = yield] of [iterable]) {} }",
        )
        .unwrap();
        let generator = match ctx.eval("fn()").unwrap() {
            Value::Generator(generator) => generator,
            _ => panic!("expected generator"),
        };
        crate::eval::function::call_value_with_this(
            async_generator_next_fn(Rc::clone(&generator)),
            vec![],
            Value::Generator(Rc::clone(&generator)),
        )
        .unwrap();
        crate::builtins::promise::execute_pending_microtasks().unwrap();
        assert!(generator.borrow().for_of_suspend.is_some());
    }

    #[test]
    fn async_generator_for_await_rejected_yield_closes_generator() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var error = new Error(); var first; var second; \
             async function* gen() { for await (let value of [Promise.reject(error), 'unreachable']) { yield value; } } \
             var iter = gen(); iter.next().catch(function(value) { first = value; iter.next().then(function(result) { second = result; }); });",
        )
        .unwrap();
        assert_eq!(ctx.eval("first === error").unwrap(), Value::Boolean(true));
        assert_eq!(
            ctx.eval("second.done === true && second.value === undefined")
                .unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn async_generator_for_await_destructuring_rejects_with_type_error_object() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var constructor; var iterCount = 0; async function* fn() { for await ([[ _ ]] of [[null]]) { iterCount += 1; } } \
             fn().next().catch(function(error) { constructor = error.constructor; });",
        )
        .unwrap();
        assert_eq!(
            ctx.eval("constructor === TypeError").unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(ctx.eval("iterCount").unwrap(), Value::Number(0.0));
    }

    #[test]
    fn promise_rejection_arrow_object_pattern_receives_reason() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var seen; Promise.reject({ constructor: TypeError }).then(undefined, ({ constructor }) => { seen = constructor; });",
        )
        .unwrap();
        crate::builtins::promise::execute_pending_microtasks().unwrap();
        assert_eq!(
            ctx.eval("seen === TypeError").unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn async_generator_return_microtask_order() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var actual = []; \
             async function* implicit() {} \
             async function* explicit() { return undefined; } \
             Promise.resolve().then(function() { actual.push('tick 1'); }) \
               .then(function() { actual.push('tick 2'); }); \
             implicit().next().then(function() { actual.push('implicit'); }); \
             explicit().next().then(function() { actual.push('explicit'); });",
        )
        .unwrap();
        let result = ctx.eval("actual.join(',')").unwrap();
        assert_eq!(
            result,
            Value::String("tick 1,implicit,tick 2,explicit".into())
        );
    }

    #[test]
    fn async_generator_queued_next_does_not_reenter_body() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "var count = 0; \
                 async function* f() { count += 1; yield; } \
                 var iterator = f(); iterator.next(); iterator.next(); count",
            )
            .unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn async_generator_queued_next_keeps_each_result() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var first; var second; \
             async function* f() { yield; } \
             var iterator = f(); \
             iterator.next().then(function(result) { first = result.done; }); \
             iterator.next().then(function(result) { second = result.done; });",
        )
        .unwrap();
        let result = ctx.eval("String(first) + ',' + String(second)").unwrap();
        assert_eq!(result, Value::String("false,true".into()));
    }

    #[test]
    fn async_generator_with_unscopable_updates_lexical_binding() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "let count = 0; \
                 globalThis[Symbol.unscopables] = { count: true }; \
                 async function* f() { with (globalThis) { count++; } } \
                 f().next(); count",
            )
            .unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn async_generator_with_unscopable_falls_through_to_hoisted_var() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var observed = 1; var failure; globalThis.v = 1; \
             globalThis[Symbol.unscopables] = { v: true }; \
             async function* f() { \
               with (globalThis) { observed = v; } \
               var v = 10; \
             } \
             f().next().catch(function(error) { failure = error; });",
        )
        .unwrap();
        let result = ctx
            .eval("String(observed) + ',' + String(failure)")
            .unwrap();
        assert_eq!(result, Value::String("undefined,undefined".into()));
    }

    #[test]
    fn async_generator_with_unscopable_updates_hoisted_var() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var before; var during; var after; var global; var failure; \
             globalThis.v = 1; globalThis[Symbol.unscopables] = { v: true }; \
             async function* f(x) { \
               with (globalThis) { before = v; } \
               var v = x; \
               with (globalThis) { during = v; v = 20; } \
               after = v; global = globalThis.v; \
             } \
             f(10).next().catch(function(error) { failure = error; });",
        )
        .unwrap();
        let result = ctx
            .eval(
                "String(before) + ',' + String(during) + ',' + String(after) + ',' + \
                 String(global) + ',' + String(failure)",
            )
            .unwrap();
        assert_eq!(result, Value::String("undefined,10,20,1,undefined".into()));
    }

    #[test]
    fn async_generator_with_calls_runs_synchronously_until_completion() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "let count = 0; var v = 1; \
                 var assert = { sameValue: function(actual, expected) { \
                   if (actual !== expected) throw actual; \
                 } }; \
                 globalThis[Symbol.unscopables] = { v: true }; \
                 { \
                 count++; \
                 async function* f(x) { \
                   count++; \
                   with (globalThis) { count++; assert.sameValue(v, undefined); } \
                   count++; var v = x; \
                   with (globalThis) { count++; assert.sameValue(v, 10); v = 20; } \
                   assert.sameValue(v, 20); assert.sameValue(globalThis.v, 1); \
                 } \
                 f(10).next(); count++; } \
                 String(count) + ',' + String(globalThis.count)",
            )
            .unwrap();
        assert_eq!(result, Value::String("6,undefined".into()));
    }

    #[test]
    fn async_generator_return_awaits_thenable_getter() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var calls = 0; async function* f() { yield 1; } \
             var iterator = f(); iterator.next(); \
             iterator.return({ get then() { calls += 1; } });",
        )
        .unwrap();
        let result = ctx.eval("calls").unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn async_generator_return_thenable_microtask_order() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var actual = []; async function* f() { actual.push('start'); yield 1; } \
             Promise.resolve().then(function() { actual.push('tick 1'); }) \
               .then(function() { actual.push('tick 2'); }); \
             var iterator = f(); iterator.next(); \
             iterator.return({ get then() { actual.push('get then'); } });",
        )
        .unwrap();
        let result = ctx.eval("actual.join(',')").unwrap();
        assert_eq!(result, Value::String("start,tick 1,get then,tick 2".into()));
    }

    #[test]
    fn async_generator_yield_delegate_awaits_next_result() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var yielded; var iterator = { \
               [Symbol.asyncIterator]: function() { return this; }, \
               next: function() { return Promise.resolve({ value: 42, done: false }); } \
             }; \
             async function* f() { return yield* iterator; } \
             f().next().then(function(result) { yielded = result.value; });",
        )
        .unwrap();
        let result = ctx.eval("yielded").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn async_generator_for_await_rejects_yielded_rejected_promise() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var error = {}; var result; var count = 0; \
             async function* readFile() { yield Promise.reject(error); yield 'unreachable'; } \
             async function* gen() { count += 1; for await (let line of readFile()) { yield line; } } \
             gen().next().then(function() { result = 'resolved'; }, function(reason) { result = reason; });",
        )
        .unwrap();
        crate::builtins::promise::execute_pending_microtasks().unwrap();
        assert_eq!(ctx.eval("count").unwrap(), Value::Number(1.0));
        assert_eq!(ctx.eval("result === error").unwrap(), Value::Boolean(true));
    }

    #[test]
    fn async_generator_yield_delegate_forwards_next_value() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var calls = []; var first; var second; var count = 0; \
             var iterator = { \
               [Symbol.asyncIterator]: function() { return this; }, \
               next: function(value) { calls.push(value); count += 1; \
                 return Promise.resolve(count === 1 \
                   ? { value: 42, done: false } : { value: 99, done: true }); \
               } \
             }; \
             async function* f() { return yield* iterator; } \
             var generator = f(); \
             generator.next().then(function(result) { first = result.value; }); \
             generator.next(7).then(function(result) { second = result.value; });",
        )
        .unwrap();
        let result = ctx
            .eval("String(calls[0]) + ',' + calls[1] + ',' + first + ',' + second")
            .unwrap();
        assert_eq!(result, Value::String("undefined,7,42,99".into()));
    }

    #[test]
    fn async_generator_yield_delegate_forwards_return() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var received; var returned; var iterator = { \
               [Symbol.asyncIterator]: function() { return this; }, \
               next: function() { return Promise.resolve({ value: 1, done: false }); }, \
               return: function(value) { received = value; \
                 return Promise.resolve({ value: value + 1, done: true }); \
               } \
             }; \
             async function* f() { return yield* iterator; } \
             var generator = f(); generator.next(); \
             generator.return(7).then(function(result) { returned = result.value; });",
        )
        .unwrap();
        let result = ctx.eval("received + ',' + returned").unwrap();
        assert_eq!(result, Value::String("7,8".into()));
    }

    #[test]
    fn async_generator_yield_delegate_forwards_throw() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var received; var returned; var iterator = { \
               [Symbol.asyncIterator]: function() { return this; }, \
               next: function() { return Promise.resolve({ value: 1, done: false }); }, \
               throw: function(value) { received = value; \
                 return Promise.resolve({ value: value + 1, done: true }); \
               } \
             }; \
             async function* f() { return yield* iterator; } \
             var generator = f(); generator.next(); \
             generator.throw(7).then(function(result) { returned = result.value; });",
        )
        .unwrap();
        let result = ctx.eval("received + ',' + returned").unwrap();
        assert_eq!(result, Value::String("7,8".into()));
    }

    #[test]
    fn async_generator_yield_delegate_throw_value_getter_error_is_caught() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var token = {}; var result; var iterator = { \
             [Symbol.asyncIterator]: function() { return this; }, \
             next: function() { return { done: false, value: undefined }; }, \
             throw: function() { return { done: false, get value() { throw token; } }; } \
             }; \
             async function* f() { var thrown; try { yield* iterator; } catch (e) { thrown = e; } return thrown; } \
             var generator = f(); generator.next().then(function() { generator.throw().then(function(step) { result = step.value; }); });",
        )
        .unwrap();
        crate::builtins::promise::execute_pending_microtasks().unwrap();
        assert_eq!(ctx.eval("result === token").unwrap(), Value::Boolean(true));
    }

    #[test]
    fn async_generator_yield_delegate_return_value_getter_rejects() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var reason = {}; var caught; var iterator = { \
               [Symbol.asyncIterator]: function() { return this; }, \
               next: function() { return { value: 1, done: false }; }, \
               return: function() { return { \
                 done: false, get value() { throw reason; } \
               }; } \
             }; \
             async function* f() { return yield* iterator; } \
             var generator = f(); generator.next(); \
             generator.return().catch(function(error) { caught = error; });",
        )
        .unwrap();
        let result = ctx.eval("caught === reason").unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn async_generator_yield_delegate_forwards_repeated_throw() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var calls = []; var values = []; var iterator = { \
               [Symbol.asyncIterator]: function() { return this; }, \
               next: function() { return { value: 1, done: false }; }, \
             throw: function(value) { calls.push(value); return calls.length === 1 \
                 ? { value: 2, done: false } : { value: 3, done: true }; } \
             }; \
             async function* f() { return yield* iterator; } \
             var generator = f(); generator.next().then(function() { \
               return generator.throw(7); \
             }).then(function(result) { values.push(result.value); \
               return generator.throw(8); \
             }).then(function(result) { values.push(result.value); });",
        )
        .unwrap();
        let result = ctx
            .eval("calls.join(',') + ';' + values.join(',')")
            .unwrap();
        assert_eq!(result, Value::String("7,8;2,3".into()));
    }

    #[test]
    fn async_generator_yield_delegate_queues_return_argument() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var actual = []; var iterator = { \
               [Symbol.asyncIterator]: function() { return this; }, \
               next: function() { return { value: 1, done: false }; }, \
               get return() { actual.push('get return'); } \
             }; \
             Promise.resolve().then(function() { actual.push('tick 1'); }) \
               .then(function() { actual.push('tick 2'); }); \
             async function* f() { yield* iterator; } \
             var generator = f(); generator.next(); \
             generator.return({ get then() { actual.push('get then'); } });",
        )
        .unwrap();
        let result = ctx.eval("actual.join(',')").unwrap();
        assert_eq!(
            result,
            Value::String("tick 1,get then,tick 2,get return,get then".into())
        );
    }

    #[test]
    fn async_generator_yield_delegate_missing_return_awaits_value() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var returned; var iterator = { \
               [Symbol.asyncIterator]: function() { return this; }, \
               next: function() { return { value: 1, done: false }; } \
             }; \
             async function* f() { yield* iterator; } \
             var generator = f(); generator.next().then(function() { \
               return generator.return(Promise.resolve(3)); \
             }).then(function(result) { returned = result.value; });",
        )
        .unwrap();
        let result = ctx.eval("returned").unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn async_generator_function_exposes_iterator_prototype() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "var completed = false; \
             var proto = Object.getPrototypeOf(\
               (async function*(){}).constructor.prototype.prototype); \
             Object.defineProperty(proto, Symbol.iterator, { get: function() { throw 1; } }); \
             Object.defineProperty(proto, Symbol.asyncIterator, { get: function() { throw 2; } }); \
             async function* f() { yield* []; } \
             f().next().then(function() { completed = true; });",
        )
        .unwrap();
        let result = ctx.eval("completed").unwrap();
        assert_eq!(result, Value::Boolean(true));
    }
}
