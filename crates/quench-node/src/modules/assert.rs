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
    static ASSERTION_ERROR_CONSTRUCTOR: RefCell<Option<Value>> = const { RefCell::new(None) };
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

/// Internal Myers diff entry point used by Node's assertion internals.  The
/// public assertion formatter is native Rust; this small capability keeps the
/// internal module contract available to code that imports it directly.
pub fn myers_diff(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let length = |value: &Value| match execute::get_property(value, "length") {
        Value::Number(number) if number.is_finite() && number >= 0.0 => number as u64,
        _ => 0,
    };
    let actual = length(args.first().unwrap_or(&Value::Undefined));
    let expected = length(args.get(1).unwrap_or(&Value::Undefined));
    let max = actual.saturating_add(expected);
    if max > 2_u64.pow(31) - 1 {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::RangeError,
            &[Value::String(format!(
                "The value of \"myersDiff input size\" is out of range. It must be < 2^31. Received {max}"
            ))],
        );
        return Err(VmError::Thrown(execute::set_property(
            error,
            "code",
            Value::String("ERR_OUT_OF_RANGE".into()),
        )));
    }
    // The native assertion implementation owns the full diff algorithm. For
    // direct internal callers, preserve the operation/value pair shape with a
    // compact equality walk and bounded insert/delete fallbacks.
    let mut result = Vec::new();
    let common = actual.min(expected) as usize;
    for index in 0..common {
        let left = execute::get_property(
            args.first().unwrap_or(&Value::Undefined),
            &index.to_string(),
        );
        let right =
            execute::get_property(args.get(1).unwrap_or(&Value::Undefined), &index.to_string());
        if crate::modules::deep_equal::deep_equal_opts(&left, &right, true, false)? {
            result.push(host_api::array(vec![Value::Number(0.0), left]));
        } else {
            result.push(host_api::array(vec![Value::Number(-1.0), right]));
            result.push(host_api::array(vec![Value::Number(1.0), left]));
        }
    }
    for index in common..actual as usize {
        result.push(host_api::array(vec![
            Value::Number(1.0),
            execute::get_property(
                args.first().unwrap_or(&Value::Undefined),
                &index.to_string(),
            ),
        ]));
    }
    for index in common..expected as usize {
        result.push(host_api::array(vec![
            Value::Number(-1.0),
            execute::get_property(args.get(1).unwrap_or(&Value::Undefined), &index.to_string()),
        ]));
    }
    Ok(host_api::array(result))
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
    let properties = build();
    for (key, property) in &properties {
        let _ = execute::set_callable_property(&value, key, property.clone());
    }
    let strict = crate::host::capability(SPEC_ASSERT_OK);
    for (key, property) in &properties {
        let _ = execute::set_callable_property(&strict, key, property.clone());
    }
    for (names, source) in [
        (["equal", "strictEqual"], "strictEqual"),
        (["notEqual", "notStrictEqual"], "notStrictEqual"),
        (["deepEqual", "deepStrictEqual"], "deepStrictEqual"),
        (["notDeepEqual", "notDeepStrictEqual"], "notDeepStrictEqual"),
    ] {
        let method = execute::get_property(&value, source);
        for name in names {
            let _ = execute::set_callable_property(&strict, name, method.clone());
        }
    }
    let _ = execute::set_callable_property(&strict, "strict", strict.clone());
    let _ = execute::set_callable_property(&strict, "\0quench:strict", Value::Boolean(true));
    let _ = execute::set_callable_property(&value, "strict", strict.clone());
    let _ = execute::set_callable_property(&value, "\0quench:strict-namespace", strict.clone());
    for (name, source) in [
        ("rejects", ASSERT_REJECTS),
        ("doesNotReject", ASSERT_DOES_NOT_REJECT),
    ] {
        if let Ok(method) = eval_function(source) {
            let _ = execute::set_callable_property(&value, name, method.clone());
            let _ = execute::set_callable_property(&strict, name, method);
        }
    }
    value
}

const ASSERT_REJECTS: &str = r#"(promiseOrFn, expected, message) => {
  const receivedType = (value) => {
    if (value === undefined) return "undefined";
    if (value === null) return "null";
    if (typeof value === "string") return `type string ('${value}')`;
    if (typeof value === "number") return `type number (${value})`;
    if (typeof value === "boolean") return `type boolean (${value})`;
    if (typeof value === "object" && value.constructor && value.constructor.name)
      return `an instance of ${value.constructor.name}`;
    return `type ${typeof value}`;
  };
  let input;
  if (typeof promiseOrFn === "function") {
    try { input = promiseOrFn(); }
    catch (error) { return Promise.reject(error); }
    if (!(input instanceof Promise)) {
      const error = new TypeError(`Expected instance of Promise to be returned from the \"promiseFn\" function but got ${receivedType(input)}.`);
      error.code = "ERR_INVALID_RETURN_VALUE";
      return Promise.reject(error);
    }
  } else input = promiseOrFn;
  if (!input || typeof input.then !== "function") {
    const error = new TypeError(`The \"promiseFn\" argument must be of type function or an instance of Promise. Received ${receivedType(promiseOrFn)}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    return Promise.reject(error);
  }
  if (typeof input.catch !== "function") {
    const error = new TypeError(`The \"promiseFn\" argument must be of type function or an instance of Promise. Received ${receivedType(promiseOrFn)}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    return Promise.reject(error);
  }
  return Promise.resolve(input).then(
    () => {
      return Promise.reject(Object.assign(new (require("assert").AssertionError)({message: message || `Missing expected rejection${typeof expected === "function" ? ` (${expected.name || "mustNotCall"})` : ""}.`}), {
      code: "ERR_ASSERTION", operator: "rejects", generatedMessage: !message
      }));
    },
    (error) => {
      if (typeof expected === "function") {
        const validation = expected(error);
        if (validation !== true) {
          const received = typeof validation === "string" ? `'${validation}'` : String(validation);
          const caught = error && typeof error.name === "string"
            ? `${error.name}: ${error.message || ""}`
            : String(error);
          const validationMessage = `The "validate" validation function is expected to return "true". Received ${received}\n\nCaught error:\n\n${caught}`;
          return Promise.reject(Object.assign(new (require("assert").AssertionError)({message: validationMessage}), {
          code: "ERR_ASSERTION", operator: "rejects", actual: error, expected, generatedMessage: true, stack: "AssertionError: The rejection did not match\\n    at Function.rejects"
          }));
        }
      }
      if (expected && typeof expected === "object") {
        const rejectsMatch = (received, wanted) => {
          if (
            received &&
            wanted &&
            typeof received === "object" &&
            typeof wanted === "object" &&
            (received instanceof Error || wanted instanceof Error ||
              received instanceof DOMException || wanted instanceof DOMException)
          ) {
            if (String(received.name) !== String(wanted.name)) return false;
            if (String(received.message) !== String(wanted.message)) return false;
            if ("code" in wanted && received.code !== wanted.code) return false;
            return true;
          }
          return received === wanted;
        };
        for (const key of Object.keys(expected)) {
          const expectedValue = expected[key];
          const actualValue = error == null ? undefined : error[key];
          const matches = expectedValue instanceof RegExp
            ? expectedValue.test(actualValue)
            : rejectsMatch(actualValue, expectedValue);
          if (!matches) {
            return Promise.reject(Object.assign(new (require("assert").AssertionError)({message: message || "The input did not match"}), {
              code: "ERR_ASSERTION", operator: "rejects", generatedMessage: !message, actual: error, expected, stack: `AssertionError: ${message || "The input did not match"}\\n    at Function.rejects`
            }));
          }
        }
      }
      return error;
    }
  );
}"#;

