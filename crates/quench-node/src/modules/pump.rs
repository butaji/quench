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

/// Call a timer/immediate callback. A thrown value dispatches to
/// `process.on('uncaughtException')` handlers when registered,
/// mirroring Node; without handlers it unwinds the run.
fn call_guarded(
    state: &Rc<RefCell<HostState>>,
    cb: &Value,
    receiver: &Value,
    args: &[Value],
) -> Result<(), VmError> {
    let Err(error) = call_callback(cb, receiver, args) else {
        return Ok(());
    };
    let VmError::Thrown(thrown) = error else {
        return Err(error);
    };
    if state
        .borrow()
        .process
        .uncaught_exception_handlers
        .is_empty()
    {
        return Err(VmError::Thrown(thrown));
    }
    run_uncaught_handlers(state, &thrown)
}

/// Run the registered `uncaughtException` handlers for one thrown
/// value; `once` handlers fire a single time.
fn run_uncaught_handlers(state: &Rc<RefCell<HostState>>, thrown: &Value) -> Result<(), VmError> {
    let handlers = crate::modules::timers::take_once_handlers(
        state,
        crate::modules::timers::HandlerKind::UncaughtException,
    );
    for handler in handlers {
        call_callback(&handler, &Value::Undefined, std::slice::from_ref(thrown))?;
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
    if state
        .borrow()
        .process
        .uncaught_exception_handlers
        .is_empty()
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
        if let Some(result) = settled(&data.state.borrow()) {
            return result;
        }
        crate::modules::net::poll(state)?;
        drain_ticks(state)?;
        quench_runtime::drain_promise_jobs();
        fire_due_timers(state)?;
        drain_immediates(state)?;
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
        crate::modules::net::poll(state)?;
        drain_ticks(state)?;
        quench_runtime::drain_promise_jobs();
        fire_due_timers(state)?;
        drain_immediates(state)?;
        if !has_pending(state) {
            let handlers = state.borrow().process.before_exit_handlers.clone();
            run_handlers(&handlers, 0)?;
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
    run_handlers(&handlers, code)
}

fn run_handlers(handlers: &[Value], code: i32) -> Result<(), VmError> {
    for handler in handlers {
        call_callback(handler, &Value::Undefined, &[Value::Number(code as f64)])?;
    }
    Ok(())
}

fn drain_ticks(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    loop {
        if !drain_one_tick(state)? {
            quench_runtime::drain_promise_jobs();
            return Ok(());
        }
        quench_runtime::drain_promise_jobs();
    }
}

pub(crate) fn drain_one_tick(state: &Rc<RefCell<HostState>>) -> Result<bool, VmError> {
    let item = {
        let host = state.borrow();
        let mut queue = host.event_loop.microtasks.borrow_mut();
        (!queue.is_empty()).then(|| queue.remove(0))
    };
    let Some((callback, args)) = item else {
        return Ok(false);
    };
    call_guarded(state, &callback, &Value::Undefined, &args)?;
    Ok(true)
}

fn fire_due_timers(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let now = super::timers::monotonic_ms();
    let mut due: Vec<(u64, u64)> = state
        .borrow()
        .timers
        .timers
        .iter()
        .filter(|(_, t)| !matches!(t.kind, TimerKind::Immediate) && t.active && t.fire_at <= now)
        .map(|(id, t)| (t.fire_at, *id))
        .collect();
    due.sort();
    for (_, id) in due {
        fire_one_timer(state, id, now)?;
        drain_ticks(state)?;
    }
    Ok(())
}

fn fire_one_timer(state: &Rc<RefCell<HostState>>, id: u64, now: u64) -> Result<(), VmError> {
    let (cb, receiver, args, resource, destroy) = {
        let mut guard = state.borrow_mut();
        let registry = &mut guard.timers;
        let Some(timer) = registry.timers.get_mut(&id) else {
            return Ok(());
        };
        match timer.kind {
            TimerKind::Timeout => {
                timer.active = false;
                super::timers::mark_destroyed(timer);
                (
                    timer.callback.clone(),
                    timer.object.clone(),
                    timer.args.clone(),
                    timer.async_resource.clone(),
                    true,
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
                )
            }
        }
    };
    let result = call_guarded(state, &cb, &receiver, &args);
    if destroy {
        super::timers::async_destroy(&resource);
    }
    result
}

fn drain_immediates(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let queued: Vec<(Value, Vec<Value>)> = state
        .borrow()
        .event_loop
        .immediates
        .borrow_mut()
        .drain(..)
        .collect();
    for (cb, args) in queued {
        call_guarded(state, &cb, &Value::Undefined, &args)?;
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
        let result = call_guarded(state, &timer.callback, &timer.object, &timer.args);
        super::timers::async_destroy(&timer.async_resource);
        result?;
        drain_ticks(state)?;
    }
    Ok(())
}

fn has_pending(state: &Rc<RefCell<HostState>>) -> bool {
    let guard = state.borrow();
    !guard.event_loop.microtasks.borrow().is_empty()
        || !guard.event_loop.immediates.borrow().is_empty()
        || guard
            .timers
            .timers
            .values()
            .any(|t| t.referenced && t.active)
        || crate::modules::net::has_work(state)
}

fn sleep_until_next(state: &Rc<RefCell<HostState>>) {
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
    // No timers, but net I/O may keep the loop alive: poll at a small
    // fixed cadence instead of busy-spinning.
    if crate::modules::net::has_work(state) {
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
}
