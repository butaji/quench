//! The `assert` module (`lib/assert.js`, `lib/internal/assert/assertion_error.js`).
//!
//! `require('assert')` is the `ok` function carrying every assertion; `assert.strict` is the
//! `strict` function with the strict comparisons under the loose names. Failures throw an
//! `AssertionError` whose generated message follows Node's: a short `actual !== expected`
//! line or a line diff of the values inspected one entry per line, the falsy call's own
//! source for `ok`, and the throws/rejects/match families' explanations.
use super::console_format_shared_vm::{self as format, Layout, Options};
use super::util_comparisons_shared_vm::{self as comparisons, Mode};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;

const MODULE_KEY: &str = "\0builtin:assert/module";
const ERROR_CLASS_KEY: &str = "\0builtin:assert/AssertionError";
const ERROR_CODE: &str = "ERR_ASSERTION";
const ERROR_NAME: &str = "AssertionError";
/// `getSimpleDiff`'s bound on the inspected values' combined length.
const MAX_SHORT_LENGTH: usize = 12;
/// A `notStrictEqual` value inspected longer than this goes on its own line.
const MAX_INLINE_UNEQUAL: usize = 5;
/// The loose messages truncate each inspected value to this length, with `...`.
const MAX_LOOSE_LENGTH: usize = 512;
const MAX_LOOSE_SAME_LENGTH: usize = 1024;
const TRUNCATION: &str = "...";
const DIFF_HEADER: &str = "+ actual - expected";
/// `inspectValue`'s depth.
const INSPECT_DEPTH: f64 = 1000.0;
const DEFAULT_DIFF: &str = "simple";

/// Every assertion function; its function data is its position in `ALL`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Assertion {
    Ok,
    Fail,
    Equal,
    NotEqual,
    DeepEqual,
    NotDeepEqual,
    DeepStrictEqual,
    NotDeepStrictEqual,
    StrictEqual,
    NotStrictEqual,
    PartialDeepStrictEqual,
    Match,
    DoesNotMatch,
    Throws,
    Rejects,
    DoesNotThrow,
    DoesNotReject,
    IfError,
    Strict,
}

impl Assertion {
    const ALL: [Assertion; 19] = [
        Assertion::Ok,
        Assertion::Fail,
        Assertion::Equal,
        Assertion::NotEqual,
        Assertion::DeepEqual,
        Assertion::NotDeepEqual,
        Assertion::DeepStrictEqual,
        Assertion::NotDeepStrictEqual,
        Assertion::StrictEqual,
        Assertion::NotStrictEqual,
        Assertion::PartialDeepStrictEqual,
        Assertion::Match,
        Assertion::DoesNotMatch,
        Assertion::Throws,
        Assertion::Rejects,
        Assertion::DoesNotThrow,
        Assertion::DoesNotReject,
        Assertion::IfError,
        Assertion::Strict,
    ];

    /// The function's name, also the operator its failures report.
    fn name(self) -> &'static str {
        match self {
            Assertion::Ok => "ok",
            Assertion::Fail => "fail",
            Assertion::Equal => "equal",
            Assertion::NotEqual => "notEqual",
            Assertion::DeepEqual => "deepEqual",
            Assertion::NotDeepEqual => "notDeepEqual",
            Assertion::DeepStrictEqual => "deepStrictEqual",
            Assertion::NotDeepStrictEqual => "notDeepStrictEqual",
            Assertion::StrictEqual => "strictEqual",
            Assertion::NotStrictEqual => "notStrictEqual",
            Assertion::PartialDeepStrictEqual => "partialDeepStrictEqual",
            Assertion::Match => "match",
            Assertion::DoesNotMatch => "doesNotMatch",
            Assertion::Throws => "throws",
            Assertion::Rejects => "rejects",
            Assertion::DoesNotThrow => "doesNotThrow",
            Assertion::DoesNotReject => "doesNotReject",
            Assertion::IfError => "ifError",
            Assertion::Strict => "strict",
        }
    }

    fn length(self) -> f64 {
        match self {
            Assertion::Ok | Assertion::Strict => 0.0,
            Assertion::Fail => 5.0,
            Assertion::Throws | Assertion::Rejects | Assertion::DoesNotThrow | Assertion::DoesNotReject | Assertion::IfError => 1.0,
            _ => 3.0,
        }
    }

    fn code(self) -> f64 {
        Self::ALL.iter().position(|assertion| *assertion == self).unwrap_or_default() as f64
    }
}

/// The module's members after `AssertionError`, in Node's order.
const MEMBERS: [Assertion; 18] = [
    Assertion::Ok,
    Assertion::Fail,
    Assertion::Equal,
    Assertion::NotEqual,
    Assertion::DeepEqual,
    Assertion::NotDeepEqual,
    Assertion::DeepStrictEqual,
    Assertion::NotDeepStrictEqual,
    Assertion::StrictEqual,
    Assertion::NotStrictEqual,
    Assertion::PartialDeepStrictEqual,
    Assertion::Match,
    Assertion::DoesNotMatch,
    Assertion::Throws,
    Assertion::Rejects,
    Assertion::DoesNotThrow,
    Assertion::DoesNotReject,
    Assertion::IfError,
];

/// `assert.strict`'s loose names and the strict assertions they become.
/// `Assert.prototype`'s methods, in Node's order; its `ok` is its own function.
const PROTOTYPE_MEMBERS: [Assertion; 18] = [
    Assertion::Fail,
    Assertion::Ok,
    Assertion::Equal,
    Assertion::NotEqual,
    Assertion::DeepEqual,
    Assertion::NotDeepEqual,
    Assertion::DeepStrictEqual,
    Assertion::NotDeepStrictEqual,
    Assertion::StrictEqual,
    Assertion::NotStrictEqual,
    Assertion::PartialDeepStrictEqual,
    Assertion::Throws,
    Assertion::Rejects,
    Assertion::DoesNotThrow,
    Assertion::DoesNotReject,
    Assertion::IfError,
    Assertion::Match,
    Assertion::DoesNotMatch,
];

/// The diffs `new Assert({ diff })` accepts.
const DIFFS: [&str; 2] = [DEFAULT_DIFF, "full"];
/// The description of `Assert` instances' options symbol (`kOptions`).
const OPTIONS_DESCRIPTION: &str = "options";
const OPTIONS_KEY: &str = "\0builtin:assert/kOptions";
const ASSERT_CLASS: &str = "Assert";

/// One assertion call: its function and the receiver's diff option
/// (`this?.[kOptions]?.diff`, undefined for a detached call).
#[derive(Clone, Copy)]
struct Invocation {
    assertion: Assertion,
    diff: RootId,
}

const STRICT_ALIASES: [(&str, Assertion); 4] = [
    ("equal", Assertion::StrictEqual),
    ("deepEqual", Assertion::DeepStrictEqual),
    ("notEqual", Assertion::NotStrictEqual),
    ("notDeepEqual", Assertion::NotDeepStrictEqual),
];

