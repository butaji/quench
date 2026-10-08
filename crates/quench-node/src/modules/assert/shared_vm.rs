//! Shared-VM operations for the Node `assert` CommonJS builtin.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::collections::HashSet;

const MISSING_ASSERT_ARGS: &str = "The \"actual\" and \"expected\" arguments must be specified";
const ASSERTION_FAILED: &str = "Expected values to be strictly equal";
const ASSERTION_NOT_OK: &str = "The expression evaluated to a falsy value";
const ASSERTION_NOT_UNEQUAL: &str = "Expected actual and expected to be strictly unequal";

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    if let Some(module) = context.host_mut().state().borrow().assert_module {
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
    let fail = context.host_function(crate::host::shared_vm::operation("fail"))?;
    let throws = context.host_function(crate::host::shared_vm::operation("throws"))?;
    let assertion_error = context.evaluate_script_rooted(
        quench_js_check::checked_js!(
            r#"class AssertionError extends Error {
  constructor(options = {}) {
    super(options.message);
    this.name = "AssertionError";
    Object.assign(this, options);
  }
}
AssertionError"#
        ),
        "node:assert/AssertionError.js",
    )?;
    let rejects =
        context.evaluate_script_rooted(super::ASSERT_REJECTS, "node:assert/rejects.js")?;

    set(context, assert, "ok", ok)?;
    set(context, assert, "strictEqual", strict_equal)?;
    set(context, assert, "notStrictEqual", not_strict_equal)?;
    set(context, assert, "deepStrictEqual", deep_strict_equal)?;
    set(context, assert, "fail", fail)?;
    set(context, assert, "strict", strict)?;
    set(context, assert, "throws", throws)?;
    set(context, assert, "rejects", rejects)?;
    set(context, assert, "AssertionError", assertion_error)?;
    set(context, strict, "ok", ok)?;
    set(context, strict, "strictEqual", strict_equal)?;
    set(context, strict, "notStrictEqual", not_strict_equal)?;
    set(context, strict, "deepStrictEqual", deep_strict_equal)?;
    set(context, strict, "fail", fail)?;
    set(context, strict, "equal", strict_equal)?;
    set(context, strict, "strict", strict)?;
    set(context, strict, "throws", throws)?;
    set(context, strict, "rejects", rejects)?;
    set(context, strict, "AssertionError", assertion_error)?;

    let retained = context.retain(assert)?;
    context.host_mut().state().borrow_mut().assert_module = Some(retained);
    Ok(assert)
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
    let (message, generated) =
        assertion_message(context, args.get(2).copied(), ASSERTION_NOT_UNEQUAL)?;
    assertion_error(
        context,
        actual,
        expected,
        "notStrictEqual",
        &message,
        generated,
    )
}

pub(crate) fn deep_strict_equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() < 2 {
        return missing_assert_arguments(context);
    }
    let (actual, expected) = (args[0], args[1]);
    if deep_equal(context, actual, expected, &mut HashSet::new())? {
        return Ok(context.undefined());
    }
    let (message, generated) = assertion_message(
        context,
        args.get(2).copied(),
        "Expected values to be strictly deep-equal",
    )?;
    assertion_error(
        context,
        actual,
        expected,
        "deepStrictEqual",
        &message,
        generated,
    )
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
    seen: &mut HashSet<(RootId, RootId)>,
) -> Result<bool, RootedError> {
    if context.same_value_rooted(actual, expected)? {
        return Ok(true);
    }
    if is_primitive(context, actual) || is_primitive(context, expected) {
        return Ok(false);
    }
    if !seen.insert((actual, expected)) {
        return Ok(true);
    }
    if !same_prototype(context, actual, expected)?
        || array_kind(context, actual)? != array_kind(context, expected)?
    {
        return Ok(false);
    }
    let actual_keys = enumerable_key_names(context, actual)?;
    let expected_keys = enumerable_key_names(context, expected)?;
    if actual_keys != expected_keys {
        return Ok(false);
    }
    for key in actual_keys {
        let key = context.string_rooted(&key);
        let left = context.get_property_rooted(actual, key)?;
        let right = context.get_property_rooted(expected, key)?;
        if !deep_equal(context, left, right, seen)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn enumerable_key_names(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Vec<String>, RootedError> {
    let keys = object_keys(context, value)?;
    let mut names = keys
        .into_iter()
        .map(|key| {
            context
                .string_text(key)?
                .ok_or_else(|| RootedError::host("Object.keys returned a non-string key"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    Ok(names)
}

fn is_primitive(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_none_or(|value| !value.is_heap())
        || context.string_text(value).ok().flatten().is_some()
}

fn same_prototype(
    context: &mut NativeContext<'_, NodeHost>,
    actual: RootId,
    expected: RootId,
) -> Result<bool, RootedError> {
    let global = context.global_root()?;
    let object = get(context, global, "Object")?;
    let get_prototype = get(context, object, "getPrototypeOf")?;
    let actual_prototype = context.call_rooted(get_prototype, object, &[actual])?;
    let expected_prototype = context.call_rooted(get_prototype, object, &[expected])?;
    context.same_value_rooted(actual_prototype, expected_prototype)
}

fn array_kind(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<bool, RootedError> {
    let global = context.global_root()?;
    let array = get(context, global, "Array")?;
    let is_array = get(context, array, "isArray")?;
    let result = context.call_rooted(is_array, array, &[value])?;
    Ok(context
        .rooted_value(result)
        .is_some_and(|value| value.as_bool() == Some(true)))
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
        let matched = context.call_rooted(expected, undefined, &[thrown])?;
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
    if context.is_regexp_rooted(expected)? {
        let matcher = get(context, expected, "test")?;
        let actual_text = context.to_string(thrown)?;
        let actual = context.string_rooted(&actual_text);
        let matched = context.call_rooted(matcher, expected, &[actual])?;
        if context.truthy_rooted(matched)? {
            return Ok(thrown);
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
        Some(enumerable_keys(context, expected)?)
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

pub(crate) fn fail(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let message = match args.first().copied() {
        Some(message)
            if !context
                .rooted_value(message)
                .is_some_and(|value| value.is_undefined()) =>
        {
            context.to_string(message)?
        }
        _ => "Failed".to_owned(),
    };
    let undefined = context.undefined();
    assertion_error(context, undefined, undefined, "fail", &message, false)
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
    for key in keys {
        let expected_value = context.get_property_rooted(expected, *key)?;
        let actual_value = context.get_property_rooted(actual, *key)?;
        if !context.same_value_rooted(expected_value, actual_value)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn enumerable_keys(
    context: &mut NativeContext<'_, NodeHost>,
    expected: RootId,
) -> Result<Vec<RootId>, RootedError> {
    let keys = object_keys(context, expected)?;
    if keys.is_empty() {
        let error =
            context.type_error_rooted("The argument 'error' may not be an empty object.")?;
        let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    Ok(keys)
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
    let error = context.error_rooted(message)?;
    let name = context.string_rooted("AssertionError");
    let code = context.string_rooted("ERR_ASSERTION");
    let operator = context.string_rooted(operator);
    let generated = context.boolean(generated);
    set(context, error, "name", name)?;
    set(context, error, "code", code)?;
    set(context, error, "actual", actual)?;
    set(context, error, "expected", expected)?;
    set(context, error, "operator", operator)?;
    set(context, error, "generatedMessage", generated)?;
    Err(context.throw(error))
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
