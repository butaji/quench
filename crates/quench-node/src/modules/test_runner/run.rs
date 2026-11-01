//! The test lifecycle as a state machine: each node steps through Node's `Test.run` /
//! `Suite.run` (parent `before`, `beforeEach`, body, `afterEach`, `after`, or a suite's
//! children), suspending on a returned thenable and resuming in its reaction.
use super::failure::{self, Cause, FailureType, Inspection};
use super::{context, function, get, put, with_state, Context, HookKind, Kind, NodeId, Output, State, ROOT};
use quench_runtime::{RootId, RootedError};
use std::time::Instant;

const CANCELLED_BY_PARENT: &str = "test did not finish before its parent and was cancelled";
const CANCELLED_AT_EXIT: &str = "Promise resolution is still pending but the event loop has already resolved";
const CALLBACK_AND_PROMISE: &str = "passed a callback but also returned a Promise";
/// Node's `kGenericUserError`.
const FAILURE_EXIT_CODE: f64 = 1.0;
const MILLISECONDS_PER_SECOND: f64 = 1_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Pending,
    /// A suite awaiting its body's promise (Node's `buildSuite`).
    Build,
    /// Awaiting the parent's `before` hooks that ran as they were added.
    AwaitBefore,
    ParentBefore(usize),
    OwnBefore(usize),
    BeforeEach(usize),
    Body,
    Children,
    AfterEach(usize),
    After(usize),
    Finished,
}

/// The step after `step` for a node of `kind`: the order of Node's `run()`.
fn next(kind: Kind, step: Step) -> Step {
    match (kind, step) {
        (Kind::Test, Step::Pending) | (Kind::Suite, Step::Build) => Step::AwaitBefore,
        (_, Step::AwaitBefore) => Step::ParentBefore(0),
        (Kind::Test, Step::ParentBefore(_)) => Step::BeforeEach(0),
        (Kind::Test, Step::BeforeEach(_)) => Step::Body,
        (Kind::Test, Step::Body) => Step::AfterEach(0),
        (Kind::Test, Step::AfterEach(_)) => Step::After(0),
        (Kind::Suite, Step::Pending) => Step::Build,
        (Kind::Suite, Step::ParentBefore(_)) => Step::OwnBefore(0),
        (Kind::Root, Step::Pending) => Step::OwnBefore(0),
        (_, Step::OwnBefore(_)) => Step::Children,
        (_, Step::Children) => Step::After(0),
        _ => Step::Finished,
    }
}

/// Where a node goes when a step before its body fails: straight to its cleanup hooks.
fn cleanup(kind: Kind) -> Step {
    if kind == Kind::Test {
        Step::AfterEach(0)
    } else {
        Step::After(0)
    }
}

/// The hooks a hook step runs and the context they receive.
fn hooks(state: &State, id: NodeId, step: Step) -> (Vec<RootId>, NodeId) {
    let node = &state.tree[id];
    let chain = || std::iter::successors(node.parent, |parent| state.tree[*parent].parent);
    match step {
        Step::ParentBefore(_) => match node.parent {
            Some(parent) if !state.tree[parent].ran_before => (state.tree[parent].hooks.before.clone(), parent),
            _ => (Vec::new(), id),
        },
        Step::OwnBefore(_) if !node.ran_before => (node.hooks.before.clone(), id),
        Step::BeforeEach(_) if !node.skipped() => {
            let mut ancestors = chain().collect::<Vec<_>>();
            ancestors.reverse();
            (ancestors.iter().flat_map(|ancestor| state.tree[*ancestor].hooks.list(HookKind::BeforeEach).to_vec()).collect(), id)
        }
        Step::AfterEach(_) if !node.skipped() => {
            (chain().flat_map(|ancestor| state.tree[ancestor].hooks.list(HookKind::AfterEach).to_vec()).collect(), id)
        }
        Step::After(_) => (node.hooks.after.clone(), id),
        _ => (Vec::new(), id),
    }
}

