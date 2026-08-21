//! `assert` module — Node assertion semantics in pure Rust.
//!
//! The exported value is the callable `assert` function itself
//! (`assert(x)` === `assert.ok(x)`) with every assertion attached as
//! a host capability property. Failures throw `VmError::Thrown` with
//! an AssertionError-shaped object (`name`, `message`, `operator`,
//! `actual`, `expected`), which is catchable via `try`/`catch`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::host::HostState;
use crate::registry::*;
use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::{PromiseData, PromiseState, Value};

/// Namespace pairs following the module convention; `build_value`
/// also attaches them to the callable `assert` capability value.
pub fn build() -> Vec<(String, Value)> {
    vec![
        pair("ok", SPEC_ASSERT_OK),
        pair("strictEqual", SPEC_ASSERT_STRICT_EQUAL),
        pair("notStrictEqual", SPEC_ASSERT_NOT_STRICT_EQUAL),
        pair("equal", SPEC_ASSERT_EQUAL),
        pair("notEqual", SPEC_ASSERT_NOT_EQUAL),
        pair("deepStrictEqual", SPEC_ASSERT_DEEP_STRICT_EQUAL),
        pair("notDeepStrictEqual", SPEC_ASSERT_NOT_DEEP_STRICT_EQUAL),
        // Legacy deep equality aliases share the strict engine for now.
        pair("deepEqual", SPEC_ASSERT_DEEP_STRICT_EQUAL),
        pair("notDeepEqual", SPEC_ASSERT_NOT_DEEP_STRICT_EQUAL),
        pair("throws", SPEC_ASSERT_THROWS),
        pair("doesNotThrow", SPEC_ASSERT_DOES_NOT_THROW),
        pair("fail", SPEC_ASSERT_FAIL),
        pair("ifError", SPEC_ASSERT_IF_ERROR),
        pair("match", SPEC_ASSERT_MATCH),
        pair("doesNotMatch", SPEC_ASSERT_DOES_NOT_MATCH),
        pair("rejects", SPEC_ASSERT_REJECTS),
        pair_value("AssertionError", assertion_error_type()),
    ]
}

/// The callable `assert` export: `assert(value)` is `assert.ok`.
pub fn build_value() -> Value {
    let value = crate::host::capability(SPEC_ASSERT_OK);
    for (key, property) in build() {
        let _ = execute::set_callable_property(&value, &key, property);
    }
    // `assert.strict === assert` in Node's strict entry point.
    let _ = execute::set_callable_property(&value, "strict", value.clone());
    value
}

fn pair(name: &str, spec: NodeSpec) -> (String, Value) {
    (name.to_string(), crate::host::capability(spec))
}
fn pair_value(name: &str, value: Value) -> (String, Value) {
    (name.to_string(), value)
}

fn assertion_error_type() -> Value {
    host_api::object(vec![(
        "name".to_string(),
        Value::String("AssertionError".to_string()),
    )])
}

/// Build a settled promise (fulfilled with `value`, rejected with
/// `error`) for assert.rejects returns.
fn settle(result: Result<Value, VmError>) -> Value {
    let state = match result {
        Ok(value) => PromiseState::Fulfilled(value),
        Err(VmError::Thrown(value)) => PromiseState::Rejected(value),
        Err(_) => PromiseState::Rejected(Value::String("I/O error".to_string())),
    };
    Value::Promise(Rc::new(PromiseData::new(state)))
}
/// message was produced by assert itself, false when user-supplied.
pub fn assertion_error(
    message: String,
    operator: &str,
    actual: Value,
    expected: Value,
    generated: bool,
) -> VmError {
    VmError::Thrown(host_api::object(vec![
        (
            "name".to_string(),
            Value::String("AssertionError".to_string()),
        ),
        ("message".to_string(), Value::String(message.clone())),
        ("operator".to_string(), Value::String(operator.to_string())),
        ("actual".to_string(), actual),
        ("expected".to_string(), expected),
        (
            "code".to_string(),
            Value::String("ERR_ASSERTION".to_string()),
        ),
        ("generatedMessage".to_string(), Value::Boolean(generated)),
        (
            "stack".to_string(),
            Value::String(format!("AssertionError: {message}")),
        ),
    ]))
}

/// Optional trailing message argument; `None` when absent/undefined.
pub fn custom_message(args: &[Value], index: usize) -> Option<String> {
    match args.get(index) {
        Some(Value::String(message)) => Some(message.clone()),
        Some(Value::Undefined) | None => None,
        Some(value) => Some(crate::modules::util::inspect(value)),
    }
}

fn rendered(value: &Value) -> String {
    crate::modules::util::inspect(value)
}

pub(crate) fn arg(args: &[Value], index: usize) -> Value {
    args.get(index).cloned().unwrap_or(Value::Undefined)
}

pub fn ok(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if execute::is_truthy(&arg(args, 0)) {
        return Ok(Value::Undefined);
    }
    let custom = custom_message(args, 1);
    let generated = custom.is_none();
    let message = custom.unwrap_or_else(|| {
        format!(
            "The expression evaluated to a falsy value:\n\n  assert.ok({})\n",
            rendered(&arg(args, 0))
        )
    });
    Err(assertion_error(
        message,
        "ok",
        arg(args, 0),
        Value::Boolean(true),
        generated,
    ))
}

