//! The coded errors the stream family raises, with Node's codes, classes and messages.

use super::{call_method, get, is_nullish, put, Context, Outcome};
use quench_runtime::{RootId, RootedError};

/// A stream error, by its Node code.
pub(crate) enum StreamError<'a> {
    /// `ERR_METHOD_NOT_IMPLEMENTED`, naming the method.
    MethodNotImplemented(&'a str),
    /// `ERR_STREAM_DESTROYED`, naming the operation.
    Destroyed(&'a str),
    /// `ERR_STREAM_ALREADY_FINISHED`, naming the operation.
    AlreadyFinished(&'a str),
    PushAfterEof,
    UnshiftAfterEndEvent,
    MultipleCallback,
    CannotPipe,
    NullValues,
    WriteAfterEnd,
    PrematureClose,
    /// `ERR_MISSING_ARGS`, naming the argument.
    MissingArgs(&'a str),
    /// `ERR_UNKNOWN_ENCODING`, with the encoding as given.
    UnknownEncoding(String),
    /// `ERR_INVALID_RETURN_VALUE`: what was expected, the function's name, what it returned.
    InvalidReturnValue(&'a str, String, RootId),
}

#[derive(Clone, Copy)]
enum ErrorClass {
    Error,
    TypeError,
}

impl StreamError<'_> {
    fn code(&self) -> &'static str {
        match self {
            Self::MethodNotImplemented(_) => "ERR_METHOD_NOT_IMPLEMENTED",
            Self::Destroyed(_) => "ERR_STREAM_DESTROYED",
            Self::AlreadyFinished(_) => "ERR_STREAM_ALREADY_FINISHED",
            Self::PushAfterEof => "ERR_STREAM_PUSH_AFTER_EOF",
            Self::UnshiftAfterEndEvent => "ERR_STREAM_UNSHIFT_AFTER_END_EVENT",
            Self::MultipleCallback => "ERR_MULTIPLE_CALLBACK",
            Self::CannotPipe => "ERR_STREAM_CANNOT_PIPE",
            Self::NullValues => "ERR_STREAM_NULL_VALUES",
            Self::WriteAfterEnd => "ERR_STREAM_WRITE_AFTER_END",
            Self::PrematureClose => "ERR_STREAM_PREMATURE_CLOSE",
            Self::MissingArgs(_) => "ERR_MISSING_ARGS",
            Self::UnknownEncoding(_) => "ERR_UNKNOWN_ENCODING",
            Self::InvalidReturnValue(..) => "ERR_INVALID_RETURN_VALUE",
        }
    }

    fn class(&self) -> ErrorClass {
        match self {
            Self::NullValues
            | Self::MissingArgs(_)
            | Self::UnknownEncoding(_)
            | Self::InvalidReturnValue(..) => ErrorClass::TypeError,
            _ => ErrorClass::Error,
        }
    }

    fn message(&self, c: &mut Context<'_>) -> Result<String, RootedError> {
        Ok(match self {
            Self::MethodNotImplemented(method) => format!("The {method} method is not implemented"),
            Self::Destroyed(operation) => format!("Cannot call {operation} after a stream was destroyed"),
            Self::AlreadyFinished(operation) => format!("Cannot call {operation} after a stream was finished"),
            Self::PushAfterEof => "stream.push() after EOF".to_owned(),
            Self::UnshiftAfterEndEvent => "stream.unshift() after end event".to_owned(),
            Self::MultipleCallback => "Callback called multiple times".to_owned(),
            Self::CannotPipe => "Cannot pipe, not readable".to_owned(),
            Self::NullValues => "May not write null values to stream".to_owned(),
            Self::WriteAfterEnd => "write after end".to_owned(),
            Self::PrematureClose => "Premature close".to_owned(),
            Self::MissingArgs(name) => format!("The \"{name}\" argument must be specified"),
            Self::UnknownEncoding(encoding) => format!("Unknown encoding: {encoding}"),
            Self::InvalidReturnValue(expected, name, value) => {
                format!("Expected {expected} to be returned from the \"{name}\" function but got {}.", returned_type(c, *value)?)
            }
        })
    }
}

/// `ERR_INVALID_RETURN_VALUE`'s description of what a function returned.
fn returned_type(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    if !is_nullish(c, value) {
        let constructor = get(c, value, "constructor")?;
        if !is_nullish(c, constructor) {
            let name = get(c, constructor, "name")?;
            if c.truthy_rooted(name)? {
                return Ok(format!("instance of {}", c.to_string(name)?));
            }
        }
    }
    let kind = match c.value_type_rooted(value) {
        Some(quench_runtime::ValueType::Undefined) | None => "undefined",
        Some(quench_runtime::ValueType::Null | quench_runtime::ValueType::Object) => "object",
        Some(quench_runtime::ValueType::Boolean) => "boolean",
        Some(quench_runtime::ValueType::Number) => "number",
        Some(quench_runtime::ValueType::String) => "string",
        Some(quench_runtime::ValueType::Symbol) => "symbol",
        Some(quench_runtime::ValueType::BigInt) => "bigint",
        Some(quench_runtime::ValueType::Function) => "function",
    };
    Ok(format!("type {kind}"))
}

/// The error, as Node's `new ERR_*()` makes it (not thrown).
pub(crate) fn create(c: &mut Context<'_>, error: StreamError<'_>) -> Outcome {
    let message = error.message(c)?;
    let code = error.code();
    match error.class() {
        ErrorClass::Error => {
            let value = c.error_rooted(&message)?;
            super::super::node_errors_shared_vm::decorate_error(c, value, code)?;
            Ok(value)
        }
        ErrorClass::TypeError => {
            let value = c.type_error_rooted(&message)?;
            super::super::node_errors_shared_vm::decorate_type_error(c, value, code)?;
            Ok(value)
        }
    }
}

pub(crate) fn throw(c: &mut Context<'_>, error: StreamError<'_>) -> RootedError {
    match create(c, error) {
        Ok(value) => c.throw(value),
        Err(error) => error,
    }
}

/// An `ERR_INVALID_ARG_TYPE` error value: the thrown validation error, caught.
pub(crate) fn invalid_argument_type(c: &mut Context<'_>, value: RootId, name: &str, expected: &str) -> Outcome {
    let thrown = super::super::util_shared_vm::argument_type_error_with_expected(c, value, name, expected)?;
    caught(thrown)
}

/// The value a thrown error carries.
pub(crate) fn caught(error: RootedError) -> Outcome {
    match error.exception {
        Some(value) => Ok(value),
        None => Err(error),
    }
}

/// The expected-type list for chunks Node accepts.
pub(crate) const CHUNK_TYPES: &str = "of type string or an instance of Buffer, TypedArray, or DataView";

/// `new AbortError(undefined, { cause })`: `The operation was aborted`, named `AbortError`,
/// coded `ABORT_ERR`; `cause` only when one is given.
pub(crate) fn abort_error(c: &mut Context<'_>, cause: Option<RootId>) -> Outcome {
    let error = c.error_rooted("The operation was aborted")?;
    if let Some(cause) = cause {
        let key = c.string_rooted("cause");
        c.define_data_property_rooted(error, key, cause, true, false, true)?;
    }
    let code = c.string_rooted("ABORT_ERR");
    put(c, error, "code", code)?;
    let name = c.string_rooted("AbortError");
    put(c, error, "name", name)?;
    Ok(error)
}

/// `new AbortError(undefined, { cause: signal.reason })`.
pub(crate) fn abort_error_for(c: &mut Context<'_>, signal: RootId) -> Outcome {
    let reason = get(c, signal, "reason")?;
    abort_error(c, Some(reason))
}

/// `aggregateTwoErrors(inner, outer)`.
pub(crate) fn aggregate_two(c: &mut Context<'_>, inner: RootId, outer: RootId) -> Outcome {
    if c.truthy_rooted(inner)? && c.truthy_rooted(outer)? && !c.strict_equal_rooted(inner, outer)? {
        let errors = get(c, outer, "errors")?;
        if c.is_array_rooted(errors)? {
            call_method(c, errors, "push", &[inner])?;
            return Ok(outer);
        }
        let global = c.global_root()?;
        let aggregate = get(c, global, "AggregateError")?;
        let list = c.array_rooted(&[outer, inner])?;
        let message = get(c, outer, "message")?;
        let error = c.construct_rooted(aggregate, aggregate, &[list, message])?;
        let code = get(c, outer, "code")?;
        put(c, error, "code", code)?;
        return Ok(error);
    }
    Ok(if c.truthy_rooted(inner)? { inner } else { outer })
}

/// Whether `error` carries `code`.
pub(crate) fn has_code(c: &mut Context<'_>, error: RootId, code: &str) -> Result<bool, RootedError> {
    if is_nullish(c, error) {
        return Ok(false);
    }
    let value = get(c, error, "code")?;
    Ok(c.string_text(value)?.is_some_and(|text| text == code))
}
