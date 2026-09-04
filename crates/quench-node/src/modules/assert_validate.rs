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
    let (expected, user_message) = match (args.get(1), args.get(2)) {
        (Some(Value::String(message)), None) => (None, Some(message.clone())),
        (expected, message) => {
            let message = match message {
                Some(Value::String(value)) => Some(value.clone()),
                Some(Value::Undefined) | None => None,
                Some(value) => Some(crate::modules::util::inspect(value)),
            };
            (expected, message)
        }
    };
    let thrown = match invoke(&function)? {
        Ok(()) => {
            let expected_suffix = expected
                .filter(|value| is_error_constructor(value))
                .map(|value| format!(" ({})", name_of(value)))
                .unwrap_or_default();
            let suffix = user_message
                .as_ref()
                .map(|message| format!(": {message}"))
                .unwrap_or_else(|| ".".to_string());
            let suffix = if expected_suffix.is_empty() {
                suffix
            } else {
                format!("{expected_suffix}{suffix}")
            };
            return Err(assertion_error(
                format!("Missing expected exception{suffix}"),
                "throws",
                Value::Undefined,
                expected.cloned().unwrap_or(Value::Undefined),
                true,
            ));
        }
        Err(thrown) => thrown,
    };
    if expected.is_none() {
        if let Some(message) = user_message.as_ref() {
            let thrown_message = match &thrown {
                Value::String(value) => Some(value.clone()),
                _ => match execute::get_property(&thrown, "message") {
                    Value::String(value) => Some(value),
                    _ => None,
                },
            };
            if thrown_message.as_deref() == Some(message.as_str()) {
                let error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::TypeError,
                    &[Value::String(format!(
                        "The \"error/message\" argument is ambiguous. The error{} \"{}\" is identical to the message.",
                        if matches!(&thrown, Value::String(_)) { "" } else { " message" },
                        message
                    ))],
                );
                return Err(VmError::Thrown(execute::set_property(
                    error,
                    "code",
                    Value::String("ERR_AMBIGUOUS_ARGUMENT".into()),
                )));
            }
        }
    }
    validate_expected(&thrown, expected, user_message)
}

