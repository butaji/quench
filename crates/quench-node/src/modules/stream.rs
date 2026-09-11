//! `stream` module. Constructors remain backed by the existing stream state
//! machine; static orchestration is exposed through Rust capabilities.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::registry::{
    SPEC_FS_WRITE_STREAM_AUTO_CLOSE_GET, SPEC_FS_WRITE_STREAM_AUTO_CLOSE_SET,
    SPEC_STREAM_ADD_ABORT_SIGNAL, SPEC_STREAM_COMPOSE, SPEC_STREAM_DESTROY, SPEC_STREAM_DUPLEX,
    SPEC_STREAM_CONSTRUCTOR_ADAPTER, SPEC_STREAM_DUPLEX_PAIR, SPEC_STREAM_DUPLEX_PAIR_FINAL,
    SPEC_STREAM_DUPLEX_PAIR_UNCORK, SPEC_STREAM_DUPLEX_PAIR_WRITE, SPEC_STREAM_FINISHED,
    SPEC_STREAM_FINISHED_ABORT, SPEC_STREAM_FINISHED_CLEANUP, SPEC_STREAM_FINISHED_EVENT,
    SPEC_STREAM_GET_DEFAULT_HWM, SPEC_STREAM_IS_DISTURBED, SPEC_STREAM_IS_ERRORED,
    SPEC_STREAM_IS_READABLE, SPEC_STREAM_IS_WRITABLE, SPEC_STREAM_PIPELINE,
    SPEC_STREAM_PROMISES_CALLBACK, SPEC_STREAM_PROMISES_FINISHED, SPEC_STREAM_PROMISES_PIPELINE,
    SPEC_STREAM_READABLE, SPEC_STREAM_READABLE_BUFFER, SPEC_STREAM_READABLE_WRAP,
    SPEC_STREAM_READABLE_PUSH_ADAPTER, SPEC_STREAM_READABLE_READ_ADAPTER,
    SPEC_STREAM_READABLE_WRAP_EVENT,
    SPEC_STREAM_READABLE_WRAP_PROXY, SPEC_STREAM_SET_DEFAULT_HWM,
    SPEC_STREAM_TRANSFORM, SPEC_STREAM_WEB_PIPELINE_COMPLETE, SPEC_STREAM_WEB_PIPELINE_ERROR,
    SPEC_STREAM_WRITABLE, SPEC_STREAM_WRITABLE_HAS_INSTANCE, SPEC_STREAM_WRITABLE_WRITE_ADAPTER,
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
    let terminal = raw_stages
        .last()
        .filter(|value| is_terminal_pipeline_function(value))
        .cloned();
    let stream_args = terminal.as_ref().map_or_else(
        || raw_stages.clone(),
        |_| raw_stages[..raw_stages.len() - 1].to_vec(),
    );
    let stages = normalize_pipeline(state, stream_args)?;
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
    if let Some(terminal) = terminal {
        validate_terminal_pipeline(&stages)?;
        return run_terminal_pipeline(state, &stages, terminal, callback.expect("validated"));
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

fn run_terminal_pipeline(
    state: &Rc<RefCell<HostState>>,
    stages: &[Value],
    terminal: Value,
    callback: Value,
) -> Result<Value, VmError> {
    for pair in stages.windows(2) {
        pipe(&pair[0], &pair[1]).map_err(|error| VmError::Thrown(unable_to_pipe(error)))?;
    }
    let source = stages.last().expect("validated terminal pipeline").clone();
    let output = execute::call(&terminal, &Value::Undefined, &[source])?;
    if matches!(output, Value::Promise(_)) {
        let fulfilled = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_WEB_PIPELINE_COMPLETE),
            vec![callback.clone(), Value::Boolean(true)],
        );
        let rejected = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_WEB_PIPELINE_ERROR),
            vec![callback],
        );
        quench_runtime::promise_then(Some(&output), &[fulfilled, rejected])?;
        return Ok(stages.last().cloned().unwrap_or(Value::Undefined));
    }
    let output_stream = readable_from(state, output.clone())?;
    let options = host_api::object(vec![
        ("readable".into(), Value::Boolean(true)),
        ("writable".into(), Value::Boolean(false)),
    ]);
    finished(state, None, &[output_stream.clone(), options, callback])?;
    Ok(output_stream)
}

