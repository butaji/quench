//! `assert.throws` / `doesNotThrow` / `match` / `doesNotMatch` —
//! exception and regular-expression validation for the assert module.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::regexp;
use quench_runtime::value::Value;

use super::assert::{arg, assertion_error, custom_message};
use crate::host::HostState;

pub fn throws(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let function = arg(args, 0);
    let thrown = match invoke(&function)? {
        Ok(()) => {
            let suffix = custom_message(args, 2)
                .map(|message| format!(": {message}"))
                .unwrap_or_else(|| ".".to_string());
            return Err(assertion_error(
                format!("Missing expected exception{suffix}"),
                "throws",
                Value::Undefined,
                arg(args, 1),
                true,
            ));
        }
        Err(thrown) => thrown,
    };
    validate_expected(&thrown, args.get(1), args.get(2))
}

pub fn does_not_throw(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let function = arg(args, 0);
    if let Err(thrown) = invoke(&function)? {
        let custom = custom_message(args, 2);
        let generated = custom.is_none();
        let message =
            custom.unwrap_or_else(|| format!("Got unwanted exception: {}", error_text(&thrown)));
        return Err(assertion_error(
            message,
            "doesNotThrow",
            thrown,
            Value::Undefined,
            generated,
        ));
    }
    Ok(Value::Undefined)
}

pub fn matches(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    match_assert(args, true)
}

pub fn does_not_match(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    match_assert(args, false)
}

/// Invoke a callable, capturing only thrown JavaScript values;
/// internal VM errors propagate.
fn invoke(function: &Value) -> Result<Result<(), Value>, VmError> {
    if !is_callable(function) {
        return Err(execute::type_error(
            "The \"fn\" argument must be of type function",
        ));
    }
    match execute::call(function, &Value::Undefined, &[]) {
        Ok(_) => Ok(Ok(())),
        Err(VmError::Thrown(value)) => Ok(Err(value)),
        Err(error) => Err(error),
    }
}

fn is_callable(value: &Value) -> bool {
    matches!(
        value,
        Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_) | Value::HostCapability(_)
    )
}

/// RegExp detection via the internal slot's observable parts.
fn is_regexp(value: &Value) -> bool {
    quench_runtime::regexp::has_regexp_internal_slot(value)
}

fn error_text(error: &Value) -> String {
    match execute::get_property(error, "message") {
        Value::String(message) => message,
        _ => crate::modules::util::inspect(error),
    }
}

/// Node tests the expected RegExp against the inspected error, which
/// renders coded errors as `Name [CODE]: message`. Match the message
/// first, then that bracketed form.
fn coded_text(error: &Value) -> Option<String> {
    let Value::String(name) = execute::get_property(error, "name") else {
        return None;
    };
    let Value::String(code) = execute::get_property(error, "code") else {
        return None;
    };
    Some(format!("{name} [{code}]: {}", error_text(error)))
}

fn regexp_matches(pattern: &Value, input: &str) -> Result<bool, VmError> {
    let result = regexp::test(Some(pattern), &[Value::String(input.to_string())])?;
    Ok(execute::is_truthy(&result))
}

fn validate_expected(
    error: &Value,
    expected: Option<&Value>,
    message: Option<&Value>,
) -> Result<Value, VmError> {
    let user_message = match message {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Undefined) | None => None,
        Some(value) => Some(crate::modules::util::inspect(value)),
    };
    match expected {
        None | Some(Value::Undefined) => Ok(Value::Undefined),
        Some(pattern) if is_regexp(pattern) => validate_regexp(error, pattern, user_message),
        Some(expected) if is_callable(expected) => validate_callable(error, expected, user_message),
        Some(expected) => validate_object(error, expected, user_message),
    }
}

fn invalid_expected(user_message: Option<String>, detail: String) -> VmError {
    let generated = user_message.is_none();
    let message = user_message.unwrap_or(detail);
    assertion_error(
        message,
        "throws",
        Value::Undefined,
        Value::Undefined,
        generated,
    )
}