/// `kReadableOperator`.
fn readable(operator: &str) -> Option<&'static str> {
    Some(match operator {
        "deepStrictEqual" => "Expected values to be strictly deep-equal:",
        "partialDeepStrictEqual" => "Expected values to be partially and strictly deep-equal:",
        "strictEqual" => "Expected values to be strictly equal:",
        "strictEqualObject" => "Expected \"actual\" to be reference-equal to \"expected\":",
        "deepEqual" => "Expected values to be loosely deep-equal:",
        "notDeepStrictEqual" => "Expected \"actual\" not to be strictly deep-equal to:",
        "notStrictEqual" => "Expected \"actual\" to be strictly unequal to:",
        "notStrictEqualObject" => "Expected \"actual\" not to be reference-equal to \"expected\":",
        "notDeepEqual" => "Expected \"actual\" not to be loosely deep-equal to:",
        "notIdentical" => "Values have same structure but are not reference-equal:",
        "notDeepEqualUnequal" => "Expected values not to be loosely deep-equal:",
        _ => return None,
    })
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn is_nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_null() || value.is_undefined())
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

/// `typeof value === 'object' && value !== null || typeof value === 'function'`.
fn is_object_like(c: &Context<'_>, value: RootId) -> bool {
    matches!(c.value_type_rooted(value), Some(ValueType::Object | ValueType::Function))
}

fn cached(c: &mut Context<'_>, key: &str) -> Option<RootId> {
    c.host_mut().shared_state().borrow().builtin_cache.get(key).copied()
}

fn cache(c: &mut Context<'_>, key: &str, value: RootId) -> Result<(), RootedError> {
    let retained = c.retain(value)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(key.into(), retained);
    Ok(())
}

fn assertion_function(c: &mut Context<'_>, assertion: Assertion) -> Result<RootId, RootedError> {
    let data = c.number(assertion.code());
    let function = c.host_function_named_with_data(crate::host::shared_vm::operation("assertCall"), assertion.name(), data)?;
    let length = c.number(assertion.length());
    c.define_named_data_property_rooted(function, "length", length, false, false, true)?;
    Ok(function)
}

/// The `assert` module, built once.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(module) = cached(c, MODULE_KEY) {
        return Ok(module);
    }
    let error_class = error_class(c)?;
    let mut functions = Vec::with_capacity(MEMBERS.len());
    for assertion in MEMBERS {
        functions.push((assertion, assertion_function(c, assertion)?));
    }
    let module = functions[0].1;
    let class = assert_class(c, &functions)?;
    put(c, module, "AssertionError", error_class)?;
    for &(assertion, function) in &functions {
        put(c, module, assertion.name(), function)?;
    }
    let strict = assertion_function(c, Assertion::Strict)?;
    put(c, strict, "AssertionError", error_class)?;
    for &(assertion, function) in &functions {
        let alias = STRICT_ALIASES.iter().find(|(name, _)| *name == assertion.name()).map(|&(_, target)| target);
        let function = match alias {
            Some(target) => functions.iter().find(|(candidate, _)| *candidate == target).map(|&(_, function)| function).unwrap_or(function),
            None => function,
        };
        put(c, strict, assertion.name(), function)?;
    }
    put(c, strict, ASSERT_CLASS, class)?;
    put(c, strict, "strict", strict)?;
    put(c, module, "strict", strict)?;
    put(c, module, ASSERT_CLASS, class)?;
    cache(c, MODULE_KEY, module)?;
    Ok(module)
}

/// `Assert`, whose prototype carries the module's assertions (and an `ok` of its own).
fn assert_class(c: &mut Context<'_>, functions: &[(Assertion, RootId)]) -> Result<RootId, RootedError> {
    let class = c.host_constructor_named(crate::host::shared_vm::operation("assertAssert"), ASSERT_CLASS)?;
    let length = c.number(1.0);
    c.define_named_data_property_rooted(class, "length", length, false, false, true)?;
    let prototype = get(c, class, "prototype")?;
    for assertion in PROTOTYPE_MEMBERS {
        let function = match functions.iter().find(|(candidate, _)| *candidate == assertion && assertion != Assertion::Ok) {
            Some(&(_, function)) => function,
            None => assertion_function(c, assertion)?,
        };
        c.define_named_data_property_rooted(prototype, assertion.name(), function, true, false, true)?;
    }
    Ok(class)
}

fn options_symbol(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(symbol) = cached(c, OPTIONS_KEY) {
        return Ok(symbol);
    }
    let symbol = c.symbol_rooted(Some(OPTIONS_DESCRIPTION));
    cache(c, OPTIONS_KEY, symbol)?;
    Ok(symbol)
}

/// `new Assert(options)`: the options over `{ strict: true }`, a valid `diff`, and with
/// `strict` the loose assertions replaced by the strict ones.
pub(crate) fn assert_constructor(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let Some(target) = c.new_target_rooted() else {
        let error = c.type_error_rooted("Class constructor Assert cannot be invoked without `new`")?;
        super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_CONSTRUCT_CALL_REQUIRED")?;
        return Err(c.throw(error));
    };
    let options = c.null_object_rooted()?;
    let strict = c.boolean(true);
    put(c, options, "strict", strict)?;
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let assign = get(c, object, "assign")?;
    let given = argument(c, args, 0);
    c.call_rooted(assign, object, &[options, given])?;
    let diff = get(c, options, "diff")?;
    if !is_undefined(c, diff) && !DIFFS.iter().any(|name| c.string_text(diff).ok().flatten().as_deref() == Some(name)) {
        let allowed = DIFFS.map(|name| format!("'{name}'")).join(", ");
        return Err(super::util_shared_vm::argument_value_error(c, diff, "options.diff", &format!("must be one of: {allowed}"))?);
    }
    let prototype = get(c, target, "prototype")?;
    let instance = c.object_rooted()?;
    c.set_prototype_rooted(instance, prototype)?;
    let error_class = error_class(c)?;
    put(c, instance, "AssertionError", error_class)?;
    let symbol = options_symbol(c)?;
    c.define_data_property_rooted(instance, symbol, options, false, false, false)?;
    let strict = get(c, options, "strict")?;
    if c.truthy_rooted(strict)? {
        for (loose, target) in STRICT_ALIASES {
            let function = get(c, instance, target.name())?;
            put(c, instance, loose, function)?;
        }
    }
    Ok(instance)
}

/// The receiver's `diff` option, undefined unless it is an `Assert` given one.
fn receiver_diff(c: &mut Context<'_>, receiver: RootId) -> Result<RootId, RootedError> {
    if !c.is_object_rooted(receiver)? {
        return Ok(c.undefined());
    }
    let symbol = options_symbol(c)?;
    let options = c.get_property_rooted(receiver, symbol)?;
    if !c.is_object_rooted(options)? {
        return Ok(c.undefined());
    }
    get(c, options, "diff")
}