fn validate_terminal_pipeline(stages: &[Value]) -> Result<(), VmError> {
    let first = stages.first().expect("terminal pipeline has a source");
    if !has_callable(first, "pipe") || !has_callable(first, "on") {
        return Err(pipeline_error(
            "The \"streams\" argument must contain stream instances",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    for stream in stages.iter().skip(1) {
        if !has_callable(stream, "pipe")
            || !has_callable(stream, "on")
            || !has_callable(stream, "write")
            || !has_callable(stream, "end")
        {
            return Err(pipeline_error(
                "The \"streams\" argument must contain stream instances",
                "ERR_INVALID_ARG_TYPE",
            ));
        }
    }
    Ok(())
}

/// Promise APIs reuse the callback pipeline state machine. The promise
/// capability is created before validation so synchronous failures become a
/// rejected promise, while option validation performed by `finished` remains
/// synchronous as required by Node.
pub fn promises_pipeline(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (promise, resolve, reject) = promise_resolvers()?;
    let callback = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(SPEC_STREAM_PROMISES_CALLBACK),
        vec![resolve, reject.clone()],
    );
    let mut pipeline_args = strip_pipeline_options(args);
    pipeline_args.push(callback);
    if let Err(error) = pipeline(state, &pipeline_args) {
        settle_rejected(&reject, error);
    }
    Ok(promise)
}

pub fn promises_finished(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    validate_finished_options(args.get(1))?;
    let (promise, resolve, reject) = promise_resolvers()?;
    let callback = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(SPEC_STREAM_PROMISES_CALLBACK),
        vec![resolve, reject.clone()],
    );
    let mut finished_args = args.to_vec();
    finished_args.push(callback);
    if let Err(error) = finished(state, None, &finished_args) {
        settle_rejected(&reject, error);
    }
    Ok(promise)
}

pub fn promises_callback(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let rejected = !matches!(args.get(2), None | Some(Value::Undefined | Value::Null));
    let target = if rejected { args.get(1) } else { args.first() };
    if let Some(target) = target.filter(|value| quench_runtime::is_callable(value)) {
        let arguments = if rejected {
            vec![args.get(2).cloned().unwrap_or(Value::Undefined)]
        } else {
            Vec::new()
        };
        execute::call(target, &Value::Undefined, &arguments)?;
    }
    Ok(Value::Undefined)
}

fn promise_resolvers() -> Result<(Value, Value, Value), VmError> {
    let global = quench_runtime::vm::current_global_object();
    let promise_ctor = execute::get_property(&global, "Promise");
    let with_resolvers = execute::get_property(&promise_ctor, "withResolvers");
    let capability = execute::call(&with_resolvers, &promise_ctor, &[])?;
    Ok((
        execute::get_property(&capability, "promise"),
        execute::get_property(&capability, "resolve"),
        execute::get_property(&capability, "reject"),
    ))
}

fn settle_rejected(reject: &Value, error: VmError) {
    let reason = match error {
        VmError::Thrown(value) => value,
        _ => Value::Undefined,
    };
    let _ = execute::call(reject, &Value::Undefined, &[reason]);
}

fn strip_pipeline_options(args: &[Value]) -> Vec<Value> {
    let mut values = args.to_vec();
    if values.len() > 1 {
        if let Some(last) = values.last() {
            let streamish = ["pipe", "write", "read", "getReader", "getWriter"]
                .iter()
                .any(|name| quench_runtime::is_callable(&execute::get_property(last, name)));
            if !streamish && matches!(last, Value::Object(_) | Value::ObjectAlias(_)) {
                values.pop();
            }
        }
    }
    values
}

fn validate_finished_options(value: Option<&Value>) -> Result<(), VmError> {
    let Some(options) = value else {
        return Ok(());
    };
    let cleanup = execute::get_property(options, "cleanup");
    if !matches!(cleanup, Value::Undefined | Value::Boolean(_)) {
        return Err(pipeline_error(
            "The \"cleanup\" option must be of type boolean",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    Ok(())
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
    if let Some(context) = args.first().filter(|value| is_pipeline_callback_context(value)) {
        settle_pipeline_callback(context, None)?;
        return Ok(Value::Undefined);
    }
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    if quench_runtime::is_callable(&callback) {
        let arguments = if matches!(args.get(1), Some(Value::Boolean(true))) {
            vec![
                Value::Undefined,
                args.get(2).cloned().unwrap_or(Value::Undefined),
            ]
        } else {
            Vec::new()
        };
        execute::call(&callback, &Value::Undefined, &arguments)?;
    }
    Ok(Value::Undefined)
}

pub fn web_pipeline_error(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(context) = args.first().filter(|value| is_pipeline_callback_context(value)) {
        settle_pipeline_callback(context, args.get(1).cloned())?;
        return Ok(Value::Undefined);
    }
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
        if quench_runtime::is_callable(&first) {
            let result = execute::call(&first, &Value::Undefined, &[])?;
            if matches!(result, Value::Undefined) {
                return Err(pipeline_error(
                    "The pipeline function must return an AsyncIterable",
                    "ERR_INVALID_RETURN_VALUE",
                ));
            }
            stages[0] = readable_from(state, result)?;
        } else if let Value::Array(ref array) = first {
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
    pipeline_function_kind(stage) == Some("GeneratorFunction")
}

fn is_terminal_pipeline_function(stage: &Value) -> bool {
    quench_runtime::is_callable(stage)
        && !matches!(
            pipeline_function_kind(stage),
            Some("GeneratorFunction" | "AsyncGeneratorFunction")
        )
}

fn pipeline_function_kind(stage: &Value) -> Option<&str> {
    let constructor = execute::get_property(stage, "constructor");
    match execute::get_property(&constructor, "name") {
        Value::String(name) if name == "GeneratorFunction" => Some("GeneratorFunction"),
        Value::String(name) if name == "AsyncGeneratorFunction" => {
            Some("AsyncGeneratorFunction")
        }
        _ => None,
    }
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
        let context = host_api::object(vec![
            ("\0pipelineCallbackContext".into(), Value::Boolean(true)),
            ("callback".into(), callback.clone()),
            ("stream".into(), last.clone()),
            ("settled".into(), Value::Boolean(false)),
            ("cleanupError".into(), Value::Boolean(has_callable(last, "read"))),
        ]);
        let complete = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_WEB_PIPELINE_COMPLETE),
            vec![context.clone()],
        );
        let failed = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_WEB_PIPELINE_ERROR),
            vec![context.clone()],
        );
        execute::set_property_in_place(&context, "errorHandler", failed.clone());
        execute::call(
            &once,
            last,
            &[Value::String("finish".into()), complete],
        )?;
        execute::call(&once, last, &[Value::String("error".into()), failed])?;
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

fn is_pipeline_callback_context(value: &Value) -> bool {
    matches!(
        execute::get_property(value, "\0pipelineCallbackContext"),
        Value::Boolean(true)
    )
}

fn settle_pipeline_callback(context: &Value, error: Option<Value>) -> Result<(), VmError> {
    if execute::is_truthy(&execute::get_property(context, "settled")) {
        return Ok(());
    }
    execute::set_property_in_place(context, "settled", Value::Boolean(true));
    if error.is_none() && execute::is_truthy(&execute::get_property(context, "cleanupError")) {
        let stream = execute::get_property(context, "stream");
        let remove = execute::get_property(&stream, "removeListener");
        let handler = execute::get_property(context, "errorHandler");
        if quench_runtime::is_callable(&remove) {
            execute::call(&remove, &stream, &[Value::String("error".into()), handler])?;
        }
    }
    let callback = execute::get_property(context, "callback");
    let arguments = error.into_iter().collect::<Vec<_>>();
    execute::call(&callback, &Value::Undefined, &arguments).map(|_| ())
}

fn pipeline_error(message: &str, code: &str) -> VmError {
    let error = execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::TypeError),
        &Value::Undefined,
        &[Value::String(message.into())],
    )
    .unwrap_or_else(|_| host_api::object(Vec::new()));
    execute::set_property_in_place(&error, "code", Value::String(code.into()));
    execute::set_property_in_place(&error, "\0node_error_to_string_code", Value::Boolean(true));
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

const READABLE_WRAP_SOURCE: &str = "\0quench:stream:wrap-source";

/// Forward one event from a legacy stream into the native Readable state
/// machine.  `wrap()` is deliberately implemented as a host capability: the
/// event subscriptions and their retained target/source identities are
/// observable lifecycle state, not a second JavaScript stream implementation.
pub fn readable_wrap(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let target = receiver.cloned().ok_or(VmError::NotCallable)?;
    let source = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(
        source,
        Value::Object(_)
            | Value::ObjectAlias(_)
            | Value::Array(_)
            | Value::Function(_)
            | Value::BoundFunction(_)
    ) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"stream\" argument must be an object".into(),
        ));
    }
    execute::set_property_in_place(&target, READABLE_WRAP_SOURCE, source.clone());

    // Node's wrapper copies callable own properties without replacing the
    // Readable API itself.  Bind each forwarding method to the old stream so
    // `this` remains the legacy source rather than the new Readable.
    for key in execute::own_enumerable_keys(&source) {
        let method = execute::get_property(&source, &key);
        if !quench_runtime::is_callable(&method)
            || !matches!(execute::get_property(&target, &key), Value::Undefined)
        {
            continue;
        }
        let proxy = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_READABLE_WRAP_PROXY),
            vec![source.clone(), Value::String(key.clone())],
        );
        execute::set_property_in_place(&target, &key, proxy);
    }

    let on = execute::get_property(&source, "on");
    if !quench_runtime::is_callable(&on) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"stream\" argument must provide an on() method".into(),
        ));
    }
    for event in ["data", "end", "error", "close", "destroy"] {
        let listener = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_READABLE_WRAP_EVENT),
            vec![target.clone(), Value::String(event.into())],
        );
        execute::call(
            &on,
            &source,
            &[Value::String(event.into()), listener],
        )?;
    }
    Ok(target)
}

