//! Event-loop pump: drives nextTick, promise jobs, timers, and
//! immediates until no referenced work remains, then `beforeExit`
//! and `exit` handlers — Node's end-of-run sequence.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::timers::TimerKind;

fn call_callback(cb: &Value, receiver: &Value, args: &[Value]) -> Result<(), VmError> {
    quench_runtime::execute::call(cb, receiver, args)?;
    Ok(())
}

fn call_timer(
    state: &Rc<RefCell<HostState>>,
    domain: Option<&Value>,
    callback: &Value,
    receiver: &Value,
    args: &[Value],
) -> Result<(), VmError> {
    if !quench_runtime::is_callable(callback) {
        return Ok(());
    }
    if let Some(domain) = domain {
        let mut call_args = Vec::with_capacity(args.len() + 1);
        call_args.push(callback.clone());
        call_args.extend(args.iter().cloned());
        crate::modules::domain::run(state, Some(domain), &call_args).map(|_| ())
    } else {
        call_guarded(state, callback, receiver, args)
    }
}

/// Call a timer/immediate callback. A thrown value dispatches to
/// `process.on('uncaughtException')` handlers when registered,
/// mirroring Node; without handlers it unwinds the run.
fn call_guarded(
    state: &Rc<RefCell<HostState>>,
    cb: &Value,
    receiver: &Value,
    args: &[Value],
) -> Result<(), VmError> {
    call_guarded_report(state, cb, receiver, args).map(|_| ())
}

/// Invoke one callback and report whether an exception was handled by the
/// uncaught-exception boundary.  Immediate queues use this bit to preserve
/// Node's phase rule: after a handled throw, the rest of the current
/// immediate snapshot runs before its newly queued nextTick callbacks.
fn call_guarded_report(
    state: &Rc<RefCell<HostState>>,
    cb: &Value,
    receiver: &Value,
    args: &[Value],
) -> Result<bool, VmError> {
    let Err(error) = call_callback(cb, receiver, args) else {
        return Ok(false);
    };
    let VmError::Thrown(thrown) = error else {
        return Err(error);
    };
    let has_capture = state
        .borrow()
        .process
        .uncaught_exception_capture_callback
        .is_some();
    if state
        .borrow()
        .process
        .uncaught_exception_handlers
        .is_empty()
        && !has_capture
    {
        return Err(VmError::Thrown(thrown));
    }
    run_uncaught_handlers(state, &thrown)?;
    Ok(true)
}

/// Run the registered `uncaughtException` handlers for one thrown
/// value; `once` handlers fire a single time.
fn run_uncaught_handlers(state: &Rc<RefCell<HostState>>, thrown: &Value) -> Result<(), VmError> {
    if let Some(callback) = state
        .borrow()
        .process
        .uncaught_exception_capture_callback
        .clone()
    {
        return call_callback(&callback, &Value::Undefined, std::slice::from_ref(thrown));
    }
    let handlers = crate::modules::timers::take_once_handlers(
        state,
        crate::modules::timers::HandlerKind::UncaughtException,
    );
    for handler in handlers {
        call_callback(
            &handler,
            &Value::Undefined,
            &[
                thrown.clone(),
                Value::String("uncaughtException".to_string()),
            ],
        )?;
    }
    Ok(())
}

/// Route an uncaught exception the way Node does: when
/// `uncaughtException` handlers are registered, the thrown value is
/// stashed as pending and the caller drives `__quench_uncaught__()`
/// inside an active VM frame to run them; the process then
/// continues. Without handlers the error unwinds the run.
pub fn handle_uncaught(state: &Rc<RefCell<HostState>>, error: VmError) -> Result<(), VmError> {
    let VmError::Thrown(thrown) = error else {
        return Err(error);
    };
    let has_capture = state
        .borrow()
        .process
        .uncaught_exception_capture_callback
        .is_some();
    if state
        .borrow()
        .process
        .uncaught_exception_handlers
        .is_empty()
        && !has_capture
    {
        return Err(VmError::Thrown(thrown));
    }
    state.borrow_mut().pending_uncaught = Some(thrown);
    Ok(())
}

