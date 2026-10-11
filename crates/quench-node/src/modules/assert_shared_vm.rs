//! Shared-VM operations for the Node `assert` CommonJS builtin.

use crate::host::NodeHost;
use quench_runtime::{BuiltinData, NativeContext, RootId, RootedError};

const MISSING_ASSERT_ARGS: &str = "The \"actual\" and \"expected\" arguments must be specified";
const ASSERTION_FAILED: &str = "Expected values to be strictly equal";
const ASSERTION_NOT_OK: &str = "The expression evaluated to a falsy value";
const ASSERTION_NOT_UNEQUAL: &str = "Expected actual and expected to be strictly unequal";
const ASSERTION_NOT_DEEP_UNEQUAL: &str = "Expected \"actual\" not to be strictly deep-equal to";
const ASSERTION_DIFF: &str = "simple";

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
    util: RootId,
) -> Result<RootId, RootedError> {
    if let Some(module) = context.host_mut().shared_state().borrow().assert_module {
        return Ok(module);
    }

    let assert = context.host_function(crate::host::shared_vm::operation("assert"))?;
    let ok = context.host_function(crate::host::shared_vm::operation("ok"))?;
    let strict = context.host_function(crate::host::shared_vm::operation("strict"))?;
    let strict_equal = context.host_function(crate::host::shared_vm::operation("strictEqual"))?;
    let not_strict_equal =
        context.host_function(crate::host::shared_vm::operation("notStrictEqual"))?;
    let deep_strict_equal =
        context.host_function(crate::host::shared_vm::operation("deepStrictEqual"))?;
    let not_deep_strict_equal =
        context.host_function(crate::host::shared_vm::operation("notDeepStrictEqual"))?;
    let match_string = context.host_function(crate::host::shared_vm::operation("match"))?;
    let does_not_match =
        context.host_function(crate::host::shared_vm::operation("doesNotMatch"))?;
    let fail = context.host_function(crate::host::shared_vm::operation("fail"))?;
    let throws = context.host_function(crate::host::shared_vm::operation("throws"))?;
    let does_not_throw =
        context.host_function(crate::host::shared_vm::operation("doesNotThrow"))?;
    let inspect = get(context, util, "inspect")?;
    let if_error =
        context.host_function_with_data(crate::host::shared_vm::operation("ifError"), inspect)?;
    let initialize = context.host_function(crate::host::shared_vm::operation(
        "assertionErrorInitialize",
    ))?;
    let assertion_factory = context.evaluate_script_rooted(
        quench_js_check::checked_js!(
            r#"(initialize) => class AssertionError extends Error {
  constructor(options = {}) {
    super(options.message);
    initialize(this, options);
  }
}"#
        ),
        "node:assert/AssertionError.js",
    )?;
    let undefined = context.undefined();
    let assertion_error = context.call_rooted(assertion_factory, undefined, &[initialize])?;
    let rejects_factory = context.evaluate_script_rooted(
        crate::modules::assert_rejects_source::ASSERT_REJECTS,
        "node:assert/rejects.js",
    )?;
    let undefined = context.undefined();
    let rejects = context.call_rooted(rejects_factory, undefined, &[assertion_error])?;

    set(context, assert, "ok", ok)?;
    for name in ["equal", "notEqual", "deepEqual", "notDeepEqual"] {
        let value = context.host_function(crate::host::shared_vm::operation(name))?;
        set(context, assert, name, value)?;
    }
    set(context, assert, "strictEqual", strict_equal)?;
    set(context, assert, "notStrictEqual", not_strict_equal)?;
    set(context, assert, "deepStrictEqual", deep_strict_equal)?;
    set(context, assert, "notDeepStrictEqual", not_deep_strict_equal)?;
    let partial =
        context.host_function(crate::host::shared_vm::operation("partialDeepStrictEqual"))?;
    set(context, assert, "partialDeepStrictEqual", partial)?;
    set(context, assert, "match", match_string)?;
    set(context, assert, "doesNotMatch", does_not_match)?;
    set(context, assert, "fail", fail)?;
    set(context, assert, "strict", strict)?;
    set(context, assert, "throws", throws)?;
    set(context, assert, "doesNotThrow", does_not_throw)?;
    set(context, assert, "ifError", if_error)?;
    set(context, assert, "rejects", rejects)?;
    set(context, assert, "AssertionError", assertion_error)?;
    let assert_factory = context.evaluate_script_rooted(
        r#"(assert, strict) => {
  function Assert(options = {}) {
    if (!new.target) {
      const error = new TypeError('Class constructor Assert cannot be invoked without \'new\'');
      error.code = 'ERR_CONSTRUCT_CALL_REQUIRED';
      throw error;
    }
    if (options === null || typeof options !== 'object') {
      const error = new TypeError('The "options" argument must be of type object');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    const diff = options.diff === undefined ? 'simple' : options.diff;
    if (diff !== 'simple' && diff !== 'full') {
      const error = new TypeError(`The property 'options.diff' must be one of: 'simple', 'full'. Received '${diff}'`);
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    const selected = options.strict === false ? assert : strict;
    const wrap = (fn) => function(...args) {
      try { return fn(...args); }
      catch (error) {
        if (error && error.code === 'ERR_ASSERTION') error.diff = diff;
        throw error;
      }
    };
    for (const name of ['fail', 'ok', 'strictEqual', 'notStrictEqual',
      'deepStrictEqual', 'notDeepStrictEqual', 'partialDeepStrictEqual',
      'match', 'doesNotMatch', 'throws', 'doesNotThrow', 'ifError', 'rejects']) {
      this[name] = wrap(selected[name]);
    }
    this.AssertionError = assert.AssertionError;
    this.equal = options.strict === false ? wrap(assert.equal) : this.strictEqual;
    this.notEqual = options.strict === false ? wrap(assert.notEqual) : this.notStrictEqual;
    this.deepEqual = options.strict === false ? wrap(assert.deepEqual) : this.deepStrictEqual;
    this.notDeepEqual = options.strict === false ? wrap(assert.notDeepEqual) : this.notDeepStrictEqual;
  }
  return Assert;
}"#,
        "node:assert/Assert.js",
    )?;
    let strict_module = get(context, assert, "strict")?;
    let assert_constructor =
        context.call_rooted(assert_factory, undefined, &[assert, strict_module])?;
    set(context, assert, "Assert", assert_constructor)?;
    set(context, strict, "ok", ok)?;
    set(context, strict, "strictEqual", strict_equal)?;
    set(context, strict, "notStrictEqual", not_strict_equal)?;
    set(context, strict, "deepStrictEqual", deep_strict_equal)?;
    set(context, strict, "partialDeepStrictEqual", partial)?;
    set(context, strict, "notDeepStrictEqual", not_deep_strict_equal)?;
    set(context, strict, "match", match_string)?;
    set(context, strict, "doesNotMatch", does_not_match)?;
    set(context, strict, "fail", fail)?;
    set(context, strict, "equal", strict_equal)?;
    set(context, strict, "strict", strict)?;
    set(context, strict, "throws", throws)?;
    set(context, strict, "doesNotThrow", does_not_throw)?;
    set(context, strict, "ifError", if_error)?;
    set(context, strict, "rejects", rejects)?;
    set(context, strict, "AssertionError", assertion_error)?;
    set(context, strict, "Assert", assert_constructor)?;

    let retained = context.retain(assert)?;
    context.host_mut().shared_state().borrow_mut().assert_module = Some(retained);
    Ok(assert)
}