pub fn readable_wrap_event(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let target = args.first().cloned().unwrap_or(Value::Undefined);
    let event = match args.get(1) {
        Some(Value::String(event)) => event.as_str(),
        _ => return Ok(Value::Undefined),
    };
    match event {
        "data" => {
            let push = execute::get_property(&target, "push");
            if quench_runtime::is_callable(&push) {
                let chunk = args.get(2).cloned().unwrap_or(Value::Undefined);
                let accepted = execute::call(&push, &target, &[chunk])?;
                if matches!(accepted, Value::Boolean(false)) {
                    let source = execute::get_property(&target, READABLE_WRAP_SOURCE);
                    let pause = execute::get_property(&source, "pause");
                    if quench_runtime::is_callable(&pause) {
                        execute::call(&pause, &source, &[])?;
                    }
                }
            }
        }
        "end" => {
            let push = execute::get_property(&target, "push");
            if quench_runtime::is_callable(&push) {
                execute::call(&push, &target, &[Value::Null])?;
            }
        }
        "error" => {
            let error = args.get(2).cloned().unwrap_or(Value::Undefined);
            let state = execute::get_property(&target, "_readableState");
            execute::set_property_in_place(&state, "errored", error.clone());
            let auto_destroy = !matches!(execute::get_property(&state, "autoDestroy"), Value::Boolean(false));
            if auto_destroy {
                let destroy = execute::get_property(&target, "destroy");
                if quench_runtime::is_callable(&destroy) {
                    execute::call(&destroy, &target, &[error])?;
                }
            } else {
                execute::set_property_in_place(&state, "errorEmitted", Value::Boolean(true));
                emit_wrapped_event(&target, "error", &[error])?;
            }
        }
        "close" | "destroy" => {
            let destroyed = execute::get_property(&target, "destroyed");
            if !matches!(destroyed, Value::Boolean(true)) {
                let destroy = execute::get_property(&target, "destroy");
                if quench_runtime::is_callable(&destroy) {
                    execute::call(&destroy, &target, &[])?;
                }
            }
        }
        _ => {}
    }
    Ok(Value::Undefined)
}

fn emit_wrapped_event(target: &Value, event: &str, args: &[Value]) -> Result<(), VmError> {
    let emitter = execute::get_property(target, "_emitter");
    let emit = execute::get_property(&emitter, "emit");
    if quench_runtime::is_callable(&emit) {
        let mut call_args = Vec::with_capacity(args.len() + 1);
        call_args.push(Value::String(event.into()));
        call_args.extend_from_slice(args);
        execute::call(&emit, &emitter, &call_args)?;
    }
    Ok(())
}

pub fn readable_wrap_proxy(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = args.first().cloned().unwrap_or(Value::Undefined);
    let key = match args.get(1) {
        Some(Value::String(key)) => key,
        _ => return Err(VmError::NotCallable),
    };
    let method = execute::get_property(&source, key);
    if !quench_runtime::is_callable(&method) {
        return Err(VmError::NotCallable);
    }
    execute::call(&method, &source, args.get(2..).unwrap_or_default())
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

/// Implement Writable's cross-family `instanceof` contract without making
/// subclasses inherit a structural brand. Duplex and Transform copy the
/// writable methods/state because JavaScript has no multiple inheritance, so
/// the canonical Writable constructor accepts those instances as well. A
/// subclass that inherits Writable's `@@hasInstance`, however, must still be
/// checked against its own prototype rather than every object carrying a
/// `_writableState` slot.
pub fn writable_has_instance(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Boolean(false));
    };
    let Some(value) = args.first() else {
        return Ok(Value::Boolean(false));
    };
    if matches!(
        value,
        Value::Null
            | Value::Undefined
            | Value::Boolean(_)
            | Value::Number(_)
            | Value::String(_)
            | Value::StringUnits(_)
            | Value::BigInt(_)
    ) {
        return Ok(Value::Boolean(false));
    }
    let prototype = execute::get_property(receiver, "prototype");
    let mut current = execute::get_prototype_of(value).ok();
    for _ in 0..1_024 {
        let Some(current_value) = current else {
            break;
        };
        if execute::same_value(&current_value, &prototype) {
            return Ok(Value::Boolean(true));
        }
        current = match current_value {
            Value::Null | Value::Undefined => None,
            value => execute::get_prototype_of(&value).ok(),
        };
        if matches!(current, Some(Value::Null | Value::Undefined)) {
            current = None;
        }
    }

    let canonical_writable = state
        .borrow()
        .stream_module
        .as_ref()
        .map(|module| execute::get_property(module, "Writable"));
    if canonical_writable
        .as_ref()
        .is_some_and(|writable| execute::same_value(receiver, writable))
        && !matches!(
            execute::get_property(value, "_writableState"),
            Value::Null | Value::Undefined
        )
    {
        return Ok(Value::Boolean(true));
    }
    Ok(Value::Boolean(false))
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