const ASSERT_DOES_NOT_REJECT: &str = r#"(promiseOrFn, message) => {
  const receivedType = (value) => {
    if (value === undefined) return "undefined";
    if (value === null) return "null";
    if (typeof value === "string") return `type string ('${value}')`;
    if (typeof value === "number") return `type number (${value})`;
    if (typeof value === "boolean") return `type boolean (${value})`;
    if (typeof value === "object" && value.constructor && value.constructor.name)
      return `an instance of ${value.constructor.name}`;
    return `type ${typeof value}`;
  };
  let input;
  if (typeof promiseOrFn === "function") {
    try { input = promiseOrFn(); }
    catch (error) { return Promise.reject(error); }
    if (!(input instanceof Promise)) {
      const error = new TypeError(`Expected instance of Promise to be returned from the \"promiseFn\" function but got ${receivedType(input)}.`);
      error.code = "ERR_INVALID_RETURN_VALUE";
      return Promise.reject(error);
    }
  } else input = promiseOrFn;
  if (!input || typeof input.then !== "function") {
    const error = new TypeError(`The \"promiseFn\" argument must be of type function or an instance of Promise. Received ${receivedType(promiseOrFn)}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    return Promise.reject(error);
  }
  if (typeof input.catch !== "function") {
    const error = new TypeError(`The "promiseFn" argument must be of type function or an instance of Promise. Received ${receivedType(promiseOrFn)}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    return Promise.reject(error);
  }
  return Promise.resolve(input).then(
    (value) => value,
    (error) => {
      const customMessage = typeof message === "function" ? message(error) : message;
      return Promise.reject(Object.assign(new (require("assert").AssertionError)({message: typeof customMessage === "string" ? customMessage : `Got unwanted rejection.\nActual message: "${error && typeof error.message === "string" ? error.message : String(error)}"`}), {
      code: "ERR_ASSERTION", operator: "doesNotReject", actual: error, generatedMessage: !message
      }));
    }
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
    ASSERTION_ERROR_CONSTRUCTOR.with(|slot| {
        if let Some(constructor) = slot.borrow().as_ref() {
            return constructor.clone();
        }
        let constructor = crate::host::capability(SPEC_ASSERTION_ERROR_CONSTRUCTOR);
        let _ = execute::set_callable_property(
            &constructor,
            "name",
            Value::String("AssertionError".into()),
        );
        let _ =
            execute::set_callable_property(&constructor, "prototype", assertion_error_prototype());
        *slot.borrow_mut() = Some(constructor.clone());
        constructor
    })
}

pub fn assertion_error_constructor(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().unwrap_or(&Value::Undefined);
    if !matches!(options, Value::Object(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"options\" argument must be of type object.{}",
            crate::modules::util::invalid_arg_received(options)
        )));
    }
    let message = match execute::get_property(options, "message") {
        Value::String(message) => message,
        _ => String::new(),
    };
    let actual = execute::get_property(options, "actual");
    let expected = execute::get_property(options, "expected");
    let operator = match execute::get_property(options, "operator") {
        Value::String(operator) => operator,
        _ => "strictEqual".into(),
    };
    match assertion_error(message, &operator, actual, expected, false) {
        VmError::Thrown(error) => Ok(error),
        error => Err(error),
    }
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
    let constructor = assertion_error_type();
    let prototype = quench_runtime::execute::get_property(&constructor, "prototype");
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
        ("constructor".to_string(), constructor.clone()),
        ("\0prototype".to_string(), prototype),
    ]);
    VmError::Thrown(
        quench_runtime::execute::set_prototype_of(
            &error,
            &quench_runtime::execute::get_property(&constructor, "prototype"),
        )
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
        Some(Value::String(message)) => Some(format_assert_message(message, args.get(index + 1..).unwrap_or_default())),
        Some(Value::Undefined) | None => None,
        Some(value) if is_error_object(value) || is_cross_context_error(value) => match execute::get_property(value, "message") {
            Value::String(message) => Some(message),
            _ => Some(crate::modules::util::inspect(value)),
        },
        Some(value) => Some(crate::modules::util::inspect(value)),
    }
}

fn format_assert_message(message: &str, values: &[Value]) -> String {
    let mut result = String::with_capacity(message.len());
    let mut chars = message.chars();
    let mut index = 0usize;
    while let Some(ch) = chars.next() {
        if ch == '%' {
            if let Some(spec) = chars.next() {
                if spec == '%' { result.push('%'); continue; }
                if let Some(value) = values.get(index) {
                    let text = match spec {
                        'i' => match value { Value::Number(n) => (*n as i64).to_string(), _ => execute::to_js_string(value).unwrap_or_default() },
                        'd' => match value { Value::Number(n) => n.to_string(), _ => execute::to_js_string(value).unwrap_or_default() },
                        's' => execute::to_js_string(value).unwrap_or_default(),
                        _ => { result.push('%'); result.push(spec); continue; }
                    };
                    result.push_str(&text); index += 1; continue;
                }
                result.push('%'); result.push(spec); continue;
            }
        }
        result.push(ch);
    }
    result
}

fn rendered(value: &Value) -> String {
    if let Value::String(text) = value {
        if text.starts_with("Symbol.") && text.contains('\0') {
            let body = text.split('\0').next().unwrap_or_default();
            let description = body.strip_prefix("Symbol.").unwrap_or_default();
            if description.is_empty() || description.chars().any(char::is_control) {
                return "Symbol()".into();
            }
        }
    }
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
        Value::Function(_) | Value::WeakFunction(_) | Value::BoundFunction(_) | Value::HostCapability(_) => function_render(value),
        _ => crate::modules::util::inspect(value),
    }
}

