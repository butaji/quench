//! `assert` module — Node assertion semantics in pure Rust.
//!
//! The exported value is the callable `assert` function itself
//! (`assert(x)` === `assert.ok(x)`) with every assertion attached as
//! a host capability property. Failures throw `VmError::Thrown` with
//! an AssertionError-shaped object (`name`, `message`, `operator`,
//! `actual`, `expected`), which is catchable via `try`/`catch`.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::registry::*;

thread_local! {
    static ASSERTION_ERROR_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static ASSERT_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
}

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
        pair(
            "partialDeepStrictEqual",
            SPEC_ASSERT_PARTIAL_DEEP_STRICT_EQUAL,
        ),
        // Legacy deep equality aliases share the strict engine for now.
        pair("deepEqual", SPEC_ASSERT_DEEP_EQUAL),
        pair("notDeepEqual", SPEC_ASSERT_NOT_DEEP_EQUAL),
        pair("throws", SPEC_ASSERT_THROWS),
        pair("doesNotThrow", SPEC_ASSERT_DOES_NOT_THROW),
        pair("fail", SPEC_ASSERT_FAIL),
        pair("ifError", SPEC_ASSERT_IF_ERROR),
        pair("match", SPEC_ASSERT_MATCH),
        pair("doesNotMatch", SPEC_ASSERT_DOES_NOT_MATCH),
        ("AssertionError".to_string(), assertion_error_type()),
        ("Assert".to_string(), assert_constructor()),
    ]
}

fn assert_constructor() -> Value {
    let constructor = crate::host::capability(crate::registry::SPEC_ASSERT_CONSTRUCTOR);
    let prototype = assert_prototype();
    let _ = quench_runtime::execute::set_callable_property(
        &constructor,
        "name",
        Value::String("Assert".into()),
    );
    let _ = quench_runtime::execute::set_callable_property(&constructor, "prototype", prototype);
    constructor
}

fn assert_prototype() -> Value {
    ASSERT_PROTOTYPE.with(|slot| {
        if let Some(prototype) = slot.borrow().as_ref() {
            return prototype.clone();
        }
        let prototype = host_api::object(vec![]);
        *slot.borrow_mut() = Some(prototype.clone());
        prototype
    })
}

pub fn constructor_call(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        (
            "message".into(),
            Value::String("Class constructor Assert cannot be invoked without 'new'".into()),
        ),
        (
            "code".into(),
            Value::String("ERR_CONSTRUCT_CALL_REQUIRED".into()),
        ),
    ])))
}

pub fn constructor_new(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let options = args.first().unwrap_or(&Value::Undefined);
    if !matches!(options, Value::Undefined | Value::Object(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"options\" argument must be of type object.{}",
            crate::modules::util::invalid_arg_received(options)
        )));
    }
    let strict = !matches!(
        quench_runtime::execute::get_property(options, "strict"),
        Value::Boolean(false)
    );
    let equal_spec = if strict {
        SPEC_ASSERT_STRICT_EQUAL
    } else {
        SPEC_ASSERT_EQUAL
    };
    let not_equal_spec = if strict {
        SPEC_ASSERT_NOT_STRICT_EQUAL
    } else {
        SPEC_ASSERT_NOT_EQUAL
    };
    let diff = match quench_runtime::execute::get_property(options, "diff") {
        Value::Undefined | Value::Null => "simple".to_string(),
        Value::String(value) if value == "simple" || value == "full" => value,
        value => {
            let received = match value {
                Value::String(ref value) => value.clone(),
                _ => crate::modules::util::inspect(&value),
            };
            return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                "The property 'options.diff' must be one of: 'simple', 'full'. Received '{}'",
                received
            )));
        }
    };
    let instance = host_api::object(vec![
        ("strict".into(), Value::Boolean(strict)),
        (
            "skipPrototype".into(),
            Value::Boolean(matches!(
                quench_runtime::execute::get_property(options, "skipPrototype"),
                Value::Boolean(true)
            )),
        ),
        ("diff".into(), Value::String(diff)),
        ("AssertionError".into(), assertion_error_type()),
    ]);
    let methods = [
        ("ok", SPEC_ASSERT_OK),
        ("fail", SPEC_ASSERT_FAIL),
        ("equal", equal_spec),
        ("notEqual", not_equal_spec),
        ("strictEqual", SPEC_ASSERT_STRICT_EQUAL),
        ("notStrictEqual", SPEC_ASSERT_NOT_STRICT_EQUAL),
        ("deepEqual", SPEC_ASSERT_DEEP_EQUAL),
        ("notDeepEqual", SPEC_ASSERT_NOT_DEEP_EQUAL),
        ("deepStrictEqual", SPEC_ASSERT_DEEP_STRICT_EQUAL),
        ("notDeepStrictEqual", SPEC_ASSERT_NOT_DEEP_STRICT_EQUAL),
        (
            "partialDeepStrictEqual",
            SPEC_ASSERT_PARTIAL_DEEP_STRICT_EQUAL,
        ),
        ("throws", SPEC_ASSERT_THROWS),
        ("doesNotThrow", SPEC_ASSERT_DOES_NOT_THROW),
        ("ifError", SPEC_ASSERT_IF_ERROR),
        ("match", SPEC_ASSERT_MATCH),
        ("doesNotMatch", SPEC_ASSERT_DOES_NOT_MATCH),
    ];
    let instance = methods
        .into_iter()
        .fold(instance, |instance, (name, spec)| {
            quench_runtime::execute::set_property(instance, name, crate::host::capability(spec))
        });
    let instance = if strict {
        let strict_equal = crate::host::capability(SPEC_ASSERT_STRICT_EQUAL);
        let not_strict_equal = crate::host::capability(SPEC_ASSERT_NOT_STRICT_EQUAL);
        let deep_strict_equal = crate::host::capability(SPEC_ASSERT_DEEP_STRICT_EQUAL);
        let not_deep_strict_equal = crate::host::capability(SPEC_ASSERT_NOT_DEEP_STRICT_EQUAL);
        let instance =
            quench_runtime::execute::set_property(instance, "strictEqual", strict_equal.clone());
        let instance = quench_runtime::execute::set_property(instance, "equal", strict_equal);
        let instance = quench_runtime::execute::set_property(
            instance,
            "notStrictEqual",
            not_strict_equal.clone(),
        );
        let instance =
            quench_runtime::execute::set_property(instance, "notEqual", not_strict_equal);
        let instance = quench_runtime::execute::set_property(
            instance,
            "deepStrictEqual",
            deep_strict_equal.clone(),
        );
        let instance =
            quench_runtime::execute::set_property(instance, "deepEqual", deep_strict_equal);
        let instance = quench_runtime::execute::set_property(
            instance,
            "notDeepStrictEqual",
            not_deep_strict_equal.clone(),
        );
        quench_runtime::execute::set_property(instance, "notDeepEqual", not_deep_strict_equal)
    } else {
        instance
    };
    quench_runtime::execute::set_prototype_of(&instance, &assert_prototype())
}