/// Drive the loop until a promise settles. Returns the rejection
/// reason as a thrown error; errors when the loop empties with the
/// promise still pending (Node would hang — we fail honestly).
pub fn await_promise(state: &Rc<RefCell<HostState>>, promise: &Value) -> Result<(), VmError> {
    let Value::Promise(data) = promise else {
        return Ok(());
    };
    loop {
        if let Some(error) = crate::modules::async_hooks::take_fatal_error(state) {
            return Err(VmError::Thrown(error));
        }
        if let Some(result) = settled(&data.state.borrow()) {
            return result;
        }
        crate::modules::net::poll(state)?;
        quench_runtime::expire_async_waiters();
        drain_ticks(state)?;
        quench_runtime::drain_promise_jobs();
        if let Some(error) = crate::modules::async_hooks::take_fatal_error(state) {
            return Err(VmError::Thrown(error));
        }
        drain_unhandled_rejections(state)?;
        fire_due_timers(state)?;
        drain_immediates(state)?;
        cleanup_retired_timers(state);
        // Timer/immediate callbacks can queue promise jobs; drain them
        // before deciding whether any work remains.
        drain_ticks(state)?;
        quench_runtime::drain_promise_jobs();
        if !has_pending(state) {
            if let Some(result) = settled(&data.state.borrow()) {
                return result;
            }
            let message = Value::String("test did not complete: promise never settled".into());
            return Err(VmError::Thrown(message));
        }
        sleep_until_next(state);
    }
}

pub fn await_promise_with_timeout(
    state: &Rc<RefCell<HostState>>,
    promise: &Value,
    timeout_ms: f64,
) -> Result<bool, VmError> {
    let Value::Promise(data) = promise else {
        return Ok(false);
    };
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs_f64(timeout_ms / 1000.0);
    loop {
        if let Some(error) = crate::modules::async_hooks::take_fatal_error(state) {
            return Err(VmError::Thrown(error));
        }
        if std::time::Instant::now() >= deadline {
            return Ok(true);
        }
        if let Some(result) = settled(&data.state.borrow()) {
            result?;
            return Ok(false);
        }
        crate::modules::net::poll(state)?;
        quench_runtime::expire_async_waiters();
        drain_ticks(state)?;
        quench_runtime::drain_promise_jobs();
        if let Some(error) = crate::modules::async_hooks::take_fatal_error(state) {
            return Err(VmError::Thrown(error));
        }
        drain_unhandled_rejections(state)?;
        fire_due_timers(state)?;
        drain_immediates(state)?;
        drain_ticks(state)?;
        quench_runtime::drain_promise_jobs();
        sleep_until_next(state);
    }
}

fn settled(state: &quench_runtime::value::PromiseState) -> Option<Result<(), VmError>> {
    use quench_runtime::value::PromiseState;
    match state {
        PromiseState::Fulfilled(_) => Some(Ok(())),
        PromiseState::Rejected(reason) => Some(Err(VmError::Thrown(reason.clone()))),
        PromiseState::Pending => None,
    }
}

/// `__quench_uncaught__()` — run the stashed uncaught exception
/// through the registered handlers. Must be called inside an active
/// execution frame (the runner drives it like `__quench_run_exit__`).
pub fn run_uncaught(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let Some(thrown) = state.borrow_mut().pending_uncaught.take() else {
        return Ok(());
    };
    run_uncaught_handlers(state, &thrown)
}