fn validate_regexp(
    error: &Value,
    pattern: &Value,
    user_message: Option<String>,
) -> Result<Value, VmError> {
    if regexp_matches(pattern, &error_text(error))? {
        return Ok(Value::Undefined);
    }
    if let Some(coded) = coded_text(error) {
        if regexp_matches(pattern, &coded)? {
            return Ok(Value::Undefined);
        }
    }
    Err(invalid_expected(
        user_message,
        format!(
            "The input did not match the regular expression {}. Input:\n\n'{}'\n",
            crate::modules::util::inspect(pattern),
            error_text(error)
        ),
    ))
}

fn validate_callable(
    error: &Value,
    expected: &Value,
    user_message: Option<String>,
) -> Result<Value, VmError> {
    let expected_name = name_of(expected);
    // Only genuine Error constructors take the instanceof path; other
    // callables (including ones that happen to have a `prototype`
    // property, like common.mustCall wrappers) are validation functions.
    if is_error_constructor(expected) {
        if !expected_name.is_empty() && expected_name == name_of(error) {
            return Ok(Value::Undefined);
        }
        return Err(invalid_expected(
            user_message,
            format!("The error is not an instance of {expected_name}"),
        ));
    }
    match execute::call(expected, &Value::Undefined, std::slice::from_ref(error)) {
        Ok(result) if execute::is_truthy(&result) => Ok(Value::Undefined),
        Ok(_) => Err(invalid_expected(
            user_message,
            "The validation function returned a falsy value".to_string(),
        )),
        Err(VmError::Thrown(_)) => Err(invalid_expected(
            user_message,
            "The error did not pass the expected validation function".to_string(),
        )),
        Err(internal) => Err(internal),
    }
}

fn is_error_constructor(expected: &Value) -> bool {
    use quench_runtime::ops::Builtin;
    let mut proto = execute::get_property(expected, "prototype");
    for _ in 0..8 {
        proto = match &proto {
            Value::Builtin(
                Builtin::ErrorPrototype
                | Builtin::RangeErrorPrototype
                | Builtin::TypeErrorPrototype
                | Builtin::EvalErrorPrototype
                | Builtin::ReferenceErrorPrototype
                | Builtin::SyntaxErrorPrototype
                | Builtin::URIErrorPrototype
                | Builtin::AggregateErrorPrototype
                | Builtin::SuppressedErrorPrototype,
            ) => return true,
            Value::Object(_) => match execute::get_prototype_of(&proto) {
                Ok(next) => next,
                Err(_) => return false,
            },
            _ => return false,
        };
    }
    false
}

fn name_of(value: &Value) -> String {
    match execute::get_property(value, "name") {
        Value::String(name) => name,
        _ => String::new(),
    }
}

fn validate_object(
    error: &Value,
    expected: &Value,
    user_message: Option<String>,
) -> Result<Value, VmError> {
    for key in execute::own_enumerable_keys(expected) {
        let expected_value = execute::get_property_result(expected, &key)?;
        let actual_value = execute::get_property_result(error, &key)?;
        if !expected_property_matches(&expected_value, &actual_value)? {
            return Err(invalid_expected(
                user_message,
                format!("The error did not match the expected object (key \"{key}\")"),
            ));
        }
    }
    Ok(Value::Undefined)
}

fn expected_property_matches(expected: &Value, actual: &Value) -> Result<bool, VmError> {
    if is_regexp(expected) {
        let input = match actual {
            Value::String(text) => text.clone(),
            _ => crate::modules::util::inspect(actual),
        };
        return regexp_matches(expected, &input);
    }
    crate::modules::deep_equal::deep_equal(expected, actual, true)
}

fn match_assert(args: &[Value], should_match: bool) -> Result<Value, VmError> {
    let input = match arg(args, 0) {
        Value::String(text) => text,
        _ => {
            return Err(execute::type_error(
                "The \"string\" argument must be of type string",
            ))
        }
    };
    let pattern = arg(args, 1);
    if !is_regexp(&pattern) {
        return Err(execute::type_error(
            "The \"regexp\" argument must be an instance of RegExp",
        ));
    }
    let operator = if should_match {
        "match"
    } else {
        "doesNotMatch"
    };
    if regexp_matches(&pattern, &input)? == should_match {
        return Ok(Value::Undefined);
    }
    let message = custom_message(args, 2)
        .unwrap_or_else(|| format!("The input did not satisfy {operator}: '{input}'"));
    Err(assertion_error(
        message,
        operator,
        arg(args, 0),
        pattern,
        true,
    ))
}
