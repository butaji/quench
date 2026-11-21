//! `node:test/reporters`: Node's reporter exports. `tap`, `dot` and `junit` are async
//! generator functions over an event source; `spec` and `lcov` are Transform classes.
//! Each renders events through [`Reporter`], one at a time, as Node's reporters do.
use super::report::{Directive, Kind, Reporter, Style, TestReport};
#[path = "reporters_failure.rs"]
mod failure_text;
use crate::host::shared_vm::commonjs::{cached_native_binding, stream_module};
use crate::modules::host_members::{captured, constructor, current, function, leading, members, methods};
use crate::modules::stream_shared_vm::{get, inherit, is_undefined, put};
use quench_runtime::{NativeContext, RootId, RootedError};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

type Context<'a> = NativeContext<'a>;
type Outcome = Result<RootId, RootedError>;

/// The property naming a Transform instance's reporter.
const REPORTER_ID: &str = "\0quench:reporter";
/// Iterator state properties: the source's iterator, the reporter, the pending header text
/// and whether the stream has ended.
const ITERATOR: &str = "\0quench:iterator";
const HEADER: &str = "\0quench:header";
const FINISHED: &str = "\0quench:finished";
/// The first reporter id; ids are never reused.
const FIRST_REPORTER_ID: u64 = 1;
/// The arguments of a Transform's `_transform(event, encoding, callback)`, the callback last.
const TRANSFORM_ARGUMENTS: usize = 3;

thread_local! {
    static REPORTERS: RefCell<BTreeMap<u64, Reporter>> = const { RefCell::new(BTreeMap::new()) };
    static NEXT_ID: Cell<u64> = const { Cell::new(FIRST_REPORTER_ID) };
}

members!(Member, "nodeTestReporters", [
    /// `tap(source)`, `dot(source)` and `junit(source)`: the reporter's async generator.
    Tap,
    Dot,
    Junit,
    /// `new spec()` and `new lcov()`: the Transform classes.
    SpecConstruct,
    LcovConstruct,
    /// `spec`'s `_transform(event, encoding, callback)` and `_flush(callback)`.
    SpecTransform,
    SpecFlush,
    /// `lcov`'s `_transform`: it reacts only to coverage events, which Quench does not produce.
    LcovTransform,
    /// The async iterator's `next`, `return`, `throw` and `[Symbol.asyncIterator]`.
    Next,
    Return,
    Throw,
    Iterator,
    /// The settlement of the source's `next()` promise, captured with the iterator's state.
    Settled,
]);

/// One test event, as the reporter consumes it.
enum Event {
    Report(TestReport),
    Subtest { nesting: usize, name: String },
    Plan { nesting: usize, count: f64 },
    Diagnostic { nesting: usize, message: String },
    Output(String),
    Ignored,
}

/// The `node:test/reporters` builtin, its exports built once.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    cached_native_binding(c, "\0node:test/reporters", |c| {
        let stream = stream_module(c)?;
        let transform = get(c, stream, "Transform")?;
        let spec = transform_class(c, Member::SpecConstruct, "SpecReporter", transform, &[
            ("_transform", Member::SpecTransform),
            ("_flush", Member::SpecFlush),
        ])?;
        let lcov = transform_class(c, Member::LcovConstruct, "LcovReporter", transform, &[(
            "_transform",
            Member::LcovTransform,
        )])?;
        let instance = c.construct_rooted(lcov, lcov, &[])?;
        let dot = function(c, Member::Dot, "dot", &[])?;
        let junit = function(c, Member::Junit, "junit", &[])?;
        let tap = function(c, Member::Tap, "tap", &[])?;
        let members = [("dot", dot), ("junit", junit), ("lcov", instance), ("spec", spec), ("tap", tap)];
        let default = c.object_rooted()?;
        for (name, value) in members {
            put(c, default, name, value)?;
        }
        let exports = c.object_rooted()?;
        put(c, exports, "default", default)?;
        for (name, value) in members {
            put(c, exports, name, value)?;
        }
        Ok(exports)
    })
}