/// Drive the event loop until no referenced work remains, then run
/// `beforeExit` and `exit` handlers. First callback error unwinds,
/// mirroring Node's uncaught-exception exit.
pub fn run_event_loop(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    loop {
        if let Some(error) = crate::modules::async_hooks::take_fatal_error(state) {
            return Err(VmError::Thrown(error));
        }
        crate::modules::net::poll(state)?;
        quench_runtime::expire_async_waiters();
        drain_ticks(state)?;
        quench_runtime::drain_promise_jobs();
        drain_unhandled_rejections(state)?;
        if let Some(error) = crate::modules::async_hooks::take_fatal_error(state) {
            return Err(VmError::Thrown(error));
        }
        fire_due_timers(state)?;
        drain_immediates(state)?;
        // Work created by a timer/immediate belongs to this turn.  Drain its
        // promise and rejection edges before deciding that lifecycle work is
        // quiescent, otherwise `exit` observers race the final rejection.
        drain_ticks(state)?;
        quench_runtime::drain_promise_jobs();
        drain_unhandled_rejections(state)?;
        if !has_pending(state) {
            let handlers = state.borrow().process.before_exit_handlers.clone();
            run_lifecycle_handlers(state, &handlers, 0)?;
            if !has_pending(state) {
                break;
            }
        }
        sleep_until_next(state);
    }
    run_exit_handlers(state)
}

/// Run `process.on('exit')` handlers once with the exit code.
pub fn run_exit_handlers(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    {
        let mut guard = state.borrow_mut();
        if guard.process.exit_handlers_ran {
            return Ok(());
        }
        guard.process.exit_handlers_ran = true;
    }
    let code = state.borrow().process.exit_code.unwrap_or(0);
    let handlers = state.borrow().process.exit_handlers.clone();
    run_lifecycle_handlers(state, &handlers, code)
}

fn run_lifecycle_handlers(
    state: &Rc<RefCell<HostState>>,
    handlers: &[(Value, bool)],
    code: i32,
) -> Result<(), VmError> {
    for (handler, once) in handlers {
        if *once {
            let mut guard = state.borrow_mut();
            if let Some(index) = guard
                .process
                .exit_handlers
                .iter()
                .position(|(candidate, is_once)| *is_once && candidate == handler)
            {
                guard.process.exit_handlers.remove(index);
            }
            if let Some(index) = guard
                .process
                .before_exit_handlers
                .iter()
                .position(|(candidate, is_once)| *is_once && candidate == handler)
            {
                guard.process.before_exit_handlers.remove(index);
            }
        }
        call_callback(handler, &Value::Undefined, &[Value::Number(code as f64)])?;
    }
    Ok(())
}

fn drain_ticks(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    loop {
        let drained = match drain_one_tick(state) {
            Ok(value) => value,
            Err(error) => {
                return Err(error);
            }
        };
        if !drained {
            quench_runtime::drain_promise_jobs();
            return Ok(());
        }
    }
}

fn drain_unhandled_rejections(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    for (promise, reason) in quench_runtime::take_unhandled_rejections() {
        if promise.rejection_handled() {
            continue;
        }
        let mode = state.borrow().process.unhandled_rejection_mode;
        let has_handlers = !state
            .borrow()
            .process
            .unhandled_rejection_handlers
            .is_empty();
        if matches!(mode, crate::modules::process::UnhandledRejectionMode::None) {
            if has_handlers {
                emit_unhandled_event(state, &promise, &reason)?;
            }
        } else if matches!(mode, crate::modules::process::UnhandledRejectionMode::Warn) {
            crate::modules::process::emit_unhandled_rejection_warnings(state, &reason);
            emit_unhandled_event(state, &promise, &reason)?;
        } else if has_handlers {
            emit_unhandled_event(state, &promise, &reason)?;
            if matches!(mode, crate::modules::process::UnhandledRejectionMode::Throw) {
                continue;
            }
            emit_uncaught_rejection(state, &reason)?;
        } else if !state
            .borrow()
            .process
            .uncaught_exception_handlers
            .is_empty()
        {
            emit_uncaught_rejection(state, &reason)?;
        } else if matches!(mode, crate::modules::process::UnhandledRejectionMode::Throw) {
            // With no lifecycle handler Node terminates the process for an
            // unhandled rejection and reports the rejection reason as the
            // uncaught error. Preserve both edges: the exit status and the
            // thrown value must cross the host boundary independently.
            state.borrow_mut().process.exit_code = Some(1);
            return Err(VmError::Thrown(reason.clone()));
        }
    }
    Ok(())
}