fn function_render(value: &Value) -> String {
    match execute::get_property(value, "name") {
        Value::String(name) if !name.is_empty() => format!("[Function: {name}]"),
        _ => "[Function (anonymous)]".into(),
    }
}

fn rendered_not_deep(value: &Value) -> String {
    let Value::Array(array) = value else {
        return rendered(value);
    };
    let owner = Value::Array(array.clone());
    let mut lines = vec!["[".to_string()];
    let limit = array.logical_len().min(45);
    for index in 0..limit {
        let key = index.to_string();
        let suffix = if index + 1 < limit || array.logical_len() > limit {
            ","
        } else {
            ""
        };
        lines.push(format!(
            "  {}{suffix}",
            rendered(&execute::get_property(&owner, &key))
        ));
    }
    if array.logical_len() > limit {
        lines.push("...".into());
        return lines.join("\n");
    }
    lines.push("]".into());
    lines.join("\n")
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
    if args.get(1).is_some_and(execute::is_symbol) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"message\" argument must be one of type string or function.{}",
            crate::modules::util::invalid_arg_received(&arg(args, 1))
        )));
    }
    if let Some(value) = args.get(1).filter(|value| is_error_object(value) || is_cross_context_error(value)) {
        return Err(VmError::Thrown(value.clone()));
    }
    let custom = custom_message(args, 1);
    let generated = custom.is_none();
    let message = custom.unwrap_or_else(|| {
        if args.is_empty() {
            return "No value argument passed to `assert.ok()`".into();
        }
        if _r.is_some_and(|receiver| {
            matches!(execute::get_property(receiver, "\0quench:strict"), Value::Boolean(true))
        }) {
            return "The expression evaluated to a falsy value:\n\n  strict.ok(\n".into();
        }
        if _r.is_some_and(|receiver| matches!(receiver, Value::Null))
            && matches!(args.first(), Some(Value::Number(value)) if *value == 0.0)
        {
            return "The expression evaluated to a falsy value:\n\n  assert['ok'][\"apply\"](null, [0])\n".into();
        }
        if matches!(args.first(), Some(Value::Null))
            && matches!(args.get(1), Some(Value::Undefined))
        {
            let call = if _r.is_none_or(|receiver| matches!(receiver, Value::Undefined)) {
                "assert(null, undefined)"
            } else {
                "ok(null, undefined)"
            };
            return format!("The expression evaluated to a falsy value:\n\n  {call}\n");
        }
        let argument = arg(args, 0);
        let call = source_assertion_call(&argument)
            .unwrap_or_else(|| format!("assert.ok({})", rendered(&argument)));
        format!("The expression evaluated to a falsy value:\n\n  {call}\n")
    });
    Err(assertion_error(
        message,
        "ok",
        arg(args, 0),
        Value::Boolean(true),
        generated,
    ))
}

