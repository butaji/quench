//! `stream` module. Constructors remain backed by the existing stream state
//! machine; static orchestration is exposed through Rust capabilities.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::registry::{
    SPEC_STREAM_ADD_ABORT_SIGNAL, SPEC_STREAM_COMPOSE, SPEC_STREAM_DESTROY, SPEC_STREAM_DUPLEX,
    SPEC_STREAM_FINISHED, SPEC_STREAM_FINISHED_ABORT, SPEC_STREAM_FINISHED_CLEANUP,
    SPEC_STREAM_FINISHED_EVENT, SPEC_STREAM_IS_DISTURBED, SPEC_STREAM_IS_ERRORED,
    SPEC_STREAM_IS_READABLE, SPEC_STREAM_IS_WRITABLE, SPEC_STREAM_PIPELINE, SPEC_STREAM_READABLE,
    SPEC_STREAM_TRANSFORM, SPEC_STREAM_WRITABLE, SPEC_STREAM_DUPLEX_PAIR,
    SPEC_STREAM_DUPLEX_PAIR_WRITE, SPEC_STREAM_DUPLEX_PAIR_UNCORK, SPEC_STREAM_DUPLEX_PAIR_FINAL,
    SPEC_STREAM_WEB_PIPELINE_COMPLETE, SPEC_STREAM_WEB_PIPELINE_ERROR,
};

const PRELUDE: &str = include_str!("stream_prelude.js");

pub fn new_readable(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Readable"))
}
pub fn new_writable(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Writable"))
}
pub fn new_duplex(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Duplex"))
}
pub fn new_transform(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Transform"))
}

fn stream_object(name: &str) -> Value {
    host_api::object(vec![
        ("readable".to_string(), Value::Boolean(true)),
        ("writable".to_string(), Value::Boolean(true)),
        ("name".to_string(), Value::String(name.into())),
    ])
}

pub fn pipeline(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let (raw_stages, callback) = split_pipeline_args(args);
    if raw_stages.iter().any(is_web_stage) {
        return web_pipeline(state, &raw_stages, callback);
    }
    let stages = normalize_pipeline(state, raw_stages)?;
    if stages.is_empty() {
        let code = if args.is_empty() {
            "ERR_INVALID_ARG_TYPE"
        } else {
            "ERR_MISSING_ARGS"
        };
        return Err(pipeline_error(
            "The pipeline requires at least two streams",
            code,
        ));
    }
    validate_pipeline(&stages)?;
    if callback.is_none() {
        let code = if stages.len() > 2 {
            "ERR_INVALID_ARG_TYPE"
        } else if args.is_empty() {
            "ERR_INVALID_ARG_TYPE"
        } else {
            "ERR_MISSING_ARGS"
        };
        return Err(pipeline_error("The pipeline requires a callback", code));
    }
    for pair in stages.windows(2) {
        if let Err(error) = pipe(&pair[0], &pair[1]) {
            let error = unable_to_pipe(error);
            if let Some(callback) = callback.as_ref() {
                execute::call(callback, &Value::Undefined, std::slice::from_ref(&error))?;
            }
            return Ok(stages.last().cloned().unwrap_or(Value::Undefined));
        }
    }
    if let Some(callback) = callback {
        attach_pipeline_callback(&stages, callback)?;
    }
    Ok(stages.last().cloned().unwrap_or(Value::Undefined))
}

fn is_web_stage(value: &Value) -> bool {
    quench_runtime::is_callable(&execute::get_property(value, "getReader"))
        || quench_runtime::is_callable(&execute::get_property(value, "getWriter"))
}

fn web_readable(value: &Value) -> Value {
    if quench_runtime::is_callable(&execute::get_property(value, "pipeTo")) {
        value.clone()
    } else {
        execute::get_property(value, "readable")
    }
}

fn web_writable(value: &Value) -> Value {
    if quench_runtime::is_callable(&execute::get_property(value, "getWriter")) {
        value.clone()
    } else {
        execute::get_property(value, "writable")
    }
}

fn web_pipeline(
    state: &Rc<RefCell<HostState>>,
    stages: &[Value],
    callback: Option<Value>,
) -> Result<Value, VmError> {
    if stages.len() < 2 {
        return Err(pipeline_error(
            "The pipeline requires at least two streams",
            "ERR_MISSING_ARGS",
        ));
    }
    let callback = callback.ok_or_else(|| {
        pipeline_error("The pipeline requires a callback", "ERR_INVALID_ARG_TYPE")
    })?;
    let mut pipes = Vec::with_capacity(stages.len() - 1);
    for pair in stages.windows(2) {
        let source = web_readable(&pair[0]);
        let destination = web_writable(&pair[1]);
        let pipe_to = execute::get_property(&source, "pipeTo");
        if !quench_runtime::is_callable(&pipe_to) {
            return Err(pipeline_error(
                "The \"streams\" argument must contain stream instances",
                "ERR_INVALID_ARG_TYPE",
            ));
        }
        pipes.push(execute::call(&pipe_to, &source, &[destination])?);
    }
    let global = quench_runtime::vm::current_global_object();
    let promise_ctor = execute::get_property(&global, "Promise");
    let all = execute::get_property(&promise_ctor, "all");
    let all = execute::call(&all, &promise_ctor, &[host_api::array(pipes)])?;
    let complete = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(SPEC_STREAM_WEB_PIPELINE_COMPLETE),
        vec![callback.clone()],
    );
    let failed = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(SPEC_STREAM_WEB_PIPELINE_ERROR),
        vec![callback],
    );
    let _ = quench_runtime::promise_then(Some(&all), &[complete, failed])?;
    Ok(stages.last().cloned().unwrap_or(Value::Undefined))
}