fn premature_close_error() -> Value {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("Premature close".into())],
    );
    execute::set_property(
        error,
        "code",
        Value::String("ERR_STREAM_PREMATURE_CLOSE".into()),
    )
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
    // `finished()` stores its completion callback on a host-owned lifecycle
    // record, so the eventual EventEmitter edge does not pass through the
    // ordinary JS callback-registration wrapper. Capture the registration
    // resource once and invoke the same callback under that context later.
    let callback = capture_async_callback(state, callback)?;
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
        let cleanup = execute::get_property(options, "cleanup");
        if !matches!(cleanup, Value::Undefined | Value::Boolean(_)) {
            return Err(pipeline_error(
                "The \"cleanup\" option must be of type boolean",
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
        execute::get_property(&stream, crate::modules::http::INCOMING_CLOSE_PENDING_PROP,),
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
            execute::get_property(
                &execute::get_property(&stream, "_writableState"),
                "writable"
            ),
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
    let auto_destroy = matches!(
        execute::get_property(
            &execute::get_property(&stream, "_writableState"),
            "autoDestroy"
        ),
        Value::Boolean(true)
    ) || matches!(
        execute::get_property(
            &execute::get_property(&stream, "_readableState"),
            "autoDestroy"
        ),
        Value::Boolean(true)
    );
    let state_object = host_api::object(vec![
        ("done".into(), Value::Boolean(false)),
        ("destroyedPending".into(), Value::Boolean(false)),
        ("closeWanted".into(), Value::Boolean(auto_destroy)),
        ("closeSeen".into(), Value::Boolean(false)),
        ("pendingScheduled".into(), Value::Boolean(false)),
        (
            "cleanup".into(),
            Value::Boolean(matches!(
                options.map(|value| execute::get_property(value, "cleanup")),
                Some(Value::Boolean(true))
            )),
        ),
        ("abortedPending".into(), Value::Boolean(false)),
        ("readableWanted".into(), Value::Boolean(want_readable)),
        ("writableWanted".into(), Value::Boolean(want_writable)),
        ("readableDone".into(), Value::Boolean(!want_readable)),
        ("writableDone".into(), Value::Boolean(!want_writable)),
    ]);
    execute::set_property_in_place(&state_object, "stream", stream.clone());
    if matches!(
        execute::get_property(&stream, "destroyed"),
        Value::Boolean(true)
    ) && !matches!(
        execute::get_property(
            &execute::get_property(&stream, "_writableState"),
            "finished"
        ),
        Value::Boolean(true)
    ) {
        let error = execute::set_property(
            quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String("Premature close".into())],
            ),
            "code",
            Value::String("ERR_STREAM_PREMATURE_CLOSE".into()),
        );
        // Destruction is observed on the next tick.  This gives callers the
        // same-tick disposer window as Node (`finished(s, cb)();`) while
        // still reporting the premature-close error when the disposer is not
        // used.
        execute::set_property_in_place(&state_object, "destroyedPending", Value::Boolean(true));
        execute::set_property_in_place(&state_object, "pendingError", error);
    }
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
    // Node's finished() keeps an `end` observer even for writable-only
    // streams; callers can observe that listener when cleanup is disabled.
    if !web_writable {
        execute::call(&once, &stream, &[Value::String("end".into()), on_end])?;
    }
    if want_writable && !web_writable {
        execute::call(&once, &stream, &[Value::String("finish".into()), on_finish])?;
    }
    if !web_writable {
        execute::call(
            &once,
            &stream,
            &[Value::String("error".into()), on_error.clone()],
        )?;
        execute::call(&once, &stream, &[Value::String("close".into()), on_close])?;
    } else {
        let closed = execute::get_property(&stream, "_closedPromise");
        let then = execute::get_property(&closed, "then");
        let fulfilled = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_FINISHED_EVENT),
            vec![
                state_object.clone(),
                callback.clone(),
                Value::String("writable".into()),
            ],
        );
        let rejected = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_FINISHED_EVENT),
            vec![
                state_object.clone(),
                callback.clone(),
                Value::String("error".into()),
            ],
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
    // `finished()` may be installed after `end()` synchronously completed.
    // Project already-terminal sides into the same record used by event
    // callbacks so the promise observes the canonical state machine.
    if !matches!(
        execute::get_property(&state_object, "done"),
        Value::Boolean(true)
    ) && !matches!(
        execute::get_property(&state_object, "destroyedPending"),
        Value::Boolean(true)
    ) {
        let writable_state = execute::get_property(&stream, "_writableState");
        if want_writable
            && !matches!(
                execute::get_property(&writable_state, "writable"),
                Value::Undefined | Value::Null
            )
            && matches!(
                execute::get_property(&writable_state, "finished"),
                Value::Boolean(true)
            )
        {
            execute::set_property_in_place(&state_object, "writableDone", Value::Boolean(true));
        }
        if want_readable
            && matches!(
                execute::get_property(
                    &execute::get_property(&stream, "_readableState"),
                    "endEmitted"
                ),
                Value::Boolean(true)
            )
        {
            execute::set_property_in_place(&state_object, "readableDone", Value::Boolean(true));
        }
        if matches!(
            execute::get_property(&state_object, "readableDone"),
            Value::Boolean(true)
        ) && matches!(
            execute::get_property(&state_object, "writableDone"),
            Value::Boolean(true)
        ) && (!matches!(
            execute::get_property(&state_object, "closeWanted"),
            Value::Boolean(true)
        ) || matches!(
            execute::get_property(&stream, "closed"),
            Value::Boolean(true)
        )) {
            execute::set_property_in_place(&state_object, "done", Value::Boolean(true));
            finished_cleanup(state, None, &[state_object.clone(), stream.clone()])?;
            execute::call(&callback, &Value::Undefined, &[])?;
        }
    }
    Ok(host_api::bound_capability_with_arguments(
        crate::host::capability_ref(SPEC_STREAM_FINISHED_CLEANUP),
        // An explicit invocation of the disposer must remove listeners even
        // when the `cleanup` option was not requested.  Internal completion
        // paths call the same capability with only the lifecycle record and
        // stream, so keep that distinction in the argument vector rather
        // than giving the state machine a second cleanup implementation.
        vec![state_object, stream, Value::Boolean(true)],
    ))
}