/// Dispatches a call of any `Member` function: the exports and Transform classes here, the
/// async iterator's methods in `iterator_member`.
pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    match current::<Member>(c)? {
        Member::Tap => export(c, Style::Tap, args),
        Member::Dot => export(c, Style::Dot, args),
        Member::Junit => export(c, Style::Junit, args),
        Member::SpecConstruct => construct(c, Some(Style::Spec { colors: false })),
        Member::LcovConstruct => construct(c, None),
        Member::SpecTransform => spec_transform(c, this, args),
        Member::SpecFlush => spec_flush(c, this, args),
        Member::LcovTransform => lcov_transform(c, args),
        iterator => iterator_member(c, iterator, this, args),
    }
}

/// The async iterator's `next`, `return`, `throw`, `[Symbol.asyncIterator]` and the source's
/// settlement, each over the state it captured (or `this` for the iterator itself).
fn iterator_member(c: &mut Context<'_>, member: Member, this: RootId, args: &[RootId]) -> Outcome {
    match member {
        Member::Next => with_state(c, next),
        Member::Return => with_state(c, close_and_resolve),
        Member::Throw => {
            let ([reason], _) = leading::<1>(c, args);
            with_state(c, |c, state| {
                close(c, state)?;
                reject(c, reason)
            })
        }
        Member::Iterator => Ok(this),
        Member::Settled => {
            let ([result], _) = leading::<1>(c, args);
            with_state(c, |c, state| settled(c, state, result))
        }
        _ => unreachable!("not an async iterator member"),
    }
}

/// `_transform` of lcov: it reacts only to coverage events, so the callback runs with no chunk.
fn lcov_transform(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let ([_, _, callback], _) = leading::<TRANSFORM_ARGUMENTS>(c, args);
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[])
}

/// `_transform(event, encoding, callback)` of spec: the event's text, or none.
fn spec_transform(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let ([chunk, _, callback], _) = leading::<TRANSFORM_ARGUMENTS>(c, args);
    let event = parse(c, chunk)?;
    let text = match reporter_id(c, this)? {
        Some(id) => with_reporter(id, |reporter| feed(reporter, event)).unwrap_or_default(),
        None => String::new(),
    };
    finish_chunk(c, callback, text)
}

/// `_flush(callback)` of spec: the reporter's end.
fn spec_flush(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let ([callback], _) = leading::<1>(c, args);
    let text = match reporter_id(c, this)? {
        Some(id) => finish_and_release(id),
        None => String::new(),
    };
    finish_chunk(c, callback, text)
}

/// The iterator state a host function captured.
fn with_state(c: &mut Context<'_>, act: impl FnOnce(&mut Context<'_>, RootId) -> Outcome) -> Outcome {
    let [state] = captured::<1>(c)?;
    act(c, state)
}

/// `return()`: the stream ends, its result done.
fn close_and_resolve(c: &mut Context<'_>, state: RootId) -> Outcome {
    close(c, state)?;
    let result = iter_result(c, None, true)?;
    resolved(c, result)
}

/// `tap(source)` and its siblings: an async iterator yielding the reporter's text.
fn export(c: &mut Context<'_>, style: Style, args: &[RootId]) -> Outcome {
    let ([source], _) = leading::<1>(c, args);
    let iterator = iterator_of(c, source)?;
    let id = register(Reporter::new(style, String::new()));
    let header = with_reporter(id, Reporter::start).unwrap_or_default();
    let state = c.object_rooted()?;
    put(c, state, ITERATOR, iterator)?;
    let id_value = c.number(id as f64);
    put(c, state, REPORTER_ID, id_value)?;
    let header_value = c.string_rooted(&header);
    put(c, state, HEADER, header_value)?;
    let object = c.object_rooted()?;
    for (name, member) in [("next", Member::Next), ("return", Member::Return), ("throw", Member::Throw)] {
        let method = function(c, member, name, &[state])?;
        put(c, object, name, method)?;
    }
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let key = get(c, symbol, "asyncIterator")?;
    let identity = function(c, Member::Iterator, "[Symbol.asyncIterator]", &[])?;
    c.define_data_property_rooted(object, key, identity, true, false, true)?;
    Ok(object)
}