/// The callable `assert` export: `assert(value)` is `assert.ok`.
pub fn build_value() -> Value {
    let value = crate::host::capability(SPEC_ASSERT_OK);
    for (key, property) in build() {
        let _ = execute::set_callable_property(&value, &key, property);
    }
    // `assert.strict === assert` in Node's strict entry point.
    let _ = execute::set_callable_property(&value, "strict", value.clone());
    for (name, source) in [
        ("rejects", ASSERT_REJECTS),
        ("doesNotReject", ASSERT_DOES_NOT_REJECT),
    ] {
        if let Ok(method) = eval_function(source) {
            let _ = execute::set_callable_property(&value, name, method);
        }
    }
    value
}

const ASSERT_REJECTS: &str = r#"(promiseOrFn, expected, message) => {
  let input;
  if (typeof promiseOrFn === "function") {
    try { input = promiseOrFn(); }
    catch (error) { return Promise.reject(error); }
  } else input = promiseOrFn;
  if (!input || typeof input.then !== "function") {
    const error = new TypeError("The promiseFn argument must be a Promise");
    error.code = "ERR_INVALID_ARG_TYPE";
    return Promise.reject(error);
  }
  return Promise.resolve(input).then(
    () => Promise.reject(Object.assign(new Error(message || "Missing expected rejection"), {
      code: "ERR_ASSERTION", operator: "rejects"
    })),
    (error) => {
      if (typeof expected === "function" && expected(error) !== true) {
        return Promise.reject(Object.assign(new Error("The rejection did not match"), {
          code: "ERR_ASSERTION", operator: "rejects"
        }));
      }
      if (expected && typeof expected === "object") {
        for (const key of Object.keys(expected)) {
          if (error == null || error[key] !== expected[key]) {
            return Promise.reject(Object.assign(new Error(message || "The input did not match"), {
              code: "ERR_ASSERTION", operator: "rejects"
            }));
          }
        }
      }
      return error;
    }
  );
}"#;

const ASSERT_DOES_NOT_REJECT: &str = r#"(promiseOrFn, message) => {
  const input = typeof promiseOrFn === "function" ? promiseOrFn() : promiseOrFn;
  if (!input || typeof input.then !== "function") {
    const error = new TypeError("The promiseFn argument must be a Promise");
    error.code = "ERR_INVALID_ARG_TYPE";
    return Promise.reject(error);
  }
  return Promise.resolve(input).then(
    (value) => value,
    (error) => Promise.reject(Object.assign(new Error(message || "Got unwanted rejection"), {
      code: "ERR_ASSERTION", operator: "doesNotReject", actual: error
    }))
  );
}"#;

fn eval_function(source: &str) -> Result<Value, VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
}

fn pair(name: &str, spec: NodeSpec) -> (String, Value) {
    (name.to_string(), crate::host::capability(spec))
}

fn assertion_error_type() -> Value {
    let prototype = assertion_error_prototype();
    let constructor =
        host_api::bound_builtin(quench_runtime::ops::Builtin::Error, Value::Undefined);
    let _ = execute::set_callable_property(
        &constructor,
        "name",
        Value::String("AssertionError".into()),
    );
    let _ = execute::set_callable_property(&constructor, "prototype", prototype);
    constructor
}

fn assertion_error_prototype() -> Value {
    ASSERTION_ERROR_PROTOTYPE.with(|slot| {
        if let Some(prototype) = slot.borrow().as_ref() {
            return prototype.clone();
        }
        let prototype = host_api::object(vec![]);
        let global = quench_runtime::vm::current_global_object();
        let prototype = quench_runtime::execute::get_property_result(&global, "Error")
            .ok()
            .and_then(|error| {
                quench_runtime::execute::get_property_result(&error, "prototype").ok()
            })
            .and_then(|error_prototype| {
                quench_runtime::execute::set_prototype_of(&prototype, &error_prototype).ok()
            })
            .unwrap_or(prototype);
        *slot.borrow_mut() = Some(prototype.clone());
        prototype
    })
}

/// AssertionError-shaped thrown value, catchable from JavaScript.
/// `generated` mirrors Node's `generatedMessage`: true when the
/// message was produced by assert itself, false when user-supplied.
pub fn assertion_error(
    message: String,
    operator: &str,
    actual: Value,
    expected: Value,
    generated: bool,
) -> VmError {
    let error = host_api::object(vec![
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
        ("diff".to_string(), Value::String("simple".into())),
    ]);
    VmError::Thrown(
        quench_runtime::execute::set_prototype_of(&error, &assertion_error_prototype())
            .unwrap_or(error),
    )
}