/// Set host-owned assertion fields directly, without guest property-descriptor
/// objects that can inherit polluted accessor fields from Object.prototype.
pub(crate) fn initialize_assertion_error(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (error, options) = (args[0], args[1]);
    let generated = get(context, options, "generatedMessage")?;
    let generated = if nullish(context, generated) {
        let message = get(context, options, "message")?;
        let generated = !context.truthy_rooted(message)?;
        context.boolean(generated)
    } else {
        generated
    };
    set(context, error, "generatedMessage", generated)?;
    let key = context.string_rooted("name");
    let name = context.string_rooted("AssertionError [ERR_ASSERTION]");
    if !context.define_data_property_rooted(error, key, name, true, false, true)? {
        return Err(RootedError::host("cannot define assertion error name"));
    }
    let code = assertion_option(context, options, "code", "ERR_ASSERTION")?;
    set(context, error, "code", code)?;
    for name in ["actual", "expected", "operator"] {
        let value = get(context, options, name)?;
        set(context, error, name, value)?;
    }
    let _ = get(context, error, "stack")?;
    let name = context.string_rooted("AssertionError");
    set(context, error, "name", name)?;
    let diff = assertion_option(context, options, "diff", ASSERTION_DIFF)?;
    set(context, error, "diff", diff)?;
    Ok(context.undefined())
}

fn assertion_option(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
    name: &str,
    fallback: &str,
) -> Result<RootId, RootedError> {
    let value = get(context, options, name)?;
    Ok(if nullish(context, value) {
        context.string_rooted(fallback)
    } else {
        value
    })
}

fn nullish(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined() || value.is_null())
}

pub(crate) fn ok(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let actual = args.first().copied().unwrap_or_else(|| context.undefined());
    if context.truthy_rooted(actual)? {
        return Ok(context.undefined());
    }
    if let Some(message) = args.get(1).copied() {
        if error_instance(context, message)? {
            return Err(context.throw(message));
        }
    }
    let expected = context.boolean(true);
    let (message, generated) = assertion_message(context, args.get(1).copied(), ASSERTION_NOT_OK)?;
    assertion_error(context, actual, expected, "==", &message, generated)
}

pub(crate) fn strict_equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() < 2 {
        return missing_assert_arguments(context);
    }

    let actual = args[0];
    let expected = args[1];
    if context.same_value_rooted(actual, expected)? {
        return Ok(context.undefined());
    }

    let (message, generated) = assertion_message(context, args.get(2).copied(), ASSERTION_FAILED)?;
    assertion_error(
        context,
        actual,
        expected,
        "strictEqual",
        &message,
        generated,
    )
}