/// `new spec()` and `new lcov()`: a Transform in object mode for writing, its prototype the
/// new target's, and a reporter when it formats events.
fn construct(c: &mut Context<'_>, style: Option<Style>) -> Outcome {
    let undefined = c.undefined();
    let target = c.new_target_rooted().unwrap_or(undefined);
    if is_undefined(c, target) {
        let error = c.type_error_rooted("Class constructor cannot be invoked without 'new'")?;
        return Err(c.throw(error));
    }
    let stream = stream_module(c)?;
    let base = get(c, stream, "Transform")?;
    let options = c.object_rooted()?;
    let flag = c.boolean(true);
    put(c, options, "writableObjectMode", flag)?;
    let instance = c.construct_rooted(base, base, &[options])?;
    let prototype = get(c, target, "prototype")?;
    c.set_prototype_rooted(instance, prototype)?;
    if let Some(style) = style {
        let id = c.number(register(Reporter::new(style, String::new())) as f64);
        put(c, instance, REPORTER_ID, id)?;
    }
    Ok(instance)
}

/// A Transform class with its prototype methods, inheriting from `transform`.
fn transform_class(
    c: &mut Context<'_>,
    member: Member,
    name: &str,
    transform: RootId,
    prototype_methods: &[(&str, Member)],
) -> Outcome {
    let class = constructor(c, member, name)?;
    let prototype = get(c, class, "prototype")?;
    methods(c, prototype, prototype_methods)?;
    inherit(c, class, transform)?;
    Ok(class)
}

/// `source[Symbol.asyncIterator]()`, or `source[Symbol.iterator]()` for a sync source.
fn iterator_of(c: &mut Context<'_>, source: RootId) -> Outcome {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let async_key = get(c, symbol, "asyncIterator")?;
    let mut method = c.get_property_rooted(source, async_key)?;
    if !c.is_callable_rooted(method)? {
        let sync_key = get(c, symbol, "iterator")?;
        method = c.get_property_rooted(source, sync_key)?;
    }
    c.call_rooted(method, source, &[])
}

/// `next()`: the header first, then the next event's text once the source yields one.
fn next(c: &mut Context<'_>, state: RootId) -> Outcome {
    let header = text_of(c, state, HEADER)?.unwrap_or_default();
    if !header.is_empty() {
        let empty = c.string_rooted("");
        put(c, state, HEADER, empty)?;
        let result = iter_result(c, Some(header), false)?;
        return resolved(c, result);
    }
    step(c, state)
}

/// Pulls one result from the source; `then` adopts the promise `settled` returns.
fn step(c: &mut Context<'_>, state: RootId) -> Outcome {
    if flag(c, state, FINISHED)? {
        let result = iter_result(c, None, true)?;
        return resolved(c, result);
    }
    let iterator = get(c, state, ITERATOR)?;
    let method = get(c, iterator, "next")?;
    let result = c.call_rooted(method, iterator, &[])?;
    let promise = resolved(c, result)?;
    let handler = function(c, Member::Settled, "", &[state])?;
    then(c, promise, handler)
}