fn emit_unhandled_event(
    state: &Rc<RefCell<HostState>>,
    promise: &Rc<quench_runtime::value::PromiseData>,
    reason: &Value,
) -> Result<(), VmError> {
    crate::modules::process::emit(
        state,
        &[
            Value::String("unhandledRejection".into()),
            reason.clone(),
            Value::Promise(promise.clone()),
        ],
    )?;
    if promise.rejection_handled() {
        crate::modules::process::emit(
            state,
            &[
                Value::String("rejectionHandled".into()),
                Value::Promise(promise.clone()),
            ],
        )?;
    }
    Ok(())
}

fn emit_uncaught_rejection(state: &Rc<RefCell<HostState>>, reason: &Value) -> Result<(), VmError> {
    if !state
        .borrow()
        .process
        .uncaught_exception_handlers
        .is_empty()
    {
        let error = unhandled_rejection_error(reason);
        crate::modules::process::emit(
            state,
            &[
                Value::String("uncaughtException".into()),
                error,
                Value::String("unhandledRejection".into()),
            ],
        )?;
    }
    Ok(())
}

fn unhandled_rejection_error(reason: &Value) -> Value {
    if !matches!(reason, Value::Null | Value::Undefined) {
        return reason.clone();
    }
    let rendered = match reason {
        Value::Null => "null",
        _ => "undefined",
    };
    let message = format!(
        "This error originated either by throwing inside of an async function without a catch block, or by rejecting a promise which was not handled with .catch(). The promise rejected with the reason \"{rendered}\"."
    );
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message)],
    );
    let error = quench_runtime::execute::set_property(
        error,
        "name",
        Value::String("UnhandledPromiseRejection".into()),
    );
    quench_runtime::execute::set_property(
        error,
        "code",
        Value::String("ERR_UNHANDLED_REJECTION".into()),
    )
}

pub(crate) fn drain_one_tick(state: &Rc<RefCell<HostState>>) -> Result<bool, VmError> {
    let item = {
        let host = state.borrow();
        let mut queue = host.event_loop.microtasks.borrow_mut();
        (!queue.is_empty()).then(|| queue.remove(0))
    };
    let Some(task) = item else {
        return Ok(false);
    };
    let previous_scope = state.borrow().cluster.process_scope();
    let previous_event_scope = state.borrow().event_loop.process_scope();
    if task.process_scope != 0 {
        state
            .borrow_mut()
            .cluster
            .set_process_scope(task.process_scope);
    }
    state
        .borrow()
        .event_loop
        .set_process_scope(task.process_scope);
    if let Some(resource) = &task.resource {
        crate::modules::async_hooks::resource_before(state, Some(resource), &[])?;
    }
    let previous_stack = task.domain_stack.as_ref().map(|stack| {
        let previous = crate::modules::domain::stack_values(state);
        let base = stack.get(..stack.len().saturating_sub(1)).unwrap_or(&[]);
        crate::modules::domain::replace_stack(state, base);
        previous
    });
    let result = if let Some(domain) = task.domain.as_ref() {
        let mut call_args = Vec::with_capacity(task.args.len() + 1);
        call_args.push(task.callback.clone());
        call_args.extend(task.args.iter().cloned());
        crate::modules::domain::run(state, Some(domain), &call_args).map(|_| ())
    } else {
        call_guarded(
            state,
            &task.callback,
            task.receiver.as_ref().unwrap_or(&Value::Undefined),
            &task.args,
        )
    };
    if let Some(previous) = previous_stack.as_ref() {
        crate::modules::domain::replace_stack(state, previous);
    }
    if task.resource.is_some() {
        crate::modules::async_hooks::resource_after(state, None, &[])?;
    }
    let result = match result {
        Err(error) if task.process_scope != 0 => {
            if crate::modules::cluster::fail_fork_process(state, task.process_scope, 1)? {
                Ok(())
            } else {
                Err(error)
            }
        }
        result => result,
    };
    state.borrow_mut().cluster.set_process_scope(previous_scope);
    state
        .borrow()
        .event_loop
        .set_process_scope(previous_event_scope);
    result.map(|()| true)
}

