//! Shared-VM operations for the Node `assert` CommonJS builtin.

use crate::host::NodeHost;
use quench_runtime_next::{NativeContext, RootId, RootedError};
use std::collections::HashSet;

const MISSING_ASSERT_ARGS: &str = "The \"actual\" and \"expected\" arguments must be specified";
const ASSERTION_FAILED: &str = "Expected values to be strictly equal";
const ASSERTION_NOT_OK: &str = "The expression evaluated to a falsy value";
const ASSERTION_NOT_UNEQUAL: &str = "Expected actual and expected to be strictly unequal";
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
    let match_string = context.host_function(crate::host::shared_vm::operation("match"))?;
    let fail = context.host_function(crate::host::shared_vm::operation("fail"))?;
    let throws = context.host_function(crate::host::shared_vm::operation("throws"))?;
    let inspect = get(context, util, "inspect")?;
    let if_error =
        context.host_function_with_data(crate::host::shared_vm::operation("ifError"), inspect)?;
    let assertion_error = context.evaluate_script_rooted(
        quench_js_check::checked_js!(
            r#"class AssertionError extends Error {
  constructor(options = {}) {
    super(options.message);
    this.generatedMessage = options.generatedMessage ?? !options.message;
    Object.defineProperty(this, "name", {
      value: "AssertionError [ERR_ASSERTION]",
      writable: true,
      configurable: true,
    });
    this.code = options.code ?? "ERR_ASSERTION";
    this.actual = options.actual;
    this.expected = options.expected;
    this.operator = options.operator;
    this.stack;
    this.name = "AssertionError";
    this.diff = options.diff ?? "simple";
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
    set(context, assert, "match", match_string)?;
    set(context, assert, "fail", fail)?;
    set(context, assert, "strict", strict)?;
    set(context, assert, "throws", throws)?;
    set(context, assert, "ifError", if_error)?;
    set(context, assert, "rejects", rejects)?;
    set(context, assert, "AssertionError", assertion_error)?;
    set(context, strict, "ok", ok)?;
    set(context, strict, "strictEqual", strict_equal)?;
    set(context, strict, "notStrictEqual", not_strict_equal)?;
    set(context, strict, "deepStrictEqual", deep_strict_equal)?;
    set(context, strict, "match", match_string)?;
    set(context, strict, "fail", fail)?;
    set(context, strict, "equal", strict_equal)?;
    set(context, strict, "strict", strict)?;
    set(context, strict, "throws", throws)?;
    set(context, strict, "ifError", if_error)?;
    set(context, strict, "rejects", rejects)?;
    set(context, strict, "AssertionError", assertion_error)?;

    let retained = context.retain(assert)?;
    context.host_mut().shared_state().borrow_mut().assert_module = Some(retained);
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

pub(crate) fn match_string(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() < 2 {
        return missing_assert_arguments(context);
    }
    let (actual, expected) = (args[0], args[1]);
    if context.string_text(actual)?.is_none() {
        let error = context.type_error_rooted("The \"string\" argument must be of type string")?;
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

fn regexp_match(
    context: &mut NativeContext<'_, NodeHost>,
    expected: RootId,
    thrown: RootId,
) -> Result<Option<bool>, RootedError> {
    let global = context.global_root()?;
    let object = get(context, global, "Object")?;
    let object_prototype = get(context, object, "prototype")?;
    let is_prototype_of = get(context, object_prototype, "isPrototypeOf")?;
    let regexp = get(context, global, "RegExp")?;
    let regexp_prototype = get(context, regexp, "prototype")?;
    let inherits_regexp = context.call_rooted(is_prototype_of, regexp_prototype, &[expected])?;
    if !context.truthy_rooted(inherits_regexp)? {
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
    for key in keys {
        let expected_value = context.get_property_rooted(expected, *key)?;
        let actual_value = context.get_property_rooted(actual, *key)?;
        let matches = match regexp_match(context, expected_value, actual_value)? {
            Some(matches) => matches,
            None => context.same_value_rooted(expected_value, actual_value)?,
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