pub(crate) fn not_strict_equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() < 2 {
        return missing_assert_arguments(context);
    }
    let (actual, expected) = (args[0], args[1]);
    if !context.same_value_rooted(actual, expected)? {
        return Ok(context.undefined());
    }
    let (mut message, generated) =
        assertion_message(context, args.get(2).copied(), ASSERTION_NOT_UNEQUAL)?;
    if generated {
        let value = match context.string_text(actual)? {
            Some(value) => {
                let quoted = format!("'{value}'");
                if quoted.len() > 40 {
                    format!("\n\n{quoted}")
                } else {
                    format!(" {quoted}")
                }
            }
            None => format!(" {}", context.to_string(actual)?),
        };
        message = format!("Expected \"actual\" to be strictly unequal to:{value}");
    }
    assertion_error(
        context,
        actual,
        expected,
        "notStrictEqual",
        &message,
        generated,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComparisonMode {
    Loose,
    Strict,
    StrictWithoutPrototypes,
    Partial,
}

pub(crate) fn equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    loose_equality(context, args, false)
}

pub(crate) fn not_equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    loose_equality(context, args, true)
}

fn loose_equality(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    negate: bool,
) -> Result<RootId, RootedError> {
    if args.len() < 2 {
        return missing_assert_arguments(context);
    }
    let (actual, expected) = (args[0], args[1]);
    if (context.same_value_rooted(actual, expected)? || context.equal_rooted(actual, expected)?)
        != negate
    {
        return Ok(context.undefined());
    }
    let fallback = if negate {
        "Expected values to be loosely unequal"
    } else {
        "Expected values to be loosely equal"
    };
    let (message, generated) = assertion_message(context, args.get(2).copied(), fallback)?;
    assertion_error(
        context,
        actual,
        expected,
        if negate { "!=" } else { "==" },
        &message,
        generated,
    )
}

pub(crate) fn deep_strict_equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    deep_assertion(context, args, ComparisonMode::Strict, false)
}

pub(crate) fn not_deep_strict_equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    deep_assertion(context, args, ComparisonMode::Strict, true)
}

pub(crate) fn partial_deep_strict_equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    deep_assertion(context, args, ComparisonMode::Partial, false)
}

pub(crate) fn legacy_deep_equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    deep_assertion(context, args, ComparisonMode::Loose, false)
}

pub(crate) fn legacy_not_deep_equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    deep_assertion(context, args, ComparisonMode::Loose, true)
}

fn deep_assertion(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    mode: ComparisonMode,
    negate: bool,
) -> Result<RootId, RootedError> {
    if args.len() < 2 {
        return missing_assert_arguments(context);
    }
    let (actual, expected) = (args[0], args[1]);
    if deep_equal(context, actual, expected, mode, &mut Vec::new())? != negate {
        return Ok(context.undefined());
    }
    let (operator, fallback) = match (mode, negate) {
        (ComparisonMode::Loose, false) => ("deepEqual", "Expected values to be loosely deep-equal"),
        (ComparisonMode::Loose, true) => (
            "notDeepEqual",
            "Expected actual not to be loosely deep-equal to",
        ),
        (ComparisonMode::Partial, false) => (
            "partialDeepStrictEqual",
            "Expected values to be partially and strictly deep-equal",
        ),
        (_, false) => (
            "deepStrictEqual",
            "Expected values to be strictly deep-equal",
        ),
        (_, true) => ("notDeepStrictEqual", ASSERTION_NOT_DEEP_UNEQUAL),
    };
    let (message, generated) = assertion_message(context, args.get(2).copied(), fallback)?;
    assertion_error(context, actual, expected, operator, &message, generated)
}

pub(crate) fn is_deep_strict_equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let actual = args.first().copied().unwrap_or_else(|| context.undefined());
    let expected = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let mode = if args
        .get(2)
        .copied()
        .map(|root| context.truthy_rooted(root))
        .transpose()?
        .unwrap_or(false)
    {
        ComparisonMode::StrictWithoutPrototypes
    } else {
        ComparisonMode::Strict
    };
    let matches = deep_equal(context, actual, expected, mode, &mut Vec::new())?;
    Ok(context.boolean(matches))
}