/// `assert/strict`.
pub(crate) fn strict_module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = module(c)?;
    get(c, module, "strict")
}

/// The `AssertionError` class, built once: a constructor over `Error` whose instances are
/// genuine errors.
fn error_class(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(class) = cached(c, ERROR_CLASS_KEY) {
        return Ok(class);
    }
    let class = c.host_constructor_named(crate::host::shared_vm::operation("assertAssertionError"), ERROR_NAME)?;
    let global = c.global_root()?;
    let error = get(c, global, "Error")?;
    c.set_prototype_rooted(class, error)?;
    let prototype = get(c, class, "prototype")?;
    let error_prototype = get(c, error, "prototype")?;
    c.set_prototype_rooted(prototype, error_prototype)?;
    let to_string = c.host_function_named(crate::host::shared_vm::operation("assertErrorToString"), "toString")?;
    c.define_named_data_property_rooted(prototype, "toString", to_string, true, false, true)?;
    c.define_named_data_property_rooted(class, "prototype", prototype, false, false, false)?;
    cache(c, ERROR_CLASS_KEY, class)?;
    Ok(class)
}

/// `AssertionError.prototype.toString`: `${name} [${code}]: ${message}`.
pub(crate) fn error_to_string(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let mut parts = Vec::new();
    for name in ["name", "code", "message"] {
        let value = get(c, receiver, name)?;
        parts.push(c.to_string(value)?);
    }
    Ok(c.string_rooted(&format!("{} [{}]: {}", parts[0], parts[1], parts[2])))
}

/// `new AssertionError({ message, actual, expected, operator, stackStartFn, diff })`.
pub(crate) fn error_constructor(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let options = argument(c, args, 0);
    if !c.is_object_rooted(options)? {
        return Err(super::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
    }
    let fields = ["message", "actual", "expected", "operator", "stackStartFn", "diff"].map(|name| get(c, options, name));
    let [message, actual, expected, operator, start, diff] = fields;
    let operator = operator?;
    let operator_text = if is_undefined(c, operator) { String::new() } else { c.to_string(operator)? };
    let class = error_class(c)?;
    let target = c.new_target_rooted().unwrap_or(class);
    let failure = Failure { actual: actual?, expected: expected?, operator, operator_text, message: message?, start: start?, diff: diff? };
    build_error(c, &failure, target)
}

/// What an `AssertionError` reports.
struct Failure {
    actual: RootId,
    expected: RootId,
    operator: RootId,
    operator_text: String,
    message: RootId,
    start: RootId,
    diff: RootId,
}

fn build_error(c: &mut Context<'_>, failure: &Failure, target: RootId) -> Result<RootId, RootedError> {
    let generated = is_nullish(c, failure.message);
    let text = if generated {
        let (actual, expected) = (failure.actual, failure.expected);
        let (actual, expected) = if is_error_with_stack(c, actual)? && is_error_with_stack(c, expected)? { (copy_error(c, actual)?, copy_error(c, expected)?) } else { (actual, expected) };
        generated_message(c, actual, expected, &failure.operator_text, None)?
    } else if matches!(failure.operator_text.as_str(), "deepStrictEqual" | "strictEqual" | "partialDeepStrictEqual") {
        let custom = c.to_string(failure.message)?;
        generated_message(c, failure.actual, failure.expected, &failure.operator_text, Some(custom))?
    } else {
        c.to_string(failure.message)?
    };
    let error = c.error_rooted(&text)?;
    let prototype = get(c, target, "prototype")?;
    c.set_prototype_rooted(error, prototype)?;
    let generated = c.boolean(generated);
    put(c, error, "generatedMessage", generated)?;
    let code = c.string_rooted(ERROR_CODE);
    put(c, error, "code", code)?;
    put(c, error, "actual", failure.actual)?;
    put(c, error, "expected", failure.expected)?;
    put(c, error, "operator", failure.operator)?;
    let diff = if is_undefined(c, failure.diff) { c.string_rooted(DEFAULT_DIFF) } else { failure.diff };
    put(c, error, "diff", diff)?;
    // The stack's header names the code, as Node's does, then the name is the class's.
    let coded = c.string_rooted(&format!("{ERROR_NAME} [{ERROR_CODE}]"));
    c.define_named_data_property_rooted(error, "name", coded, true, false, true)?;
    let global = c.global_root()?;
    let error_constructor = get(c, global, "Error")?;
    let capture = get(c, error_constructor, "captureStackTrace")?;
    if c.is_callable_rooted(failure.start)? {
        c.call_rooted(capture, error_constructor, &[error, failure.start])?;
    } else {
        c.call_rooted(capture, error_constructor, &[error])?;
    }
    get(c, error, "stack")?;
    let name = c.string_rooted(ERROR_NAME);
    c.define_named_data_property_rooted(error, "name", name, true, false, true)?;
    Ok(error)
}

/// An `Error` instance with a `stack`, whose message shows a stackless copy.
fn is_error_with_stack(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(false);
    }
    let stack = c.string_rooted("stack");
    if !c.has_property_rooted(value, stack)? {
        return Ok(false);
    }
    let global = c.global_root()?;
    let error = get(c, global, "Error")?;
    c.instance_of_rooted(value, error)
}

/// `copyError`: the error's prototype and own enumerable properties, its message and
/// (copied) cause as hidden own properties, and no stack.
fn copy_error(c: &mut Context<'_>, source: RootId) -> Result<RootId, RootedError> {
    let target = c.object_rooted()?;
    let prototype = c.prototype_rooted(source)?;
    c.set_prototype_rooted(target, prototype)?;
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let assign = get(c, object, "assign")?;
    c.call_rooted(assign, object, &[target, source])?;
    let message = get(c, source, "message")?;
    c.define_named_data_property_rooted(target, "message", message, false, false, false)?;
    if c.has_own_property_rooted(source, "cause")? {
        let cause = get(c, source, "cause")?;
        let cause = if super::util_inspect_shared_vm::is_error(c, cause)? { copy_error(c, cause)? } else { cause };
        c.define_named_data_property_rooted(target, "cause", cause, false, false, false)?;
    }
    Ok(target)
}

/// `inspectValue`: every container one entry per line, keys sorted, no custom inspection.
fn inspect_value(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let options = Options {
        depth: Some(INSPECT_DEPTH),
        max_array_length: usize::MAX,
        layout: Layout::Expanded,
        custom_inspect: false,
        show_hidden: false,
        sorted: true,
    };
    format::inspect(c, value, options)
}