pub fn web_pipeline_complete(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    if quench_runtime::is_callable(&callback) {
        execute::call(&callback, &Value::Undefined, &[])?;
    }
    Ok(Value::Undefined)
}

pub fn web_pipeline_error(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    let error = args.get(1).cloned().unwrap_or(Value::Undefined);
    if quench_runtime::is_callable(&callback) {
        execute::call(&callback, &Value::Undefined, &[error])?;
    }
    Ok(Value::Undefined)
}

fn split_pipeline_args(args: &[Value]) -> (Vec<Value>, Option<Value>) {
    let mut stages = args.to_vec();
    let callback = stages
        .last()
        .filter(|value| quench_runtime::is_callable(value))
        .cloned();
    if callback.is_some() {
        stages.pop();
    }
    (stages, callback)
}

fn normalize_pipeline(
    state: &Rc<RefCell<HostState>>,
    mut stages: Vec<Value>,
) -> Result<Vec<Value>, VmError> {
    if let Some(first) = stages.first().cloned() {
        if let Value::Array(ref array) = first {
            let values: Vec<Value> = (0..array.logical_len())
                .map(|index| {
                    execute::get_property(&Value::Array(array.clone()), &index.to_string())
                })
                .collect();
            if is_stage_list(&values) {
                stages.splice(0..1, values);
            } else {
                stages[0] = readable_from(state, first)?;
            }
        } else if matches!(
            first,
            Value::String(_) | Value::Generator(_) | Value::Iterator(_)
        ) {
            stages[0] = readable_from(state, first)?;
        }
    }
    for stage in stages.iter_mut() {
        if quench_runtime::is_callable(stage) {
            if is_sync_generator(stage) {
                return Err(pipeline_error(
                    "The pipeline function must return an AsyncIterable",
                    "ERR_INVALID_RETURN_VALUE",
                ));
            }
            *stage = compose_stage(state, stage.clone())?;
        }
    }
    Ok(stages)
}

fn is_sync_generator(stage: &Value) -> bool {
    let constructor = execute::get_property(stage, "constructor");
    matches!(
        execute::get_property(&constructor, "name"),
        Value::String(name) if name == "GeneratorFunction"
    )
}

fn is_stage_list(values: &[Value]) -> bool {
    values.len() >= 2
        && values[..values.len() - 1]
            .iter()
            .all(|value| has_callable(value, "pipe") && has_callable(value, "on"))
        && values
            .last()
            .is_some_and(|value| has_callable(value, "on") && has_callable(value, "write"))
}

fn readable_from(state: &Rc<RefCell<HostState>>, source: Value) -> Result<Value, VmError> {
    let module = state
        .borrow()
        .stream_module
        .clone()
        .ok_or(VmError::NotCallable)?;
    let readable = execute::get_property(&module, "Readable");
    let from = execute::get_property(&readable, "from");
    execute::call(&from, &readable, &[source])
}

fn compose_stage(state: &Rc<RefCell<HostState>>, stage: Value) -> Result<Value, VmError> {
    let module = state
        .borrow()
        .stream_module
        .clone()
        .ok_or(VmError::NotCallable)?;
    let compose = execute::get_property(&module, "compose");
    let mut composed = execute::call(&compose, &module, &[stage.clone()])?;
    if quench_runtime::is_callable(&stage) {
        let writable_state = execute::get_property(&composed, "_writableState");
        let writable_state =
            execute::set_property(writable_state, "objectMode", Value::Boolean(true));
        execute::set_property_in_place(&writable_state, "objectMode", Value::Boolean(true));
        execute::set_property_in_place(&composed, "_writableState", writable_state.clone());
        composed = execute::set_property(composed, "_writableState", writable_state);
    }
    Ok(composed)
}