/// One source result: a done result ends the stream with the reporter's last text; any
/// other becomes the reporter's text, or the next pull when it has none.
fn settled(c: &mut Context<'_>, state: RootId, result: RootId) -> Outcome {
    let done = if c.is_object_rooted(result)? {
        let value = get(c, result, "done")?;
        c.truthy_rooted(value)?
    } else {
        true
    };
    let id = state_id(c, state)?;
    if done {
        set_flag(c, state, FINISHED)?;
        let text = finish_and_release(id);
        let ended = text.is_empty();
        return iter_result(c, (!ended).then_some(text), ended);
    }
    let value = get(c, result, "value")?;
    let event = parse(c, value)?;
    let text = with_reporter(id, |reporter| feed(reporter, event)).unwrap_or_default();
    if text.is_empty() {
        return step(c, state);
    }
    iter_result(c, Some(text), false)
}

/// `return()` and `throw()` end the stream: the reporter is released unprinted.
fn close(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    set_flag(c, state, FINISHED)?;
    release(state_id(c, state)?);
    let iterator = get(c, state, ITERATOR)?;
    let method = get(c, iterator, "return")?;
    if c.is_callable_rooted(method)? {
        c.call_rooted(method, iterator, &[])?;
    }
    Ok(())
}

/// `callback(null, text)`, with no chunk for empty text.
fn finish_chunk(c: &mut Context<'_>, callback: RootId, text: String) -> Outcome {
    let null = c.null();
    let undefined = c.undefined();
    let chunk = if text.is_empty() { undefined } else { c.string_rooted(&text) };
    c.call_rooted(callback, undefined, &[null, chunk])
}

fn iter_result(c: &mut Context<'_>, value: Option<String>, done: bool) -> Outcome {
    let object = c.object_rooted()?;
    let value = match value {
        Some(text) => c.string_rooted(&text),
        None => c.undefined(),
    };
    put(c, object, "value", value)?;
    let done = c.boolean(done);
    put(c, object, "done", done)?;
    Ok(object)
}

/// `Promise.resolve(value)`.
fn resolved(c: &mut Context<'_>, value: RootId) -> Outcome {
    let global = c.global_root()?;
    let promise = get(c, global, "Promise")?;
    let resolve = get(c, promise, "resolve")?;
    c.call_rooted(resolve, promise, &[value])
}

/// `Promise.reject(reason)`.
fn reject(c: &mut Context<'_>, reason: RootId) -> Outcome {
    let global = c.global_root()?;
    let promise = get(c, global, "Promise")?;
    let method = get(c, promise, "reject")?;
    c.call_rooted(method, promise, &[reason])
}

fn then(c: &mut Context<'_>, promise: RootId, handler: RootId) -> Outcome {
    let method = get(c, promise, "then")?;
    c.call_rooted(method, promise, &[handler])
}

fn parse(c: &mut Context<'_>, event: RootId) -> Result<Event, RootedError> {
    if !c.is_object_rooted(event)? {
        return Ok(Event::Ignored);
    }
    let kind = text_of(c, event, "type")?.unwrap_or_default();
    let data = get(c, event, "data")?;
    if !c.is_object_rooted(data)? {
        return Ok(Event::Ignored);
    }
    let nesting = number_of(c, data, "nesting")?.unwrap_or(0.0) as usize;
    Ok(match kind.as_str() {
        "test:pass" => Event::Report(report(c, data, true, nesting)?),
        "test:fail" => Event::Report(report(c, data, false, nesting)?),
        "test:start" => Event::Subtest { nesting, name: text_of(c, data, "name")?.unwrap_or_default() },
        "test:plan" => Event::Plan { nesting, count: number_of(c, data, "count")?.unwrap_or(0.0) },
        "test:diagnostic" => Event::Diagnostic { nesting, message: text_of(c, data, "message")?.unwrap_or_default() },
        "test:stdout" | "test:stderr" => Event::Output(text_of(c, data, "message")?.unwrap_or_default()),
        _ => Event::Ignored,
    })
}