fn capture_async_callback(
    state: &Rc<RefCell<HostState>>,
    callback: Value,
) -> Result<Value, VmError> {
    // Node's end-of-stream observer is bound to a dedicated
    // `STREAM_END_OF_STREAM` AsyncResource.  Besides preserving
    // AsyncLocalStorage state, creating the resource is observable through
    // async_hooks' `init`/`before`/`after` edges (and is required by callers
    // that use the bindAsyncResource path).
    let resource = crate::modules::async_hooks::new_resource(
        state,
        &[Value::String("STREAM_END_OF_STREAM".into())],
    )?;
    crate::modules::async_hooks::resource_bind(state, Some(&resource), &[callback])
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
    if side != Some("error")
        && side != Some("close")
        && (matches!(
            execute::get_property(&state, "destroyedPending"),
            Value::Boolean(true)
        ) || matches!(
            execute::get_property(&execute::get_property(&state, "stream"), "destroyed"),
            Value::Boolean(true)
        ) && !matches!(
            execute::get_property(&execute::get_property(&state, "stream"), "closed"),
            Value::Boolean(true)
        ))
    {
        return Ok(Value::Undefined);
    }
    if side == Some("error") {
        let stream = execute::get_property(&state, "stream");
        if matches!(execute::get_property(&stream, "destroyed"), Value::Boolean(true))
            && !matches!(execute::get_property(&stream, "closed"), Value::Boolean(true))
        {
            if let Some(error) = args.get(3).cloned() {
                execute::set_property_in_place(&state, "pendingError", error);
            }
            execute::set_property_in_place(&state, "destroyedPending", Value::Boolean(true));
            return Ok(Value::Undefined);
        }
        let error = match args.get(3).cloned().unwrap_or(Value::Undefined) {
            Value::Undefined | Value::Null => {
                let pending = execute::get_property(&state, "pendingError");
                pending
            }
            value => value,
        };
        execute::set_property_in_place(&state, "done", Value::Boolean(true));
        finished_cleanup(
            _state,
            None,
            &[state.clone(), execute::get_property(&state, "stream")],
        )?;
        execute::call(
            &callback,
            &Value::Undefined,
            &[error],
        )?;
        return Ok(Value::Undefined);
    }
    if side == Some("close") {
        // A stream destroyed in the current turn has a close edge queued by
        // the stream implementation.  Its completion is represented by the
        // deferred pending error below so that an immediately-called
        // disposer can cancel the callback before that edge runs.
        if matches!(
            execute::get_property(&state, "destroyedPending"),
            Value::Boolean(true)
        ) {
            let stream = execute::get_property(&state, "stream");
            if matches!(execute::get_property(&stream, "destroyed"), Value::Boolean(true)) {
                execute::set_property_in_place(&stream, "closed", Value::Boolean(true));
            }
            let pending_error = execute::get_property(&state, "pendingError");
            if matches!(
                execute::get_property(&state, "pendingScheduled"),
                Value::Boolean(true)
            ) {
                return Ok(Value::Undefined);
            }
            execute::set_property_in_place(&state, "pendingScheduled", Value::Boolean(true));
            let error = host_api::bound_capability_with_arguments(
                crate::host::capability_ref(SPEC_STREAM_FINISHED_EVENT),
                vec![state.clone(), callback.clone(), Value::String("error".into())],
            );
            crate::modules::timers::set_immediate(_state, &[error, pending_error])?;
            return Ok(Value::Undefined);
        }
        let stream = execute::get_property(&state, "stream");
        execute::set_property_in_place(&state, "closeSeen", Value::Boolean(true));
        if matches!(execute::get_property(&stream, "destroyed"), Value::Boolean(true)) {
            execute::set_property_in_place(&stream, "closed", Value::Boolean(true));
        }
        // A close after the stream recorded an error must not be treated as a
        // clean terminal edge merely because the writable side also reports
        // `finished`.  Node's end-of-stream logic gives the stored error
        // precedence; boolean error markers used by legacy stream shims are
        // normalized to the standard premature-close error.
        let stream_error = [
            execute::get_property(
                &execute::get_property(&stream, "_readableState"),
                "errored",
            ),
            execute::get_property(
                &execute::get_property(&stream, "_writableState"),
                "errored",
            ),
        ]
        .into_iter()
        .find(|value| {
            !matches!(value, Value::Undefined | Value::Null | Value::Boolean(false))
        });
        if let Some(stream_error) = stream_error {
            let error = match stream_error {
                Value::Object(_) | Value::ObjectAlias(_) => stream_error,
                _ => premature_close_error(),
            };
            execute::set_property_in_place(&state, "done", Value::Boolean(true));
            finished_cleanup(
                _state,
                None,
                &[state.clone(), execute::get_property(&state, "stream")],
            )?;
            execute::call(&callback, &Value::Undefined, &[error])?;
            return Ok(Value::Undefined);
        }
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
        let readable_errored = match execute::get_property(
            &execute::get_property(&stream, "_readableState"),
            "errored",
        ) {
            Value::Undefined | Value::Null | Value::Boolean(false) => false,
            _ => true,
        };
        let writable_errored = match execute::get_property(
            &execute::get_property(&stream, "_writableState"),
            "errored",
        ) {
            Value::Undefined | Value::Null | Value::Boolean(false) => false,
            _ => true,
        };
        if readable_errored || writable_errored {
            // An errored side is not terminal merely because its `finished`
            // bit was set; close must report the premature-close error.
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
        if readable_terminal && writable_terminal {
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
                    &[state.clone(), stream.clone()],
                )?;
                execute::call(&callback, &Value::Undefined, &[])?;
            }
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
    ) && (!matches!(
        execute::get_property(&state, "closeWanted"),
        Value::Boolean(true)
    ) || matches!(
        execute::get_property(&state, "closeSeen"),
        Value::Boolean(true)
    ));
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
    let explicit_dispose = matches!(args.get(2), Some(Value::Boolean(true)));
    if !explicit_dispose
        && !matches!(
            execute::get_property(&record, "cleanup"),
            Value::Boolean(true)
        )
    {
        return Ok(Value::Undefined);
    }
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
    if matches!(
        execute::get_property(first, "writable"),
        Value::Boolean(false)
    ) {
        execute::set_property_in_place(&composed, "writable", Value::Boolean(false));
    }
    if matches!(
        execute::get_property(last, "readable"),
        Value::Boolean(false)
    ) {
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
    let corked = execute::get_property(&execute::get_property(source, "_writableState"), "corked");
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
    let options = args
        .first()
        .cloned()
        .unwrap_or_else(|| host_api::object(Vec::new()));
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

/// Forward Writable.prototype.write through the Rust host boundary so byte
/// views use Node's internal `buffer` encoding marker without sending that
/// marker through the public string-encoding validator. Node ignores the
/// optional encoding argument for Buffer/typed-array chunks; the prelude's
/// state machine can then perform its ordinary normalization and lifecycle
/// handling unchanged.
pub fn writable_write_adapter(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let original = args.first().ok_or(VmError::NotCallable)?;
    if !quench_runtime::is_callable(original) {
        return Err(VmError::NotCallable);
    }
    let mut write_args = args.get(1..).unwrap_or_default().to_vec();
    // A WriteStream is auto-destroyed after its `close` edge, but Node still
    // reports a subsequent callback-bearing write as WRITE_AFTER_END when the
    // writable side had already been ended.  The prelude's generic check sees
    // `destroyed` first and would otherwise report STREAM_DESTROYED.  Preserve
    // the observable precedence at this shared Rust boundary while leaving
    // destroyed-only streams on the ordinary prelude path.
    let ended = receiver.is_some_and(|stream| {
        matches!(
            execute::get_property(&execute::get_property(stream, "_writableState"), "ended"),
            Value::Boolean(true)
        ) || matches!(execute::get_property(stream, "writableEnded"), Value::Boolean(true))
    });
    let destroyed = receiver.is_some_and(|stream| {
        matches!(execute::get_property(stream, "destroyed"), Value::Boolean(true))
            || matches!(
                execute::get_property(&execute::get_property(stream, "_writableState"), "destroyed"),
                Value::Boolean(true)
            )
    });
    let callback_index = write_args
        .iter()
        .enumerate()
        .rev()
        .filter(|(index, _)| *index > 0)
        .find_map(|(index, value)| quench_runtime::is_callable(value).then_some(index));
    if ended && destroyed {
        if let Some(index) = callback_index {
            let callback = write_args.remove(index);
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String("write after end".into())],
            );
            let error = execute::set_property(
                error,
                "code",
                Value::String("ERR_STREAM_WRITE_AFTER_END".into()),
            );
            crate::modules::fs::defer(state, &callback, vec![error]);
            return Ok(Value::Boolean(false));
        }
    }
    let byte_view = write_args.first().is_some_and(|chunk| {
        matches!(
            chunk,
            Value::Uint8Array(_) | Value::DataView(_) | Value::ArrayBuffer(_)
        )
    });
    if byte_view && matches!(write_args.get(1), Some(Value::String(_))) {
        write_args[1] = Value::Undefined;
    }
    let receiver = receiver.unwrap_or(&Value::Undefined);
    execute::call(original, receiver, &write_args)
}