fn missing_args() -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::TypeError,
        &[Value::String(
            "The \"actual\" and \"expected\" arguments must be specified".into(),
        )],
    );
    VmError::Thrown(quench_runtime::execute::set_property(
        error,
        "code",
        Value::String("ERR_MISSING_ARGS".into()),
    ))
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
    if execute::is_symbol(value) {
        return crate::modules::util::inspect(value);
    }
    match value {
        // Assertion messages contain the complete operands; object inspection
        // may truncate embedded strings, but the observable message must not.
        Value::String(text) => {
            let text = text.strip_suffix('\n').unwrap_or(text);
            format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
        }
        _ => crate::modules::util::inspect(value),
    }
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
    receiver: Option<&Value>,
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
        let label = match operator {
            "strictEqual" => "strictly equal",
            "notStrictEqual" => "strictly unequal",
            "equal" => "loosely equal",
            "notEqual" => "loosely unequal",
            _ => operator,
        };
        let full = receiver.is_some_and(|value| {
            matches!(execute::get_property(value, "diff"), Value::String(diff) if diff == "full")
        });
        if full {
            match operator {
                "strictEqual" => format!(
                    "Expected values to be strictly equal:\n+ actual - expected\n\n+ {}\n- {}\n",
                    rendered(&actual),
                    rendered(&expected)
                ),
                "notStrictEqual" => format!(
                    "Expected \"actual\" to be strictly unequal to:\n\n{}",
                    if matches!(&actual, Value::String(text) if text.ends_with('\n')) {
                        format!("{}\n", rendered(&actual))
                    } else {
                        rendered(&actual)
                    }
                ),
                _ => format!(
                    "Expected values to be {}:\n\n{} {} {}\n",
                    label,
                    rendered(&actual),
                    relation,
                    rendered(&expected)
                ),
            }
        } else {
            match (&actual, &expected) {
                (Value::String(actual), Value::String(expected)) =>
                {
                    simple_binary_message(operator, actual, expected)
                }
                _ if operator == "notDeepEqual" && execute::same_value(&actual, &expected) =>
                    format!(
                        "Expected \"actual\" not to be loosely deep-equal to:\n\n{}",
                        rendered(&actual)
                    ),
                _ if operator == "notDeepEqual" => format!(
                    "Expected values to be loosely deep-equal:\n\n{}\n\nshould not loosely deep-equal\n\n{}",
                    rendered(&actual),
                    rendered(&expected)
                ),
                _ if operator == "notDeepStrictEqual" => format!(
                    "Expected \"actual\" not to be strictly deep-equal to:\n\n{}",
                    rendered(&actual)
                ),
                _ => format!(
                    "Expected values to be {}:\n\n{} {} {}\n",
                    label,
                    rendered(&actual),
                    relation,
                    rendered(&expected)
                ),
            }
        }
    });
    Err(with_instance_diff(
        assertion_error(message, operator, actual, expected, generated),
        receiver,
    ))
}

fn simple_binary_message(operator: &str, actual: &str, expected: &str) -> String {
    let value = |text: &str, limit: usize| {
        if text.contains('\n') {
            simple_side(text, "", limit)
        } else {
            format!("'{text}'")
        }
    };
    match operator {
        "strictEqual" => {
            if actual.len() <= 10 && expected.len() <= 10 {
                format!("Expected values to be strictly equal:\n\n'{actual}' !== '{expected}'\n")
            } else {
                format!(
                    "Expected values to be strictly equal:\n+ actual - expected\n\n{}\n{}\n",
                    if actual.contains('\n') {
                        simple_side(actual, "+", 100)
                    } else {
                        format!("+ '{actual}'")
                    },
                    if expected.contains('\n') {
                        simple_side(expected, "-", 100)
                    } else {
                        format!("- '{expected}'")
                    }
                )
            }
        }
        "notStrictEqual" => format!(
            "Expected \"actual\" to be strictly unequal to:\n\n{}",
            value(actual, 48)
        ),
        _ => format!(
            "Expected values to be {}:\n\n{} !== {}\n",
            if operator == "equal" {
                "loosely equal"
            } else {
                "loosely unequal"
            },
            simple_side(actual, "", 53),
            simple_side(expected, "", 53)
        ),
    }
}

fn simple_loose_message(actual: &str, expected: &str) -> String {
    let scalar = |value: &str| {
        if value.contains('\n') {
            simple_side(value, "", 52)
        } else if value.len() > 508 {
            format!("'{}...", &value[..508])
        } else {
            format!("'{value}'")
        }
    };
    format!(
        "Expected values to be loosely deep-equal:\n\n{}\n\nshould loosely deep-equal\n\n{}",
        scalar(actual),
        scalar(expected)
    )
}

fn simple_side(value: &str, marker: &str, limit: usize) -> String {
    let lines = value.lines().take(limit).collect::<Vec<_>>();
    let mut rendered = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let indent = if index == 0 { "" } else { "  " };
            format!("{marker}{indent}'{line}\\n' +")
        })
        .collect::<Vec<_>>();
    while rendered.len() < limit {
        rendered.push(format!("{marker}  '...'"));
    }
    rendered.join("\n")
}

fn with_instance_diff(error: VmError, receiver: Option<&Value>) -> VmError {
    let Some(receiver) = receiver else {
        return error;
    };
    let Value::String(diff) = execute::get_property(receiver, "diff") else {
        return error;
    };
    match error {
        VmError::Thrown(value) => {
            VmError::Thrown(execute::set_property(value, "diff", Value::String(diff)))
        }
        error => error,
    }
}

pub fn strict_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // Node's `strictEqual` is `Object.is`, not `===` (NaN equals NaN).
    binary_assert(_r, args, "strictEqual", true, |a, b| {
        Ok(execute::same_value(a, b))
    })
}

pub fn not_strict_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    binary_assert(_r, args, "notStrictEqual", false, |a, b| {
        Ok(execute::same_value(a, b))
    })
}

pub fn equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    binary_assert(_r, args, "equal", true, execute::abstract_equal)
}

pub fn not_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    binary_assert(_r, args, "notEqual", false, execute::abstract_equal)
}