fn validate_pipeline(stages: &[Value]) -> Result<(), VmError> {
    if stages.len() < 2 {
        return Err(pipeline_error(
            "The pipeline requires at least two streams",
            "ERR_MISSING_ARGS",
        ));
    }
    let first = &stages[0];
    if !has_callable(first, "pipe") || !has_callable(first, "on") {
        return Err(pipeline_error(
            "The \"streams\" argument must contain stream instances",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    for stream in &stages[1..stages.len() - 1] {
        if !has_callable(stream, "pipe") || !has_callable(stream, "on") {
            return Err(pipeline_error(
                "The \"streams\" argument must contain stream instances",
                "ERR_INVALID_ARG_TYPE",
            ));
        }
    }
    let last = stages.last().expect("validated length");
    if !has_callable(last, "on") || !has_callable(last, "write") || !has_callable(last, "end") {
        return Err(pipeline_error(
            "The \"streams\" argument must contain stream instances",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    Ok(())
}

fn has_callable(target: &Value, key: &str) -> bool {
    quench_runtime::is_callable(&execute::get_property(target, key))
}

fn pipe(source: &Value, destination: &Value) -> Result<(), VmError> {
    let method = execute::get_property(source, "pipe");
    execute::call(&method, source, std::slice::from_ref(destination)).map(|_| ())
}

fn attach_pipeline_callback(stages: &[Value], callback: Value) -> Result<(), VmError> {
    let last = stages.last().expect("validated length");
    let once = execute::get_property(last, "once");
    if quench_runtime::is_callable(&once) {
        // `pipeline` owns a writable terminal even when it is also
        // readable (for example PassThrough). Node completes the callback on
        // that terminal's `finish`; waiting for `end` would require a reader
        // to consume the destination and leaves empty pipelines pending.
        let event = "finish";
        execute::call(
            &once,
            last,
            &[Value::String(event.into()), callback.clone()],
        )?;
        execute::call(&once, last, &[Value::String("error".into()), callback])?;
    }
    for pair in stages.windows(2) {
        let source_error = execute::get_property(&pair[0], "once");
        let destroy = execute::get_property(&pair[1], "destroy");
        if !quench_runtime::is_callable(&source_error) || !quench_runtime::is_callable(&destroy) {
            continue;
        }
        let bind = execute::get_property(&destroy, "bind");
        if !quench_runtime::is_callable(&bind) {
            continue;
        }
        let bound = execute::call(&bind, &destroy, std::slice::from_ref(&pair[1]))?;
        execute::call(
            &source_error,
            &pair[0],
            &[Value::String("error".into()), bound],
        )?;
    }
    Ok(())
}

fn pipeline_error(message: &str, code: &str) -> VmError {
    let error = execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::TypeError),
        &Value::Undefined,
        &[Value::String(message.into())],
    )
    .unwrap_or_else(|_| host_api::object(Vec::new()));
    execute::set_property_in_place(&error, "code", Value::String(code.into()));
    VmError::Thrown(error)
}

fn unable_to_pipe(error: VmError) -> Value {
    let value = match error {
        VmError::Thrown(value) => value,
        _ => host_api::object(Vec::new()),
    };
    if matches!(execute::get_property(&value, "code"), Value::Undefined) {
        execute::set_property_in_place(
            &value,
            "code",
            Value::String("ERR_STREAM_UNABLE_TO_PIPE".into()),
        );
    }
    value
}

pub fn is_readable(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(args.first().map(is_readable_value).unwrap_or(Value::Null))
}

fn is_readable_value(value: &Value) -> Value {
    let state = execute::get_property(value, "_readableState");
    if matches!(state, Value::Undefined | Value::Null) {
        return if matches!(execute::get_property(value, "readable"), Value::Boolean(_)) {
            Value::Boolean(false)
        } else {
            Value::Null
        };
    }
    if matches!(
        execute::get_property(value, "destroyed"),
        Value::Boolean(true)
    ) || matches!(
        execute::get_property(value, "readable"),
        Value::Boolean(false)
    ) || matches!(
        execute::get_property(&state, "endEmitted"),
        Value::Boolean(true)
    ) {
        Value::Boolean(false)
    } else {
        Value::Boolean(true)
    }
}

pub fn is_writable(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(args.first().map(is_writable_value).unwrap_or(Value::Null))
}

fn is_writable_value(value: &Value) -> Value {
    let state = execute::get_property(value, "_writableState");
    if matches!(state, Value::Undefined | Value::Null) {
        return if matches!(execute::get_property(value, "writable"), Value::Boolean(_)) {
            Value::Boolean(false)
        } else {
            Value::Null
        };
    }
    if matches!(
        execute::get_property(value, "destroyed"),
        Value::Boolean(true)
    ) || matches!(
        execute::get_property(value, "writable"),
        Value::Boolean(false)
    ) || matches!(execute::get_property(&state, "ended"), Value::Boolean(true))
    {
        Value::Boolean(false)
    } else {
        Value::Boolean(true)
    }
}

pub fn is_errored(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(args
        .first()
        .map(|value| {
            let readable =
                execute::get_property(&execute::get_property(value, "_readableState"), "errored");
            let writable =
                execute::get_property(&execute::get_property(value, "_writableState"), "errored");
            if !matches!(readable, Value::Undefined | Value::Null) {
                readable
            } else if !matches!(writable, Value::Undefined | Value::Null) {
                writable
            } else if !matches!(
                execute::get_property(value, "_readableState"),
                Value::Undefined | Value::Null
            ) || !matches!(
                execute::get_property(value, "_writableState"),
                Value::Undefined | Value::Null
            ) {
                Value::Boolean(false)
            } else {
                Value::Boolean(false)
            }
        })
        .unwrap_or(Value::Boolean(false)))
}

pub fn is_disturbed(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(args.first().is_some_and(|value| {
        matches!(
            execute::get_property(value, "readableDidRead"),
            Value::Boolean(true)
        ) || matches!(
            execute::get_property(value, "destroyed"),
            Value::Boolean(true)
        ) || matches!(
            execute::get_property(
                &execute::get_property(value, "_readableState"),
                "endEmitted"
            ),
            Value::Boolean(true)
        )
    })))
}

/// Static `stream.destroy(stream[, error])` delegates to the stream's own
/// state machine, preserving the implementation's error/close ordering.
pub fn destroy(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = args.first().cloned().unwrap_or(Value::Undefined);
    let error = args.get(1).cloned().unwrap_or(Value::Undefined);
    let error = if matches!(error, Value::Undefined) {
        abort_error()
    } else {
        error
    };
    let method = execute::get_property(&stream, "destroy");
    if quench_runtime::is_callable(&method) {
        let _ = execute::call(&method, &stream, &[error])?;
    } else if matches!(stream, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_property_in_place(&stream, "destroyed", Value::Boolean(true));
    }
    Ok(stream)
}

pub(crate) fn abort_error_for_host() -> Value {
    let error = execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::Error),
        &Value::Undefined,
        &[Value::String("The operation was aborted".into())],
    )
    .unwrap_or_else(|_| host_api::object(Vec::new()));
    let error = execute::set_property(error, "name", Value::String("AbortError".into()));
    execute::set_property(error, "code", Value::String("ABORT_ERR".into()))
}

fn abort_error() -> Value {
    abort_error_for_host()
}

pub fn add_abort_signal(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let signal = args.first().cloned().unwrap_or(Value::Undefined);
    let stream = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !matches!(
        execute::get_property(&signal, crate::modules::event_target::ABORT_SIGNAL_BRAND),
        Value::Boolean(true)
    ) {
        return Err(pipeline_error(
            "The \"signal\" argument must be an instance of AbortSignal",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    let destroyable = quench_runtime::is_callable(&execute::get_property(&stream, "destroy"));
    let cancelable = quench_runtime::is_callable(&execute::get_property(&stream, "cancel"));
    let readable_web = !matches!(
        execute::get_property(&stream, "getReader"),
        Value::Undefined | Value::Null
    );
    if !destroyable && !cancelable && !readable_web {
        return Err(pipeline_error(
            "The \"stream\" argument must be an instance of Stream",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    let listener = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(SPEC_STREAM_ADD_ABORT_SIGNAL),
        vec![stream.clone(), signal.clone()],
    );
    crate::modules::events::add_abort_listener(state, &[signal, listener])?;
    Ok(stream)
}

pub fn finished(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = args.first().cloned().unwrap_or(Value::Undefined);
    let callback = args
        .iter()
        .rev()
        .find(|value| quench_runtime::is_callable(value))
        .cloned()
        .ok_or_else(|| pipeline_error("The callback must be a function", "ERR_INVALID_ARG_TYPE"))?;
    let options = args
        .get(1)
        .filter(|value| !quench_runtime::is_callable(value));
    if let Some(options) = options {
        if !matches!(
            options,
            Value::Null | Value::Object(_) | Value::ObjectAlias(_)
        ) {
            return Err(pipeline_error(
                "The options argument must be an object",
                "ERR_INVALID_ARG_TYPE",
            ));
        }
        let signal = execute::get_property(options, "signal");
        if !matches!(signal, Value::Undefined | Value::Null)
            && !quench_runtime::is_callable(&execute::get_property(&signal, "addEventListener"))
        {
            return Err(pipeline_error(
                "The signal option must be an AbortSignal",
                "ERR_INVALID_ARG_TYPE",
            ));
        }
    }
    let no_stream_sides = matches!(
        execute::get_property(&stream, "readable"),
        Value::Boolean(false)
    ) && matches!(
        execute::get_property(&stream, "writable"),
        Value::Boolean(false)
    );
    let has_readable_state = !matches!(
        execute::get_property(&stream, "_readableState"),
        Value::Undefined | Value::Null
    );
    let has_writable_state = !matches!(
        execute::get_property(&stream, "_writableState"),
        Value::Undefined | Value::Null
    );
    let incoming_message = matches!(
        execute::get_property(
            &stream,
            crate::modules::http::INCOMING_CLOSE_PENDING_PROP,
        ),
        Value::Boolean(_)
    );
    let server_response = matches!(
        execute::get_property(&stream, crate::modules::http::RES_ID_PROP),
        Value::Number(value) if value.is_finite() && value >= 0.0
    );
    // Web WritableStreams expose their completion promise internally in the
    // polyfill.  They have no EventEmitter surface and may already be locked
    // by the caller, so observe that promise directly instead of calling
    // getWriter() a second time.
    let web_writable = !matches!(
        execute::get_property(&stream, "_closedPromise"),
        Value::Undefined | Value::Null
    );
    let want_readable = option_enabled(options, "readable")
        && (has_readable_state || incoming_message)
        && !matches!(
            execute::get_property(&stream, "readable"),
            Value::Boolean(false)
        );
    let want_writable = (option_enabled(options, "writable")
        && has_writable_state
        && !matches!(
            execute::get_property(&stream, "writable"),
            Value::Boolean(false)
        ))
        || (option_enabled(options, "writable") && no_stream_sides)
        || (option_enabled(options, "writable") && server_response)
        || (option_enabled(options, "writable") && web_writable);
    let has_stream_state =
        ["_readableState", "_writableState"].iter().any(|key| {
            !matches!(
                execute::get_property(&stream, key),
                Value::Undefined | Value::Null
            )
        }) || quench_runtime::is_callable(&execute::get_property(&stream, "getReader"))
            || quench_runtime::is_callable(&execute::get_property(&stream, "getWriter"))
            || quench_runtime::is_callable(&execute::get_property(&stream, "destroy"))
            || quench_runtime::is_callable(&execute::get_property(&stream, "pipe"))
        || quench_runtime::is_callable(&execute::get_property(&stream, "write"))
        || web_writable;
    if !has_stream_state {
        return Err(pipeline_error(
            "The \"stream\" argument must be a stream",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    let once = execute::get_property(&stream, "once");
    if !web_writable && !quench_runtime::is_callable(&once) {
        return Err(pipeline_error(
            "The \"stream\" argument must be a stream",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    let state_object = host_api::object(vec![
        ("done".into(), Value::Boolean(false)),
        ("abortedPending".into(), Value::Boolean(false)),
        ("readableWanted".into(), Value::Boolean(want_readable)),
        ("writableWanted".into(), Value::Boolean(want_writable)),
        ("readableDone".into(), Value::Boolean(!want_readable)),
        ("writableDone".into(), Value::Boolean(!want_writable)),
    ]);
    execute::set_property_in_place(&state_object, "stream", stream.clone());
    let event = |side: &str| {
        host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_FINISHED_EVENT),
            vec![
                state_object.clone(),
                callback.clone(),
                Value::String(side.into()),
            ],
        )
    };
    let on_end = event("readable");
    let on_finish = event("writable");
    let on_error = event("error");
    let on_close = event("close");
    execute::set_property_in_place(&state_object, "onEnd", on_end.clone());
    execute::set_property_in_place(&state_object, "onFinish", on_finish.clone());
    execute::set_property_in_place(&state_object, "onError", on_error.clone());
    execute::set_property_in_place(&state_object, "onClose", on_close.clone());
    if want_readable {
        execute::call(&once, &stream, &[Value::String("end".into()), on_end])?;
    }
    if want_writable && !web_writable {
        execute::call(&once, &stream, &[Value::String("finish".into()), on_finish])?;
    }
    if !web_writable {
        execute::call(&once, &stream, &[Value::String("error".into()), on_error])?;
        execute::call(&once, &stream, &[Value::String("close".into()), on_close])?;
    } else {
        let closed = execute::get_property(&stream, "_closedPromise");
        let then = execute::get_property(&closed, "then");
        let fulfilled = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_FINISHED_EVENT),
            vec![state_object.clone(), callback.clone(), Value::String("writable".into())],
        );
        let rejected = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_FINISHED_EVENT),
            vec![state_object.clone(), callback.clone(), Value::String("error".into())],
        );
        execute::call(&then, &closed, &[fulfilled, rejected])?;
    }
    if let Some(options) = options {
        let signal = execute::get_property(options, "signal");
        if !matches!(signal, Value::Undefined | Value::Null) {
            let on_abort = host_api::bound_capability_with_arguments(
                crate::host::capability_ref(SPEC_STREAM_FINISHED_ABORT),
                vec![state_object.clone(), stream.clone(), callback.clone()],
            );
            let pre_aborted = execute::is_truthy(&execute::get_property(&signal, "aborted"));
            let dispose = crate::modules::events::add_abort_listener(state, &[signal, on_abort])?;
            execute::set_property_in_place(&state_object, "abortDispose", dispose);
            if pre_aborted {
                execute::set_property_in_place(
                    &state_object,
                    "abortedPending",
                    Value::Boolean(true),
                );
                let synchronous = matches!(
                    execute::get_property(options, "Symbol(kEosNodeSynchronousCallback)"),
                    Value::Boolean(true)
                );
                if synchronous {
                    finished_abort(
                        state,
                        None,
                        &[state_object.clone(), stream.clone(), callback.clone()],
                    )?;
                }
            }
        }
    }
    // ServerResponse emits `close` after `finish`; callers commonly install
    // `finished()` from that close listener.  Reconcile the already-terminal
    // writable fact instead of waiting for an event that has passed.
    if server_response
        && matches!(
            execute::get_property(&stream, "finished"),
            Value::Boolean(true)
        )
    {
        execute::set_property_in_place(&state_object, "done", Value::Boolean(true));
        finished_cleanup(state, None, &[state_object.clone(), stream.clone()])?;
        execute::call(&callback, &Value::Undefined, &[])?;
    }
    // IncomingMessage marks its transport close before notifying listeners;
    // a `finished()` call made from that listener observes a completed
    // message rather than a future close edge.
    if matches!(
        execute::get_property(&stream, crate::modules::http::REQ_CLOSE_PROP),
        Value::Boolean(true)
    ) {
        execute::set_property_in_place(&state_object, "done", Value::Boolean(true));
        finished_cleanup(state, None, &[state_object.clone(), stream.clone()])?;
        execute::call(&callback, &Value::Undefined, &[])?;
    }
    Ok(host_api::bound_capability_with_arguments(
        crate::host::capability_ref(SPEC_STREAM_FINISHED_CLEANUP),
        vec![state_object, stream],
    ))
}

/// Event callback used by `finished`; its fixed arguments are the shared
/// completion record, user callback, and side name, followed by event data.
pub fn finished_event(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let state = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(execute::get_property(&state, "done"), Value::Boolean(true)) {
        return Ok(Value::Undefined);
    }
    let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
    let side = args.get(2).and_then(|value| match value {
        Value::String(side) => Some(side.as_str()),
        _ => None,
    });
    if side == Some("error") {
        execute::set_property_in_place(&state, "done", Value::Boolean(true));
        finished_cleanup(
            _state,
            None,
            &[state.clone(), execute::get_property(&state, "stream")],
        )?;
        execute::call(
            &callback,
            &Value::Undefined,
            &[args.get(3).cloned().unwrap_or(Value::Undefined)],
        )?;
        return Ok(Value::Undefined);
    }
    if side == Some("close") {
        let stream = execute::get_property(&state, "stream");
        let readable_terminal = !matches!(
            execute::get_property(&state, "readableWanted"),
            Value::Boolean(true)
        ) || matches!(
            execute::get_property(&execute::get_property(&stream, "_readableState"), "ended"),
            Value::Boolean(true)
        ) || matches!(
            execute::get_property(
                &execute::get_property(&stream, "_readableState"),
                "endEmitted"
            ),
            Value::Boolean(true)
        );
        let writable_terminal = !matches!(
            execute::get_property(&state, "writableWanted"),
            Value::Boolean(true)
        ) || matches!(
            execute::get_property(&execute::get_property(&stream, "_writableState"), "ended"),
            Value::Boolean(true)
        ) || matches!(
            execute::get_property(
                &execute::get_property(&stream, "_writableState"),
                "finished"
            ),
            Value::Boolean(true)
        );
        if readable_terminal && writable_terminal {
            return Ok(Value::Undefined);
        }
        if matches!(
            execute::get_property(&state, "abortedPending"),
            Value::Boolean(true)
        ) {
            return Ok(Value::Undefined);
        }
        let readable_done = matches!(
            execute::get_property(&state, "readableDone"),
            Value::Boolean(true)
        );
        let writable_done = matches!(
            execute::get_property(&state, "writableDone"),
            Value::Boolean(true)
        );
        if readable_done && writable_done {
            return Ok(Value::Undefined);
        }
        let error = execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::Error),
            &Value::Undefined,
            &[Value::String("Premature close".into())],
        )
        .unwrap_or_else(|_| host_api::object(Vec::new()));
        execute::set_property_in_place(
            &error,
            "code",
            Value::String("ERR_STREAM_PREMATURE_CLOSE".into()),
        );
        execute::set_property_in_place(&state, "done", Value::Boolean(true));
        finished_cleanup(
            _state,
            None,
            &[state.clone(), execute::get_property(&state, "stream")],
        )?;
        execute::call(&callback, &Value::Undefined, &[error])?;
        return Ok(Value::Undefined);
    }
    if let Some(side) = side {
        execute::set_property_in_place(&state, &format!("{side}Done"), Value::Boolean(true));
    }
    let complete = matches!(
        execute::get_property(&state, "readableDone"),
        Value::Boolean(true)
    ) && matches!(
        execute::get_property(&state, "writableDone"),
        Value::Boolean(true)
    );
    if complete {
        execute::set_property_in_place(&state, "done", Value::Boolean(true));
        finished_cleanup(
            _state,
            None,
            &[state.clone(), execute::get_property(&state, "stream")],
        )?;
        execute::call(&callback, &Value::Undefined, &[])?;
    }
    Ok(Value::Undefined)
}

pub fn finished_abort(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let record = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(execute::get_property(&record, "done"), Value::Boolean(true)) {
        return Ok(Value::Undefined);
    }
    finished_cleanup(
        state,
        None,
        &[
            record.clone(),
            args.get(1).cloned().unwrap_or(Value::Undefined),
        ],
    )?;
    let callback = args.get(2).cloned().unwrap_or(Value::Undefined);
    execute::call(&callback, &Value::Undefined, &[abort_error()])?;
    Ok(Value::Undefined)
}

pub fn finished_cleanup(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let record = args.first().cloned().unwrap_or(Value::Undefined);
    let stream = args.get(1).cloned().unwrap_or(Value::Undefined);
    execute::set_property_in_place(&record, "done", Value::Boolean(true));
    let remove = execute::get_property(&stream, "removeListener");
    if quench_runtime::is_callable(&remove) {
        for (event, key) in [
            ("end", "onEnd"),
            ("finish", "onFinish"),
            ("error", "onError"),
            ("close", "onClose"),
        ] {
            let listener = execute::get_property(&record, key);
            if quench_runtime::is_callable(&listener) {
                execute::call(&remove, &stream, &[Value::String(event.into()), listener])?;
            }
        }
    }
    let dispose = execute::get_property(&record, "abortDispose");
    let dispose = execute::get_property(&dispose, "Symbol.dispose");
    if quench_runtime::is_callable(&dispose) {
        execute::call(&dispose, &Value::Undefined, &[])?;
    }
    Ok(Value::Undefined)
}

pub fn compose(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(pipeline_error(
            "The streams argument must be an array or at least two streams",
            "ERR_MISSING_ARGS",
        ));
    }
    let implementation = state
        .borrow()
        .stream_compose_impl
        .clone()
        .ok_or(VmError::NotCallable)?;
    let module = state
        .borrow()
        .stream_module
        .clone()
        .unwrap_or(Value::Undefined);
    let original_composed = execute::call(&implementation, &module, args)?;
    let composed = execute::canonical_value(&original_composed);
    let first = args.first().expect("non-empty compose arguments");
    let last = args.last().expect("non-empty compose arguments");
    set_stream_mode(
        &composed,
        "_writableState",
        stream_mode(first, "writableObjectMode"),
    );
    set_stream_mode(
        &composed,
        "_readableState",
        stream_mode(last, "readableObjectMode"),
    );
    if matches!(execute::get_property(first, "writable"), Value::Boolean(false)) {
        execute::set_property_in_place(&composed, "writable", Value::Boolean(false));
    }
    if matches!(execute::get_property(last, "readable"), Value::Boolean(false)) {
        execute::set_property_in_place(&composed, "readable", Value::Boolean(false));
    }
    Ok(composed)
}

fn stream_mode(stream: &Value, key: &str) -> bool {
    matches!(execute::get_property(stream, key), Value::Boolean(true))
}

fn set_stream_mode(stream: &Value, state_key: &str, mode: bool) {
    let nested = execute::get_property(stream, state_key);
    execute::set_property_in_place(&nested, "objectMode", Value::Boolean(mode));
    execute::set_property_in_place(stream, state_key, nested);
}

fn pair_values(args: &[Value]) -> Option<(&Value, &Value)> {
    args.first().zip(args.get(1))
}

fn pair_push(destination: &Value, chunk: Value, encoding: Value) -> Result<(), VmError> {
    let push = execute::get_property(destination, "push");
    if !quench_runtime::is_callable(&push) {
        return Err(VmError::NotCallable);
    }
    if matches!(encoding, Value::String(ref value) if value == "buffer") {
        execute::call(&push, destination, &[chunk])?;
    } else {
        execute::call(&push, destination, &[chunk, encoding])?;
    }
    Ok(())
}

fn pair_flush(source: &Value, destination: &Value) -> Result<(), VmError> {
    let pending = execute::get_property(source, "__pairPending");
    let Value::Array(ref array) = pending else {
        return Ok(());
    };
    let entries = (0..array.logical_len())
        .map(|index| execute::get_property(&pending, &index.to_string()))
        .collect::<Vec<_>>();
    execute::set_array_length_in_place(&pending, 0);
    for entry in entries {
        let chunk = execute::get_property(&entry, "0");
        let encoding = execute::get_property(&entry, "1");
        let callback = execute::get_property(&entry, "2");
        pair_push(destination, chunk, encoding)?;
        if quench_runtime::is_callable(&callback) {
            execute::call(&callback, &Value::Undefined, &[])?;
        }
    }
    Ok(())
}

/// Rust-owned half of `stream.duplexPair`: forwarding a writable side into
/// the opposite readable side is a data flow edge, not a second stream
/// implementation. The Duplex constructor remains the shared stream object
/// factory, while these methods carry only the pair-specific state.
pub fn duplex_pair_write(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some((source, destination)) = pair_values(args) else {
        return Err(VmError::NotCallable);
    };
    let chunk = args.get(2).cloned().unwrap_or(Value::Undefined);
    let encoding = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| Value::String("utf8".into()));
    let callback = args.get(4).cloned().unwrap_or(Value::Undefined);
    let corked = execute::get_property(
        &execute::get_property(source, "_writableState"),
        "corked",
    );
    if matches!(corked, Value::Number(value) if value > 0.0) {
        let pending = match execute::get_property(source, "__pairPending") {
            Value::Array(_) => execute::get_property(source, "__pairPending"),
            _ => {
                let value = host_api::array(Vec::new());
                execute::set_property_in_place(source, "__pairPending", value.clone());
                value
            }
        };
        let index = match &pending {
            Value::Array(array) => array.logical_len(),
            _ => 0,
        };
        execute::set_array_element_in_place(
            &pending,
            index,
            host_api::array(vec![chunk, encoding, callback]),
        );
    } else {
        pair_push(destination, chunk, encoding)?;
        if quench_runtime::is_callable(&callback) {
            execute::call(&callback, &Value::Undefined, &[])?;
        }
    }
    Ok(Value::Undefined)
}

pub fn duplex_pair_uncork(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some((source, destination)) = pair_values(args) else {
        return Err(VmError::NotCallable);
    };
    let writable = execute::get_property(source, "_writableState");
    let corked = execute::get_property(&writable, "corked");
    if let Value::Number(value) = corked {
        execute::set_property_in_place(
            &writable,
            "corked",
            Value::Number((value.max(1.0) - 1.0).max(0.0)),
        );
    }
    let now = execute::get_property(&writable, "corked");
    if matches!(now, Value::Number(value) if value == 0.0) {
        pair_flush(source, destination)?;
    }
    Ok(source.clone())
}

pub fn duplex_pair_final(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some((source, destination)) = pair_values(args) else {
        return Err(VmError::NotCallable);
    };
    let callback = args.get(2).cloned().unwrap_or(Value::Undefined);
    let writable = execute::get_property(source, "_writableState");
    execute::set_property_in_place(&writable, "corked", Value::Number(0.0));
    let source = source.clone();
    let destination = destination.clone();
    quench_runtime::module_bindings::enqueue_job(Rc::new(move || {
        let _ = pair_flush(&source, &destination);
        let push = execute::get_property(&destination, "push");
        if quench_runtime::is_callable(&push) {
            let _ = execute::call(&push, &destination, &[Value::Null]);
        }
        if quench_runtime::is_callable(&callback) {
            let _ = execute::call(&callback, &Value::Undefined, &[]);
        }
    }));
    Ok(Value::Undefined)
}

pub fn duplex_pair(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let module = state
        .borrow()
        .stream_module
        .clone()
        .ok_or(VmError::NotCallable)?;
    let duplex = execute::get_property(&module, "Duplex");
    if !quench_runtime::is_callable(&duplex) {
        return Err(VmError::NotCallable);
    }
    let options = args.first().cloned().unwrap_or_else(|| host_api::object(Vec::new()));
    let left = execute::construct_value(&duplex, std::slice::from_ref(&options))?;
    let right = execute::construct_value(&duplex, std::slice::from_ref(&options))?;
    for (source, destination) in [(left.clone(), right.clone()), (right.clone(), left.clone())] {
        execute::set_property_in_place(&source, "__pairPending", host_api::array(Vec::new()));
        let write = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_DUPLEX_PAIR_WRITE),
            vec![source.clone(), destination.clone()],
        );
        let uncork = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_DUPLEX_PAIR_UNCORK),
            vec![source.clone(), destination.clone()],
        );
        let finalizer = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_DUPLEX_PAIR_FINAL),
            vec![source.clone(), destination.clone()],
        );
        execute::set_property_in_place(&source, "_write", write);
        execute::set_property_in_place(&source, "uncork", uncork);
        execute::set_property_in_place(&source, "_final", finalizer);
    }
    Ok(host_api::array(vec![left, right]))
}