fn index(step: Step) -> Option<usize> {
    match step {
        Step::ParentBefore(index) | Step::OwnBefore(index) | Step::BeforeEach(index) | Step::AfterEach(index) | Step::After(index) => Some(index),
        _ => None,
    }
}

fn with_index(step: Step, index: usize) -> Step {
    match step {
        Step::ParentBefore(_) => Step::ParentBefore(index),
        Step::OwnBefore(_) => Step::OwnBefore(index),
        Step::BeforeEach(_) => Step::BeforeEach(index),
        Step::AfterEach(_) => Step::AfterEach(index),
        Step::After(_) => Step::After(index),
        step => step,
    }
}

/// A call's completion: settled now, or a thenable whose reaction resumes the node.
enum Completion {
    Settled(Result<(), RootId>),
    Suspended,
}

/// Run `callee` for node `id`; a thenable result suspends the node until it settles.
fn invoke(c: &mut Context<'_>, id: NodeId, callee: RootId, this: RootId, args: &[RootId]) -> Result<Completion, RootedError> {
    with_state(c, |state| state.building.push(id));
    let result = c.call_rooted(callee, this, args);
    with_state(c, |state| state.building.pop());
    match result {
        Ok(value) => wait(c, id, value),
        Err(RootedError { exception: Some(error), .. }) => Ok(Completion::Settled(Err(error))),
        Err(error) => Err(error),
    }
}

fn thenable(c: &mut Context<'_>, value: RootId) -> Result<Option<RootId>, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(None);
    }
    let then = get(c, value, "then")?;
    Ok(c.is_callable_rooted(then)?.then_some(then))
}

fn wait(c: &mut Context<'_>, id: NodeId, value: RootId) -> Result<Completion, RootedError> {
    let Some(then) = thenable(c, value)? else {
        return Ok(Completion::Settled(Ok(())));
    };
    let data = reaction_data(c, id)?;
    let fulfilled = function(c, "nodeTestSettledFulfilled", "", data)?;
    let rejected = function(c, "nodeTestSettledRejected", "", data)?;
    match c.call_rooted(then, value, &[fulfilled, rejected]) {
        Ok(_) => Ok(Completion::Suspended),
        Err(RootedError { exception: Some(error), .. }) => Ok(Completion::Settled(Err(error))),
        Err(error) => Err(error),
    }
}

/// `[node, awaiting]`: which node a reaction resumes and which settlement it reports.
fn reaction_data(c: &mut Context<'_>, id: NodeId) -> Result<RootId, RootedError> {
    let token = with_state(c, |state| {
        let node = &mut state.tree[id];
        node.awaiting += 1;
        node.awaiting
    });
    let node = c.number(id as f64);
    let token = c.number(f64::from(token));
    c.array_rooted(&[node, token])
}

/// The node a reaction resumes, unless it finished or awaits a newer settlement.
fn reaction_target(c: &mut Context<'_>) -> Result<Option<NodeId>, RootedError> {
    let data = c.host_function_data()?;
    let node = get(c, data, "0")?;
    let token = get(c, data, "1")?;
    let id = c.rooted_value(node).and_then(|value| value.as_number()).unwrap_or_default() as NodeId;
    let token = c.rooted_value(token).and_then(|value| value.as_number()).unwrap_or_default() as u32;
    Ok(with_state(c, |state| {
        let node = &mut state.tree[id];
        let current = !node.finished && node.awaiting == token;
        if current {
            node.awaiting += 1;
        }
        current.then_some(id)
    }))
}

pub(crate) fn settled_fulfilled(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    if let Some(id) = reaction_target(c)? {
        complete(c, id, Ok(()))?;
        drive(c, id)?;
    }
    Ok(c.undefined())
}

pub(crate) fn settled_rejected(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if let Some(id) = reaction_target(c)? {
        let reason = args.first().copied().unwrap_or_else(|| c.undefined());
        let reason = c.retain(reason)?;
        complete(c, id, Err(reason))?;
        drive(c, id)?;
    }
    Ok(c.undefined())
}

