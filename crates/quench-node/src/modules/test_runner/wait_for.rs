//! `t.waitFor(condition[, { interval, timeout }])`: poll `condition` until it returns or
//! fulfills, rejecting with `ERR_TEST_FAILURE` once `timeout` elapses.
use super::{function, get, put, Context};
use quench_runtime::{RootId, RootedError};

const DEFAULT_INTERVAL_MS: f64 = 50.0;
const DEFAULT_TIMEOUT_MS: f64 = 1_000.0;
const TIMED_OUT: &str = "waitFor() timed out";
const TIMEOUT_FAILURE: &str = "testTimeoutFailure";

fn timer(c: &mut Context<'_>, name: &str, callback: RootId, delay: RootId) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    let set_timeout = get(c, global, name)?;
    c.call_rooted(set_timeout, global, &[callback, delay])
}

fn clear(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let clear_timeout = get(c, global, "clearTimeout")?;
    for name in ["poller", "timer"] {
        let id = get(c, state, name)?;
        c.call_rooted(clear_timeout, global, &[id])?;
    }
    Ok(())
}

fn option(c: &mut Context<'_>, options: Option<RootId>, name: &str, fallback: f64) -> Result<RootId, RootedError> {
    if let Some(options) = options {
        let value = get(c, options, name)?;
        if !c.rooted_value(value).is_some_and(|value| value.is_undefined()) {
            return Ok(value);
        }
    }
    Ok(c.number(fallback))
}

pub(crate) fn wait_for(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let condition = args.first().copied().unwrap_or(undefined);
    if !c.is_callable_rooted(condition)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, condition, "condition", "function")?);
    }
    let options = args.get(1).copied().filter(|options| c.is_object_rooted(*options).unwrap_or(false));
    let interval = option(c, options, "interval", DEFAULT_INTERVAL_MS)?;
    let timeout = option(c, options, "timeout", DEFAULT_TIMEOUT_MS)?;
    let promise = c.pending_promise_rooted()?;
    let state = c.object_rooted()?;
    for (name, value) in [("condition", condition), ("interval", interval), ("promise", promise), ("poller", undefined)] {
        put(c, state, name, value)?;
    }
    let expire = function(c, "nodeTestWaitTimeout", "", state)?;
    let timer = timer(c, "setTimeout", expire, timeout)?;
    put(c, state, "timer", timer)?;
    poll(c, state)?;
    Ok(promise)
}

fn poll(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let condition = get(c, state, "condition")?;
    let undefined = c.undefined();
    let fulfilled = function(c, "nodeTestWaitFulfilled", "", state)?;
    let rejected = function(c, "nodeTestWaitRejected", "", state)?;
    match c.call_rooted(condition, undefined, &[]) {
        Ok(result) => {
            let then = if c.is_object_rooted(result)? { get(c, result, "then")? } else { undefined };
            if c.is_callable_rooted(then)? {
                c.call_rooted(then, result, &[fulfilled, rejected])?;
            } else {
                c.call_rooted(fulfilled, undefined, &[result])?;
            }
        }
        Err(RootedError { exception: Some(error), .. }) => {
            c.call_rooted(rejected, undefined, &[error])?;
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

pub(crate) fn poll_again(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    poll(c, state)?;
    Ok(c.undefined())
}

fn settled(c: &mut Context<'_>, state: RootId) -> Result<bool, RootedError> {
    let done = get(c, state, "done")?;
    c.truthy_rooted(done)
}

pub(crate) fn fulfilled(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    if !settled(c, state)? {
        let yes = c.boolean(true);
        put(c, state, "done", yes)?;
        clear(c, state)?;
        let promise = get(c, state, "promise")?;
        let value = args.first().copied().unwrap_or_else(|| c.undefined());
        c.resolve_promise_rooted(promise, value)?;
    }
    Ok(c.undefined())
}

pub(crate) fn rejected(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    if !settled(c, state)? {
        let cause = args.first().copied().unwrap_or_else(|| c.undefined());
        put(c, state, "cause", cause)?;
        let interval = get(c, state, "interval")?;
        let again = function(c, "nodeTestWaitPoll", "", state)?;
        let poller = timer(c, "setTimeout", again, interval)?;
        put(c, state, "poller", poller)?;
    }
    Ok(c.undefined())
}

pub(crate) fn timed_out(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    if settled(c, state)? {
        return Ok(c.undefined());
    }
    let yes = c.boolean(true);
    put(c, state, "done", yes)?;
    clear(c, state)?;
    let error = c.error_rooted(TIMED_OUT)?;
    crate::modules::node_errors_shared_vm::decorate_error(c, error, "ERR_TEST_FAILURE")?;
    let failure = c.string_rooted(TIMEOUT_FAILURE);
    put(c, error, "failureType", failure)?;
    let key = c.string_rooted("cause");
    let cause = get(c, state, "cause")?;
    if c.has_property_rooted(state, key)? {
        put(c, error, "cause", cause)?;
    }
    let promise = get(c, state, "promise")?;
    c.reject_promise_rooted(promise, error)?;
    Ok(c.undefined())
}