/// Advance timers under the deterministic mock clock. Mock `tick()` is an
/// explicit host event-loop turn, so it also runs unref'd timers that would
/// not keep a real process alive on their own.
pub(crate) fn drain_mock_timers(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    // Mock-clock turns follow Node's check phase: immediates scheduled for the
    // turn run before zero-delay timeouts, even when both are due at the same
    // virtual timestamp.
    drain_immediates(state)?;
    fire_due_timers(state)?;
    drain_ticks(state)?;
    quench_runtime::drain_promise_jobs();
    Ok(())
}

fn fire_due_timers(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let now = super::timers::monotonic_ms();
    let mut due: Vec<(u64, u64, u64)> = state
        .borrow()
        .timers
        .timers
        .iter()
        .filter(|(_, t)| !matches!(t.kind, TimerKind::Immediate) && t.active && t.fire_at <= now)
        .map(|(id, t)| (t.fire_at, t.order, *id))
        .collect();
    due.sort();
    for (_, _, id) in due {
        fire_one_timer(state, id, now)?;
        drain_ticks(state)?;
    }
    Ok(())
}

fn cleanup_retired_timers(state: &Rc<RefCell<HostState>>) {
    let retired = state
        .borrow()
        .timers
        .timers
        .iter()
        .filter_map(|(id, timer)| timer.retired.then_some(*id))
        .collect::<Vec<_>>();
    for id in retired {
        if let Some(timer) = state.borrow_mut().timers.timers.remove(&id) {
            super::timers::clear_timer_metadata(&timer.object);
        }
    }
}

fn fire_one_timer(state: &Rc<RefCell<HostState>>, id: u64, now: u64) -> Result<(), VmError> {
    let (cb, receiver, args, resource, destroy, domain) = {
        let mut guard = state.borrow_mut();
        let registry = &mut guard.timers;
        let Some(timer) = registry.timers.get_mut(&id) else {
            return Ok(());
        };
        match timer.kind {
            TimerKind::Timeout => {
                super::timers::mark_destroyed(timer);
                (
                    timer.callback.clone(),
                    timer.object.clone(),
                    timer.args.clone(),
                    timer.async_resource.clone(),
                    true,
                    timer.domain.clone(),
                )
            }
            _ => {
                timer.fire_at = now.saturating_add(timer.period.max(1));
                (
                    timer.callback.clone(),
                    timer.object.clone(),
                    timer.args.clone(),
                    timer.async_resource.clone(),
                    false,
                    timer.domain.clone(),
                )
            }
        }
    };
    crate::modules::async_hooks::resource_before(state, Some(&resource), &[])?;
    let result = call_timer(state, domain.as_ref(), &cb, &receiver, &args);
    crate::modules::async_hooks::resource_after(state, None, &[])?;
    let converted = destroy
        && result.is_ok()
        && quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
            &receiver, "_repeat",
        ));
    if converted {
        if let Some(timer) = state.borrow_mut().timers.timers.get_mut(&id) {
            timer.kind = TimerKind::Interval;
            timer.active = true;
            timer.retired = false;
            timer.referenced = true;
            timer.fire_at = now.saturating_add(timer.period.max(1));
            super::timers::mark_reactivated(timer);
        }
    }
    if !destroy && result.is_ok() {
        let disabled = matches!(
            quench_runtime::execute::get_property(&receiver, "_onTimeout"),
            Value::Null
        ) || matches!(
            quench_runtime::execute::get_property(&receiver, "_idleTimeout"),
            Value::Number(value) if value < 0.0
        );
        if disabled {
            if let Some(timer) = state.borrow_mut().timers.timers.get_mut(&id) {
                timer.active = false;
            }
        }
    }
    // A one-shot timeout normally disappears after its callback.  `refresh()`
    // is the observable exception: it reactivates the same registry entry
    // while the callback is running, so retain it with its updated deadline.
    let refreshed = destroy
        && !converted
        && state.borrow().timers.timers.get(&id).is_some_and(|timer| {
            timer.active && matches!(*timer.destroyed.borrow(), Value::Boolean(false))
        });
    if destroy && !converted && !refreshed {
        if let Some(timer) = state.borrow_mut().timers.timers.get_mut(&id) {
            timer.active = false;
            timer.retired = true;
            super::timers::mark_destroyed(timer);
        }
        crate::modules::async_hooks::resource_destroy(state, Some(&resource), &[])?;
    }
    result
}