fn option_enabled(options: Option<&Value>, key: &str) -> bool {
    !matches!(
        options.map(|value| execute::get_property(value, key)),
        Some(Value::Boolean(false))
    )
}

pub fn build(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    if let Some(cached) = state.borrow().stream_module.clone() {
        return Ok(cached);
    }
    let program = quench_runtime::reduce::reduce_global_script_source(PRELUDE)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    let factory = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
    })?;
    let deps = host_api::object(vec![
        ("events".to_string(), crate::modules::events::build()),
        (
            "string_decoder".to_string(),
            crate::host::namespace_object_from_pairs(crate::modules::string_decoder::build()),
        ),
    ]);
    let mut module = match quench_runtime::vm::call_value(&factory, &Value::Undefined, &[deps]) {
        Ok(module) => module,
        Err(_) => {
            // Keep module loading total when the optional JS stream layer hits
            // an unsupported dynamic construct; the native constructors are
            // the canonical fallback and preserve the public API shape.
            host_api::object(vec![
                (
                    "Readable".into(),
                    crate::host::capability(SPEC_STREAM_READABLE),
                ),
                (
                    "Writable".into(),
                    crate::host::capability(SPEC_STREAM_WRITABLE),
                ),
                ("Duplex".into(), crate::host::capability(SPEC_STREAM_DUPLEX)),
                (
                    "Transform".into(),
                    crate::host::capability(SPEC_STREAM_TRANSFORM),
                ),
                (
                    "pipeline".into(),
                    crate::host::capability(SPEC_STREAM_PIPELINE),
                ),
            ])
        }
    };
    if let Ok(compose) = quench_runtime::execute::get_property_result(&module, "compose") {
        state.borrow_mut().stream_compose_impl = Some(compose);
    }
    if let Ok(pipeline) = quench_runtime::execute::get_property_result(&module, "pipeline") {
        state.borrow_mut().stream_pipeline_impl = Some(pipeline);
    }
    // Node exposes `stream` itself as the callable Stream constructor and
    // hangs the family namespace off that same function.  Preserve one
    // identity rather than returning a parallel object namespace.
    if let Ok(mut stream) = quench_runtime::execute::get_property_result(&module, "Stream") {
        if matches!(stream, Value::Function(_) | Value::BoundFunction(_)) {
            for name in [
                "Readable",
                "Writable",
                "Duplex",
                "Transform",
                "PassThrough",
                "Stream",
                "duplexPair",
                "destroy",
                "addAbortSignal",
                "finished",
                "pipeline",
                "compose",
                "isReadable",
                "isWritable",
                "isErrored",
                "isDisturbed",
            ] {
                if let Ok(value) = quench_runtime::execute::get_property_result(&module, name) {
                    stream = quench_runtime::execute::set_property(stream, name, value);
                }
            }
            for (name, spec) in [
                ("isReadable", SPEC_STREAM_IS_READABLE),
                ("isWritable", SPEC_STREAM_IS_WRITABLE),
                ("isErrored", SPEC_STREAM_IS_ERRORED),
                ("isDisturbed", SPEC_STREAM_IS_DISTURBED),
                ("destroy", SPEC_STREAM_DESTROY),
                ("addAbortSignal", SPEC_STREAM_ADD_ABORT_SIGNAL),
                ("finished", SPEC_STREAM_FINISHED),
                ("compose", SPEC_STREAM_COMPOSE),
            ] {
                stream = quench_runtime::execute::set_property(
                    stream,
                    name,
                    crate::host::capability(spec),
                );
            }
            let pair = crate::host::capability(crate::registry::SPEC_STREAM_DUPLEX_PAIR);
            let pair = quench_runtime::execute::define_property(
                pair,
                "length",
                host_api::object(vec![
                    ("value".into(), Value::Number(1.0)),
                    ("writable".into(), Value::Boolean(false)),
                    ("enumerable".into(), Value::Boolean(false)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            )
            .unwrap_or_else(|_| crate::host::capability(crate::registry::SPEC_STREAM_DUPLEX_PAIR));
            stream = quench_runtime::execute::set_property(stream, "duplexPair", pair);
            module = stream;
        }
    }
    state.borrow_mut().stream_module = Some(module.clone());
    Ok(module)
}

/// Build the stream-consumer namespace from one shared consumption reducer.
pub fn build_consumers(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    if let Some(cached) = state.borrow().stream_consumers_module.clone() {
        return Ok(cached);
    }
    let module = crate::modules::stream_consumers::build();
    state.borrow_mut().stream_consumers_module = Some(module.clone());
    Ok(module)
}