pub fn fail(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // `assert.fail(err)` rethrows the error itself.
    let first = arg(args, 0);
    if matches!(first, Value::Object(_))
        && !matches!(execute::get_property(&first, "message"), Value::Undefined)
    {
        return Err(VmError::Thrown(first));
    }
    let custom = custom_message(args, 0);
    let generated = custom.is_none();
    let message = custom.unwrap_or_else(|| "Failed".to_string());
    Err(assertion_error(
        message,
        "fail",
        Value::Undefined,
        Value::Undefined,
        generated,
    ))
}

pub fn if_error(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = arg(args, 0);
    if matches!(value, Value::Null | Value::Undefined) {
        return Ok(Value::Undefined);
    }
    let custom = custom_message(args, 1);
    let generated = custom.is_none();
    let message =
        custom.unwrap_or_else(|| format!("ifError got unwanted exception: {}", describe(&value)));
    Err(assertion_error(
        message,
        "ifError",
        value,
        Value::Null,
        generated,
    ))
}

/// Node's ifError rendering: an object with a `message` property uses
/// it verbatim (even empty); an error with an empty message falls back
/// to its name unless the constructor is null/missing; anything else
/// goes through inspect.
fn describe(value: &Value) -> String {
    if let Value::String(text) = execute::get_property(value, "message") {
        if !text.is_empty() {
            return text;
        }
        let constructor = execute::get_property(value, "constructor");
        if matches!(constructor, Value::Null | Value::Undefined) {
            return text;
        }
        if let Value::String(name) = execute::get_property(value, "name") {
            if !name.is_empty() {
                return name;
            }
        }
        return text;
    }
    rendered(value)
}

fn binary_assert(
    args: &[Value],
    operator: &str,
    expect_equal: bool,
    compare: impl Fn(&Value, &Value) -> Result<bool, VmError>,
) -> Result<Value, VmError> {
    let actual = arg(args, 0);
    let expected = arg(args, 1);
    let equal = compare(&actual, &expected)?;
    if equal == expect_equal {
        return Ok(Value::Undefined);
    }
    let relation = if expect_equal { "!==" } else { "===" };
    let custom = custom_message(args, 2);
    let generated = custom.is_none();
    let message = custom.unwrap_or_else(|| {
        format!(
            "Expected values to be {}:\n\n{} {} {}\n",
            operator,
            rendered(&actual),
            relation,
            rendered(&expected)
        )
    });
    Err(assertion_error(
        message, operator, actual, expected, generated,
    ))
}

pub fn strict_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // Node's `strictEqual` is `Object.is`, not `===` (NaN equals NaN).
    binary_assert(args, "strictEqual", true, |a, b| {
        Ok(execute::same_value(a, b))
    })
}

pub fn not_strict_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    binary_assert(args, "notStrictEqual", false, |a, b| {
        Ok(execute::same_value(a, b))
    })
}

pub fn equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    binary_assert(args, "equal", true, execute::abstract_equal)
}

pub fn not_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    binary_assert(args, "notEqual", false, execute::abstract_equal)
}

pub fn deep_strict_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    binary_assert(args, "deepStrictEqual", true, |a, b| {
        crate::modules::deep_equal::deep_equal(a, b, true)
    })
}

pub fn not_deep_strict_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    binary_assert(args, "notDeepStrictEqual", false, |a, b| {
        crate::modules::deep_equal::deep_equal(a, b, true)
    })
}

/// `assert.rejects(asyncFn|promise, [error[, message]])` — returns a
/// promise that resolves when the input rejects (matching the optional
/// validator) and rejects with an AssertionError otherwise.
pub fn rejects(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let first = arg(args, 0);
    let validator = arg(args, 1);
    let promise = match first {
        Value::Promise(p) => p.clone(),
        Value::Function(_) | Value::BoundFunction(_) => {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                (
                    "message".into(),
                    Value::String("assert.rejects requires a Promise".into()),
                ),
            ])));
        }
        _ => {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                (
                    "message".into(),
                    Value::String(
                        "The \"asyncFn\" argument must be of type Function or Promise".into(),
                    ),
                ),
            ])));
        }
    };
    let state = promise.state.borrow().clone();
    match state {
        PromiseState::Fulfilled(_) => Ok(assertion_rejected("The function did not reject")),
        PromiseState::Pending => Ok(assertion_rejected("The input promise is still pending")),
        PromiseState::Rejected(reason) => {
            if !matches!(validator, Value::Undefined) && !matches_validator(&validator, &reason) {
                Ok(assertion_rejected(
                    "The rejection did not match the expected validator",
                ))
            } else {
                Ok(settle(Ok(Value::Undefined)))
            }
        }
    }
}

fn assertion_rejected(msg: &str) -> Value {
    use quench_runtime::execute::VmError;
    settle(Err(VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("AssertionError".into())),
        ("message".into(), Value::String(msg.into())),
        ("operator".into(), Value::String("rejects".into())),
        ("code".into(), Value::String("ERR_ASSERTION".into())),
    ]))))
}

/// Match a Node `assert.rejects`/`assert.throws` validator against an
/// error value. Object validators: string fields use strict equality;
/// other types (RegExp, function, class) are best-effort.
fn matches_validator(validator: &Value, error: &Value) -> bool {
    if let Value::Object(o) = validator {
        for (k, v) in o.iter() {
            let actual = execute::get_property(error, k);
            let ok = match v {
                Value::String(s) => matches!(actual, Value::String(a) if a == *s),
                _ => true,
            };
            if !ok {
                return false;
            }
        }
        true
    } else {
        true
    }
}