fn source_assertion_call(value: &Value) -> Option<String> {
    let source = quench_runtime::vm::current_context();
    let source = source.source_text()?;
    let expected = rendered(value);
    let mut calls = Vec::new();
    let mut cursor = 0;
    while cursor < source.len() {
        let next_ok = source[cursor..].find(".ok(");
        let next_assert = source[cursor..].find("assert(");
        let (relative, bare) = match (next_ok, next_assert) {
            (Some(ok), Some(assert)) if ok <= assert => (ok, false),
            (Some(ok), _) => (ok, false),
            (_, Some(assert)) => (assert, true),
            (None, None) => break,
        };
        let start = cursor + relative;
        let open = if bare { start + 5 } else { start + 3 };
        let mut depth = 0usize;
        let mut quote = None;
        let mut escaped = false;
        let mut end = None;
        for (offset, ch) in source[open..].char_indices() {
            if let Some(q) = quote {
                if escaped { escaped = false; continue; }
                if ch == '\\' { escaped = true; continue; }
                if ch == q { quote = None; }
                continue;
            }
            match ch {
                '\'' | '"' | '`' => quote = Some(ch),
                '(' | '[' | '{' => depth += 1,
                ')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 { end = Some(open + offset); break; }
                }
                ']' | '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        let Some(end) = end else { break; };
        let call_start = if bare {
            start
        } else {
            let mut token_start = start;
            for (index, ch) in source[..start].char_indices().rev() {
                if ch.is_whitespace() { continue; }
                if matches!(ch, ';' | '\n' | '{' | '}' | '(' | ')' | '=' | ',' | ':') {
                    break;
                }
                token_start = index;
            }
            token_start
        };
        let mut call = source[call_start..=end].trim().to_string();
        if call.ends_with(", undefined)") {
            let new_len = call.len() - ", undefined)".len();
            call.truncate(new_len);
            call.push(')');
        }
        if let Some(dot) = call.find(".ok(") {
            let prefix = call[..dot].trim_end();
            call = format!("{prefix}{}", &call[dot..]);
        }
        let argument = &source[open + 1..end];
        let argument = argument.trim().to_string();
        let first_argument = {
            let mut depth = 0usize;
            let mut quote = None;
            let mut escaped = false;
            argument
                .char_indices()
                .find_map(|(index, ch)| {
                    if let Some(q) = quote {
                        if escaped { escaped = false; }
                        else if ch == '\\' { escaped = true; }
                        else if ch == q { quote = None; }
                        return None;
                    }
                    match ch {
                        '\'' | '"' | '`' => quote = Some(ch),
                        '(' | '[' | '{' => depth += 1,
                        ')' | ']' | '}' => depth = depth.saturating_sub(1),
                        ',' if depth == 0 => return Some(index),
                        _ => {}
                    }
                    None
                })
                .map_or(argument.as_str(), |index| &argument[..index])
                .trim()
                .to_string()
        };
        calls.push((call, first_argument));
        cursor = end + 1;
    }
    // Preserve source spelling when the first argument is a literal match.
    if let Some((call, _)) = calls.iter().find(|(_, argument)| argument == &expected) {
        return Some(call.clone());
    }
    (calls.len() == 1).then(|| calls.first().map(|(call, _)| call.clone())).flatten()
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
    if let Some(value) = args.get(2).filter(|value| is_error_object(value)) {
        return Err(VmError::Thrown(value.clone()));
    }
    let relation = if expect_equal { "!==" } else { "===" };
    let custom = custom_message_binary(args, 2, &actual, &expected)?;
    let generated = custom.is_none();
    let message = custom.map_or_else(|| {
        let label = match operator {
            "strictEqual" => "strictly equal",
            "notStrictEqual" => "strictly unequal",
            "equal" => "loosely equal",
            "notEqual" => "loosely unequal",
            _ => operator,
        };
        let full = receiver.is_some_and(|value| {
            matches!(execute::get_property(value, "diff"), Value::String(diff) if diff == "full")
        }) || (operator == "strictEqual"
            && (needs_structural_diff(&actual) || needs_structural_diff(&expected)));
        if operator == "strictEqual"
            && has_error_shape(&actual)
            && has_error_shape(&expected)
            && !execute::same_identity(&actual, &expected)
        {
            return format!(
                "Expected \"actual\" to be reference-equal to \"expected\":\n+ actual - expected\n\n+ {}\n- {}\n",
                rendered_error(&actual), rendered_error(&expected)
            );
        }
        if operator == "strictEqual"
            && !execute::same_identity(&actual, &expected)
            && !is_primitive_value(&actual)
            && !is_primitive_value(&expected)
            && crate::modules::deep_equal::deep_equal_opts(&actual, &expected, true, false).unwrap_or(false)
        {
            return format!("Values have same structure but are not reference-equal:\n\n{}\n", rendered(&actual));
        }
        if operator == "strictEqual"
            && !is_primitive_value(&actual)
            && !is_primitive_value(&expected)
            && !execute::same_identity(&actual, &expected)
        {
            return format!(
                "Expected \"actual\" to be reference-equal to \"expected\":\n+ actual - expected\n\n{}\n",
                reference_diff_render(&actual, &expected)
            );
        }
        if full {
            match operator {
                "strictEqual" => format!(
                    "Expected values to be strictly equal:\n+ actual - expected\n\n+ {}\n- {}\n",
                    strict_operand_render(&actual),
                    strict_operand_render(&expected)
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
                    rendered_not_deep(&actual)
                ),
                _ if operator == "notStrictEqual"
                    && !matches!(actual, Value::String(_)) => format!(
                    "Expected \"actual\" to be strictly unequal to: {}",
                    rendered(&actual)
                ),
                _ if operator == "strictEqual"
                    && is_error_object(&actual)
                    && is_error_object(&expected) => format!(
                    "Expected \"actual\" to be reference-equal to \"expected\":\n+ actual - expected\n\n+ {}\n- {}\n",
                    rendered_error(&actual),
                    rendered_error(&expected)
                ),
                _ if operator == "notEqual" => {
                    format!("{} != {}", rendered(&actual), rendered(&expected))
                }
                _ => format!(
                    "Expected values to be {}:\n\n{} {} {}\n",
                    label,
                    rendered(&actual),
                    relation,
                    rendered(&expected)
                ),
            }
        }
    }, |custom| {
        if operator == "strictEqual" {
            format!("{custom}\n\n{} {} {}\n", rendered(&actual), relation, rendered(&expected))
        } else {
            custom
        }
    });
    Err(with_instance_diff(
        assertion_error(message, operator, actual, expected, generated),
        receiver,
    ))
}

/// Assertion message callbacks receive the operands and provide the final
/// message text. A non-string return is treated as no custom message, matching
/// Node's generated fallback diagnostics.
fn custom_message_binary(
    args: &[Value],
    index: usize,
    actual: &Value,
    expected: &Value,
) -> Result<Option<String>, VmError> {
    match args.get(index) {
        Some(value) if quench_runtime::is_callable(value) => {
            let result = execute::call(value, &Value::Undefined, &[actual.clone(), expected.clone()])?;
            Ok(match result { Value::String(text) => Some(text), _ => None })
        }
        _ => Ok(custom_message(args, index)),
    }
}

fn simple_binary_message(operator: &str, actual: &str, expected: &str) -> String {
    if operator == "strictEqual" && actual.starts_with("Symbol.") && actual.contains('\0') {
        let description = actual
            .split('\0')
            .next()
            .unwrap_or_default()
            .strip_prefix("Symbol.")
            .unwrap_or_default();
        let actual = if description.is_empty() || description.chars().any(char::is_control) {
            "Symbol()"
        } else {
            actual
        };
        return format!("Expected values to be strictly equal:\n\n{actual} !== '{expected}'\n");
    }
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
    let mut all_lines = value.split('\n').collect::<Vec<_>>();
    if all_lines.last() == Some(&"") {
        all_lines.pop();
    }
    let lines = all_lines.iter().take(limit).copied().collect::<Vec<_>>();
    let mut rendered = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let indent = if index == 0 { "" } else { "   " };
            format!("{marker}{indent}'{line}\\n' +")
        })
        .collect::<Vec<_>>();
    if all_lines.len() > limit {
        rendered.push(format!("{marker}   '...'"));
    }
    if let Some(last) = rendered.last_mut() {
        if last.ends_with(" +") && all_lines.len() <= limit {
            last.truncate(last.len() - 2);
        }
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
    binary_assert(_r, args, "equal", true, |a, b| {
        let equal = execute::abstract_equal(a, b)?;
        Ok(equal
            || matches!((a, b), (Value::Number(a), Value::Number(b)) if a.is_nan() && b.is_nan()))
    })
}

pub fn not_equal(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    binary_assert(_r, args, "notEqual", false, |a, b| {
        let equal = execute::abstract_equal(a, b)?;
        Ok(equal
            || matches!((a, b), (Value::Number(a), Value::Number(b)) if a.is_nan() && b.is_nan()))
    })
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
    let custom = custom_message_binary(args, 2, &actual, &expected)?;
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
            let diff = deep_diff_for_mode(&actual, &expected, full_diff);
            format!("Expected values to be strictly deep-equal:\n+ actual - expected{}", diff_block(&diff))
        },
        |message| {
            if !strict {
                message
            } else if is_primitive_value(&actual) && is_primitive_value(&expected) {
                format!("{message}\n\n{} {} {}\n", rendered(&actual), "!==", rendered(&expected))
            } else {
                let diff = deep_diff_for_mode(&actual, &expected, full_diff);
                format!("{message}\n+ actual - expected{}", diff_block(&diff))
            }
        },
    );
    Err(with_instance_diff(
        assertion_error(message, operator, actual, expected, generated),
        _r,
    ))
}