pub fn does_not_throw(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let function = arg(args, 0);
    let (expected, custom) = match (args.get(1), args.get(2)) {
        (Some(Value::String(message)), None) => (None, Some(message.clone())),
        (expected, _) => (expected, custom_message(args, 2)),
    };
    if let Err(thrown) = invoke(&function)? {
        if let Some(expected) = expected {
            if is_callable(expected)
                && is_error_constructor(expected)
                && name_of(expected) != name_of(&thrown)
            {
                return Err(VmError::Thrown(thrown));
            }
        }
        let generated = custom.is_none();
        let message = match custom {
            Some(message) => format!(
                "Got unwanted exception: {message}\nActual message: \"{}\"",
                error_text(&thrown)
            ),
            None => format!(
                "Got unwanted exception.\nActual message: \"{}\"",
                error_text(&thrown)
            ),
        };
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
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"fn\" argument must be of type function.{}",
            crate::modules::util::invalid_arg_received(function)
        )));
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
    user_message: Option<String>,
) -> Result<Value, VmError> {
    match expected {
        None | Some(Value::Undefined) => Ok(Value::Undefined),
        Some(pattern) if is_regexp(pattern) => validate_regexp(error, pattern, user_message),
        Some(expected) if is_callable(expected) => validate_callable(error, expected, user_message),
        Some(expected) => {
            if execute::own_enumerable_keys(expected).is_empty() {
                let error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::TypeError,
                    &[Value::String("The argument 'error' may not be an empty object. Received {}".into())],
                );
                return Err(VmError::Thrown(execute::set_property(
                    error,
                    "code",
                    Value::String("ERR_INVALID_ARG_VALUE".into()),
                )));
            }
            validate_object(error, expected, user_message)
        }
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
    if let Value::String(name) = execute::get_property(error, "name") {
        let full = format!("{name}: {}", error_text(error));
        if regexp_matches(pattern, &full)? {
            return Ok(Value::Undefined);
        }
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
    // Constructors use the same prototype-chain fact as JavaScript's
    // `instanceof`; arbitrary callable values remain validation predicates.
    if is_error_constructor(expected) || is_function_constructor(expected) {
        if is_instance_of(error, expected) {
            return Ok(Value::Undefined);
        }
        if expected_name == "Error" && is_error_instance(error) {
            return Ok(Value::Undefined);
        }
        if expected_name == "DOMException" && is_error_instance(error) {
            return Ok(Value::Undefined);
        }
        if expected_name == "AssertionError"
            && is_error_instance(error)
            && matches!(name_of(error).as_str(), "Error" | "AssertionError")
        {
            return Ok(Value::Undefined);
        }
        if !expected_name.is_empty() && expected_name == name_of(error) {
            return Ok(Value::Undefined);
        }
        let detail = format!(
            "The error is expected to be an instance of \"{expected_name}\". Received \"{}\"\n\nError message:\n\n{}",
            name_of(error),
            error_text(error)
        );
        return Err(match user_message {
            Some(message) => {
                assertion_error(message, "throws", error.clone(), expected.clone(), false)
            }
            None => assertion_error(detail, "throws", error.clone(), expected.clone(), true),
        });
    }
    if matches!(expected, Value::Builtin(_)) {
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

fn is_error_instance(value: &Value) -> bool {
    let mut current = execute::get_prototype_of(value).ok();
    while let Some(prototype) = current {
        if matches!(
            prototype,
            Value::Builtin(
                quench_runtime::ops::Builtin::ErrorPrototype
                    | quench_runtime::ops::Builtin::RangeErrorPrototype
                    | quench_runtime::ops::Builtin::TypeErrorPrototype
                    | quench_runtime::ops::Builtin::EvalErrorPrototype
                    | quench_runtime::ops::Builtin::ReferenceErrorPrototype
                    | quench_runtime::ops::Builtin::SyntaxErrorPrototype
                    | quench_runtime::ops::Builtin::URIErrorPrototype
                    | quench_runtime::ops::Builtin::AggregateErrorPrototype
                    | quench_runtime::ops::Builtin::SuppressedErrorPrototype
                    | quench_runtime::ops::Builtin::DOMExceptionPrototype
            )
        ) {
            return true;
        }
        current = execute::get_prototype_of(&prototype).ok();
    }
    false
}

fn is_error_constructor(expected: &Value) -> bool {
    use quench_runtime::ops::Builtin;
    if name_of(expected) == "DOMException" {
        return true;
    }
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
                | Builtin::SuppressedErrorPrototype
                | Builtin::DOMExceptionPrototype,
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

fn is_function_constructor(value: &Value) -> bool {
    use quench_runtime::ops::FunctionKind;
    match value {
        // Ordinary functions are validation predicates in assert.throws.
        // Error-like ordinary constructors are already recognized by their
        // prototype chain in `is_error_constructor`; only class constructors
        // need this remaining constructor path.
        Value::Function(function) => matches!(function.kind, FunctionKind::ClassConstructor),
        Value::BoundFunction(bound) => is_function_constructor(&bound.target),
        _ => false,
    }
}

fn is_instance_of(value: &Value, constructor: &Value) -> bool {
    let prototype = execute::get_property(constructor, "prototype");
    if !matches!(
        prototype,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Builtin(_)
    ) {
        return false;
    }
    let mut current = execute::get_prototype_of(value).ok();
    while let Some(candidate) = current {
        if execute::same_identity(&candidate, &prototype) {
            return true;
        }
        current = execute::get_prototype_of(&candidate).ok();
    }
    false
}

fn name_of(value: &Value) -> String {
    let name = match execute::get_property(value, "name") {
        Value::String(name) => name,
        _ => String::new(),
    };
    if name != "Error" || !matches!(value, Value::Object(_)) {
        return name;
    }
    match execute::get_property(&execute::get_property(value, "constructor"), "name") {
        Value::String(constructor) if !constructor.is_empty() => constructor,
        _ => name,
    }
}

fn validate_object(
    error: &Value,
    expected: &Value,
    user_message: Option<String>,
) -> Result<Value, VmError> {
    for key in execute::own_enumerable_keys(expected) {
        if key == "actual" || key == "expected" || key == "generatedMessage" {
            continue;
        }
        let expected_value = execute::get_property_result(expected, &key)?;
        let actual_value = execute::get_property_result(error, &key)?;
        let actual_value = if key == "operator" {
            match actual_value {
                Value::String(value) => Value::String(
                    match value.as_str() {
                        "equal" => "==",
                        "notEqual" => "!=",
                        "strictEqual" => "===",
                        "notStrictEqual" => "!==",
                        _ => value.as_str(),
                    }
                    .to_string(),
                ),
                value => value,
            }
        } else {
            actual_value
        };
        if !expected_property_matches(&expected_value, &actual_value)? {
            if execute::has_own_property(expected, "message")
                && execute::has_own_property(expected, "operator")
            {
                return Err(comparison_mismatch(error, expected, user_message));
            }
            return Err(invalid_expected(
                user_message,
                format!("The error did not match the expected object (key \"{key}\")"),
            ));
        }
    }
    Ok(Value::Undefined)
}

fn comparison_mismatch(actual: &Value, expected: &Value, user_message: Option<String>) -> VmError {
    if let Some(message) = user_message {
        return assertion_error(message, "throws", Value::Undefined, Value::Undefined, false);
    }
    let inspect = |value: Value, marker: &str| {
        let rendered = crate::modules::util::inspect(&value);
        let rendered = rendered
            .strip_suffix("\n  ''")
            .and_then(|value| value.strip_suffix(" +"))
            .unwrap_or(&rendered);
        rendered.replace("\n  ", &format!("\n{marker}     "))
    };
    let actual_operator = execute::get_property(actual, "operator");
    let actual_operator = match actual_operator {
        Value::String(value) => Value::String(
            match value.as_str() {
                "equal" => "==",
                "notEqual" => "!=",
                "strictEqual" => "===",
                "notStrictEqual" => "!==",
                _ => value.as_str(),
            }
            .to_string(),
        ),
        value => value,
    };
    let message = format!(
        "Expected values to be strictly deep-equal:\n+ actual - expected\n\n  Comparison {{\n+   message: {},\n+   operator: {}\n-   message: {},\n-   operator: {}\n  }}\n",
        inspect(execute::get_property(actual, "message"), "+"),
        inspect(actual_operator, "+"),
        inspect(execute::get_property(expected, "message"), "-"),
        inspect(execute::get_property(expected, "operator"), "-"),
    );
    assertion_error(message, "throws", actual.clone(), expected.clone(), true)
}

fn expected_property_matches(expected: &Value, actual: &Value) -> Result<bool, VmError> {
    if matches!(expected, Value::String(_) | Value::StringUnits(_))
        && matches!(actual, Value::String(_) | Value::StringUnits(_))
    {
        return Ok(execute::to_js_string(expected)? == execute::to_js_string(actual)?);
    }
    if is_regexp(expected) {
        let input = match actual {
            Value::String(text) => text.clone(),
            _ => crate::modules::util::inspect(actual),
        };
        return regexp_matches(expected, &input);
    }
    if is_error_constructor(expected) && is_callable(actual) {
        let expected_name = name_of(expected);
        let actual_name = name_of(actual);
        return Ok(
            expected_name == actual_name || (expected_name == "Error" && !actual_name.is_empty())
        );
    }
    if is_error_instance(expected) && is_error_instance(actual) {
        return Ok(execute::same_value(
            &execute::get_property(expected, "name"),
            &execute::get_property(actual, "name"),
        ) && execute::same_value(
            &execute::get_property(expected, "message"),
            &execute::get_property(actual, "message"),
        ));
    }
    crate::modules::deep_equal::deep_equal(expected, actual, true)
}

fn match_assert(args: &[Value], should_match: bool) -> Result<Value, VmError> {
    let pattern = arg(args, 1);
    if !is_regexp(&pattern) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"regexp\" argument must be an instance of RegExp.{}",
            crate::modules::util::invalid_arg_received(&pattern)
        )));
    }
    let input = match arg(args, 0) {
        Value::String(text) => text,
        value => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"string\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(&value)
            )))
        }
    };
    let operator = if should_match {
        "match"
    } else {
        "doesNotMatch"
    };
    if let (Some(message), Some(candidate)) = (args.get(3), args.get(2)) {
        if matches!(message, Value::String(_)) && (is_callable(candidate) || is_error_instance(candidate)) {
            let label = if is_callable(candidate) {
                match execute::get_property(candidate, "name") {
                    Value::String(name) if !name.is_empty() => name,
                    _ => "anonymous".into(),
                }
            } else {
                error_text(candidate)
            };
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::TypeError,
                &[Value::String(format!("The \"error/message\" argument is ambiguous. The error message \"{label}\" is identical to the message."))],
            );
            return Err(VmError::Thrown(execute::set_property(error, "code", Value::String("ERR_AMBIGUOUS_ARGUMENT".into()))));
        }
    }
    if regexp_matches(&pattern, &input)? == should_match {
        return Ok(Value::Undefined);
    }
    let message = match args.get(2) {
        Some(value) if is_callable(value) => match execute::call(value, &Value::Undefined, &[Value::String(input.clone()), pattern.clone()])? {
            Value::String(text) => text,
            _ => format!("'{}' {} {}", input, operator, crate::modules::util::inspect(&pattern)),
        },
        _ => custom_message(args, 2)
            .unwrap_or_else(|| format!("The input did not satisfy {operator}: '{input}'")),
    };
    Err(assertion_error(
        message,
        operator,
        arg(args, 0),
        pattern,
        true,
    ))
}