fn deep_assert(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
    strict_override: Option<bool>,
) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Err(missing_args());
    }
    let actual = arg(args, 0);
    let expected = arg(args, 1);
    let operator = if strict_override == Some(true) {
        "deepStrictEqual"
    } else if strict_override == Some(false) {
        "deepEqual"
    } else if _r.is_some_and(|receiver| {
        matches!(
            execute::get_property(receiver, "strict"),
            Value::Boolean(false)
        )
    }) {
        "deepEqual"
    } else {
        "deepStrictEqual"
    };
    let strict = strict_override.unwrap_or(operator == "deepStrictEqual");
    let skip_prototype = receiver_skip_prototype(_r);
    if crate::modules::deep_equal::deep_equal_opts(&actual, &expected, strict, skip_prototype)?
        && typed_props_equal(&actual, &expected, strict)
    {
        return Ok(Value::Undefined);
    }
    let custom = custom_message(args, 2);
    let generated = custom.is_none();
    let full_diff = _r.is_some_and(|receiver| {
        matches!(execute::get_property(receiver, "diff"), Value::String(mode) if mode == "full")
    });
    let simple_diff = _r.is_some_and(|receiver| {
        matches!(execute::get_property(receiver, "diff"), Value::String(mode) if mode == "simple")
    });
    let message = custom.map_or_else(
        || if !strict {
            match (&actual, &expected) {
                (Value::String(actual), Value::String(expected))
                    if simple_diff =>
                {
                    simple_loose_message(actual, expected)
                }
                _ => format!(
                    "Expected values to be loosely deep-equal:\n\n{}\n\nshould loosely deep-equal\n\n{}",
                    rendered_deep_for_mode(&actual, full_diff),
                    rendered_deep_for_mode(&expected, full_diff)
                ),
            }
        } else if is_primitive_value(&actual) && is_primitive_value(&expected) {
            format!(
                "Expected values to be strictly deep-equal:\n\n{} !== {}\n",
                rendered(&actual),
                rendered(&expected)
            )
        } else {
            format!(
                "Expected values to be strictly deep-equal:\n+ actual - expected\n\n{}\n",
                deep_diff_for_mode(&actual, &expected, full_diff)
            )
        },
        |message| format!(
            "{message}\n+ actual - expected\n\n{}\n",
            deep_diff_for_mode(&actual, &expected, full_diff)
        ),
    );
    Err(with_instance_diff(
        assertion_error(message, operator, actual, expected, generated),
        _r,
    ))
}

fn is_primitive_value(value: &Value) -> bool {
    matches!(
        value,
        Value::Number(_)
            | Value::Boolean(_)
            | Value::String(_)
            | Value::StringUnits(_)
            | Value::BigInt(_)
            | Value::Null
            | Value::Undefined
    )
}

fn deep_diff_for_mode(actual: &Value, expected: &Value, full: bool) -> String {
    if full {
        if let (Value::String(actual), Value::String(expected)) = (actual, expected) {
            return format!("+ '{actual}'\n- '{expected}'");
        }
    }
    deep_diff(actual, expected)
}

fn rendered_deep_for_mode(value: &Value, full: bool) -> String {
    if full {
        if let Value::String(value) = value {
            return format!("'{}'", value.trim_end_matches('\n'));
        }
    }
    rendered_deep(value)
}

fn rendered_deep(value: &Value) -> String {
    match value {
        Value::Map(map) => {
            let mut entries = map
                .keys
                .borrow()
                .iter()
                .zip(map.values.borrow().iter())
                .map(|(key, value)| {
                    format!(
                        "{} => {}",
                        collection_atom(&Value::Map(map.clone()), key),
                        collection_atom(&Value::Map(map.clone()), value)
                    )
                })
                .collect::<Vec<_>>();
            entries.sort();
            collection_render("Map", entries)
        }
        Value::Set(set) => {
            let owner = Value::Set(set.clone());
            let mut entries = set
                .values
                .borrow()
                .iter()
                .map(|value| collection_atom(&owner, value))
                .collect::<Vec<_>>();
            entries.sort();
            collection_render("Set", entries)
        }
        _ => crate::modules::util::inspect_with_options(value, 1000, false, None, true),
    }
}

fn collection_atom(owner: &Value, value: &Value) -> String {
    let circular = match (owner, value) {
        (Value::Map(left), Value::Map(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Set(left), Value::Set(right)) => std::rc::Rc::ptr_eq(left, right),
        _ => false,
    };
    if circular {
        "[Circular]".into()
    } else if let Value::Set(set) = value {
        let owner = Value::Set(set.clone());
        let entries = set
            .values
            .borrow()
            .iter()
            .map(|entry| collection_atom(&owner, entry))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            "Set(0) {}".into()
        } else {
            format!("Set({}) {{ {} }}", entries.len(), entries.join(", "))
        }
    } else if let Value::Map(map) = value {
        let owner = Value::Map(map.clone());
        let entries = map
            .keys
            .borrow()
            .iter()
            .zip(map.values.borrow().iter())
            .map(|(key, entry)| {
                format!(
                    "{} => {}",
                    collection_atom(&owner, key),
                    collection_atom(&owner, entry)
                )
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            "Map(0) {}".into()
        } else {
            format!("Map({}) {{ {} }}", entries.len(), entries.join(", "))
        }
    } else {
        crate::modules::util::inspect_with_options(value, 1000, false, None, true)
    }
}

fn collection_render(name: &str, entries: Vec<String>) -> String {
    if entries.is_empty() {
        return format!("{name}(0) {{}}");
    }
    let mut lines = vec![format!("{name}({}) {{", entries.len())];
    for (index, entry) in entries.iter().enumerate() {
        let comma = (index + 1 < entries.len()).then_some(',').unwrap_or(' ');
        lines.push(format!("  {entry}{comma}"));
    }
    if let Some(last) = lines.last_mut() {
        last.pop();
    }
    lines.push("}".into());
    lines.join("\n")
}

pub fn deep_strict_equal(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    deep_assert(state, receiver, args, Some(true))
}

pub fn deep_equal(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    deep_assert(state, receiver, args, Some(false))
}

fn typed_props_equal(actual: &Value, expected: &Value, strict: bool) -> bool {
    if !strict {
        return true;
    }
    match (typed_array_kind(actual), typed_array_kind(expected)) {
        (Some(_), Some(_)) => typed_array_props(actual) == typed_array_props(expected),
        _ => true,
    }
}

fn receiver_skip_prototype(receiver: Option<&Value>) -> bool {
    receiver.is_some_and(|value| {
        matches!(
            execute::get_property(value, "skipPrototype"),
            Value::Boolean(true)
        )
    }) || matches!(
        execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "__nodeAssertSkipPrototype"
        ),
        Value::Boolean(true)
    )
}