/// The generated message of `operator`'s failure, or `custom`'s with the diff.
fn generated_message(c: &mut Context<'_>, actual: RootId, expected: RootId, operator: &str, custom: Option<String>) -> Result<String, RootedError> {
    match operator {
        "deepStrictEqual" | "strictEqual" | "partialDeepStrictEqual" => difference(c, actual, expected, operator, custom),
        "notDeepStrictEqual" | "notStrictEqual" => {
            let base = if operator == "notStrictEqual" && is_object_like(c, actual) { "notStrictEqualObject" } else { operator };
            let base = readable(base).unwrap_or_default();
            let inspected = inspect_value(c, actual)?;
            Ok(match inspected.lines().count() {
                0 | 1 => format!("{base}{}{inspected}", if inspected.len() > MAX_INLINE_UNEQUAL { "\n\n" } else { " " }),
                _ => format!("{base}\n\n{inspected}\n"),
            })
        }
        _ => {
            let mut actual_text = inspect_value(c, actual)?;
            let mut expected_text = inspect_value(c, expected)?;
            let known = readable(operator);
            if operator == "notDeepEqual" && actual_text == expected_text {
                let mut text = format!("{}\n\n{actual_text}", known.unwrap_or_default());
                truncate(&mut text, MAX_LOOSE_SAME_LENGTH);
                return Ok(text);
            }
            truncate(&mut actual_text, MAX_LOOSE_LENGTH);
            truncate(&mut expected_text, MAX_LOOSE_LENGTH);
            if operator == "deepEqual" {
                actual_text = format!("{}\n\n{actual_text}\n\nshould loosely deep-equal\n\n", known.unwrap_or_default());
            } else if let Some(unequal) = readable(&format!("{operator}Unequal")) {
                actual_text = format!("{unequal}\n\n{actual_text}\n\nshould not loosely deep-equal\n\n");
            } else {
                expected_text = format!(" {operator} {expected_text}");
            }
            Ok(format!("{actual_text}{expected_text}"))
        }
    }
}

fn truncate(text: &mut String, limit: usize) {
    if text.len() > limit {
        let cut = (0..=limit - TRUNCATION.len()).rev().find(|&index| text.is_char_boundary(index)).unwrap_or_default();
        text.truncate(cut);
        text.push_str(TRUNCATION);
    }
}

/// `createErrDiff`: a short `actual !== expected` line, or the line diff of the inspected
/// values under a `+ actual - expected` header.
fn difference(c: &mut Context<'_>, actual: RootId, expected: RootId, operator: &str, custom: Option<String>) -> Result<String, RootedError> {
    let operator = if operator == "strictEqual" && is_object_like(c, actual) && is_object_like(c, expected) { "strictEqualObject" } else { operator };
    let actual_text = inspect_value(c, actual)?;
    let expected_text = inspect_value(c, expected)?;
    let head = custom.unwrap_or_else(|| readable(operator).unwrap_or_default().to_owned());
    if actual_text == expected_text {
        let head = readable("notIdentical").unwrap_or_default();
        return Ok(format!("{head}\n\n{actual_text}\n"));
    }
    let actual_lines = actual_text.split('\n').collect::<Vec<_>>();
    let expected_lines = expected_text.split('\n').collect::<Vec<_>>();
    let simple = actual_lines.len() == 1 && expected_lines.len() == 1 && (!is_object_like(c, actual) || !is_object_like(c, expected));
    if simple {
        let mut length = actual_text.len() + expected_text.len();
        for value in [actual, expected] {
            if c.value_type_rooted(value) == Some(ValueType::String) {
                length -= 2;
            }
        }
        let both_zero = [actual, expected]
            .iter()
            .all(|&value| c.rooted_value(value).and_then(|value| value.as_number()) == Some(0.0));
        if length <= MAX_SHORT_LENGTH && !both_zero {
            return Ok(format!("{head}\n\n{actual_text} !== {expected_text}\n"));
        }
        return Ok(format!("{head}\n{DIFF_HEADER}\n\n+ {actual_text}\n- {expected_text}\n"));
    }
    // Object lines that differ only by a trailing comma are the same entry.
    let comma_blind = is_object_like(c, actual);
    let (lines, skipped) = print_diff(&line_diff(&actual_lines, &expected_lines, comma_blind));
    let skipped = if skipped { SKIPPED_HEADER } else { "" };
    Ok(format!("{head}\n{DIFF_HEADER}{skipped}\n\n{lines}\n"))
}