/// Derive Node's non-enumerable `readableBuffer` inspection view from the
/// stream prelude's canonical unread chunk sequence. Returning a snapshot
/// keeps inspection from mutating the stream state or creating a second
/// buffering representation.
pub fn readable_buffer(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let state = execute::get_property(receiver, "_readableState");
    let buffer = execute::get_property(&state, "buffer");
    let Value::Array(ref array) = buffer else {
        return Ok(host_api::array(Vec::new()));
    };
    let values = (0..array.logical_len())
        .map(|index| execute::get_property(&buffer, &index.to_string()))
        .collect();
    Ok(host_api::array(values))
}

/// Forward `Readable.prototype.push` through Rust for the empty-chunk rule.
/// Node treats `push()` and `push(undefined)` as a successful no-op, including
/// when the readable high-water mark is zero; the grandfathered prelude cannot
/// distinguish that case from an actual queued `undefined` value.
pub fn readable_push_adapter(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let original = args.first().ok_or(VmError::NotCallable)?;
    if !quench_runtime::is_callable(original) {
        return Err(VmError::NotCallable);
    }
    let receiver = receiver.unwrap_or(&Value::Undefined);
    let empty = args
        .get(1)
        .is_none_or(|value| matches!(value, Value::Undefined));
    let state = execute::get_property(receiver, "_readableState");
    let active = !matches!(
        execute::get_property(receiver, "destroyed"),
        Value::Boolean(true)
    ) && !matches!(execute::get_property(&state, "ended"), Value::Boolean(true))
        && matches!(execute::get_property(&state, "errored"), Value::Null | Value::Undefined);
    if empty && active {
        return Ok(Value::Boolean(true));
    }
    execute::call(original, receiver, args.get(1..).unwrap_or_default())
}

/// Validate the maximum byte count accepted by `Readable.read(size)` before
/// forwarding to the canonical stream implementation.  Node caps explicit
/// reads at 1 GiB even though high-water marks may exceed that value.
pub fn readable_read_adapter(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let original = args.first().ok_or(VmError::NotCallable)?;
    if !quench_runtime::is_callable(original) {
        return Err(VmError::NotCallable);
    }
    if let Some(Value::Number(size)) = args.get(1) {
        if size.is_finite() && *size > 1_073_741_824.0 {
            return Err(crate::modules::buffer_enc::out_of_range(
                "size",
                "<= 1GiB",
                &crate::modules::buffer_enc::fmt_num(*size),
            ));
        }
    }
    let result = execute::call(
        original,
        receiver.unwrap_or(&Value::Undefined),
        args.get(1..).unwrap_or_default(),
    )?;
    // When a byte-oriented read asks for more data than is buffered, Node
    // keeps the readable side marked as needing another notification.  The
    // grandfathered prelude's wait path returns `null` without making that
    // state transition; derive it here at the canonical Rust adapter boundary
    // so every Readable family observes the same fact.
    if matches!(result, Value::Null) {
        if let (Some(Value::Number(requested)), Value::Number(buffered)) = (
            args.get(1),
            execute::get_property(receiver.unwrap_or(&Value::Undefined), "readableLength"),
        ) {
            let stream = receiver.unwrap_or(&Value::Undefined);
            let state = execute::get_property(stream, "_readableState");
            let ended = matches!(execute::get_property(&state, "ended"), Value::Boolean(true));
            let destroyed = matches!(execute::get_property(stream, "destroyed"), Value::Boolean(true));
            if requested.is_finite() && *requested > 0.0 && *requested > buffered && !ended && !destroyed {
                let _ = execute::set_property_in_place(&state, "needReadable", Value::Boolean(true));
            }
        }
    }
    Ok(result)
}