/// A callback-style test's `done(error)`.
pub(crate) fn done_callback(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if let Some(id) = reaction_target(c)? {
        let error = args.first().copied().filter(|error| c.truthy_rooted(*error).unwrap_or(false));
        let result = match error {
            Some(error) => Err(c.retain(error)?),
            None => Ok(()),
        };
        complete(c, id, result)?;
        drive(c, id)?;
    }
    Ok(c.undefined())
}

fn inspection(state: &State) -> Inspection {
    Inspection {
        colored: match &state.output {
            Some(Output::Render(reporter)) => reporter.style() == super::report::Style::Spec { colors: true },
            Some(Output::Stream) => true,
            None => false,
        },
    }
}

/// Record a failure unless the node already failed (Node's `fail`).
fn fail(c: &mut Context<'_>, id: NodeId, kind: FailureType, cause: Cause) -> Result<(), RootedError> {
    let (failed, inspection) = with_state(c, |state| (state.tree[id].failure.is_some(), inspection(state)));
    if failed {
        return Ok(());
    }
    let failure = failure::capture(c, kind, cause, inspection)?;
    with_state(c, |state| state.tree[id].failure = Some(failure));
    Ok(())
}

/// Apply a settled step's result and move to the step after it.
fn complete(c: &mut Context<'_>, id: NodeId, result: Result<(), RootId>) -> Result<(), RootedError> {
    let (kind, step) = with_state(c, |state| (state.tree[id].kind, state.tree[id].step));
    if step == Step::Body {
        clear_timeout(c, id)?;
    }
    let next_step = match (step, result) {
        (Step::Build, Err(error)) => {
            fail(c, id, FailureType::TestCode, Cause::Thrown(error))?;
            next(kind, step)
        }
        (Step::AwaitBefore | Step::ParentBefore(_) | Step::OwnBefore(_) | Step::BeforeEach(_), Err(error)) => {
            fail(c, id, FailureType::Hook, Cause::Thrown(error))?;
            cleanup(kind)
        }
        (Step::Body, Err(error)) => {
            fail(c, id, FailureType::TestCode, Cause::Thrown(error))?;
            next(kind, step)
        }
        (Step::Body, Ok(())) => {
            let unmet = with_state(c, |state| {
                let node = &state.tree[id];
                node.plan.filter(|expected| *expected != node.assertions).map(|expected| (expected, node.assertions))
            });
            if let Some((expected, actual)) = unmet {
                let message = format!("plan expected {expected} assertions but received {actual}");
                fail(c, id, FailureType::TestCode, Cause::Message(message))?;
            }
            next(kind, step)
        }
        (Step::AfterEach(_) | Step::After(_), Err(error)) => {
            fail(c, id, FailureType::Hook, Cause::Thrown(error))?;
            next(kind, step)
        }
        (step, Ok(())) => match index(step) {
            Some(index) => with_index(step, index + 1),
            None => next(kind, step),
        },
        (step, Err(_)) => next(kind, step),
    };
    with_state(c, |state| state.tree[id].step = next_step);
    Ok(())
}