fn deep_diff(actual: &Value, expected: &Value) -> String {
    if let Some(diff) = regexp_diff(actual, expected) {
        return diff;
    }
    if let Some(diff) = date_diff(actual, expected) {
        return diff;
    }
    if let Some(diff) = typed_array_diff(actual, expected) {
        return diff;
    }
    if let Some(diff) = error_diff(actual, expected) {
        return diff;
    }
    if let Some(diff) = collection_diff(actual, expected) {
        return diff;
    }
    if let Some(diff) = proxy_array_diff(actual, expected) {
        return diff;
    }
    if let Some(diff) = array_object_diff(actual, expected) {
        return diff;
    }
    if let Some(diff) = array_diff(actual, expected) {
        return diff;
    }
    let (Value::Object(left), Value::Object(right)) = (actual, expected) else {
        return format!("+ {}\n- {}", rendered(actual), rendered(expected));
    };
    let left_object = Value::Object(left.clone());
    let right_object = Value::Object(right.clone());
    let property_render = |object: &Value, key: &str| {
        crate::modules::util::inspect_property_with_getters(object, key, 0)
    };
    let keys = execute::own_enumerable_keys(&left_object)
        .into_iter()
        .chain(execute::own_enumerable_keys(&right_object))
        .filter(|key| !key.starts_with('\0'))
        .collect::<BTreeSet<_>>();
    let mut lines = vec!["  {".to_string()];
    for (index, key) in keys.iter().take(50).enumerate() {
        let comma = if index + 1 < keys.len() { "," } else { "" };
        let left_has = execute::has_own_property(&left_object, &key);
        let right_has = execute::has_own_property(&right_object, &key);
        let left_value = execute::get_property(&left_object, &key);
        let right_value = execute::get_property(&right_object, &key);
        let left_render = property_render(&left_object, &key);
        let right_render = property_render(&right_object, &key);
        if left_has
            && right_has
            && execute::same_value(&left_value, &right_value)
            && left_render == right_render
        {
            lines.push(format!("    {key}: {left_render}{comma}"));
        } else if left_has && right_has {
            if let Some(nested) = nested_property_diff(&left_value, &right_value, &key, 4) {
                lines.extend(nested);
            } else {
                lines.push(format!("+   {key}: {left_render}"));
                lines.push(format!("-   {key}: {right_render}"));
            }
        } else if left_has {
            lines.push(format!("+   {key}: {left_render}"));
        } else if right_has {
            lines.push(format!("-   {key}: {right_render}"));
        }
    }
    if keys.len() > 50 {
        lines.push("  ...".to_string());
    }
    lines.push("  }".to_string());
    lines.join("\n")
}

fn nested_property_diff(
    actual: &Value,
    expected: &Value,
    key: &str,
    indent: usize,
) -> Option<Vec<String>> {
    nested_property_diff_depth(actual, expected, key, indent, 0)
}

fn nested_property_diff_depth(
    actual: &Value,
    expected: &Value,
    key: &str,
    indent: usize,
    depth: usize,
) -> Option<Vec<String>> {
    if depth > 8 {
        return None;
    }
    if let (Value::Object(left), Value::Object(right)) = (actual, expected) {
        let left_value = Value::Object(left.clone());
        let right_value = Value::Object(right.clone());
        let keys = execute::own_enumerable_keys(&left_value)
            .into_iter()
            .chain(execute::own_enumerable_keys(&right_value))
            .collect::<BTreeSet<_>>();
        let mut lines = vec![format!("{}{}: {{", " ".repeat(indent), key)];
        for child in keys {
            let lv = execute::get_property(&left_value, &child);
            let rv = execute::get_property(&right_value, &child);
            if let Some(nested) =
                nested_property_diff_depth(&lv, &rv, &child, indent + 2, depth + 1)
            {
                lines.extend(nested);
            } else {
                lines.push(format!(
                    "{}{}: {}",
                    " ".repeat(indent + 2),
                    child,
                    rendered(&lv)
                ));
            }
        }
        lines.push(format!("{}}}", " ".repeat(indent)));
        return Some(lines);
    }
    let (Value::Array(left), Value::Array(right)) = (actual, expected) else {
        return None;
    };
    let mut lines = vec![format!("{}{}: [", " ".repeat(indent), key)];
    let values = |value: &Value, len: usize| {
        (0..len)
            .map(|index| execute::get_property(value, &index.to_string()))
            .collect::<Vec<_>>()
    };
    let actual_values = values(actual, left.logical_len());
    let expected_values = values(expected, right.logical_len());
    let mut sequence = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < actual_values.len() {
        let matching = (j..expected_values.len())
            .find(|candidate| execute::same_value(&actual_values[i], &expected_values[*candidate]));
        if let Some(candidate) = matching {
            while j < candidate {
                sequence.push(('-', expected_values[j].clone()));
                j += 1;
            }
            sequence.push((' ', actual_values[i].clone()));
            i += 1;
            j += 1;
        } else {
            sequence.push(('+', actual_values[i].clone()));
            i += 1;
        }
    }
    while j < expected_values.len() {
        sequence.push(('-', expected_values[j].clone()));
        j += 1;
    }
    let sequence_len = sequence.len();
    for (index, (marker, value)) in sequence.into_iter().enumerate() {
        let comma = if index + 1 < sequence_len { "," } else { "" };
        let spaces = if marker == ' ' {
            indent + 2
        } else {
            indent + 1
        };
        let marker = if marker == ' ' {
            String::new()
        } else {
            marker.to_string()
        };
        lines.push(format!(
            "{marker}{}{comma}",
            format!("{}{}", " ".repeat(spaces), rendered(&value))
        ));
    }
    lines.push(format!("{}]", " ".repeat(indent)));
    Some(lines)
}

fn proxy_array_diff(actual: &Value, expected: &Value) -> Option<String> {
    let Value::Proxy(proxy) = actual else {
        return None;
    };
    let (Value::Array(target), Value::Array(expected)) = (&proxy.target, expected) else {
        return None;
    };
    let actual_values = target.logical_len().checked_sub(0).map(|len| {
        (0..len)
            .map(|i| rendered(&execute::get_property(&proxy.target, &i.to_string())))
            .collect::<Vec<_>>()
    })?;
    let expected_values = (0..expected.logical_len())
        .map(|i| {
            rendered(&execute::get_property(
                &Value::Array(expected.clone()),
                &i.to_string(),
            ))
        })
        .collect::<Vec<_>>();
    let mut lines = vec!["+ Proxy([".to_string(), "- [".to_string()];
    for value in actual_values {
        lines.push(format!("    {value},"));
    }
    lines.push("+ ])".into());
    for value in expected_values.iter().skip(target.logical_len()) {
        lines.push(format!("-   {value}"));
    }
    lines.push("- ]".into());
    Some(lines.join("\n"))
}