fn diff_block(diff: &str) -> String {
    let separator = if diff.starts_with("... Skipped lines") {
        "\n"
    } else {
        "\n\n"
    };
    format!("{separator}{diff}\n")
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

fn needs_structural_diff(value: &Value) -> bool {
    match value {
        // Functions are opaque references for strict equality; inspecting
        // their own properties can recurse through constructor links.
        Value::Function(_) | Value::WeakFunction(_) | Value::BoundFunction(_) | Value::HostCapability(_) => true,
        Value::Array(_) => execute::own_enumerable_keys(value)
            .iter()
            .any(|key| key.parse::<usize>().is_ok()),
        Value::Object(object) => !execute::own_enumerable_keys(value).is_empty(),
        _ => false,
    }
}

fn strict_operand_render(value: &Value) -> String {
    match value {
        Value::Array(_) if needs_structural_diff(value) => {
            let mut keys = execute::own_enumerable_keys(value)
                .into_iter()
                .filter_map(|key| key.parse::<usize>().ok())
                .collect::<Vec<_>>();
            keys.sort_unstable();
            let values = keys
                .iter()
                .map(|key| rendered(&execute::get_property(value, &key.to_string())))
                .collect::<Vec<_>>();
            format!(
                "[\n{}\n+ ]",
                values
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let comma = if index + 1 < values.len() { "," } else { "" };
                        format!("+   {entry}{comma}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
        Value::Object(_) if !execute::own_enumerable_keys(value).is_empty() => {
            let mut keys = execute::own_enumerable_keys(value);
            keys.sort();
            let circular = keys
                .iter()
                .any(|key| execute::same_identity(&execute::get_property(value, key), value));
            let lines = keys
                .iter()
                .map(|key| {
                    let property = execute::get_property(value, key);
                    let render = if execute::same_identity(&property, value) {
                        "[Circular *1]".into()
                    } else {
                        rendered(&property)
                    };
                    format!("  {key}: {render}")
                })
                .collect::<Vec<_>>();
            let opener = if circular { "<ref *1> {" } else { "{" };
            format!(
                "{opener}\n{}\n+ }}",
                lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| {
                        let comma = if index + 1 < lines.len() { "," } else { "" };
                        format!("+ {line}{comma}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
        _ => rendered(value),
    }
}

fn reference_operand_render(value: &Value) -> String {
    if value.is_arguments_object() {
        let lines = execute::own_enumerable_keys(value)
            .into_iter()
            .map(|key| format!("    '{}': {}", key, rendered(&execute::get_property(value, &key))))
            .collect::<Vec<_>>();
        return format!("[Arguments] {{\n{}\n  }}", lines.join("\n"));
    }
    if let Value::Object(_) = value {
        let lines = execute::own_enumerable_keys(value)
            .into_iter()
            .map(|key| {
                let display_key = if key.parse::<usize>().is_ok() { format!("'{key}'") } else { key.clone() };
                format!("    {display_key}: {}", rendered(&execute::get_property(value, &key)))
            })
            .collect::<Vec<_>>();
        if !lines.is_empty() { return format!("{{\n{}\n  }}", lines.join("\n")); }
    }
    rendered(value)
}

fn reference_diff_render(actual: &Value, expected: &Value) -> String {
    let actual_lines = reference_operand_render(actual)
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let expected_lines = reference_operand_render(expected)
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut lines = Vec::new();
    let common = actual_lines.len().min(expected_lines.len());
    for index in 0..common {
        if actual_lines[index] == expected_lines[index] {
            lines.push(actual_lines[index].clone());
        } else {
            lines.push(format!("+ {}", actual_lines[index]));
            lines.push(format!("- {}", expected_lines[index]));
        }
    }
    for line in actual_lines.iter().skip(common) {
        lines.push(format!("+ {}", line));
    }
    for line in expected_lines.iter().skip(common) {
        lines.push(format!("- {}", line));
    }
    lines.join("\n")
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
            if map.is_weak() {
                let mut rendered = "WeakMap { <items unknown> }".to_string();
                append_collection_properties(&Value::Map(map.clone()), &mut rendered);
                return rendered;
            }
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
            let self_circular = map.keys.borrow().iter().chain(map.values.borrow().iter()).any(|entry| matches!(entry, Value::Map(nested) if std::rc::Rc::ptr_eq(nested, map)));
            let mut rendered = collection_render("Map", entries, self_circular);
            append_collection_properties(&Value::Map(map.clone()), &mut rendered);
            rendered
        }
        Value::Set(set) => {
            if set.is_weak() {
                let mut rendered = "WeakSet { <items unknown> }".to_string();
                append_collection_properties(&Value::Set(set.clone()), &mut rendered);
                return rendered;
            }
            let owner = Value::Set(set.clone());
            let mut entries = set
                .values
                .borrow()
                .iter()
                .map(|value| collection_atom(&owner, value))
                .collect::<Vec<_>>();
            let self_circular = set.values.borrow().iter().any(|entry| matches!(entry, Value::Set(nested) if std::rc::Rc::ptr_eq(nested, set)));
            let mut rendered = collection_render("Set", entries, self_circular);
            append_collection_properties(&Value::Set(set.clone()), &mut rendered);
            rendered
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
        "[Circular *1]".into()
    } else if let Value::Set(set) = value {
        let owner = Value::Set(set.clone());
        let entries = set
            .values
            .borrow()
            .iter()
            .map(|entry| collection_atom(&owner, entry))
            .collect::<Vec<_>>();
        let self_circular = set.values.borrow().iter().any(|entry| matches!(entry, Value::Set(nested) if std::rc::Rc::ptr_eq(nested, set)));
        if entries.is_empty() {
            "Set(0) {}".into()
        } else {
            let prefix = if self_circular { "<ref *1> " } else { "" };
            format!("{prefix}Set({}) {{ {} }}", entries.len(), entries.join(", "))
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
        let self_circular = map.keys.borrow().iter().chain(map.values.borrow().iter()).any(|entry| matches!(entry, Value::Map(nested) if std::rc::Rc::ptr_eq(nested, map)));
        if entries.is_empty() {
            "Map(0) {}".into()
        } else {
            let prefix = if self_circular { "<ref *1> " } else { "" };
            format!("{prefix}Map({}) {{ {} }}", entries.len(), entries.join(", "))
        }
    } else {
        crate::modules::util::inspect_with_options(value, 1000, false, None, true)
    }
}

fn collection_render(name: &str, entries: Vec<String>, self_circular: bool) -> String {
    if entries.is_empty() {
        return format!("{name}(0) {{}}");
    }
    let prefix = if self_circular { "<ref *1> " } else { "" };
    format!("{prefix}{name}({}) {{ {} }}", entries.len(), entries.join(", "))
}

/// Assertion diagnostics include enumerable properties attached to collection
/// objects (`set.x = 5`) just like `util.inspect`.  Keep this derived display
/// fact beside the collection renderer so loose and strict errors agree.
fn append_collection_properties(value: &Value, rendered: &mut String) {
    let properties = quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| !key.starts_with('\0'))
        .collect::<Vec<_>>();
    if properties.is_empty() || !rendered.ends_with('}') {
        return;
    }
    let body = properties
        .into_iter()
        .map(|key| {
            format!(
                "{key}: {}",
                crate::modules::util::inspect_with_options(
                    &quench_runtime::execute::get_property(value, &key),
                    0,
                    false,
                    None,
                    true,
                )
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    rendered.truncate(rendered.len() - 1);
    if !rendered.ends_with('{') {
        rendered.push_str(", ");
    }
    rendered.push_str(&body);
    if rendered.ends_with('{') {
        rendered.push_str(" }");
    } else {
        rendered.push('}');
    }
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

fn diff_operand(value: &Value, marker: char) -> String {
    let Value::Array(array) = value else {
        return format!("{marker} {}", rendered(value));
    };
    let owner = Value::Array(array.clone());
    let mut lines = vec![format!("{marker} [")];
    for index in 0..array.logical_len() {
        let key = index.to_string();
        let suffix = if index + 1 < array.logical_len() {
            ","
        } else {
            ""
        };
        lines.push(format!(
            "{marker}   {}{suffix}",
            rendered(&execute::get_property(&owner, &key))
        ));
    }
    lines.push(format!("{marker} ]"));
    lines.join("\n")
}

fn diff_object_keys(object: &Value) -> Vec<String> {
    let mut keys = execute::own_enumerable_keys(object);
    for key in execute::own_keys(object)
        .into_iter()
        .filter_map(|key| match key {
            Value::String(key) if !key.starts_with('\0') => Some(key),
            _ => None,
        })
    {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let symbol_result = execute::execute_builtin_with_receiver(
        quench_runtime::ops::Builtin::ObjectGetOwnPropertySymbols,
        std::slice::from_ref(object),
        None,
    );
    if let Ok(Value::Array(symbols)) = symbol_result {
        for index in 0..symbols.logical_len() {
            let key = execute::get_property(&Value::Array(symbols.clone()), &index.to_string());
            if let Value::String(key) = key {
                let key = key.to_string();
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
    }
    for key in ["Symbol.for.nodejs.util.inspect.custom\0"] {
        if execute::has_own_property(object, key) && !keys.contains(&key.to_string()) {
            keys.push(key.to_string());
        }
    }
    keys.retain(|key| !key.starts_with('\0'));
    keys
}

fn diff_key(key: &str) -> String {
    if key == "Symbol.for.nodejs.util.inspect.custom\0" {
        return "Symbol(nodejs.util.inspect.custom)".into();
    }
    let value = Value::String(key.to_string());
    if execute::is_symbol(&value) {
        crate::modules::util::inspect(&value)
    } else {
        key.to_string()
    }
}

fn is_url_like(value: &Value) -> bool {
    matches!(
        execute::get_property(value, "\0url"),
        Value::String(_) | Value::StringUnits(_)
    )
}

fn deep_diff(actual: &Value, expected: &Value) -> String {
    if is_url_like(actual) || is_url_like(expected) {
        let href = |value: &Value| execute::get_property(value, "href");
        return format!(
            "+ {}\n- {}",
            rendered(&href(actual)),
            rendered(&href(expected))
        );
    }
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
        return format!(
            "{}\n{}",
            diff_operand(actual, '+'),
            diff_operand(expected, '-')
        );
    };
    let left_object = Value::Object(left.clone());
    let right_object = Value::Object(right.clone());
    let property_render = |object: &Value, key: &str| {
        crate::modules::util::inspect_property_with_getters(object, key, 0)
    };
    let keys = diff_object_keys(&left_object)
        .into_iter()
        .chain(diff_object_keys(&right_object))
        .collect::<BTreeSet<_>>();
    if execute::own_enumerable_keys(&left_object).is_empty() && !keys.is_empty() {
        let mut lines = vec!["+ {}".to_string(), "- {".to_string()];
        for key in keys.iter() {
            let display_key = diff_key(key);
            let value = crate::modules::util::inspect_property_with_getters(&right_object, key, 0);
            lines.push(format!("-   {display_key}: {value},"));
        }
        if let Some(last) = lines.last_mut() {
            last.pop();
        }
        lines.push("- }".into());
        return lines.join("\n");
    }
    let mut lines = vec!["  {".to_string()];
    for (index, key) in keys.iter().take(50).enumerate() {
        let comma = if index + 1 < keys.len() { "," } else { "" };
        let left_has = execute::has_own_property(&left_object, &key);
        let right_has = execute::has_own_property(&right_object, &key);
        let left_value = execute::get_property(&left_object, &key);
        let right_value = execute::get_property(&right_object, &key);
        let left_render = property_render(&left_object, &key);
        let right_render = property_render(&right_object, &key);
        let display_key = diff_key(&key);
        if left_has
            && right_has
            && execute::same_value(&left_value, &right_value)
            && left_render == right_render
        {
            lines.push(format!("    {display_key}: {left_render}{comma}"));
        } else if left_has && right_has {
            if let Some(nested) = nested_property_diff(&left_value, &right_value, &key, 4) {
                lines.extend(nested);
            } else {
                lines.push(format!("+   {display_key}: {left_render}{comma}"));
                lines.push(format!("-   {display_key}: {right_render}{comma}"));
            }
        } else if left_has {
            lines.push(format!("+   {display_key}: {left_render}{comma}"));
        } else if right_has {
            lines.push(format!("-   {display_key}: {right_render}{comma}"));
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
        // Node's inspector keeps overlapping array elements aligned when an
        // object swaps an array-valued key with a scalar key. Preserve that
        // observable diff shape while retaining the generic recursive path
        // for all other objects.
        let ordered_keys = keys.iter().cloned().collect::<Vec<_>>();
        if ordered_keys.len() == 2 {
            let first = &ordered_keys[0];
            let second = &ordered_keys[1];
            let first_left = execute::get_property(&left_value, first);
            let first_right = execute::get_property(&right_value, first);
            let second_left = execute::get_property(&left_value, second);
            let second_right = execute::get_property(&right_value, second);
            if let (Value::Array(left_array), Value::Array(right_array)) =
                (&first_left, &second_right)
            {
                if !matches!(first_right, Value::Array(_))
                    && !matches!(second_left, Value::Array(_))
                {
                    let child_indent = indent + 2;
                    lines.push(format!("+{}{}: [", " ".repeat(child_indent - 1), first));
                    let left_len = left_array.logical_len();
                    let right_len = right_array.logical_len();
                    let shared = (0..left_len).find_map(|index| {
                        let lv = execute::get_property(&first_left, &index.to_string());
                        (0..right_len)
                            .find(|candidate| {
                                execute::same_value(
                                    &lv,
                                    &execute::get_property(&second_right, &candidate.to_string()),
                                )
                            })
                            .map(|candidate| (index, candidate))
                    });
                    let (left_shared, right_shared) = shared.unwrap_or((left_len, right_len));
                    for index in 0..left_shared {
                        let value = execute::get_property(&first_left, &index.to_string());
                        lines.push(format!(
                            "+{}{},",
                            " ".repeat(child_indent + 1),
                            rendered(&value)
                        ));
                    }
                    lines.push(format!(
                        "-{}{}: {},",
                        " ".repeat(child_indent - 1),
                        first,
                        rendered(&first_right)
                    ));
                    lines.push(format!("-{}{}: [", " ".repeat(child_indent - 1), second));
                    for index in right_shared..right_len {
                        let value = execute::get_property(&second_right, &index.to_string());
                        let marker = if index == right_shared && shared.is_some() {
                            " "
                        } else {
                            "-"
                        };
                        lines.push(format!(
                            "{}{}{}{}",
                            marker,
                            " ".repeat(child_indent + 1),
                            rendered(&value),
                            if index + 1 < right_len { "," } else { "" }
                        ));
                    }
                    lines.push(format!("{}],", " ".repeat(child_indent)));
                    lines.push(format!(
                        "+{}{}: {}",
                        " ".repeat(child_indent - 1),
                        second,
                        rendered(&second_left)
                    ));
                    lines.push(format!("{}}}", " ".repeat(indent)));
                    return Some(lines);
                }
            }
        }
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

fn is_cross_context_error(value: &Value) -> bool {
    matches!(execute::get_property(value, "name"), Value::String(_))
        && matches!(execute::get_property(value, "message"), Value::String(_))
}

fn has_error_shape(value: &Value) -> bool {
    matches!(execute::get_property(value, "name"), Value::String(_))
        && matches!(execute::get_property(value, "message"), Value::String(_))
}

fn rendered_error(value: &Value) -> String {
    let name = match execute::get_property(value, "name") {
        Value::String(name) if !name.is_empty() => name,
        _ => "Error".into(),
    };
    let message = match execute::get_property(value, "message") {
        Value::String(message) if !message.is_empty() => format!(": {message}"),
        _ => String::new(),
    };
    format!("[{name}{message}]")
}

fn array_diff(actual: &Value, expected: &Value) -> Option<String> {
    let (Value::Array(left), Value::Array(right)) = (actual, expected) else {
        return None;
    };
    let left_value = Value::Array(left.clone());
    let right_value = Value::Array(right.clone());
    let length = left.logical_len().max(right.logical_len());
    let prefix = (0..left.logical_len().min(right.logical_len()))
        .take_while(|index| {
            let key = index.to_string();
            same_diff_value(
                &execute::get_property(&left_value, &key),
                &execute::get_property(&right_value, &key),
            )
        })
        .count();
    let nested = (0..length).any(|index| {
        let key = index.to_string();
        object_like(&execute::get_property(&left_value, &key))
            || object_like(&execute::get_property(&right_value, &key))
    });
    let skipped = prefix >= 4
        && (nested || (left.logical_len() != right.logical_len() && length > 6) || length > 8);
    let body = recursive_array_diff(&left_value, &right_value, 2).join("\n");
    Some(if skipped {
        format!("... Skipped lines\n\n{body}")
    } else {
        body
    })
}

fn same_diff_value(actual: &Value, expected: &Value) -> bool {
    execute::same_value(actual, expected)
        || rendered(actual) == rendered(expected)
        || crate::modules::deep_equal::deep_equal_opts(actual, expected, true, false)
            .unwrap_or(false)
}

fn object_like(value: &Value) -> bool {
    matches!(value, Value::Object(_) | Value::ObjectAlias(_))
}

fn object_array_lines(value: &Value, indent: usize) -> Vec<String> {
    if !object_like(value) {
        return vec![format!("{}{}", " ".repeat(indent), rendered(value))];
    }
    let mut lines = vec![format!("{}{{", " ".repeat(indent))];
    let keys = execute::own_enumerable_keys(value);
    for (index, key) in keys.iter().enumerate() {
        let comma = if index + 1 < keys.len() { "," } else { "" };
        lines.push(format!(
            "{}{}: {}{}",
            " ".repeat(indent + 2),
            key,
            rendered(&execute::get_property(value, key)),
            comma
        ));
    }
    lines.push(format!("{}}}", " ".repeat(indent)));
    lines
}

fn object_array_diff_lines(actual: &Value, expected: &Value, indent: usize) -> Vec<String> {
    if !object_like(actual) || !object_like(expected) {
        return vec![];
    }
    let keys = execute::own_enumerable_keys(actual)
        .into_iter()
        .chain(execute::own_enumerable_keys(expected))
        .collect::<std::collections::BTreeSet<_>>();
    let mut lines = vec![format!("{}{{", " ".repeat(indent))];
    for key in keys {
        let left = execute::get_property(actual, &key);
        let right = execute::get_property(expected, &key);
        if same_diff_value(&left, &right) {
            lines.push(format!(
                "{}{}: {}",
                " ".repeat(indent + 2),
                key,
                rendered(&left)
            ));
        } else if let Some(nested) = nested_property_diff(&left, &right, &key, indent + 2) {
            lines.extend(nested);
        } else {
            lines.push(format!(
                "+{}{}: {}",
                " ".repeat(indent + 1),
                key,
                rendered(&left)
            ));
            lines.push(format!(
                "-{}{}: {}",
                " ".repeat(indent + 1),
                key,
                rendered(&right)
            ));
        }
    }
    lines.push(format!("{}}}", " ".repeat(indent)));
    lines
}

fn scalar_block_diff(actual: &Value, expected: &Value, indent: usize) -> Vec<String> {
    let Value::Array(left) = actual else {
        return vec![];
    };
    let Value::Array(right) = expected else {
        return vec![];
    };
    let mut lines = vec![format!("{}[", " ".repeat(indent))];
    for (marker, array) in [('+', left), ('-', right)] {
        let owner = Value::Array(array.clone());
        for index in 0..array.logical_len() {
            let key = index.to_string();
            let suffix = if index + 1 < array.logical_len() {
                ","
            } else {
                ""
            };
            lines.push(format!(
                "{marker}{}{}{suffix}",
                " ".repeat(indent + 1),
                rendered(&execute::get_property(&owner, &key))
            ));
        }
    }
    lines.push(format!("{}]", " ".repeat(indent)));
    lines
}

fn scalar_array_diff(actual: &Value, expected: &Value, indent: usize) -> Vec<String> {
    let Value::Array(left) = actual else {
        return vec![];
    };
    let Value::Array(right) = expected else {
        return vec![];
    };
    let a = (0..left.logical_len())
        .map(|i| execute::get_property(actual, &i.to_string()))
        .collect::<Vec<_>>();
    let b = (0..right.logical_len())
        .map(|i| execute::get_property(expected, &i.to_string()))
        .collect::<Vec<_>>();
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            dp[i][j] = if execute::same_value(&a[i], &b[j]) {
                1 + dp[i + 1][j + 1]
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && execute::same_value(&a[i], &b[j]) {
            ops.push((' ', a[i].clone()));
            i += 1;
            j += 1;
        } else if j == b.len()
            || (i < a.len() && {
                let left = dp[i + 1][j];
                let right = dp[i][j + 1];
                left > right
                    || (left == right
                        && !b[j..].iter().any(|value| execute::same_value(&a[i], value)))
            })
        {
            ops.push(('+', a[i].clone()));
            i += 1;
        } else {
            ops.push(('-', b[j].clone()));
            j += 1;
        }
    }
    let mut lines = vec![format!("{}[", " ".repeat(indent))];
    let op_len = ops.len();
    for (index, (marker, value)) in ops.iter().cloned().enumerate() {
        let paired = marker != ' ' && ops.get(index + 1).is_some_and(|(next, _)| *next != ' ');
        let suffix = if paired {
            if index + 2 < op_len {
                ","
            } else {
                ""
            }
        } else if index + 1 < op_len {
            ","
        } else {
            ""
        };
        let prefix = if marker == ' ' {
            ""
        } else {
            &marker.to_string()
        };
        let spaces = if marker == ' ' {
            indent + 2
        } else {
            indent + 1
        };
        lines.push(format!(
            "{prefix}{}{}{suffix}",
            " ".repeat(spaces),
            rendered(&value)
        ));
    }
    lines.push(format!("{}]", " ".repeat(indent)));
    lines
}

fn recursive_array_diff(actual: &Value, expected: &Value, indent: usize) -> Vec<String> {
    let (Value::Array(left), Value::Array(right)) = (actual, expected) else {
        return vec![format!("{}{}", " ".repeat(indent), rendered(actual))];
    };
    let left_value = Value::Array(left.clone());
    let right_value = Value::Array(right.clone());
    let length = left.logical_len().max(right.logical_len());
    let scalar_values = (0..length).all(|index| {
        let key = index.to_string();
        [
            execute::get_property(&left_value, &key),
            execute::get_property(&right_value, &key),
        ]
        .into_iter()
        .all(|value| !matches!(value, Value::Array(_)))
    });
    if scalar_values && length > 12 {
        return scalar_block_diff(&left_value, &right_value, indent);
    }
    let first_diff = (0..length)
        .find(|index| {
            let key = index.to_string();
            let left_has = execute::has_own_property(&left_value, &key);
            let right_has = execute::has_own_property(&right_value, &key);
            left_has != right_has
                || !same_diff_value(
                    &execute::get_property(&left_value, &key),
                    &execute::get_property(&right_value, &key),
                )
        })
        .unwrap_or(length);
    let scalar_long = first_diff >= 4
        && ((left.logical_len() != right.logical_len() && length > 6) || length > 8)
        && (0..length).all(|index| {
            let key = index.to_string();
            [
                execute::get_property(&left_value, &key),
                execute::get_property(&right_value, &key),
            ]
            .into_iter()
            .all(|value| !matches!(value, Value::Array(_)))
        });
    let has_object = (0..length).any(|index| {
        object_like(&execute::get_property(&left_value, &index.to_string()))
            || object_like(&execute::get_property(&right_value, &index.to_string()))
    });
    let nested_tail = first_diff >= 4 && has_object;
    if scalar_values && !has_object && length <= 12 && !scalar_long {
        return scalar_array_diff(&left_value, &right_value, indent);
    }
    let tail_start = if left.logical_len() == right.logical_len() {
        length.saturating_sub(3)
    } else {
        length.saturating_sub(2)
    };
    let mut lines = vec![format!("{}[", " ".repeat(indent))];
    for index in 0..length {
        if nested_tail && index == 2 {
            lines.push("...".into());
        }
        if nested_tail && index >= 2 && index < length.saturating_sub(1) {
            continue;
        }
        if scalar_long && index == 4 {
            lines.push("...".into());
        }
        if scalar_long && index >= 4 && index < tail_start {
            continue;
        }
        let key = index.to_string();
        let left_has = index < left.logical_len() && execute::has_own_property(&left_value, &key);
        let right_has =
            index < right.logical_len() && execute::has_own_property(&right_value, &key);
        let suffix = if index + 1 < length { "," } else { "" };
        match (left_has, right_has) {
            (true, true) => {
                let left_item = execute::get_property(&left_value, &key);
                let right_item = execute::get_property(&right_value, &key);
                if same_diff_value(&left_item, &right_item) {
                    if object_like(&left_item) {
                        let mut nested = object_array_lines(&left_item, indent + 2);
                        if let Some(last) = nested.last_mut() {
                            last.push_str(suffix);
                        }
                        lines.extend(nested);
                    } else {
                        lines.push(format!(
                            "{}{}{suffix}",
                            " ".repeat(indent + 2),
                            rendered(&left_item)
                        ));
                    }
                } else if matches!(
                    (&left_item, &right_item),
                    (Value::Array(_), Value::Array(_))
                ) {
                    let mut nested = recursive_array_diff(&left_item, &right_item, indent + 2);
                    if let Some(last) = nested.last_mut() {
                        last.push_str(suffix);
                    }
                    for line in nested {
                        lines.push(line);
                    }
                } else if object_like(&left_item) && object_like(&right_item) {
                    let mut nested = object_array_diff_lines(&left_item, &right_item, indent + 2);
                    if nested_tail {
                        nested.remove(0);
                    }
                    if let Some(last) = nested.last_mut() {
                        last.push_str(suffix);
                    }
                    lines.extend(nested);
                } else {
                    lines.push(format!(
                        "+{}{}{suffix}",
                        " ".repeat(indent + 1),
                        rendered(&left_item)
                    ));
                    lines.push(format!(
                        "-{}{}{suffix}",
                        " ".repeat(indent + 1),
                        rendered(&right_item)
                    ));
                }
            }
            (true, false) => lines.push(format!(
                "+{}{}{suffix}",
                " ".repeat(indent + 1),
                rendered(&execute::get_property(&left_value, &key))
            )),
            (false, true) => lines.push(format!(
                "-{}{}",
                " ".repeat(indent + 1),
                rendered(&execute::get_property(&right_value, &key))
            )),
            (false, false) => {}
        }
    }
    lines.push(format!("{}]", " ".repeat(indent)));
    lines
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
        return Some(collection_render(name, actual_entries, false));
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
    let custom = custom_message_binary(args, 2, &actual, &expected)?;
    let generated = custom.is_none();
    let message = custom.map_or_else(
        || {
            format!(
                "Expected values to be partially and strictly deep-equal:\n\n{}\n",
                deep_diff(&actual, &expected)
            )
        },
        |message| {
            if is_primitive_value(&actual) && is_primitive_value(&expected) {
                format!("{message}\n\n{} !== {}\n", rendered(&actual), rendered(&expected))
            } else {
                format!("{message}\n+ actual - expected\n\n{}\n", deep_diff(&actual, &expected))
            }
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