/// Advance node `id` until it suspends or finishes.
fn drive(c: &mut Context<'_>, id: NodeId) -> Result<(), RootedError> {
    loop {
        let (kind, step, finished) = with_state(c, |state| {
            let node = &state.tree[id];
            (node.kind, node.step, node.finished)
        });
        if finished {
            return Ok(());
        }
        match step {
            Step::Pending => {
                let filtered = with_state(c, |state| start(state, id));
                if filtered {
                    return finish(c, id);
                }
                with_state(c, |state| state.tree[id].started = Some(Instant::now()));
                with_state(c, |state| state.tree[id].step = next(kind, step));
            }
            Step::Build | Step::AwaitBefore => {
                let pending = with_state(c, |state| match step {
                    Step::Build => state.tree[id].build.take(),
                    _ => state.tree[id].parent.and_then(|parent| state.tree[parent].before_pending),
                });
                match pending {
                    Some(pending) => {
                        let completion = wait(c, id, pending)?;
                        return proceed(c, id, completion);
                    }
                    None => complete(c, id, Ok(()))?,
                }
            }
            Step::Body => {
                let body = with_state(c, |state| state.tree[id].body);
                let Some(body) = body else {
                    complete(c, id, Ok(()))?;
                    continue;
                };
                start_timeout(c, id)?;
                let receiver = context(c, id)?;
                let length = get(c, body, "length")?;
                let callback = c.rooted_value(length).and_then(|value| value.as_number()).is_some_and(|length| length >= 2.0);
                if !callback {
                    let completion = invoke(c, id, body, receiver, &[receiver])?;
                    return proceed(c, id, completion);
                }
                let data = reaction_data(c, id)?;
                let done = function(c, "nodeTestDone", "done", data)?;
                match invoke(c, id, body, receiver, &[receiver, done])? {
                    Completion::Settled(Err(error)) => return proceed(c, id, Completion::Settled(Err(error))),
                    Completion::Settled(Ok(())) => return Ok(()),
                    Completion::Suspended => {
                        fail(c, id, FailureType::CallbackAndPromisePresent, Cause::Message(CALLBACK_AND_PROMISE.to_owned()))?;
                        return Ok(());
                    }
                }
            }
            Step::Children => {
                let (next_child, running, blocked) = with_state(c, |state| {
                    let node = &state.tree[id];
                    (node.children.get(node.started_children).copied(), node.running_child, node.skipped() || node.failure.is_some())
                });
                if running.is_some() {
                    return Ok(());
                }
                match next_child {
                    Some(child) if !blocked => {
                        with_state(c, |state| state.tree[id].running_child = Some(child));
                        drive(c, child)?;
                        return Ok(());
                    }
                    None if kind == Kind::Root && !with_state(c, |state| state.draining) => return Ok(()),
                    _ => with_state(c, |state| state.tree[id].step = next(kind, step)),
                }
            }
            Step::Finished => return finish(c, id),
            step => {
                let (list, owner) = with_state(c, |state| hooks(state, id, step));
                let position = index(step).unwrap_or_default();
                if position == 0 {
                    with_state(c, |state| match step {
                        Step::ParentBefore(_) if owner != id => state.tree[owner].ran_before = true,
                        Step::OwnBefore(_) => state.tree[id].ran_before = true,
                        _ => {}
                    });
                }
                let Some(hook) = list.get(position).copied() else {
                    with_state(c, |state| state.tree[id].step = next(kind, step));
                    continue;
                };
                let receiver = context(c, if matches!(step, Step::BeforeEach(_) | Step::AfterEach(_)) { id } else { owner })?;
                let completion = invoke(c, id, hook, receiver, &[receiver])?;
                return proceed(c, id, completion);
            }
        }
    }
}

/// Node's `stopTest`: a body still running after the test's `timeout` is cancelled.
fn start_timeout(c: &mut Context<'_>, id: NodeId) -> Result<(), RootedError> {
    let Some(timeout) = with_state(c, |state| state.tree[id].timeout) else {
        return Ok(());
    };
    let data = c.number(id as f64);
    let expire = function(c, "nodeTestTimeout", "", data)?;
    let global = c.global_root()?;
    let set_timeout = get(c, global, "setTimeout")?;
    let delay = c.number(timeout);
    let timer = c.call_rooted(set_timeout, global, &[expire, delay])?;
    let timer = c.retain(timer)?;
    with_state(c, |state| state.tree[id].timer = Some(timer));
    Ok(())
}

fn clear_timeout(c: &mut Context<'_>, id: NodeId) -> Result<(), RootedError> {
    let Some(timer) = with_state(c, |state| state.tree[id].timer.take()) else {
        return Ok(());
    };
    let global = c.global_root()?;
    let clear = get(c, global, "clearTimeout")?;
    c.call_rooted(clear, global, &[timer])?;
    c.release_root(timer);
    Ok(())
}