fn array_object_diff(actual: &Value, expected: &Value) -> Option<String> {
    let mixed = matches!(actual, Value::Array(_)) != matches!(expected, Value::Array(_));
    if !mixed || !(matches!(actual, Value::Object(_)) || matches!(expected, Value::Object(_))) {
        return None;
    }
    let render = |value: &Value, marker: &str| {
        let (open, close, quoted) = if matches!(value, Value::Array(_)) {
            ('[', ']', false)
        } else {
            ('{', '}', true)
        };
        let mut lines = vec![format!("{marker} {open}")];
        let keys = execute::own_enumerable_keys(value);
        for (index, key) in keys.iter().enumerate() {
            let comma = if index + 1 < keys.len() { "," } else { "" };
            let label = if quoted {
                format!("'{key}'")
            } else {
                String::new()
            };
            let label = if quoted {
                format!("{label}: ")
            } else {
                String::new()
            };
            lines.push(format!(
                "{marker}   {label}{}{comma}",
                rendered(&execute::get_property(value, key))
            ));
        }
        lines.push(format!("{marker} {close}"));
        lines.join("\n")
    };
    Some(format!(
        "{}\n{}",
        render(actual, "+"),
        render(expected, "-")
    ))
}

fn error_diff(actual: &Value, expected: &Value) -> Option<String> {
    if !is_error_object(actual) || !is_error_object(expected) {
        return None;
    }
    let label = |value: &Value| {
        let name = match execute::get_property(value, "name") {
            Value::String(name) if !name.is_empty() => name,
            _ => "Error".into(),
        };
        let message = match execute::get_property(value, "message") {
            Value::String(message) if !message.is_empty() => format!(": {message}"),
            _ => String::new(),
        };
        format!("[{name}{message}]")
    };
    let actual_cause = execute::get_property(actual, "cause");
    let expected_cause = execute::get_property(expected, "cause");
    let actual_has =
        error_has_own_slot(actual, "cause") || !matches!(actual_cause, Value::Undefined);
    let expected_has =
        error_has_own_slot(expected, "cause") || !matches!(expected_cause, Value::Undefined);
    if !actual_has && !expected_has {
        let keys = execute::own_enumerable_keys(actual)
            .into_iter()
            .chain(execute::own_enumerable_keys(expected))
            .collect::<BTreeSet<_>>();
        if !keys.is_empty() {
            let mut lines = vec![format!("  {} {{", label(actual))];
            for key in keys {
                let actual_has = execute::has_own_property(actual, &key);
                let expected_has = execute::has_own_property(expected, &key);
                let actual_value = execute::get_property(actual, &key);
                let expected_value = execute::get_property(expected, &key);
                if actual_has && expected_has && execute::same_value(&actual_value, &expected_value)
                {
                    lines.push(format!("    {key}: {}", rendered(&actual_value)));
                } else if actual_has && expected_has {
                    lines.push(format!("+   {key}: {}", rendered(&actual_value)));
                    lines.push(format!("-   {key}: {}", rendered(&expected_value)));
                }
            }
            lines.push("  }".into());
            return Some(lines.join("\n"));
        }
        return Some(format!("  {}\n- {}", label(actual), label(expected)));
    }
    if !actual_has && expected_has {
        let expected_lines = cause_entry_lines("-", &expected_cause);
        return Some(format!(
            "+ {}\n- {} {{\n{}\n- }}",
            label(actual),
            label(expected),
            expected_lines.join("\n")
        ));
    }
    if actual_has && !expected_has {
        let actual_lines = cause_entry_lines("+", &actual_cause);
        return Some(format!(
            "+ {} {{\n{}\n+ }}\n- {}",
            label(actual),
            actual_lines.join("\n"),
            label(expected)
        ));
    }
    let cause_render = |value: &Value| {
        if is_error_object(value) {
            label(value)
        } else {
            rendered(value)
        }
    };
    let mut lines = vec![format!("  {} {{", label(actual))];
    if actual_has {
        lines.extend(cause_entry_lines("+", &actual_cause));
    }
    if expected_has {
        lines.extend(cause_entry_lines("-", &expected_cause));
    }
    lines.push("  }".into());
    Some(lines.join("\n"))
}

fn error_has_own_slot(value: &Value, key: &str) -> bool {
    if let Value::Object(properties) = value {
        if properties.iter().any(|(name, _)| name == key) {
            return true;
        }
    }
    if execute::has_own_property(value, key) {
        return true;
    }
    matches!(
        execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetOwnPropertyDescriptor),
            &Value::Undefined,
            &[value.clone(), Value::String(key.to_string())],
        ),
        Ok(Value::Object(_))
    )
}

fn cause_entry_lines(marker: &str, value: &Value) -> Vec<String> {
    let rendered = cause_render_value(value);
    let mut lines = rendered.lines();
    let first = lines.next().unwrap_or("");
    let mut output = vec![format!("{marker}   [cause]: {first}")];
    output.extend(lines.map(|line| format!("{marker}   {line}")));
    output
}

fn cause_render_value(value: &Value) -> String {
    if is_error_object(value) {
        let name = match execute::get_property(value, "name") {
            Value::String(name) if !name.is_empty() => name,
            _ => "Error".into(),
        };
        let message = match execute::get_property(value, "message") {
            Value::String(message) if !message.is_empty() => format!(": {message}"),
            _ => String::new(),
        };
        format!("[{name}{message}]")
    } else if let Value::Object(object) = value {
        let keys = execute::own_enumerable_keys(&Value::Object(object.clone()));
        let fields = keys
            .iter()
            .map(|key| format!("  {key}: {}", rendered(&execute::get_property(value, key))))
            .collect::<Vec<_>>();
        format!("{{\n{}\n}}", fields.join(",\n"))
    } else {
        rendered(value)
    }
}