const DEFAULT_BYTE_HWM: f64 = 65_536.0;
const DEFAULT_OBJECT_HWM: f64 = 16.0;

/// Construct through the original stream family, then apply the process-local
/// default only when the caller did not supply a side-specific or shared HWM.
/// Every public constructor shares the same defaults record.
pub fn constructor_adapter(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let original = args.first().ok_or(VmError::NotCallable)?;
    let defaults = args.get(1).ok_or(VmError::NotCallable)?;
    let readable = matches!(args.get(2), Some(Value::Boolean(true)));
    let writable = matches!(args.get(3), Some(Value::Boolean(true)));
    let constructor_args = args.get(4..).unwrap_or_default();
    if let Some(options) = constructor_args.first() {
        validate_high_water_mark(options, "highWaterMark")?;
        if readable {
            validate_high_water_mark(options, "readableHighWaterMark")?;
        }
        if writable {
            validate_high_water_mark(options, "writableHighWaterMark")?;
        }
    }
    // The prelude's public family functions are deliberately callable
    // factories (including the Reflect.construct-backed Duplex families).
    // Calling them also avoids introducing a second nested `newTarget` edge;
    // the outer host constructor applies the public wrapper prototype.
    let stream = execute::call(original, &Value::Undefined, constructor_args)?;
    let options = constructor_args.first().unwrap_or(&Value::Undefined);
    if readable {
        apply_default_hwm(&stream, options, defaults, "readable")?;
        // The prelude initializes `readingMore` eagerly because it cannot
        // observe whether a caller supplied a producer. Node starts an
        // explicitly constructed producer idle; demand is raised when a
        // listener or an explicit read arrives. Keep that fact at the
        // shared Rust constructor boundary rather than teaching individual
        // stream fixtures about it.
        if matches!(
            execute::get_property(options, "read"),
            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
        ) {
            let readable_state = execute::get_property(&stream, "_readableState");
            if !matches!(readable_state, Value::Undefined | Value::Null) {
                let _ = execute::set_property_in_place(
                    &readable_state,
                    "readingMore",
                    Value::Boolean(false),
                );
            }
        }
    }
    if writable {
        apply_default_hwm(&stream, options, defaults, "writable")?;
    }
    Ok(stream)
}

/// Validate stream high-water-mark options before entering the shared stream
/// constructor.  Keeping this at the Rust adapter boundary makes every stream
/// family agree on Node's integer/range contract, including constructors whose
/// grandfathered prelude otherwise leaves arbitrary values in state.
fn validate_high_water_mark(options: &Value, field: &str) -> Result<(), VmError> {
    let value = execute::get_property(options, field);
    if matches!(value, Value::Undefined | Value::Null) {
        return Ok(());
    }
    let valid = matches!(
        value,
        Value::Number(number)
            if number.is_finite()
                && number.fract() == 0.0
                && (0.0..=9_007_199_254_740_991.0).contains(&number)
    );
    if valid {
        return Ok(());
    }
    Err(crate::modules::buffer_enc::invalid_arg_value(format!(
        "The property 'options.{field}' is invalid. Received {}",
        crate::modules::util::inspect(&value)
    )))
}

pub fn constructor_adapter_construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    constructor_adapter(state, None, args)
}

fn apply_default_hwm(
    stream: &Value,
    options: &Value,
    defaults: &Value,
    side: &str,
) -> Result<(), VmError> {
    let side_option = if side == "readable" {
        "readableHighWaterMark"
    } else {
        "writableHighWaterMark"
    };
    let explicit = execute::get_property(options, side_option);
    let shared = execute::get_property(options, "highWaterMark");
    if !matches!(explicit, Value::Undefined | Value::Null)
        || !matches!(shared, Value::Undefined | Value::Null)
    {
        return Ok(());
    }
    let state = execute::get_property(stream, &format!("_{side}State"));
    if matches!(state, Value::Undefined | Value::Null) {
        return Ok(());
    }
    let object_mode = execute::is_truthy(&execute::get_property(&state, "objectMode"));
    let key = if object_mode { "object" } else { "bytes" };
    let value = execute::get_property(defaults, key);
    execute::set_property_in_place(&state, "highWaterMark", value);
    Ok(())
}

pub fn get_default_high_water_mark(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let defaults = args.first().ok_or(VmError::NotCallable)?;
    let object_mode = args.get(1).is_some_and(execute::is_truthy);
    Ok(execute::get_property(
        defaults,
        if object_mode { "object" } else { "bytes" },
    ))
}

pub fn set_default_high_water_mark(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let defaults = args.first().ok_or(VmError::NotCallable)?;
    let object_mode = args.get(1).is_some_and(execute::is_truthy);
    let value = match args.get(2) {
        Some(Value::Number(value)) if value.is_finite() && value.fract() == 0.0
            && (0.0..=9_007_199_254_740_991.0).contains(value) => *value,
        Some(Value::Number(value)) => {
            return Err(crate::modules::buffer_enc::out_of_range(
                "value",
                "an integer >= 0 && <= 9007199254740991",
                &value.to_string(),
            ));
        }
        Some(value) => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"value\" argument must be of type number.{}",
                crate::modules::util::invalid_arg_received(value)
            )));
        }
        None => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"value\" argument must be of type number. Received undefined".into(),
            ));
        }
    };
    execute::set_property_in_place(
        defaults,
        if object_mode { "object" } else { "bytes" },
        Value::Number(value),
    );
    Ok(Value::Undefined)
}

fn wrap_stream_constructor(
    original: &Value,
    defaults: &Value,
    readable: bool,
    writable: bool,
) -> Value {
    let wrapper = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(SPEC_STREAM_CONSTRUCTOR_ADAPTER),
        vec![
            original.clone(),
            defaults.clone(),
            Value::Boolean(readable),
            Value::Boolean(writable),
        ],
    );
    for name in [
        "prototype",
        "from",
        "fromWeb",
        "toWeb",
        "isDisturbed",
        "destroy",
    ] {
        if let Ok(value) = execute::get_property_result(original, name) {
            if !matches!(value, Value::Undefined) {
                execute::set_property_in_place(&wrapper, name, value);
            }
        }
    }
    wrapper
}