pub(crate) fn timed_out(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let id = c.rooted_value(data).and_then(|value| value.as_number()).unwrap_or_default() as NodeId;
    let (step, finished, timeout, timer) = with_state(c, |state| {
        let node = &mut state.tree[id];
        (node.step, node.finished, node.timeout.unwrap_or_default(), node.timer.take())
    });
    if let Some(timer) = timer {
        c.release_root(timer);
    }
    if finished || step != Step::Body {
        return Ok(c.undefined());
    }
    let failed = with_state(c, |state| state.tree[id].failure.is_some());
    if !failed {
        let message = format!("test timed out after {}ms", super::report::number(timeout));
        fail(c, id, FailureType::TestTimeout, Cause::Message(message))?;
        with_state(c, |state| state.tree[id].cancelled = true);
    }
    with_state(c, |state| {
        let node = &mut state.tree[id];
        node.awaiting += 1;
        node.step = next(node.kind, Step::Body);
    });
    drive(c, id)?;
    Ok(c.undefined())
}

/// Node awaits every hook and body, so even a synchronous one resumes its test in a
/// later microtask: a subtest the body did not await is still running when it returns.
fn proceed(c: &mut Context<'_>, id: NodeId, completion: Completion) -> Result<(), RootedError> {
    let Completion::Settled(result) = completion else {
        return Ok(());
    };
    let promise = c.pending_promise_rooted()?;
    match result {
        Ok(()) => {
            let undefined = c.undefined();
            c.resolve_promise_rooted(promise, undefined)?;
        }
        Err(error) => c.reject_promise_rooted(promise, error)?,
    }
    match wait(c, id, promise)? {
        Completion::Settled(result) => {
            complete(c, id, result)?;
            drive(c, id)
        }
        Completion::Suspended => Ok(()),
    }
}

/// A `before` hook added to a node that already started runs at once (Node's
/// `createHook`); its children await it before their own hooks.
pub(super) fn run_before_now(c: &mut Context<'_>, owner: NodeId, hook: RootId) -> Result<(), RootedError> {
    let receiver = context(c, owner)?;
    let result = c.call_rooted(hook, receiver, &[receiver]);
    let promise = c.pending_promise_rooted()?;
    match result {
        Ok(value) => c.resolve_promise_rooted(promise, value)?,
        Err(RootedError { exception: Some(error), .. }) => c.reject_promise_rooted(promise, error)?,
        Err(error) => return Err(error),
    }
    let promise = match with_state(c, |state| state.tree[owner].before_pending) {
        Some(previous) => {
            let then = get(c, previous, "then")?;
            let undefined = c.undefined();
            let chained = c.call_rooted(then, previous, &[undefined, undefined])?;
            let then = get(c, chained, "then")?;
            let after = function(c, "nodeTestChainBefore", "", promise)?;
            c.call_rooted(then, chained, &[after])?
        }
        None => promise,
    };
    c.mark_promise_handled_rooted(promise)?;
    let promise = c.retain(promise)?;
    with_state(c, |state| state.tree[owner].before_pending = Some(promise));
    Ok(())
}

pub(crate) fn chain_before(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    c.host_function_data()
}

/// A suite's body runs as it is registered; a returned promise delays its children.
pub(super) fn build_suite(c: &mut Context<'_>, id: NodeId) -> Result<(), RootedError> {
    let Some(body) = with_state(c, |state| state.tree[id].body) else {
        return Ok(());
    };
    let receiver = context(c, id)?;
    with_state(c, |state| state.building.push(id));
    let result = c.call_rooted(body, receiver, &[receiver]);
    with_state(c, |state| state.building.pop());
    match result {
        Ok(value) => {
            if thenable(c, value)?.is_some() {
                let value = c.retain(value)?;
                with_state(c, |state| state.tree[id].build = Some(value));
            }
            Ok(())
        }
        Err(RootedError { exception: Some(error), .. }) => fail(c, id, FailureType::TestCode, Cause::Thrown(error)),
        Err(error) => Err(error),
    }
}

/// Resume `id` in a microtask, after the code registering tests has finished.
pub(super) fn schedule(c: &mut Context<'_>, id: NodeId) -> Result<(), RootedError> {
    let data = c.number(id as f64);
    let resume = function(c, "nodeTestResume", "", data)?;
    c.queue_microtask_rooted(resume, &[])
}

