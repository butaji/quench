//! Shared-VM operations for the Node `assert` CommonJS builtin.

use crate::host::NodeHost;
use rqj::{NativeContext, RootId, RootedError};

const MISSING_ASSERT_ARGS: &str = "The \"actual\" and \"expected\" arguments must be specified";
const ASSERTION_FAILED: &str = "Expected values to be strictly equal";
const ASSERTION_NOT_OK: &str = "The expression evaluated to a falsy value";

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    if let Some(module) = context.host_mut().state().borrow().assert_module {
        return Ok(module);
    }

    let assert = context.host_function(crate::host::shared_vm::operation("assert"))?;
    let ok = context.host_function(crate::host::shared_vm::operation("ok"))?;
    let strict = context.host_function(crate::host::shared_vm::operation("strict"))?;
    let strict_equal = context.host_function(crate::host::shared_vm::operation("strictEqual"))?;
    let throws = context.host_function(crate::host::shared_vm::operation("throws"))?;

    set(context, assert, "ok", ok)?;
    set(context, assert, "strictEqual", strict_equal)?;
    set(context, assert, "strict", strict)?;
    set(context, assert, "throws", throws)?;
    set(context, strict, "ok", ok)?;
    set(context, strict, "strictEqual", strict_equal)?;
    set(context, strict, "equal", strict_equal)?;
    set(context, strict, "strict", strict)?;
    set(context, strict, "throws", throws)?;

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
        let error = context.type_error_rooted(MISSING_ASSERT_ARGS)?;
        let code = context.string_rooted("ERR_MISSING_ARGS");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
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
    let global = context.global_root()?;
    let object = get(context, global, "Object")?;
    let keys_function = get(context, object, "keys")?;
    let keys_array = context.call_rooted(keys_function, object, &[expected])?;
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
    if length == 0 {
        let error =
            context.type_error_rooted("The argument 'error' may not be an empty object.")?;
        let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
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