fn is_error_object(value: &Value) -> bool {
    let mut current = execute::get_prototype_of(value).ok();
    while let Some(prototype) = current {
        if matches!(
            prototype,
            Value::Builtin(
                quench_runtime::ops::Builtin::ErrorPrototype
                    | quench_runtime::ops::Builtin::TypeErrorPrototype
                    | quench_runtime::ops::Builtin::RangeErrorPrototype
                    | quench_runtime::ops::Builtin::ReferenceErrorPrototype
                    | quench_runtime::ops::Builtin::SyntaxErrorPrototype
                    | quench_runtime::ops::Builtin::EvalErrorPrototype
                    | quench_runtime::ops::Builtin::URIErrorPrototype
            )
        ) {
            return true;
        }
        current = execute::get_prototype_of(&prototype).ok();
    }
    false
}

fn array_diff(actual: &Value, expected: &Value) -> Option<String> {
    let (Value::Array(left), Value::Array(right)) = (actual, expected) else {
        return None;
    };
    let left_value = Value::Array(left.clone());
    let right_value = Value::Array(right.clone());
    let mut lines = vec!["  [".to_string()];
    let length = left.logical_len().max(right.logical_len());
    for index in 0..length {
        let key = index.to_string();
        let left_has = index < left.logical_len() && execute::has_own_property(&left_value, &key);
        let right_has =
            index < right.logical_len() && execute::has_own_property(&right_value, &key);
        let suffix = if index + 1 < left.logical_len().max(1) {
            ","
        } else {
            ""
        };
        match (left_has, right_has) {
            (true, true) => {
                let left_item = execute::get_property(&left_value, &key);
                let right_item = execute::get_property(&right_value, &key);
                if execute::same_value(&left_item, &right_item) {
                    lines.push(format!("    {}{suffix}", rendered(&left_item)));
                } else {
                    lines.push(format!("+   {}{suffix}", rendered(&left_item)));
                    lines.push(format!("-   {}{suffix}", rendered(&right_item)));
                }
            }
            (true, false) => lines.push(format!(
                "+   {}{suffix}",
                rendered(&execute::get_property(&left_value, &key))
            )),
            (false, true) => lines.push(format!(
                "-   {}",
                rendered(&execute::get_property(&right_value, &key))
            )),
            (false, false) => {}
        }
    }
    lines.push("  ]".into());
    Some(lines.join("\n"))
}

fn collection_diff(actual: &Value, expected: &Value) -> Option<String> {
    let (name, actual_entries, expected_entries) = match (actual, expected) {
        (Value::Set(left), Value::Set(right)) => (
            "Set",
            left.values
                .borrow()
                .iter()
                .map(|value| collection_atom(&Value::Set(left.clone()), value))
                .collect::<Vec<_>>(),
            right
                .values
                .borrow()
                .iter()
                .map(|value| collection_atom(&Value::Set(right.clone()), value))
                .collect::<Vec<_>>(),
        ),
        (Value::Map(left), Value::Map(right)) => (
            "Map",
            left.keys
                .borrow()
                .iter()
                .zip(left.values.borrow().iter())
                .map(|(key, value)| {
                    format!(
                        "{} => {}",
                        collection_atom(&Value::Map(left.clone()), key),
                        collection_atom(&Value::Map(left.clone()), value)
                    )
                })
                .collect::<Vec<_>>(),
            right
                .keys
                .borrow()
                .iter()
                .zip(right.values.borrow().iter())
                .map(|(key, value)| {
                    format!(
                        "{} => {}",
                        collection_atom(&Value::Map(right.clone()), key),
                        collection_atom(&Value::Map(right.clone()), value)
                    )
                })
                .collect::<Vec<_>>(),
        ),
        _ => return None,
    };
    if actual_entries == expected_entries {
        return Some(collection_render(name, actual_entries));
    }
    let mut lines = vec![format!(
        "  {name}({}) {{",
        actual_entries.len().max(expected_entries.len())
    )];
    for entry in actual_entries {
        lines.push(format!("+   {entry}"));
    }
    for entry in expected_entries {
        lines.push(format!("-   {entry}"));
    }
    lines.push("  }".into());
    Some(lines.join("\n"))
}

fn regexp_diff(actual: &Value, expected: &Value) -> Option<String> {
    let actual_lines = regexp_lines(actual)?;
    let expected_lines = regexp_lines(expected)?;
    if actual_lines == expected_lines {
        return Some(actual_lines.join("\n"));
    }
    let mut lines = actual_lines
        .into_iter()
        .map(|line| format!("+ {line}"))
        .collect::<Vec<_>>();
    lines.extend(expected_lines.into_iter().map(|line| format!("- {line}")));
    Some(lines.join("\n"))
}

fn regexp_lines(value: &Value) -> Option<Vec<String>> {
    if !quench_runtime::regexp::has_regexp_internal_slot(value) {
        return None;
    }
    let source = match execute::get_property(value, "source") {
        Value::String(source) => source,
        _ => "(?:)".into(),
    };
    let flags = match execute::get_property(value, "flags") {
        Value::String(flags) => flags,
        _ => String::new(),
    };
    let constructor = execute::get_property(value, "constructor");
    let name = match execute::get_property(&constructor, "name") {
        Value::String(name) if name != "RegExp" && !name.is_empty() => Some(name),
        _ => None,
    };
    let literal = format!("/{source}/{flags}");
    let props = execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| !key.starts_with('\0'))
        .map(|key| {
            format!(
                "  '{key}': {}",
                rendered(&execute::get_property(value, &key))
            )
        })
        .collect::<Vec<_>>();
    let prefix = name.map(|name| format!("{name} ")).unwrap_or_default();
    if props.is_empty() {
        Some(vec![format!("{prefix}{literal}")])
    } else {
        let mut lines = vec![format!("{prefix}{literal} {{")];
        lines.extend(props);
        lines.push("}".into());
        Some(lines)
    }
}

fn date_diff(actual: &Value, expected: &Value) -> Option<String> {
    let actual_lines = date_lines(actual)?;
    let expected_lines = date_lines(expected)?;
    if actual_lines == expected_lines {
        return Some(actual_lines.join("\n"));
    }
    let mut lines = actual_lines
        .into_iter()
        .map(|line| format!("+ {line}"))
        .collect::<Vec<_>>();
    lines.extend(expected_lines.into_iter().map(|line| format!("- {line}")));
    Some(lines.join("\n"))
}