pub(crate) fn resume(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let id = c.rooted_value(data).and_then(|value| value.as_number()).unwrap_or_default() as NodeId;
    pump(c, id)?;
    Ok(c.undefined())
}

/// Start the next child of `id` if none is running: a test's subtests run as `t.test`
/// registers them, a suite's and the root's at their children step.
pub(super) fn pump(c: &mut Context<'_>, id: NodeId) -> Result<(), RootedError> {
    let (kind, step, finished, running, next_child) = with_state(c, |state| {
        let node = &state.tree[id];
        (node.kind, node.step, node.finished, node.running_child, node.children.get(node.started_children).copied())
    });
    if finished || running.is_some() {
        return Ok(());
    }
    match kind {
        Kind::Test => {
            if let Some(child) = next_child {
                with_state(c, |state| state.tree[id].running_child = Some(child));
                drive(c, child)?;
            }
            Ok(())
        }
        Kind::Root if step == Step::Pending => drive(c, id),
        _ if step == Step::Children => drive(c, id),
        _ => Ok(()),
    }
}

/// Node's `start`: apply the `--test-only` filter, or take the next test number; returns
/// whether the node is filtered out. A node cancelled before it started lasts no time.
fn start(state: &mut State, id: NodeId) -> bool {
    let only_filter = state.only_filter;
    let node = &state.tree[id];
    let Some(parent) = node.parent else {
        return false;
    };
    let filtered = only_filter
        && node.failure.is_none()
        && !node.only
        && !node.has_only
        && (state.tree[parent].run_only || state.tree[parent].has_only);
    state.tree[parent].started_children += 1;
    if !filtered {
        state.tree[parent].numbered_children += 1;
        state.tree[id].number = state.tree[parent].numbered_children;
    }
    state.tree[id].filtered = filtered;
    filtered
}

/// Node's `postRun`: cancel unfinished children, fail through failed subtests, report.
fn finish(c: &mut Context<'_>, id: NodeId) -> Result<(), RootedError> {
    let children = with_state(c, |state| state.tree[id].children.clone());
    for child in &children {
        cancel(c, *child, CANCELLED_BY_PARENT)?;
    }
    let failed = with_state(c, |state| {
        children.iter().filter(|child| {
            let child = &state.tree[**child];
            child.failure.is_some() && !child.todo()
        }).count()
    });
    let passed = with_state(c, |state| state.tree[id].failure.is_none());
    if failed > 0 && passed {
        let plural = if failed > 1 { "s" } else { "" };
        fail(c, id, FailureType::SubtestsFailed, Cause::Message(format!("{failed} subtest{plural} failed")))?;
    }
    let (parent, completion, mock, held) = with_state(c, |state| {
        let node = &mut state.tree[id];
        node.finished = true;
        node.step = Step::Finished;
        node.duration_ms = node.started.map_or(0.0, |started| started.elapsed().as_secs_f64() * MILLISECONDS_PER_SECOND);
        let hooks = std::mem::take(&mut node.hooks);
        let held = [node.body.take(), node.context.take(), node.signal.take(), node.assert.take(), node.build.take(), node.before_pending.take()]
            .into_iter()
            .flatten()
            .chain([hooks.before, hooks.after, hooks.before_each, hooks.after_each].into_iter().flatten())
            .collect::<Vec<_>>();
        (node.parent, node.completion.take(), node.mock.take(), held)
    });
    if let Some(mock) = mock {
        let reset = get(c, mock, "reset")?;
        c.call_rooted(reset, mock, &[])?;
        c.release_root(mock);
    }
    for root in held {
        c.release_root(root);
    }
    if let Some(completion) = completion {
        let undefined = c.undefined();
        c.resolve_promise_rooted(completion, undefined)?;
        c.release_root(completion);
    }
    match parent {
        Some(parent) => {
            if parent == ROOT && !with_state(c, |state| state.tree[id].filtered) {
                report(c, id)?;
            }
            with_state(c, |state| {
                let parent = &mut state.tree[parent];
                if parent.running_child == Some(id) {
                    parent.running_child = None;
                }
            });
            schedule(c, parent)
        }
        None => summarize(c),
    }
}

