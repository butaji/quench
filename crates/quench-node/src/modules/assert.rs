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
        pair("partialDeepStrictEqual", SPEC_ASSERT_PARTIAL_DEEP_STRICT_EQUAL),
        // Legacy deep equality aliases share the strict engine for now.
        pair("deepEqual", SPEC_ASSERT_DEEP_STRICT_EQUAL),
        pair("notDeepEqual", SPEC_ASSERT_NOT_DEEP_STRICT_EQUAL),
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
        ("code".into(), Value::String("ERR_CONSTRUCT_CALL_REQUIRED".into())),
    ])))
}

pub fn constructor_new(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
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
    let equal_spec = if strict { SPEC_ASSERT_STRICT_EQUAL } else { SPEC_ASSERT_EQUAL };
    let not_equal_spec = if strict {
        SPEC_ASSERT_NOT_STRICT_EQUAL
    } else {
        SPEC_ASSERT_NOT_EQUAL
    };
    let diff = match quench_runtime::execute::get_property(options, "diff") {
        Value::Undefined | Value::Null => "simple".to_string(),
        Value::String(value) if value == "simple" || value == "full" => value,
        value => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                "The property 'options.diff' must be one of: 'simple', 'full'. Received '{}'",
                crate::modules::util::inspect(&value)
            )))
        }
    };
    let instance = host_api::object(vec![
        ("strict".into(), Value::Boolean(strict)),
        ("skipPrototype".into(), Value::Boolean(false)),
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
        ("deepEqual", SPEC_ASSERT_DEEP_STRICT_EQUAL),
        ("notDeepEqual", SPEC_ASSERT_NOT_DEEP_STRICT_EQUAL),
        ("deepStrictEqual", SPEC_ASSERT_DEEP_STRICT_EQUAL),
        ("notDeepStrictEqual", SPEC_ASSERT_NOT_DEEP_STRICT_EQUAL),
        ("partialDeepStrictEqual", SPEC_ASSERT_PARTIAL_DEEP_STRICT_EQUAL),
        ("throws", SPEC_ASSERT_THROWS),
        ("doesNotThrow", SPEC_ASSERT_DOES_NOT_THROW),
        ("ifError", SPEC_ASSERT_IF_ERROR),
        ("match", SPEC_ASSERT_MATCH),
        ("doesNotMatch", SPEC_ASSERT_DOES_NOT_MATCH),
    ];
    let instance = methods.into_iter().fold(instance, |instance, (name, spec)| {
        quench_runtime::execute::set_property(instance, name, crate::host::capability(spec))
    });
    let instance = if strict {
        let equal = quench_runtime::execute::get_property(&instance, "strictEqual");
        let not_equal = quench_runtime::execute::get_property(&instance, "notStrictEqual");
        let instance = quench_runtime::execute::set_property(instance, "equal", equal);
        quench_runtime::execute::set_property(instance, "notEqual", not_equal)
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
    let constructor = host_api::bound_builtin(
        quench_runtime::ops::Builtin::Error,
        Value::Undefined,
    );
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
        &[Value::String("The \"actual\" and \"expected\" arguments must be specified".into())],
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
        format!(
            "Expected values to be {}:\n\n{} {} {}\n",
            operator,
            rendered(&actual),
            relation,
            rendered(&expected)
        )
    });
    Err(with_instance_diff(
        assertion_error(
        message, operator, actual, expected, generated,
        ),
        receiver,
    ))
}

fn with_instance_diff(error: VmError, receiver: Option<&Value>) -> VmError {
    let Some(receiver) = receiver else { return error };
    let Value::String(diff) = execute::get_property(receiver, "diff") else {
        return error;
    };
    match error {
        VmError::Thrown(value) => VmError::Thrown(execute::set_property(
            value,
            "diff",
            Value::String(diff),
        )),
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

pub fn deep_strict_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Err(missing_args());
    }
    let actual = arg(args, 0);
    let expected = arg(args, 1);
    if crate::modules::deep_equal::deep_equal(&actual, &expected, true)? {
        return Ok(Value::Undefined);
    }
    let custom = custom_message(args, 2);
    let generated = custom.is_none();
    let message = custom.map_or_else(
        || format!(
            "Expected values to be strictly deep-equal:\n\n{}\n",
            deep_diff(&actual, &expected)
        ),
        |message| format!(
            "{message}\n+ actual - expected\n\n{}\n",
            deep_diff(&actual, &expected)
        ),
    );
    Err(with_instance_diff(
        assertion_error(message, "deepStrictEqual", actual, expected, generated),
        _r,
    ))
}

fn deep_diff(actual: &Value, expected: &Value) -> String {
    let (Value::Object(left), Value::Object(right)) = (actual, expected) else {
        return format!("+ {}\n- {}", rendered(actual), rendered(expected));
    };
    let left_object = Value::Object(left.clone());
    let right_object = Value::Object(right.clone());
    let keys = execute::own_enumerable_keys(&left_object)
        .into_iter()
        .chain(execute::own_enumerable_keys(&right_object))
        .filter(|key| !key.starts_with('\0'))
        .collect::<BTreeSet<_>>();
    let mut lines = vec!["  {".to_string()];
    for key in keys {
        let left_value = execute::get_property(&left_object, &key);
        let right_value = execute::get_property(&right_object, &key);
        if execute::same_value(&left_value, &right_value) {
            lines.push(format!("    {key}: {}", rendered(&left_value)));
        } else {
            lines.push(format!("+   {key}: {}", rendered(&left_value)));
            lines.push(format!("-   {key}: {}", rendered(&right_value)));
        }
    }
    lines.push("  }".to_string());
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
    binary_assert(_r, args, "notDeepStrictEqual", false, |a, b| {
        crate::modules::deep_equal::deep_equal(a, b, true)
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
        || format!(
            "Expected values to be partially and strictly deep-equal:\n\n{}\n",
            deep_diff(&actual, &expected)
        ),
        |message| format!("{message}\n+ actual - expected\n\n{}\n", deep_diff(&actual, &expected)),
    );
    Err(with_instance_diff(
        assertion_error(message, "partialDeepStrictEqual", actual, expected, generated),
        _r,
    ))
}