fn drain_immediates(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let mut defer_ticks = false;
    let queued: Vec<crate::modules::event_loop::Immediate> = state
        .borrow()
        .event_loop
        .immediates
        .borrow_mut()
        .drain(..)
        .collect();
    for task in queued {
        if let Some(resource) = task.resource.as_ref() {
            crate::modules::async_hooks::resource_before(state, Some(resource), &[])?;
        }
        let result = call_guarded_report(state, &task.callback, &Value::Undefined, &task.args);
        if task.resource.is_some() {
            crate::modules::async_hooks::resource_after(state, None, &[])?;
        }
        defer_ticks |= result?;
        if let Some(resource) = task.resource.as_ref() {
            crate::modules::async_hooks::resource_destroy(state, Some(resource), &[])?;
        }
    }
    let mut ids: Vec<u64> = state
        .borrow()
        .timers
        .timers
        .iter()
        .filter(|(_, t)| matches!(t.kind, TimerKind::Immediate))
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable();
    for id in ids {
        let Some(timer) = state.borrow_mut().timers.timers.remove(&id) else {
            continue;
        };
        super::timers::mark_destroyed(&timer);
        // An unref'd immediate is only eligible while some other referenced
        // resource keeps the loop alive. With no such resource Node exits
        // without invoking it.
        if !timer.referenced && !has_referenced_work(state) {
            super::timers::clear_timer_metadata(&timer.object);
            continue;
        }
        crate::modules::async_hooks::resource_before(state, Some(&timer.async_resource), &[])?;
        let result = match timer.domain.as_ref() {
            Some(domain) => call_timer(
                state,
                Some(domain),
                &timer.callback,
                &timer.object,
                &timer.args,
            )
            .map(|_| false),
            None => call_guarded_report(state, &timer.callback, &timer.object, &timer.args),
        };
        crate::modules::async_hooks::resource_after(state, None, &[])?;
        crate::modules::async_hooks::resource_destroy(state, Some(&timer.async_resource), &[])?;
        super::timers::clear_timer_metadata(&timer.object);
        defer_ticks |= result?;
        if !defer_ticks {
            drain_ticks(state)?;
        }
    }
    if defer_ticks {
        drain_ticks(state)?;
    }
    Ok(())
}

fn has_referenced_work(state: &Rc<RefCell<HostState>>) -> bool {
    let guard = state.borrow();
    guard
        .timers
        .timers
        .values()
        .any(|timer| timer.referenced && timer.active)
        || crate::modules::net::has_work(state)
}

fn has_pending(state: &Rc<RefCell<HostState>>) -> bool {
    let guard = state.borrow();
    quench_runtime::has_pending_promise_jobs()
        || quench_runtime::has_pending_unhandled_rejections()
        || !guard.event_loop.microtasks.borrow().is_empty()
        || !guard.event_loop.immediates.borrow().is_empty()
        || guard
            .timers
            .timers
            .values()
            .any(|t| t.referenced && t.active)
        || crate::modules::net::has_work(state)
}

fn sleep_until_next(state: &Rc<RefCell<HostState>>) {
    // Network handles are externally driven.  A future timer must not make
    // the pump sleep past readable/closed socket state: polling at a short
    // cadence lets FINs and close transitions settle before the next timer.
    if crate::modules::net::has_work(state) {
        std::thread::sleep(std::time::Duration::from_millis(4));
        return;
    }
    let guard = state.borrow();
    let next = guard
        .timers
        .timers
        .values()
        .filter(|t| t.active)
        .map(|t| t.fire_at)
        .min();
    if let Some(next) = next {
        let now = super::timers::monotonic_ms();
        if next > now {
            std::thread::sleep(std::time::Duration::from_millis(next - now));
        }
        return;
    }
}