/// Node's `#cancel` then `postRun` for an unfinished node.
fn cancel(c: &mut Context<'_>, id: NodeId, message: &str) -> Result<(), RootedError> {
    let (finished, started) = with_state(c, |state| (state.tree[id].finished, state.tree[id].started.is_some()));
    if finished {
        return Ok(());
    }
    let failed = with_state(c, |state| state.tree[id].failure.is_some());
    if !failed {
        fail(c, id, FailureType::CancelledByParent, Cause::Message(message.to_owned()))?;
        with_state(c, |state| state.tree[id].cancelled = true);
    }
    if !started {
        with_state(c, |state| start(state, id));
    }
    finish(c, id)
}

/// Node's `createProcessEventHandler`: an uncaught error fails the running test, or,
/// with none running, becomes a root diagnostic that fails the run.
pub(crate) fn process_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let event = super::data_text(c)?;
    let error = args.first().copied().unwrap_or_else(|| c.undefined());
    let running = with_state(c, |state| {
        let mut current = ROOT;
        while let Some(child) = state.tree[current].running_child {
            current = child;
        }
        (current != ROOT && !state.tree[current].finished).then_some(current)
    });
    match running {
        Some(id) => {
            let kind = if event == "uncaughtException" { FailureType::UncaughtException } else { FailureType::UnhandledRejection };
            fail(c, id, kind, Cause::Thrown(error))?;
        }
        None => {
            let text = c.to_string(error)?;
            let message = format!(
                "Error: A resource generated asynchronous activity after the test ended. This activity created the error \"{text}\" which triggered an {event} event, caught by the test runner."
            );
            with_state(c, |state| state.tree[ROOT].diagnostics.push(message.clone()));
            fail(c, ROOT, FailureType::TestCode, Cause::Message(message))?;
        }
    }
    Ok(c.undefined())
}

/// Node's `exitHandler`: when the loop drains, cancel what is still pending and end the run.
pub(crate) fn before_exit(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let (step, children) = with_state(c, |state| (state.tree[ROOT].step, state.tree[ROOT].children.clone()));
    if matches!(step, Step::After(_) | Step::Finished) {
        return Ok(c.undefined());
    }
    with_state(c, |state| state.draining = true);
    for child in children {
        cancel(c, child, CANCELLED_AT_EXIT)?;
    }
    drive(c, ROOT)?;
    Ok(c.undefined())
}

fn report_of(state: &State, id: NodeId) -> super::report::TestReport {
    let node = &state.tree[id];
    super::report::TestReport {
        name: node.name.clone(),
        nesting: node.nesting,
        number: node.number,
        kind: if node.kind == Kind::Suite { super::report::Kind::Suite } else { super::report::Kind::Test },
        directive: node.directive.clone(),
        passed: node.failure.is_none(),
        cancelled: node.cancelled,
        duration_ms: node.duration_ms,
        location: node.location.clone(),
        failure: node.failure.clone(),
        children: node
            .children
            .iter()
            .filter(|child| state.tree[**child].finished && !state.tree[**child].filtered)
            .map(|child| report_of(state, *child))
            .collect(),
        diagnostics: node.diagnostics.clone(),
    }
}

fn write(c: &mut Context<'_>, text: &str) -> Result<(), RootedError> {
    if text.is_empty() {
        return Ok(());
    }
    let process = super::process(c)?;
    let stdout = get(c, process, "stdout")?;
    let write = get(c, stdout, "write")?;
    let text = c.string_rooted(text);
    c.call_rooted(write, stdout, &[text])?;
    Ok(())
}

fn report(c: &mut Context<'_>, id: NodeId) -> Result<(), RootedError> {
    let text = with_state(c, |state| {
        let test = report_of(state, id);
        match state.output.as_mut() {
            Some(Output::Render(reporter)) => reporter.test(&test),
            Some(Output::Stream) => super::cli::stream_line(&test),
            None => String::new(),
        }
    });
    write(c, &text)
}