/// A diff line: common, only in the actual value, only in the expected one.
enum DiffLine<'a> {
    Common(&'a str),
    Actual(&'a str),
    Expected(&'a str),
}

const SKIPPED_HEADER: &str = "\n... Skipped lines";
/// `printMyersDiff`'s run of common lines written before the rest collapse.
const COMMON_LINES_SHOWN: usize = 5;

fn same_line(actual: &str, expected: &str, comma_blind: bool) -> bool {
    actual == expected || (comma_blind && (actual.strip_suffix(',') == Some(expected) || expected.strip_suffix(',') == Some(actual)))
}

/// A longest-common-subsequence diff of `actual` against `expected`, actual lines first in
/// a change. A common line differing by a comma is written with it.
fn line_diff<'a>(actual: &[&'a str], expected: &[&'a str], comma_blind: bool) -> Vec<DiffLine<'a>> {
    let (rows, columns) = (actual.len(), expected.len());
    let mut common = vec![vec![0_usize; columns + 1]; rows + 1];
    for row in (0..rows).rev() {
        for column in (0..columns).rev() {
            common[row][column] = if same_line(actual[row], expected[column], comma_blind) {
                common[row + 1][column + 1] + 1
            } else {
                common[row + 1][column].max(common[row][column + 1])
            };
        }
    }
    let (mut row, mut column) = (0, 0);
    let mut lines = Vec::with_capacity(rows + columns);
    while row < rows || column < columns {
        if row < rows && column < columns && same_line(actual[row], expected[column], comma_blind) {
            let line = if expected[column].len() > actual[row].len() { expected[column] } else { actual[row] };
            lines.push(DiffLine::Common(line));
            row += 1;
            column += 1;
        } else if row < rows && (column == columns || common[row + 1][column] >= common[row][column + 1]) {
            lines.push(DiffLine::Actual(actual[row]));
            row += 1;
        } else {
            lines.push(DiffLine::Expected(expected[column]));
            column += 1;
        }
    }
    lines
}

/// `printMyersDiff`: each run of common lines shows its first `COMMON_LINES_SHOWN`; before a
/// change, a longer run shows the lines just before it, or `...` and the last one. Returns
/// the text and whether lines were skipped.
fn print_diff(lines: &[DiffLine<'_>]) -> (String, bool) {
    let mut output = Vec::new();
    let mut skipped = false;
    let mut run: Vec<&str> = Vec::new();
    for line in lines {
        match line {
            DiffLine::Common(text) => {
                if run.len() < COMMON_LINES_SHOWN {
                    output.push(format!("  {text}"));
                }
                run.push(text);
            }
            DiffLine::Actual(text) | DiffLine::Expected(text) => {
                let hidden = run.len().saturating_sub(COMMON_LINES_SHOWN);
                match hidden {
                    0 => {}
                    1 | 2 => output.extend(run[run.len() - hidden..].iter().map(|line| format!("  {line}"))),
                    _ => {
                        output.push(TRUNCATION.to_owned());
                        output.push(format!("  {}", run[run.len() - 1]));
                        skipped = true;
                    }
                }
                run.clear();
                let sign = if matches!(line, DiffLine::Actual(_)) { '+' } else { '-' };
                output.push(format!("{sign} {text}"));
            }
        }
    }
    (output.join("\n").trim_end().to_owned(), skipped)
}

/// `innerFail`: an Error message is thrown itself, anything else as an AssertionError's.
fn fail_with(c: &mut Context<'_>, actual: RootId, expected: RootId, operator: &str, message: RootId, call: Invocation) -> Result<RootedError, RootedError> {
    if c.is_error_rooted(message)? {
        return Ok(c.throw(message));
    }
    let error = assertion_error(c, actual, expected, operator, message, call)?;
    Ok(c.throw(error))
}

fn assertion_error(c: &mut Context<'_>, actual: RootId, expected: RootId, operator: &str, message: RootId, call: Invocation) -> Result<RootId, RootedError> {
    let class = error_class(c)?;
    let operator_value = c.string_rooted(operator);
    let start = function_of(c, call.assertion)?;
    let failure = Failure { actual, expected, operator: operator_value, operator_text: operator.to_owned(), message, start, diff: call.diff };
    build_error(c, &failure, class)
}

/// The module's function for `assertion`, the stack's start.
fn function_of(c: &mut Context<'_>, assertion: Assertion) -> Result<RootId, RootedError> {
    let module = module(c)?;
    if assertion == Assertion::Strict {
        return get(c, module, "strict");
    }
    get(c, module, assertion.name())
}

/// Every assertion's host function.
pub(crate) fn call(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let code = c.host_function_data()?;
    let assertion = Assertion::ALL[c.to_number(code)? as usize];
    let diff = receiver_diff(c, receiver)?;
    let call = Invocation { assertion, diff };
    let undefined = c.undefined();
    let (actual, expected, message) = (argument(c, args, 0), argument(c, args, 1), argument(c, args, 2));
    match assertion {
        Assertion::Ok | Assertion::Strict => ok(c, call, args)?,
        Assertion::Fail => return Err(fail(c, call, args)?),
        Assertion::Equal | Assertion::NotEqual => {
            if args.len() < 2 {
                return Err(super::event_target_shared_vm::missing_args(c, &["actual", "expected"]));
            }
            let nan = |c: &Context<'_>, value: RootId| c.rooted_value(value).and_then(|value| value.as_number()).is_some_and(f64::is_nan);
            let equal = c.equal_rooted(actual, expected)? || (nan(c, actual) && nan(c, expected));
            if equal != (assertion == Assertion::Equal) {
                let operator = if assertion == Assertion::Equal { "==" } else { "!=" };
                return Err(fail_with(c, actual, expected, operator, message, call)?);
            }
        }
        Assertion::StrictEqual | Assertion::NotStrictEqual => {
            if args.len() < 2 {
                return Err(super::event_target_shared_vm::missing_args(c, &["actual", "expected"]));
            }
            if c.same_value_rooted(actual, expected)? != (assertion == Assertion::StrictEqual) {
                return Err(fail_with(c, actual, expected, assertion.name(), message, call)?);
            }
        }
        Assertion::DeepEqual | Assertion::NotDeepEqual | Assertion::DeepStrictEqual | Assertion::NotDeepStrictEqual | Assertion::PartialDeepStrictEqual => {
            if args.len() < 2 {
                return Err(super::event_target_shared_vm::missing_args(c, &["actual", "expected"]));
            }
            let mode = match assertion {
                Assertion::DeepStrictEqual | Assertion::NotDeepStrictEqual => Mode::Strict,
                Assertion::PartialDeepStrictEqual => Mode::Partial,
                _ => Mode::Loose,
            };
            let expect_equal = matches!(assertion, Assertion::DeepEqual | Assertion::DeepStrictEqual | Assertion::PartialDeepStrictEqual);
            if comparisons::deep_equal(c, actual, expected, mode)? != expect_equal {
                return Err(fail_with(c, actual, expected, assertion.name(), message, call)?);
            }
        }
        Assertion::Match | Assertion::DoesNotMatch => matches(c, call, actual, expected, message)?,
        Assertion::Throws | Assertion::DoesNotThrow => {
            let function = actual;
            if !c.is_callable_rooted(function)? {
                return Err(super::util_shared_vm::argument_type_error(c, function, "fn", "function")?);
            }
            let thrown = match c.call_rooted(function, undefined, &[]) {
                Ok(_) => None,
                Err(error) => Some(error.exception.ok_or(error)?),
            };
            let rest = args.get(1..).unwrap_or_default();
            if assertion == Assertion::Throws {
                expects_error(c, call, thrown, rest)?;
            } else {
                expects_no_error(c, call, thrown, rest)?;
            }
        }
        Assertion::Rejects | Assertion::DoesNotReject => return awaiting(c, call, args),
        Assertion::IfError => if_error(c, call, actual)?,
    }
    Ok(undefined)
}

/// `innerOk`: a falsy value fails with the message, or with the falsy call's source.
fn ok(c: &mut Context<'_>, call: Invocation, args: &[RootId]) -> Result<(), RootedError> {
    let value = argument(c, args, 0);
    if c.truthy_rooted(value)? {
        return Ok(());
    }
    let message = argument(c, args, 1);
    let (message, generated) = if args.is_empty() {
        (c.string_rooted("No value argument passed to `assert.ok()`"), true)
    } else if is_nullish(c, message) {
        match falsy_source(c) {
            Some(source) => (c.string_rooted(&format!("The expression evaluated to a falsy value:\n\n  {source}\n")), true),
            None => (c.undefined(), true),
        }
    } else {
        if c.is_error_rooted(message)? {
            return Err(c.throw(message));
        }
        (message, false)
    };
    let expected = c.boolean(true);
    let error = assertion_error(c, value, expected, "==", message, call)?;
    let generated = c.boolean(generated);
    put(c, error, "generatedMessage", generated)?;
    Err(c.throw(error))
}

/// The source of the call expression at the assertion's caller (`getErrMessage`): the
/// innermost JavaScript frame's file, read and parsed.
fn falsy_source(c: &mut Context<'_>) -> Option<String> {
    let frame = c.source_stack_frames(1).into_iter().next().filter(|frame| !frame.is_eval && !frame.script_name.is_empty())?;
    let source = std::fs::read_to_string(&frame.script_name).ok()?;
    super::assert_source::call_expression_at(&source, frame.line_number, frame.column_number)
}

/// `fail(message)`, and the legacy `fail(actual, expected, message, operator)`.
fn fail(c: &mut Context<'_>, call: Invocation, args: &[RootId]) -> Result<RootedError, RootedError> {
    let undefined = c.undefined();
    let first = argument(c, args, 0);
    let internal = is_nullish(c, first) && args.len() <= 1;
    let (actual, expected, message, operator) = if internal {
        (undefined, undefined, c.string_rooted("Failed"), undefined)
    } else if args.len() == 1 {
        (undefined, undefined, first, undefined)
    } else {
        let operator = if args.len() == 2 { c.string_rooted("!=") } else { argument(c, args, 3) };
        (first, args[1], argument(c, args, 2), operator)
    };
    let operator = if is_undefined(c, operator) { Assertion::Fail.name().to_owned() } else { c.to_string(operator)? };
    if c.is_error_rooted(message)? {
        return Ok(c.throw(message));
    }
    let error = assertion_error(c, actual, expected, &operator, message, call)?;
    if internal {
        let generated = c.boolean(true);
        put(c, error, "generatedMessage", generated)?;
    }
    Ok(c.throw(error))
}

/// `match`/`doesNotMatch(string, regexp, message)`.
fn matches(c: &mut Context<'_>, call: Invocation, string: RootId, regexp: RootId, message: RootId) -> Result<(), RootedError> {
    let assertion = call.assertion;
    if !c.has_builtin_brand_rooted(regexp, quench_runtime::BuiltinBrand::RegExp)? {
        return Err(super::util_shared_vm::argument_type_error(c, regexp, "regexp", "RegExp")?);
    }
    let expect_match = assertion == Assertion::Match;
    let is_string = c.value_type_rooted(string) == Some(ValueType::String);
    if is_string && regexp_matches(c, regexp, string)? == expect_match {
        return Ok(());
    }
    if c.is_error_rooted(message)? {
        return Err(c.throw(message));
    }
    let generated = !c.truthy_rooted(message)?;
    let message = if generated {
        let text = if !is_string {
            let kind = type_of(c, string);
            let inspected = format::inspect(c, string, Options::default())?;
            format!("The \"string\" argument must be of type string. Received type {kind} ({inspected})")
        } else {
            let lead = if expect_match { "The input did not match the regular expression " } else { "The input was expected to not match the regular expression " };
            let pattern = format::inspect(c, regexp, Options::default())?;
            let input = format::inspect(c, string, Options::default())?;
            format!("{lead}{pattern}. Input:\n\n{input}\n")
        };
        c.string_rooted(&text)
    } else {
        message
    };
    let error = assertion_error(c, string, regexp, assertion.name(), message, call)?;
    let generated = c.boolean(generated);
    put(c, error, "generatedMessage", generated)?;
    Err(c.throw(error))
}

fn type_of(c: &Context<'_>, value: RootId) -> &'static str {
    match c.value_type_rooted(value) {
        Some(ValueType::Undefined) => "undefined",
        Some(ValueType::Boolean) => "boolean",
        Some(ValueType::Number) => "number",
        Some(ValueType::String) => "string",
        Some(ValueType::Symbol) => "symbol",
        Some(ValueType::BigInt) => "bigint",
        Some(ValueType::Function) => "function",
        _ => "object",
    }
}

/// `RegExpPrototypeExec(regexp, string) !== null`.
fn regexp_matches(c: &mut Context<'_>, regexp: RootId, string: RootId) -> Result<bool, RootedError> {
    let global = c.global_root()?;
    let constructor = get(c, global, "RegExp")?;
    let prototype = get(c, constructor, "prototype")?;
    let exec = get(c, prototype, "exec")?;
    let result = c.call_rooted(exec, regexp, &[string])?;
    Ok(!c.rooted_value(result).is_some_and(|value| value.is_null()))
}

/// `(error, message)` of a throws-family call, a string `error` being the message.
fn error_and_message(c: &mut Context<'_>, call: Invocation, thrown: Option<RootId>, rest: &[RootId]) -> Result<(RootId, RootId), RootedError> {
    let assertion = call.assertion;
    let error = argument(c, rest, 0);
    let message = argument(c, rest, 1);
    if c.value_type_rooted(error) != Some(ValueType::String) {
        if !is_nullish(c, error) && !is_object_like(c, error) {
            return Err(invalid_error_argument(c, error)?);
        }
        return Ok((error, message));
    }
    if matches!(assertion, Assertion::Throws | Assertion::Rejects) {
        if rest.len() == 2 {
            return Err(invalid_error_argument(c, error)?);
        }
        if let Some(actual) = thrown {
            let ambiguous = if c.is_object_rooted(actual)? {
                let actual_message = get(c, actual, "message")?;
                c.strict_equal_rooted(actual_message, error)?.then(|| format!("The error message \"{}\" is identical to the message.", c.to_string(actual_message).unwrap_or_default()))
            } else {
                c.strict_equal_rooted(actual, error)?.then(|| format!("The error \"{}\" is identical to the message.", c.to_string(actual).unwrap_or_default()))
            };
            if let Some(detail) = ambiguous {
                let failure = c.type_error_rooted(&format!("The \"error/message\" argument is ambiguous. {detail}"))?;
                super::node_errors_shared_vm::decorate_type_error(c, failure, "ERR_AMBIGUOUS_ARGUMENT")?;
                return Err(c.throw(failure));
            }
        }
    }
    let undefined = c.undefined();
    Ok((undefined, error))
}

fn invalid_error_argument(c: &mut Context<'_>, error: RootId) -> Result<RootedError, RootedError> {
    super::util_shared_vm::argument_type_error_with_expected(c, error, "error", "of type function or an instance of Error, RegExp, or Object")
}

/// `expectsError`: no throw fails with "Missing expected exception", a thrown value is
/// checked against `error`.
fn expects_error(c: &mut Context<'_>, call: Invocation, thrown: Option<RootId>, rest: &[RootId]) -> Result<(), RootedError> {
    let assertion = call.assertion;
    let (error, message) = error_and_message(c, call, thrown, rest)?;
    let Some(actual) = thrown else {
        let mut details = String::new();
        if is_object_like(c, error) {
            let name = get(c, error, "name")?;
            if c.truthy_rooted(name)? {
                details += &format!(" ({})", c.to_string(name)?);
            }
        }
        details += &if c.truthy_rooted(message)? { format!(": {}", c.to_string(message)?) } else { ".".to_owned() };
        let kind = if assertion == Assertion::Rejects { "rejection" } else { "exception" };
        let undefined = c.undefined();
        let text = c.string_rooted(&format!("Missing expected {kind}{details}"));
        return Err(fail_with(c, undefined, error, assertion.name(), text, call)?);
    };
    if !c.truthy_rooted(error)? {
        return Ok(());
    }
    expected_exception(c, call, actual, error, message)
}

/// `expectedException`: `expected` as a regular expression, a validation object or error,
/// an error class, or a validation function.
fn expected_exception(c: &mut Context<'_>, call: Invocation, actual: RootId, expected: RootId, message: RootId) -> Result<(), RootedError> {
    let assertion = call.assertion;
    let mut generated = false;
    let mut message = message;
    if !c.is_callable_rooted(expected)? {
        if c.has_builtin_brand_rooted(expected, quench_runtime::BuiltinBrand::RegExp)? {
            let text = string_of(c, actual)?;
            if regexp_matches(c, expected, text)? {
                return Ok(());
            }
            if !c.truthy_rooted(message)? {
                generated = true;
                let pattern = format::inspect(c, expected, Options::default())?;
                let input = format::inspect(c, text, Options::default())?;
                message = c.string_rooted(&format!("The input did not match the regular expression {pattern}. Input:\n\n{input}\n"));
            }
        } else if !c.is_object_rooted(actual)? {
            let error = assertion_error(c, actual, expected, "deepStrictEqual", message, call)?;
            let operator = c.string_rooted(assertion.name());
            put(c, error, "operator", operator)?;
            return Err(c.throw(error));
        } else {
            let mut keys = c.enumerable_own_keys_rooted(expected, false, true)?;
            if c.is_error_rooted(expected)? {
                keys.push(c.string_rooted("name"));
                keys.push(c.string_rooted("message"));
            } else if keys.is_empty() {
                return Err(super::util_shared_vm::argument_value_error(c, expected, "error", "may not be an empty object")?);
            }
            for &key in &keys {
                let actual_value = c.get_property_rooted(actual, key)?;
                let expected_value = c.get_property_rooted(expected, key)?;
                if c.value_type_rooted(actual_value) == Some(ValueType::String)
                    && c.has_builtin_brand_rooted(expected_value, quench_runtime::BuiltinBrand::RegExp)?
                    && regexp_matches(c, expected_value, actual_value)?
                {
                    continue;
                }
                let present = c.has_property_rooted(actual, key)?;
                if !present || !comparisons::deep_equal(c, actual_value, expected_value, Mode::Strict)? {
                    if c.truthy_rooted(message)? {
                        return Err(fail_with(c, actual, expected, assertion.name(), message, call)?);
                    }
                    let left = comparison(c, actual, &keys, None)?;
                    let right = comparison(c, expected, &keys, Some(actual))?;
                    let undefined = c.undefined();
                    let error = assertion_error(c, left, right, "deepStrictEqual", undefined, call)?;
                    put(c, error, "actual", actual)?;
                    put(c, error, "expected", expected)?;
                    let operator = c.string_rooted(assertion.name());
                    put(c, error, "operator", operator)?;
                    return Err(c.throw(error));
                }
            }
            return Ok(());
        }
    } else {
        let prototype = get(c, expected, "prototype")?;
        if !is_undefined(c, prototype) && instance_of(c, actual, expected)? {
            return Ok(());
        }
        let global = c.global_root()?;
        let error_class = get(c, global, "Error")?;
        if is_error_class(c, expected, error_class)? {
            if !c.truthy_rooted(message)? {
                generated = true;
                let expected_name = get(c, expected, "name")?;
                let expected_name = c.to_string(expected_name)?;
                let mut text = format!("The error is expected to be an instance of \"{expected_name}\". Received ");
                if c.is_error_rooted(actual)? {
                    let constructor = get(c, actual, "constructor")?;
                    let constructor_name = if is_object_like(c, constructor) { get(c, constructor, "name")? } else { c.undefined() };
                    let name = if c.truthy_rooted(constructor_name)? { constructor_name } else { get(c, actual, "name")? };
                    let name = c.to_string(name)?;
                    if name == expected_name {
                        text += "an error with identical name but a different prototype.";
                    } else {
                        text += &format!("\"{name}\"");
                    }
                    let actual_message = get(c, actual, "message")?;
                    if c.truthy_rooted(actual_message)? {
                        text += &format!("\n\nError message:\n\n{}", c.to_string(actual_message)?);
                    }
                } else {
                    let shallow = Options { depth: Some(-1.0), ..Options::default() };
                    text += &format!("\"{}\"", format::inspect(c, actual, shallow)?);
                }
                message = c.string_rooted(&text);
            }
        } else {
            let receiver = c.object_rooted()?;
            let result = c.call_rooted(expected, receiver, &[actual])?;
            if c.rooted_value(result).and_then(|value| value.as_bool()) == Some(true) {
                return Ok(());
            }
            if !c.truthy_rooted(message)? {
                generated = true;
                let name = get(c, expected, "name")?;
                let name = c.to_string(name)?;
                let name = if name.is_empty() { String::new() } else { format!("\"{name}\" ") };
                let received = format::inspect(c, result, Options::default())?;
                let mut text = format!("The {name}validation function is expected to return \"true\". Received {received}");
                if c.is_error_rooted(actual)? {
                    let shown = c.to_string(actual)?;
                    text += &format!("\n\nCaught error:\n\n{shown}");
                }
                message = c.string_rooted(&text);
            }
        }
    }
    let error = assertion_error(c, actual, expected, assertion.name(), message, call)?;
    let generated = c.boolean(generated);
    put(c, error, "generatedMessage", generated)?;
    Err(c.throw(error))
}

/// `value instanceof class`, false for a primitive.
fn instance_of(c: &mut Context<'_>, value: RootId, class: RootId) -> Result<bool, RootedError> {
    if !is_object_like(c, value) {
        return Ok(false);
    }
    c.instance_of_rooted(value, class)
}

/// `ObjectPrototypeIsPrototypeOf(Error, expected)` or `expected === Error`'s subclass check.
fn is_error_class(c: &mut Context<'_>, expected: RootId, error_class: RootId) -> Result<bool, RootedError> {
    let mut prototype = c.prototype_rooted(expected)?;
    while is_object_like(c, prototype) {
        if c.strict_equal_rooted(prototype, error_class)? {
            return Ok(true);
        }
        prototype = c.prototype_rooted(prototype)?;
    }
    Ok(false)
}

fn string_of(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    let string = get(c, global, "String")?;
    let undefined = c.undefined();
    c.call_rooted(string, undefined, &[value])
}

/// `new Comparison(object, keys, actual)`: the compared keys, with a matching regular
/// expression replaced by the actual string.
fn comparison(c: &mut Context<'_>, object: RootId, keys: &[RootId], actual: Option<RootId>) -> Result<RootId, RootedError> {
    let class_key = "\0builtin:assert/Comparison";
    let class = match cached(c, class_key) {
        Some(class) => class,
        None => {
            let class = c.host_constructor_named(crate::host::shared_vm::operation("consoleNoop"), "Comparison")?;
            cache(c, class_key, class)?;
            class
        }
    };
    let prototype = get(c, class, "prototype")?;
    let result = c.object_rooted()?;
    c.set_prototype_rooted(result, prototype)?;
    for &key in keys {
        if !c.has_property_rooted(object, key)? {
            continue;
        }
        let value = c.get_property_rooted(object, key)?;
        let value = match actual {
            Some(actual) => {
                let actual_value = c.get_property_rooted(actual, key)?;
                if c.value_type_rooted(actual_value) == Some(ValueType::String)
                    && c.has_builtin_brand_rooted(value, quench_runtime::BuiltinBrand::RegExp)?
                    && regexp_matches(c, value, actual_value)?
                {
                    actual_value
                } else {
                    value
                }
            }
            None => value,
        };
        c.set_property_rooted(result, key, value, result)?;
    }
    Ok(result)
}

/// `hasMatchingError`.
fn has_matching_error(c: &mut Context<'_>, actual: RootId, expected: RootId) -> Result<bool, RootedError> {
    if !c.is_callable_rooted(expected)? {
        if c.has_builtin_brand_rooted(expected, quench_runtime::BuiltinBrand::RegExp)? {
            let text = string_of(c, actual)?;
            return regexp_matches(c, expected, text);
        }
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, expected, "expected", "of type function or an instance of RegExp")?);
    }
    let prototype = get(c, expected, "prototype")?;
    if !is_undefined(c, prototype) && instance_of(c, actual, expected)? {
        return Ok(true);
    }
    let global = c.global_root()?;
    let error_class = get(c, global, "Error")?;
    if is_error_class(c, expected, error_class)? {
        return Ok(false);
    }
    let receiver = c.object_rooted()?;
    let result = c.call_rooted(expected, receiver, &[actual])?;
    Ok(c.rooted_value(result).and_then(|value| value.as_bool()) == Some(true))
}

/// `expectsNoError`: a thrown value fails as unwanted when it matches `error`, else is
/// rethrown.
fn expects_no_error(c: &mut Context<'_>, call: Invocation, thrown: Option<RootId>, rest: &[RootId]) -> Result<(), RootedError> {
    let assertion = call.assertion;
    let Some(actual) = thrown else { return Ok(()) };
    let (error, message) = error_and_message(c, call, None, rest)?;
    if !c.truthy_rooted(error)? || has_matching_error(c, actual, error)? {
        let details = if c.truthy_rooted(message)? { format!(": {}", c.to_string(message)?) } else { ".".to_owned() };
        let kind = if assertion == Assertion::DoesNotReject { "rejection" } else { "exception" };
        let actual_message = if is_object_like(c, actual) { get(c, actual, "message")? } else { c.undefined() };
        let actual_message = c.to_string(actual_message)?;
        let text = c.string_rooted(&format!("Got unwanted {kind}{details}\nActual message: \"{actual_message}\""));
        return Err(fail_with(c, actual, error, assertion.name(), text, call)?);
    }
    Err(c.throw(actual))
}

/// `ifError(value)`: anything but null and undefined fails.
fn if_error(c: &mut Context<'_>, call: Invocation, value: RootId) -> Result<(), RootedError> {
    if is_nullish(c, value) {
        return Ok(());
    }
    let mut text = "ifError got unwanted exception: ".to_owned();
    let message = if c.is_object_rooted(value)? { get(c, value, "message")? } else { c.undefined() };
    match c.string_text(message)? {
        Some(message) if message.is_empty() => {
            let constructor = get(c, value, "constructor")?;
            if c.truthy_rooted(constructor)? {
                let name = get(c, constructor, "name")?;
                text += &c.to_string(name)?;
            }
        }
        Some(message) => text += &message,
        None => text += &format::inspect(c, value, Options::default())?,
    }
    let message = c.string_rooted(&text);
    let null = c.null();
    let error = assertion_error(c, value, null, "ifError", message, call)?;
    Err(c.throw(error))
}

/// `checkIsPromise`: a promise, or an object with `then` and `catch` methods.
fn is_promise_like(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if c.is_promise_rooted(value)? {
        return Ok(true);
    }
    if !c.is_object_rooted(value)? {
        return Ok(false);
    }
    for method in ["then", "catch"] {
        let function = get(c, value, method)?;
        if !c.is_callable_rooted(function)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `rejects`/`doesNotReject(promiseFn, error, message)`: the promise settles, then the
/// rejection is checked as `throws`/`doesNotThrow` check a throw.
fn awaiting(c: &mut Context<'_>, call: Invocation, args: &[RootId]) -> Result<RootId, RootedError> {
    let assertion = call.assertion;
    let promise = c.pending_promise_rooted()?;
    let source = argument(c, args, 0);
    let target = if c.is_callable_rooted(source)? {
        let undefined = c.undefined();
        match c.call_rooted(source, undefined, &[]) {
            Ok(result) if is_promise_like(c, result)? => Ok(result),
            Ok(result) => {
                let received = super::util_shared_vm::argument_type_description(c, result)?;
                let error = c.type_error_rooted(&format!("Expected instance of Promise to be returned from the \"promiseFn\" function but got {received}."))?;
                super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_RETURN_VALUE")?;
                Err(c.throw(error))
            }
            Err(error) => Err(error),
        }
    } else if is_promise_like(c, source)? {
        Ok(source)
    } else {
        super::util_shared_vm::argument_type_error_with_expected(c, source, "promiseFn", "of type function or an instance of Promise").and_then(Err)
    };
    let target = match target {
        Ok(target) => target,
        Err(error) => {
            let Some(reason) = error.exception else { return Err(error) };
            c.reject_promise_rooted(promise, reason)?;
            return Ok(promise);
        }
    };
    let rest = c.array_rooted(args.get(1..).unwrap_or_default())?;
    let code = c.number(assertion.code());
    let state = c.array_rooted(&[promise, rest, code, call.diff])?;
    let fulfilled = c.host_function_with_data(crate::host::shared_vm::operation("assertSettled"), state)?;
    let rejected = c.host_function_with_data(crate::host::shared_vm::operation("assertRejected"), state)?;
    let then = get(c, target, "then")?;
    c.call_rooted(then, target, &[fulfilled, rejected])?;
    Ok(promise)
}

fn settle(c: &mut Context<'_>, thrown: Option<RootId>) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let parts = c.array_like_values_rooted(state)?;
    let [promise, rest, code, diff] = parts[..] else { return Err(RootedError::host("assertion settlement malformed")) };
    let assertion = Assertion::ALL[c.to_number(code)? as usize];
    let call = Invocation { assertion, diff };
    let rest = c.array_like_values_rooted(rest)?;
    let checked = if assertion == Assertion::Rejects {
        expects_error(c, call, thrown, &rest)
    } else {
        expects_no_error(c, call, thrown, &rest)
    };
    match checked {
        Ok(()) => {
            let undefined = c.undefined();
            c.resolve_promise_rooted(promise, undefined)?;
        }
        Err(error) => {
            let Some(reason) = error.exception else { return Err(error) };
            c.reject_promise_rooted(promise, reason)?;
        }
    }
    Ok(c.undefined())
}

pub(crate) fn settled(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    settle(c, None)
}

pub(crate) fn rejected(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let reason = argument(c, args, 0);
    settle(c, Some(reason))
}