fn date_lines(value: &Value) -> Option<Vec<String>> {
    if !execute::has_own_property(value, "timeValue") {
        return None;
    }
    let mut prototype = execute::get_prototype_of(value).ok()?;
    let mut is_date = false;
    for _ in 0..8 {
        if matches!(
            prototype,
            Value::Builtin(quench_runtime::ops::Builtin::DatePrototype)
        ) {
            is_date = true;
            break;
        }
        prototype = execute::get_prototype_of(&prototype).ok()?;
    }
    if !is_date {
        return None;
    }
    let iso = execute::call(&execute::get_property(value, "toISOString"), value, &[])
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })?;
    let constructor = execute::get_property(value, "constructor");
    let name = match execute::get_property(&constructor, "name") {
        Value::String(name) if name != "Date" => Some(name),
        _ => None,
    };
    let props = execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| !key.starts_with('\0'))
        .map(|key| {
            format!(
                "  '{key}': {}",
                rendered(&execute::get_property(value, &key))
            )
        })
        .collect::<Vec<_>>();
    if props.is_empty() {
        return Some(vec![format!(
            "{}{}",
            name.as_deref()
                .map(|name| format!("{name} "))
                .unwrap_or_default(),
            iso
        )]);
    }
    let prefix = name
        .as_deref()
        .map(|name| format!("{name} "))
        .unwrap_or_default();
    let mut lines = vec![format!("{prefix}{iso} {{")];
    lines.extend(props);
    lines.push("}".into());
    Some(lines)
}

fn typed_array_kind(value: &Value) -> Option<(&'static str, usize)> {
    let Value::Uint8Array(view) = value else {
        return None;
    };
    let kind = if matches!(
        execute::get_property(value, "parent"),
        Value::ArrayBuffer(_)
    ) {
        "Buffer"
    } else {
        "Uint8Array"
    };
    Some((kind, view.logical_len()))
}

fn typed_array_diff(actual: &Value, expected: &Value) -> Option<String> {
    let (actual_kind, actual_len) = typed_array_kind(actual)?;
    let (expected_kind, expected_len) = typed_array_kind(expected)?;
    let actual_values = typed_array_values(actual, actual_len);
    let expected_values = typed_array_values(expected, expected_len);
    let actual_props = typed_array_props(actual);
    let expected_props = typed_array_props(expected);
    let same_shape = actual_kind == expected_kind && actual_len == expected_len;
    let mut lines = Vec::new();
    if same_shape && actual_props == expected_props {
        return Some(format_typed_block(
            actual_kind,
            actual_len,
            &actual_values,
            &actual_props,
            "  ",
        ));
    }
    if actual_kind != expected_kind || actual_len != expected_len {
        lines.push(format!("+ {} [", typed_label(actual_kind, actual_len)));
        lines.push(format!("- {} [", typed_label(expected_kind, expected_len)));
    } else {
        lines.push(format!("  {} [", typed_label(actual_kind, actual_len)));
    }
    for (index, value) in actual_values
        .iter()
        .take(actual_len.max(expected_len))
        .enumerate()
    {
        let comma = (index + 1 < actual_len.max(expected_len)
            || !actual_props.is_empty()
            || !expected_props.is_empty())
        .then_some(",")
        .unwrap_or("");
        lines.push(format!("    {value}{comma}"));
    }
    for prop in actual_props.difference(&expected_props) {
        lines.push(format!("+   {prop}"));
    }
    for prop in expected_props.difference(&actual_props) {
        lines.push(format!("-   {prop}"));
    }
    lines.push("  ]".to_string());
    Some(lines.join("\n"))
}

fn typed_label(kind: &str, length: usize) -> String {
    match kind {
        "Buffer" => format!("Buffer({length}) [Uint8Array]"),
        _ => format!("Uint8Array({length})"),
    }
}

fn typed_array_values(value: &Value, length: usize) -> Vec<String> {
    (0..length)
        .map(|index| rendered(&execute::get_property(value, &index.to_string())))
        .collect()
}

fn typed_array_props(value: &Value) -> BTreeSet<String> {
    execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| {
            key.parse::<usize>().is_err()
                && !key.starts_with('\0')
                && !matches!(key.as_str(), "offset" | "parent" | "toString")
        })
        .map(|key| format!("{key}: {}", rendered(&execute::get_property(value, &key))))
        .collect()
}

fn format_typed_block(
    kind: &str,
    length: usize,
    values: &[String],
    props: &BTreeSet<String>,
    prefix: &str,
) -> String {
    let mut lines = vec![format!("{prefix}{kind}({length}) [")];
    lines.extend(values.iter().map(|value| format!("    {value},")));
    lines.extend(props.iter().map(|prop| format!("    {prop},")));
    lines.push("  ]".to_string());
    lines.join("\n")
}

pub fn not_deep_strict_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Err(missing_args());
    }
    let skip_prototype = receiver_skip_prototype(_r);
    binary_assert(_r, args, "notDeepStrictEqual", false, |a, b| {
        Ok(
            crate::modules::deep_equal::deep_equal_opts(a, b, true, skip_prototype)?
                && typed_props_equal(a, b, true),
        )
    })
}

pub fn not_deep_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Err(missing_args());
    }
    let skip_prototype = receiver_skip_prototype(_r);
    binary_assert(_r, args, "notDeepEqual", false, |a, b| {
        Ok(
            crate::modules::deep_equal::deep_equal_opts(a, b, false, skip_prototype)?
                && typed_props_equal(a, b, true),
        )
    })
}

pub fn partial_deep_strict_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Err(missing_args());
    }
    let actual = arg(args, 0);
    let expected = arg(args, 1);
    if crate::modules::deep_equal::partial_deep_equal(&actual, &expected)? {
        return Ok(Value::Undefined);
    }
    let custom = custom_message(args, 2);
    let generated = custom.is_none();
    let message = custom.map_or_else(
        || {
            format!(
                "Expected values to be partially and strictly deep-equal:\n\n{}\n",
                deep_diff(&actual, &expected)
            )
        },
        |message| {
            format!(
                "{message}\n+ actual - expected\n\n{}\n",
                deep_diff(&actual, &expected)
            )
        },
    );
    Err(with_instance_diff(
        assertion_error(
            message,
            "partialDeepStrictEqual",
            actual,
            expected,
            generated,
        ),
        _r,
    ))
}