/// The root's end: Node's plan, counters and (for spec) failing tests, and the exit code.
fn summarize(c: &mut Context<'_>) -> Result<(), RootedError> {
    let (text, success) = with_state(c, |state| {
        let duration = state.started.map_or(0.0, |started| started.elapsed().as_secs_f64() * MILLISECONDS_PER_SECOND);
        let diagnostics = state.tree[ROOT].diagnostics.clone();
        let root_failed = state.tree[ROOT].failure.is_some();
        match state.output.as_mut() {
            Some(Output::Render(reporter)) => {
                let text = reporter.summary(&diagnostics, duration);
                (text, reporter.counters().success() && !root_failed)
            }
            _ => {
                let mut counters = super::report::Counters::default();
                for child in state.tree[ROOT].children.iter().filter(|child| !state.tree[**child].filtered) {
                    counters.count(&report_of(state, *child));
                }
                (String::new(), counters.success() && !root_failed)
            }
        }
    });
    write(c, &text)?;
    if !success {
        let process = super::process(c)?;
        let code = c.number(FAILURE_EXIT_CODE);
        let key = c.string_rooted("exitCode");
        c.set_property_rooted(process, key, code, process)?;
    }
    Ok(())
}

/// The `test:pass`/`test:fail` events of a report and its subtree, children first.
pub(super) fn events(
    c: &mut Context<'_>,
    test: &super::report::TestReport,
    file: &str,
    events: &mut Vec<RootId>,
) -> Result<(), RootedError> {
    for child in &test.children {
        self::events(c, child, file, events)?;
    }
    let data = c.object_rooted()?;
    let name = c.string_rooted(&test.name);
    put(c, data, "name", name)?;
    let nesting = c.number(test.nesting as f64);
    put(c, data, "nesting", nesting)?;
    let number = c.number(test.number as f64);
    put(c, data, "testNumber", number)?;
    let details = c.object_rooted()?;
    let duration = c.number(test.duration_ms);
    put(c, details, "duration_ms", duration)?;
    let kind = c.string_rooted(if test.kind == super::report::Kind::Suite { "suite" } else { "test" });
    put(c, details, "type", kind)?;
    put(c, data, "details", details)?;
    let path = c.string_rooted(file);
    put(c, data, "file", path)?;
    match &test.directive {
        Some(super::report::Directive::Skip(message)) => {
            let message = if message.is_empty() { c.boolean(true) } else { c.string_rooted(message) };
            put(c, data, "skip", message)?;
        }
        Some(super::report::Directive::Todo(message)) => {
            let message = if message.is_empty() { c.boolean(true) } else { c.string_rooted(message) };
            put(c, data, "todo", message)?;
        }
        None => {}
    }
    let event = c.object_rooted()?;
    let kind = c.string_rooted(if test.passed { "test:pass" } else { "test:fail" });
    put(c, event, "type", kind)?;
    put(c, event, "data", data)?;
    events.push(event);
    Ok(())
}

pub(super) fn summary_event(c: &mut Context<'_>, counters: &super::report::Counters) -> Result<RootId, RootedError> {
    let totals = c.object_rooted()?;
    for (name, value) in [
        ("tests", counters.tests),
        ("suites", counters.suites),
        ("passed", counters.passed),
        ("failed", counters.failed),
        ("cancelled", counters.cancelled),
        ("skipped", counters.skipped),
        ("todo", counters.todo),
        ("topLevel", counters.top_level),
    ] {
        let value = c.number(value as f64);
        put(c, totals, name, value)?;
    }
    let data = c.object_rooted()?;
    put(c, data, "counts", totals)?;
    let success = c.boolean(counters.success());
    put(c, data, "success", success)?;
    let event = c.object_rooted()?;
    let kind = c.string_rooted("test:summary");
    put(c, event, "type", kind)?;
    put(c, event, "data", data)?;
    Ok(event)
}