pub(crate) fn match_string(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() < 2 {
        return missing_assert_arguments(context);
    }
    let (actual, expected) = (args[0], args[1]);
    if !is_regexp(context, expected)? {
        return invalid_regexp(context, expected);
    }
    if context.string_text(actual)?.is_none() {
        let error = context.type_error_rooted("The \"string\" argument must be of type string")?;
        let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    let matched = regexp_match(context, expected, actual)?;
    match matched {
        Some(true) => Ok(context.undefined()),
        Some(false) => {
            let (message, generated) = assertion_message(
                context,
                args.get(2).copied(),
                "The input did not match the regular expression",
            )?;
            assertion_error(context, actual, expected, "match", &message, generated)
        }
        None => {
            let error = context
                .type_error_rooted("The \"regexp\" argument must be an instance of RegExp")?;
            Err(context.throw(error))
        }
    }
}

pub(crate) fn does_not_match(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() < 2 {
        return missing_assert_arguments(context);
    }
    let (actual, expected) = (args[0], args[1]);
    if !is_regexp(context, expected)? {
        return invalid_regexp(context, expected);
    }
    if context.string_text(actual)?.is_none() {
        let error = context.type_error_rooted("The \"string\" argument must be of type string")?;
        let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    match regexp_match(context, expected, actual)? {
        Some(false) => Ok(context.undefined()),
        Some(true) => {
            let (message, generated) = assertion_message(
                context,
                args.get(2).copied(),
                "The input was expected not to match the regular expression",
            )?;
            assertion_error(
                context,
                actual,
                expected,
                "doesNotMatch",
                &message,
                generated,
            )
        }
        None => {
            let error = context
                .type_error_rooted("The \"regexp\" argument must be an instance of RegExp")?;
            Err(context.throw(error))
        }
    }
}

fn missing_assert_arguments(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(MISSING_ASSERT_ARGS)?;
    let code = context.string_rooted("ERR_MISSING_ARGS");
    set(context, error, "code", code)?;
    Err(context.throw(error))
}

fn deep_equal(
    context: &mut NativeContext<'_, NodeHost>,
    actual: RootId,
    expected: RootId,
    mode: ComparisonMode,
    seen: &mut Vec<(RootId, RootId)>,
) -> Result<bool, RootedError> {
    if context.same_value_rooted(actual, expected)? {
        return Ok(true);
    }
    let actual_object = context.is_object_rooted(actual)?;
    let expected_object = context.is_object_rooted(expected)?;
    if !actual_object || !expected_object {
        return if mode == ComparisonMode::Loose
            && !actual_object
            && !expected_object
            && !context.is_callable_rooted(actual)?
            && !context.is_callable_rooted(expected)?
        {
            context.equal_rooted(actual, expected)
        } else {
            Ok(false)
        };
    }
    for (left, right) in seen.iter().copied() {
        if context.same_value_rooted(actual, left)? && context.same_value_rooted(expected, right)? {
            return Ok(true);
        }
    }
    seen.push((actual, expected));
    let array = context.is_array_rooted(actual)?;
    if (mode == ComparisonMode::Strict
        && !same_prototype(context, actual, expected)?)
        || array != context.is_array_rooted(expected)?
    {
        return Ok(false);
    }
    if array {
        let actual_length = get(context, actual, "length")?;
        let expected_length = get(context, expected, "length")?;
        if mode == ComparisonMode::Partial {
            let actual_length: usize = context.to_string(actual_length)?.parse().unwrap_or(0);
            let expected_length: usize = context.to_string(expected_length)?.parse().unwrap_or(0);
            if actual_length < expected_length {
                return Ok(false);
            }
            let mut cursor = 0;
            for index in 0..expected_length {
                let expected_value = get(context, expected, &index.to_string())?;
                let mut matched = false;
                while cursor < actual_length {
                    let actual_value = get(context, actual, &cursor.to_string())?;
                    cursor += 1;
                    let mut trial = seen.clone();
                    if deep_equal(context, actual_value, expected_value, mode, &mut trial)? {
                        *seen = trial;
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    return Ok(false);
                }
            }
        } else if !context.same_value_rooted(actual_length, expected_length)? {
            return Ok(false);
        }
    }
    let actual_data = context.builtin_data_rooted(actual)?;
    let expected_data = context.builtin_data_rooted(expected)?;
    let indices = !matches!(actual_data, Some(BuiltinData::Bytes { .. }))
        && !(array && mode == ComparisonMode::Partial);
    if !builtin_equal(
        context,
        actual,
        expected,
        actual_data,
        expected_data,
        mode,
        seen,
    )? {
        return Ok(false);
    }
    let (actual_keys, actual_symbols) =
        enumerable_keys(context, actual, mode != ComparisonMode::Loose, indices)?;
    let (expected_keys, expected_symbols) =
        enumerable_keys(context, expected, mode != ComparisonMode::Loose, indices)?;
    if mode != ComparisonMode::Partial && actual_symbols.len() != expected_symbols.len() {
        return Ok(false);
    }
    if mode == ComparisonMode::Partial {
        if expected_keys.iter().any(|key| !actual_keys.contains(key)) {
            return Ok(false);
        }
    } else if actual_keys != expected_keys {
        return Ok(false);
    }
    for key in expected_keys {
        let key = context.string_rooted(&key);
        let left = context.get_property_rooted(actual, key)?;
        let right = context.get_property_rooted(expected, key)?;
        if !deep_equal(context, left, right, mode, seen)? {
            return Ok(false);
        }
    }
    for key in expected_symbols {
        let mut present = false;
        for expected_key in &actual_symbols {
            if context.same_value_rooted(key, *expected_key)? {
                present = true;
                break;
            }
        }
        if !present {
            return Ok(false);
        }
        let left = context.get_property_rooted(actual, key)?;
        let right = context.get_property_rooted(expected, key)?;
        if !deep_equal(context, left, right, mode, seen)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn builtin_equal(
    context: &mut NativeContext<'_, NodeHost>,
    actual: RootId,
    expected: RootId,
    left: Option<BuiltinData>,
    right: Option<BuiltinData>,
    mode: ComparisonMode,
    seen: &mut Vec<(RootId, RootId)>,
) -> Result<bool, RootedError> {
    use BuiltinData::*;
    match (left, right) {
        (None, None) => Ok(true),
        (Some(Primitive { brand: a, value: x }), Some(Primitive { brand: b, value: y })) => {
            Ok(a == b && context.same_value_rooted(x, y)?)
        }
        (Some(Bytes { brand: a, bytes: x }), Some(Bytes { brand: b, bytes: y })) => {
            if a != b {
                return Ok(false);
            }
            if mode == ComparisonMode::Loose {
                if let (Some(x), Some(y)) = (
                    context.floating_view_values_rooted(actual)?,
                    context.floating_view_values_rooted(expected)?,
                ) {
                    return Ok(x.len() == y.len() && x.iter().zip(y).all(|(a, b)| *a == b));
                }
            }
            if mode == ComparisonMode::Partial && x.len() != y.len() {
                let mut cursor = 0;
                for byte in y {
                    let Some(offset) = x[cursor..].iter().position(|candidate| *candidate == byte)
                    else {
                        return Ok(false);
                    };
                    cursor += offset + 1;
                }
                return Ok(true);
            }
            Ok(x == y)
        }
        (Some(RegExp), Some(RegExp)) => {
            for key in ["source", "flags", "lastIndex"] {
                let x = get(context, actual, key)?;
                let y = get(context, expected, key)?;
                if !context.strict_equal_rooted(x, y)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (Some(Error), Some(Error)) => {
            for key in ["name", "message", "cause", "errors"] {
                let x = get(context, actual, key)?;
                let y = get(context, expected, key)?;
                if !deep_equal(context, x, y, mode, seen)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (Some(Map(x)), Some(Map(y))) => {
            let (x, y) = if mode == ComparisonMode::Partial {
                if y.len() > x.len() {
                    return Ok(false);
                }
                (y, x)
            } else {
                if x.len() != y.len() {
                    return Ok(false);
                }
                (x, y)
            };
            let mut used = vec![false; y.len()];
            for (key, value) in x {
                let mut matched = false;
                for (index, (other_key, other_value)) in y.iter().copied().enumerate() {
                    if used[index] {
                        continue;
                    }
                    let mut trial = seen.clone();
                    if deep_equal(
                        context,
                        if mode == ComparisonMode::Partial {
                            other_key
                        } else {
                            key
                        },
                        if mode == ComparisonMode::Partial {
                            key
                        } else {
                            other_key
                        },
                        mode,
                        &mut trial,
                    )? && deep_equal(
                        context,
                        if mode == ComparisonMode::Partial {
                            other_value
                        } else {
                            value
                        },
                        if mode == ComparisonMode::Partial {
                            value
                        } else {
                            other_value
                        },
                        mode,
                        &mut trial,
                    )? {
                        *seen = trial;
                        used[index] = true;
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (Some(Set(x)), Some(Set(y))) => {
            let (x, y) = if mode == ComparisonMode::Partial {
                if y.len() > x.len() {
                    return Ok(false);
                }
                (y, x)
            } else {
                if x.len() != y.len() {
                    return Ok(false);
                }
                (x, y)
            };
            let mut used = vec![false; y.len()];
            for value in x {
                let mut matched = false;
                for (index, other) in y.iter().copied().enumerate() {
                    if used[index] {
                        continue;
                    }
                    let mut trial = seen.clone();
                    if deep_equal(
                        context,
                        if mode == ComparisonMode::Partial {
                            other
                        } else {
                            value
                        },
                        if mode == ComparisonMode::Partial {
                            value
                        } else {
                            other
                        },
                        mode,
                        &mut trial,
                    )? {
                        *seen = trial;
                        used[index] = true;
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn enumerable_keys(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    symbols: bool,
    indices: bool,
) -> Result<(Vec<String>, Vec<RootId>), RootedError> {
    let mut names = Vec::new();
    let mut symbol_keys = Vec::new();
    for key in context.enumerable_own_keys_rooted(value, symbols, indices)? {
        if let Some(name) = context.string_text(key)? {
            names.push(name);
        } else if symbols {
            symbol_keys.push(key);
        }
    }
    names.sort();
    Ok((names, symbol_keys))
}

fn same_prototype(
    context: &mut NativeContext<'_, NodeHost>,
    actual: RootId,
    expected: RootId,
) -> Result<bool, RootedError> {
    let actual = context.prototype_rooted(actual)?;
    let expected = context.prototype_rooted(expected)?;
    context.same_value_rooted(actual, expected)
}

pub(crate) fn throws(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let expected = args.get(1).copied();
    let Some(callback) = args.first().copied() else {
        return invalid_assert_callback(context, "undefined");
    };
    let undefined = context.undefined();
    let thrown = match context.call_rooted(callback, undefined, &[]) {
        Ok(_) => {
            let expected = args.get(1).copied().unwrap_or(undefined);
            return assertion_error(
                context,
                undefined,
                expected,
                "throws",
                "Missing expected exception.",
                true,
            );
        }
        Err(error) => match error.exception {
            Some(exception) => exception,
            None => return Err(error),
        },
    };
    let Some(expected) = expected else {
        return Ok(undefined);
    };
    if context.is_callable_rooted(expected)? {
        let prototype = get(context, expected, "prototype")?;
        let is_error_constructor = if context.is_object_rooted(prototype)? {
            let global = context.global_root()?;
            let object = get(context, global, "Object")?;
            let object_prototype = get(context, object, "prototype")?;
            let is_prototype_of = get(context, object_prototype, "isPrototypeOf")?;
            let error_constructor = get(context, global, "Error")?;
            let error_prototype = get(context, error_constructor, "prototype")?;
            if context.same_value_rooted(prototype, error_prototype)? {
                true
            } else {
                let inherits_error =
                    context.call_rooted(is_prototype_of, error_prototype, &[prototype])?;
                context.truthy_rooted(inherits_error)?
            }
        } else {
            false
        };
        let matched = if is_error_constructor {
            let global = context.global_root()?;
            let object = get(context, global, "Object")?;
            let object_prototype = get(context, object, "prototype")?;
            let is_prototype_of = get(context, object_prototype, "isPrototypeOf")?;
            context.call_rooted(is_prototype_of, prototype, &[thrown])?
        } else {
            context.call_rooted(expected, undefined, &[thrown])?
        };
        if context.truthy_rooted(matched)? {
            return Ok(thrown);
        }
        return assertion_error(
            context,
            thrown,
            expected,
            "throws",
            "The error did not match the expected function.",
            true,
        );
    }
    if let Some(matched) = regexp_match(context, expected, thrown)? {
        if matched {
            return Ok(undefined);
        }
        return assertion_error(
            context,
            thrown,
            expected,
            "throws",
            "The error did not match the expected regular expression.",
            true,
        );
    }
    let expected_keys = if context
        .rooted_value(expected)
        .is_some_and(|value| value.is_undefined() || value.is_null())
        || context.string_text(expected)?.is_some()
    {
        None
    } else {
        let mut keys = object_keys(context, expected)?;
        if error_instance(context, expected)? {
            keys.push(context.string_rooted("name"));
            keys.push(context.string_rooted("message"));
        } else if keys.is_empty() {
            let error =
                context.type_error_rooted("The argument 'error' may not be an empty object.")?;
            let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
            set(context, error, "code", code)?;
            return Err(context.throw(error));
        }
        Some(keys)
    };
    if expected_keys.is_none() {
        return Ok(undefined);
    }
    if expected_matches_object(
        context,
        thrown,
        expected,
        expected_keys.as_deref().unwrap_or_default(),
    )? {
        Ok(undefined)
    } else {
        assertion_error(
            context,
            thrown,
            expected,
            "throws",
            "The error did not match the expected object.",
            true,
        )
    }
}

pub(crate) fn does_not_throw(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        return invalid_assert_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        let received = context.to_string(callback)?;
        return invalid_assert_callback(context, &received);
    }
    let undefined = context.undefined();
    match context.call_rooted(callback, undefined, &[]) {
        Ok(_) => Ok(undefined),
        Err(error) => {
            let Some(exception) = error.exception else {
                return Err(error);
            };
            if let Some(expected) = args.get(1).copied() {
                if !expected_error_matches(context, expected, exception)? {
                    return Err(context.throw(exception));
                }
            }
            let (message, generated) =
                assertion_message(context, args.get(2).copied(), "Got unwanted exception.")?;
            assertion_error(
                context,
                exception,
                undefined,
                "doesNotThrow",
                &message,
                generated,
            )
        }
    }
}

fn expected_error_matches(
    context: &mut NativeContext<'_, NodeHost>,
    expected: RootId,
    thrown: RootId,
) -> Result<bool, RootedError> {
    if context.is_callable_rooted(expected)? {
        let prototype = get(context, expected, "prototype")?;
        if context.is_object_rooted(prototype)? {
            let global = context.global_root()?;
            let error = get(context, global, "Error")?;
            let error_prototype = get(context, error, "prototype")?;
            let object = get(context, global, "Object")?;
            let object_prototype = get(context, object, "prototype")?;
            let is_prototype_of = get(context, object_prototype, "isPrototypeOf")?;
            let inherits_error =
                context.call_rooted(is_prototype_of, error_prototype, &[prototype])?;
            let error_constructor = context.same_value_rooted(prototype, error_prototype)?
                || context.truthy_rooted(inherits_error)?;
            if error_constructor {
                let matched = context.call_rooted(is_prototype_of, prototype, &[thrown])?;
                return context.truthy_rooted(matched);
            }
        }
        let undefined = context.undefined();
        let result = context.call_rooted(expected, undefined, &[thrown])?;
        return context.truthy_rooted(result);
    }
    if let Some(matches) = regexp_match(context, expected, thrown)? {
        return Ok(matches);
    }
    if context.is_object_rooted(expected)? {
        let keys = object_keys(context, expected)?;
        if !keys.is_empty() {
            return expected_matches_object(context, thrown, expected, &keys);
        }
    }
    Ok(false)
}

fn regexp_match(
    context: &mut NativeContext<'_, NodeHost>,
    expected: RootId,
    thrown: RootId,
) -> Result<Option<bool>, RootedError> {
    if !is_regexp(context, expected)? {
        return Ok(None);
    }
    let actual = context.to_string(thrown)?;
    let input = context.string_rooted(&actual);
    let test = get(context, expected, "test")?;
    let matched = context.call_rooted(test, expected, &[input])?;
    Ok(context
        .rooted_value(matched)
        .and_then(|value| value.as_bool()))
}

fn is_regexp(
    context: &mut NativeContext<'_, NodeHost>,
    expected: RootId,
) -> Result<bool, RootedError> {
    let global = context.global_root()?;
    let object = get(context, global, "Object")?;
    let object_prototype = get(context, object, "prototype")?;
    let is_prototype_of = get(context, object_prototype, "isPrototypeOf")?;
    let regexp = get(context, global, "RegExp")?;
    let regexp_prototype = get(context, regexp, "prototype")?;
    let inherits_regexp = context.call_rooted(is_prototype_of, regexp_prototype, &[expected])?;
    context.truthy_rooted(inherits_regexp)
}

fn invalid_regexp(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<RootId, RootedError> {
    let received = match context.string_text(value)? {
        Some(value) => format!("type string ('{value}')"),
        None => "an instance of Object".to_owned(),
    };
    let error = context.type_error_rooted(&format!(
        "The \"regexp\" argument must be an instance of RegExp. Received {received}"
    ))?;
    let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
    set(context, error, "code", code)?;
    Err(context.throw(error))
}

pub(crate) fn fail(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if let Some(message) = args.first().copied() {
        if error_instance(context, message)? {
            return Err(context.throw(message));
        }
    }
    let (message, generated) = match args.first().copied() {
        Some(message)
            if !context
                .rooted_value(message)
                .is_some_and(|value| value.is_undefined()) =>
        {
            (context.to_string(message)?, false)
        }
        _ => ("Failed".to_owned(), true),
    };
    let undefined = context.undefined();
    assertion_error(context, undefined, undefined, "fail", &message, generated)
}

pub(crate) fn if_error(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let error = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(value) = context.rooted_value(error) else {
        return Err(RootedError::host("invalid assert.ifError value root"));
    };
    if value.is_null() || value.is_undefined() {
        return Ok(context.undefined());
    }

    let is_object = context.is_object_rooted(error)?;
    let message = if is_object {
        let message_key = context.string_rooted("message");
        let first_message = context.get_property_rooted(error, message_key)?;
        if context.string_text(first_message)?.is_some() {
            let second_message = context.get_property_rooted(error, message_key)?;
            let length_key = context.string_rooted("length");
            let second_length = context.get_property_rooted(second_message, length_key)?;
            let empty = context
                .rooted_value(second_length)
                .and_then(|value| value.as_number())
                .is_some_and(|length| length == 0.0);
            if empty {
                let constructor_key = context.string_rooted("constructor");
                let constructor = context.get_property_rooted(error, constructor_key)?;
                if context.truthy_rooted(constructor)? {
                    let constructor = context.get_property_rooted(error, constructor_key)?;
                    let name_key = context.string_rooted("name");
                    let name = context.get_property_rooted(constructor, name_key)?;
                    context.to_string(name)?
                } else {
                    let message = context.get_property_rooted(error, message_key)?;
                    context.to_string(message)?
                }
            } else {
                let message = context.get_property_rooted(error, message_key)?;
                context.to_string(message)?
            }
        } else {
            inspect_value(context, error)?
        }
    } else {
        inspect_value(context, error)?
    };
    let message = format!("ifError got unwanted exception: {message}");
    let expected = context.null();
    let assertion = make_assertion_error(context, error, expected, "ifError", &message, false)?;
    merge_original_stack(context, assertion, error)?;
    Err(context.throw(assertion))
}

fn inspect_value(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<String, RootedError> {
    let inspect = context.host_function_data()?;
    let receiver = context.undefined();
    let rendered = context.call_rooted(inspect, receiver, &[value])?;
    context
        .string_text(rendered)?
        .ok_or_else(|| RootedError::host("util.inspect returned a non-string"))
}

fn merge_original_stack(
    context: &mut NativeContext<'_, NodeHost>,
    assertion: RootId,
    original: RootId,
) -> Result<(), RootedError> {
    let original_stack = get(context, original, "stack")?;
    let Some(original_stack) = context.string_text(original_stack)? else {
        return Ok(());
    };
    let Some(frame_start) = original_stack.find("\n    at").map(|index| index + 1) else {
        return Ok(());
    };

    let assertion_stack = get(context, assertion, "stack")?;
    let Some(assertion_stack) = context.string_text(assertion_stack)? else {
        return Ok(());
    };
    let original_frames = original_stack[frame_start..]
        .split('\n')
        .collect::<Vec<_>>();
    let mut assertion_frames = assertion_stack.split('\n').collect::<Vec<_>>();
    if let Some(index) = original_frames.iter().find_map(|frame| {
        assertion_frames
            .iter()
            .position(|candidate| candidate == frame)
    }) {
        assertion_frames.truncate(index);
    }
    let merged = assertion_frames
        .into_iter()
        .chain(original_frames)
        .collect::<Vec<_>>()
        .join("\n");
    let merged = context.string_rooted(&merged);
    set(context, assertion, "stack", merged)
}

fn invalid_assert_callback(
    context: &mut NativeContext<'_, NodeHost>,
    received: &str,
) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(&format!(
        "The \"fn\" argument must be of type function. Received {received}"
    ))?;
    let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
    set(context, error, "code", code)?;
    Err(context.throw(error))
}

fn expected_matches_object(
    context: &mut NativeContext<'_, NodeHost>,
    actual: RootId,
    expected: RootId,
    keys: &[RootId],
) -> Result<bool, RootedError> {
    expected_matches_object_depth(context, actual, expected, keys, 0)
}

fn expected_matches_object_depth(
    context: &mut NativeContext<'_, NodeHost>,
    actual: RootId,
    expected: RootId,
    keys: &[RootId],
    depth: usize,
) -> Result<bool, RootedError> {
    if depth > 32 {
        return Ok(false);
    }
    for key in keys {
        let expected_value = context.get_property_rooted(expected, *key)?;
        let actual_value = context.get_property_rooted(actual, *key)?;
        let matches = match regexp_match(context, expected_value, actual_value)? {
            Some(matches) => matches,
            None if context.same_value_rooted(expected_value, actual_value)? => true,
            None => {
                let expected_keys = object_keys(context, expected_value)?;
                if expected_keys.is_empty() {
                    false
                } else {
                    expected_matches_object_depth(
                        context,
                        actual_value,
                        expected_value,
                        &expected_keys,
                        depth + 1,
                    )?
                }
            }
        };
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}

fn error_instance(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<bool, RootedError> {
    let global = context.global_root()?;
    let error = get(context, global, "Error")?;
    let error_prototype = get(context, error, "prototype")?;
    let is_prototype_of = get(context, error_prototype, "isPrototypeOf")?;
    let result = context.call_rooted(is_prototype_of, error_prototype, &[value])?;
    let is_error = context
        .rooted_value(result)
        .is_some_and(|value| value.as_bool() == Some(true));
    context.release_root(result);
    Ok(is_error)
}

fn object_keys(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Vec<RootId>, RootedError> {
    let global = context.global_root()?;
    let object = get(context, global, "Object")?;
    let keys_function = get(context, object, "keys")?;
    let keys_array = context.call_rooted(keys_function, object, &[value])?;
    let length = get(context, keys_array, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .filter(|length| {
            length.is_finite()
                && *length >= 0.0
                && length.fract() == 0.0
                && *length <= usize::MAX as f64
        })
        .map(|length| length as usize)
        .ok_or_else(|| RootedError::host("Object.keys returned an invalid array length"))?;
    let mut keys = Vec::with_capacity(length);
    for index in 0..length {
        let index_key = context.string_rooted(&index.to_string());
        keys.push(context.get_property_rooted(keys_array, index_key)?);
    }
    Ok(keys)
}

fn assertion_message(
    context: &mut NativeContext<'_, NodeHost>,
    message: Option<RootId>,
    fallback: &str,
) -> Result<(String, bool), RootedError> {
    let Some(message) = message else {
        return Ok((fallback.to_owned(), true));
    };
    let value = context
        .rooted_value(message)
        .ok_or_else(|| RootedError::host("invalid assert message root"))?;
    if value.is_undefined() || value.is_null() {
        return Ok((fallback.to_owned(), true));
    }
    match context.string_text(message)? {
        Some(message) => Ok((message, false)),
        None => {
            if error_instance(context, message)? {
                let key = context.string_rooted("message");
                let text = context.get_property_rooted(message, key)?;
                if let Some(text) = context.string_text(text)? {
                    return Ok((text, false));
                }
            }
            let received = if let Some(number) = value.as_number() {
                format!("type number ({number})")
            } else if let Some(boolean) = value.as_bool() {
                format!("type boolean ({boolean})")
            } else {
                "an instance of Object".to_owned()
            };
            let error = context.type_error_rooted(&format!(
                "The \"message\" argument must be one of type string or function. Received {received}"
            ))?;
            let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
            set(context, error, "code", code)?;
            Err(context.throw(error))
        }
    }
}

fn assertion_error(
    context: &mut NativeContext<'_, NodeHost>,
    actual: RootId,
    expected: RootId,
    operator: &str,
    message: &str,
    generated: bool,
) -> Result<RootId, RootedError> {
    let error = make_assertion_error(context, actual, expected, operator, message, generated)?;
    Err(context.throw(error))
}

fn make_assertion_error(
    context: &mut NativeContext<'_, NodeHost>,
    actual: RootId,
    expected: RootId,
    operator: &str,
    message: &str,
    generated: bool,
) -> Result<RootId, RootedError> {
    let module = context
        .host_mut()
        .shared_state()
        .borrow()
        .assert_module
        .ok_or_else(|| RootedError::host("assert module is not initialized"))?;
    let constructor = get(context, module, "AssertionError")?;
    let options = context.object_rooted()?;
    let message = context.string_rooted(message);
    set(context, options, "message", message)?;
    set(context, options, "actual", actual)?;
    set(context, options, "expected", expected)?;
    let operator = context.string_rooted(operator);
    set(context, options, "operator", operator)?;
    let generated = context.boolean(generated);
    set(context, options, "generatedMessage", generated)?;
    let code = context.string_rooted("ERR_ASSERTION");
    set(context, options, "code", code)?;
    let diff = context.string_rooted(ASSERTION_DIFF);
    set(context, options, "diff", diff)?;
    context.construct_rooted(constructor, constructor, &[options])
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot set assert property {name}"
        )))
    }
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