/// A `test:pass` or `test:fail` event's data as a completed report.
fn report(c: &mut Context<'_>, data: RootId, passed: bool, nesting: usize) -> Result<TestReport, RootedError> {
    let details = get(c, data, "details")?;
    let has_details = c.is_object_rooted(details)?;
    let duration_ms = if has_details { number_of(c, details, "duration_ms")?.unwrap_or(0.0) } else { 0.0 };
    let suite = has_details && text_of(c, details, "type")?.as_deref() == Some("suite");
    let error = if has_details { get(c, details, "error")? } else { c.undefined() };
    let failure = if !passed && c.is_object_rooted(error)? { Some(failure_text::failure(c, error)?) } else { None };
    Ok(TestReport {
        name: text_of(c, data, "name")?.unwrap_or_default(),
        nesting,
        number: number_of(c, data, "testNumber")?.unwrap_or(0.0) as usize,
        kind: if suite { Kind::Suite } else { Kind::Test },
        directive: directive(c, data)?,
        passed,
        cancelled: false,
        duration_ms,
        location: None,
        failure,
        children: Vec::new(),
        diagnostics: Vec::new(),
    })
}

/// The `skip` or `todo` option: its message, or empty for `true`.
fn directive(c: &mut Context<'_>, data: RootId) -> Result<Option<Directive>, RootedError> {
    for name in ["skip", "todo"] {
        let value = get(c, data, name)?;
        if !c.truthy_rooted(value)? {
            continue;
        }
        let message = c.string_text(value)?.unwrap_or_default();
        return Ok(Some(if name == "skip" { Directive::Skip(message) } else { Directive::Todo(message) }));
    }
    Ok(None)
}

/// Routes an event to the reporter's formatting for it.
fn feed(reporter: &mut Reporter, event: Event) -> String {
    match event {
        Event::Report(report) => reporter.event(&report),
        Event::Subtest { nesting, name } => reporter.subtest(nesting, &name),
        Event::Plan { nesting, count } => reporter.plan(nesting, count),
        Event::Diagnostic { nesting, message } => reporter.diagnostic(nesting, &message),
        Event::Output(text) => reporter.output(&text),
        Event::Ignored => String::new(),
    }
}

fn register(reporter: Reporter) -> u64 {
    let id = NEXT_ID.with(|next| next.replace(next.get() + 1));
    REPORTERS.with(|reporters| {
        reporters.borrow_mut().insert(id, reporter);
    });
    id
}

fn with_reporter<T>(id: u64, act: impl FnOnce(&mut Reporter) -> T) -> Option<T> {
    REPORTERS.with(|reporters| reporters.borrow_mut().get_mut(&id).map(act))
}

fn release(id: u64) {
    REPORTERS.with(|reporters| {
        reporters.borrow_mut().remove(&id);
    });
}

fn finish_and_release(id: u64) -> String {
    let text = with_reporter(id, Reporter::finish).unwrap_or_default();
    release(id);
    text
}

fn reporter_id(c: &mut Context<'_>, object: RootId) -> Result<Option<u64>, RootedError> {
    let value = get(c, object, REPORTER_ID)?;
    Ok(c.rooted_value(value).and_then(|value| value.as_number()).map(|id| id as u64))
}

fn state_id(c: &mut Context<'_>, state: RootId) -> Result<u64, RootedError> {
    let value = get(c, state, REPORTER_ID)?;
    Ok(c.rooted_value(value).and_then(|value| value.as_number()).unwrap_or(0.0) as u64)
}

fn flag(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    let value = get(c, object, name)?;
    c.truthy_rooted(value)
}

fn set_flag(c: &mut Context<'_>, object: RootId, name: &str) -> Result<(), RootedError> {
    let value = c.boolean(true);
    put(c, object, name, value)
}

fn text_of(c: &mut Context<'_>, object: RootId, name: &str) -> Result<Option<String>, RootedError> {
    let value = get(c, object, name)?;
    c.string_text(value)
}

fn number_of(c: &mut Context<'_>, object: RootId, name: &str) -> Result<Option<f64>, RootedError> {
    let value = get(c, object, name)?;
    Ok(c.rooted_value(value).and_then(|value| value.as_number()))
}