fn install_hwm_constructor(
    module: &Value,
    defaults: &Value,
    name: &str,
    readable: bool,
    writable: bool,
) {
    let original = execute::get_property(module, name);
    if !quench_runtime::is_callable(&original) {
        return;
    }
    let wrapper = wrap_stream_constructor(&original, defaults, readable, writable);
    let prototype = execute::get_property(&original, "prototype");
    execute::set_property_in_place(&wrapper, "prototype", prototype.clone());
    execute::set_property_in_place(&prototype, "constructor", wrapper.clone());
    if name == "Writable" {
        let _ = execute::set_callable_property(
            &wrapper,
            "Symbol.hasInstance",
            crate::host::capability(SPEC_STREAM_WRITABLE_HAS_INSTANCE),
        );
    }
    execute::set_property_in_place(module, name, wrapper);
}

fn install_default_high_water_marks(module: &Value) {
    let defaults = host_api::object(vec![
        ("bytes".into(), Value::Number(DEFAULT_BYTE_HWM)),
        ("object".into(), Value::Number(DEFAULT_OBJECT_HWM)),
    ]);
    for (name, readable, writable) in [
        ("Readable", true, false),
        ("Writable", false, true),
        ("Duplex", true, true),
        ("Transform", true, true),
        ("PassThrough", true, true),
    ] {
        install_hwm_constructor(module, &defaults, name, readable, writable);
    }
    for (name, spec) in [
        ("getDefaultHighWaterMark", SPEC_STREAM_GET_DEFAULT_HWM),
        ("setDefaultHighWaterMark", SPEC_STREAM_SET_DEFAULT_HWM),
    ] {
        let function = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(spec),
            vec![defaults.clone()],
        );
        execute::set_property_in_place(module, name, function);
    }
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
    // The JS prelude's Writable state machine is retained for its lifecycle
    // semantics, but public byte-view writes must first discard the optional
    // encoding label.  Install one Rust-owned forwarding capability on each
    // writable family prototype so the rule applies uniformly to Writable,
    // Duplex, Transform, and subclasses without changing fixture code or the
    // prelude's string codec validator.
    for constructor_name in ["Writable", "Duplex", "Transform", "PassThrough"] {
        let constructor = execute::get_property(&module, constructor_name);
        let prototype = execute::get_property(&constructor, "prototype");
        let original = execute::get_property(&prototype, "write");
        if !quench_runtime::is_callable(&original) {
            continue;
        }
        let adapter = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_WRITABLE_WRITE_ADAPTER),
            vec![original],
        );
        let adapter = execute::define_property(
            adapter,
            "name",
            host_api::object(vec![
                ("value".into(), Value::String("write".into())),
                ("configurable".into(), Value::Boolean(true)),
            ]),
        )
        .unwrap_or_else(|_| {
            host_api::bound_capability_with_arguments(
                crate::host::capability_ref(SPEC_STREAM_WRITABLE_WRITE_ADAPTER),
                vec![execute::get_property(&prototype, "write")],
            )
        });
        let _ = execute::set_property_in_place(&prototype, "write", adapter);
    }
    // fs.WriteStream's bootstrap constructor derives from Writable.prototype.
    // Install the shared autoClose accessor at that semantic boundary so the
    // property survives the constructor's prototype replacement.
    let writable = execute::get_property(&module, "Writable");
    let _ = execute::set_callable_property(
        &writable,
        "Symbol.hasInstance",
        crate::host::capability(SPEC_STREAM_WRITABLE_HAS_INSTANCE),
    );
    let writable_prototype = execute::get_property(&writable, "prototype");
    let auto_close = host_api::object(vec![
        (
            "get".into(),
            crate::host::capability(SPEC_FS_WRITE_STREAM_AUTO_CLOSE_GET),
        ),
        (
            "set".into(),
            crate::host::capability(SPEC_FS_WRITE_STREAM_AUTO_CLOSE_SET),
        ),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(false)),
    ]);
    if let Ok(updated) = execute::define_property(writable_prototype, "autoClose", auto_close) {
        let _ = execute::set_property_in_place(&writable, "prototype", updated);
    }
    let readable = execute::get_property(&module, "Readable");
    let readable_prototype = execute::get_property(&readable, "prototype");
    let original_push = execute::get_property(&readable_prototype, "push");
    if quench_runtime::is_callable(&original_push) {
        let push_adapter = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_READABLE_PUSH_ADAPTER),
            vec![original_push],
        );
        let _ = execute::set_property_in_place(&readable_prototype, "push", push_adapter);
    }
    let original_read = execute::get_property(&readable_prototype, "read");
    if quench_runtime::is_callable(&original_read) {
        let read_adapter = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_STREAM_READABLE_READ_ADAPTER),
            vec![original_read],
        );
        let _ = execute::set_property_in_place(&readable_prototype, "read", read_adapter);
    }
    let readable_buffer = host_api::object(vec![
        (
            "get".into(),
            crate::host::capability(SPEC_STREAM_READABLE_BUFFER),
        ),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(false)),
    ]);
    if let Ok(updated_prototype) =
        execute::define_property(readable_prototype.clone(), "readableBuffer", readable_buffer)
    {
        let _ = execute::set_property_in_place(&readable, "prototype", updated_prototype);
    }
    let readable_prototype = execute::get_property(&readable, "prototype");
    execute::set_property_in_place(
        &readable_prototype,
        "wrap",
        crate::host::capability(SPEC_STREAM_READABLE_WRAP),
    );
    if let Ok(compose) = quench_runtime::execute::get_property_result(&module, "compose") {
        state.borrow_mut().stream_compose_impl = Some(compose);
    }
    if let Ok(pipeline) = quench_runtime::execute::get_property_result(&module, "pipeline") {
        state.borrow_mut().stream_pipeline_impl = Some(pipeline);
    }
    install_default_high_water_marks(&module);
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
                "getDefaultHighWaterMark",
                "setDefaultHighWaterMark",
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
                ("pipeline", SPEC_STREAM_PIPELINE),
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
