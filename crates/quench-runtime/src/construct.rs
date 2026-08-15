use crate::{
    facts::ProgramDb,
    ops::Op,
    value::{ObjectData, Value},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
const MAX_HOST_BUFFER_BYTES: usize = 256 * 1024 * 1024;
pub(crate) fn reduce(
    expression: &oxc::ast::ast::NewExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let callee =
        crate::reduce::reduce_expression(&expression.callee, ops, facts, next_register, locals)?;
    let (args, spreads) = crate::reduce::reduce_expressions::calls_reduce::reduce_arguments(
        &expression.arguments,
        ops,
        facts,
        next_register,
        locals,
    )?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Construct {
        dst,
        callee,
        args,
        spreads,
    });
    Some(dst)
}
pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<(), crate::execute::VmError> {
    let Op::Construct {
        dst,
        callee,
        args,
        spreads,
    } = op
    else {
        return Err(crate::execute::VmError::NotCallable);
    };
    let arguments = collect_construct_arguments(registers, args, spreads)?;
    let target = crate::execute::read_register(registers, *callee)?;
    let value = construct_value(&target, &arguments)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}
fn collect_construct_arguments(
    registers: &[Value],
    args: &[u16],
    spreads: &[bool],
) -> Result<Vec<Value>, crate::execute::VmError> {
    let mut arguments = Vec::new();
    for (index, spread) in args.iter().zip(spreads) {
        let value = crate::execute::read_register(registers, *index)?;
        if *spread {
            arguments.extend(crate::collections::iterator::collect_iterable(value)?);
        } else {
            arguments.push(value);
        }
    }
    Ok(arguments)
}
pub(crate) fn construct_value(
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    construct_with_new_target(target, target, arguments)
}
pub(crate) fn construct_value_with_new_target(
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    construct_with_new_target(target, new_target, arguments)
}
pub(crate) fn construct_super(
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    construct_with_new_target(target, new_target, arguments)
}
fn construct_with_new_target(
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let result = match target {
        Value::Builtin(builtin) => {
            let prefetched = prefetch_buffer_prototype(*builtin, target, new_target)?;
            let value = construct_builtin(*builtin, arguments)?;
            let value = with_new_target_prototype_cached(value, target, new_target, prefetched)?;
            if let Value::DataView(view) = &value {
                if *view.buffer.detached.borrow() {
                    return Err(type_error("Cannot use a detached ArrayBuffer"));
                }
                if view.is_out_of_bounds() {
                    return Err(range_error("Invalid DataView byte length"));
                }
            }
            Ok(value)
        }
        Value::Function(function) => construct_function(function, new_target, arguments),
        Value::BoundFunction(bound) => construct_bound(bound, target, new_target, arguments),
        _ => Err(crate::vm::not_callable()),
    };
    result
}
fn prefetch_buffer_prototype(
    builtin: crate::ops::Builtin,
    target: &Value,
    new_target: &Value,
) -> Result<Option<Value>, crate::execute::VmError> {
    let buffer = matches!(
        builtin,
        crate::ops::Builtin::ArrayBuffer | crate::ops::Builtin::SharedArrayBuffer
    );
    if buffer && !crate::builtins::same_value(Some(target), Some(new_target)) {
        return crate::execute::get_property_result(new_target, "prototype").map(Some);
    }
    Ok(None)
}
fn with_new_target_prototype(
    value: Value,
    target: &Value,
    new_target: &Value,
) -> Result<Value, crate::execute::VmError> {
    with_new_target_prototype_cached(value, target, new_target, None)
}

fn with_new_target_prototype_cached(
    value: Value,
    target: &Value,
    new_target: &Value,
    prefetched: Option<Value>,
) -> Result<Value, crate::execute::VmError> {
    if crate::builtins::same_value(Some(target), Some(new_target)) {
        return Ok(value);
    }
    let prototype = prefetched
        .map(Ok)
        .unwrap_or_else(|| crate::execute::get_property_result(new_target, "prototype"))?;
    let prototype = if crate::value::is_object(&prototype) {
        Some(prototype)
    } else {
        realm_default_prototype(target, new_target)
    };
    Ok(prototype.map_or(value.clone(), |prototype| {
        let value = crate::builtins::set_property(value, "\0prototype", prototype);
        drop_shadowed_error_constructor(value)
    }))
}

fn drop_shadowed_error_constructor(mut value: Value) -> Value {
    let Value::Object(data) = &mut value else {
        return value;
    };
    let data = std::rc::Rc::make_mut(data);
    let is_error = data
        .properties
        .iter()
        .any(|(key, _)| key == crate::builtins::ERROR_SLOT);
    if is_error {
        data.properties.retain(|(key, _)| key != "constructor");
    }
    value
}
include!("construct_realm.rs");
fn construct_bound(
    bound: &crate::value::BoundFunctionValue,
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let mut combined = bound.arguments.clone();
    combined.extend_from_slice(arguments);
    let same = crate::builtins::same_value(Some(target), Some(new_target));
    let new_target = if same {
        bound.target.clone()
    } else {
        new_target.to_owned()
    };
    let value = match &bound.target {
        Value::Builtin(builtin) => {
            if crate::builtin_meta::constructor_name(*builtin).is_none() {
                return Err(crate::vm::not_callable());
            }
            let value = construct_bound_in_realm(bound, *builtin, &combined)?;
            with_new_target_prototype(value, &Value::Builtin(*builtin), &new_target)?
        }
        Value::Function(function) => construct_function(function, &new_target, &combined)?,
        Value::BoundFunction(next) => construct_bound(
            next,
            &Value::BoundFunction(std::rc::Rc::clone(next)),
            &new_target,
            &combined,
        )?,
        _ => return Err(crate::vm::not_callable()),
    };
    Ok(if let Value::HostCapability(capability) = &bound.receiver {
        crate::builtins::set_property(value, "\0realm", Value::HostCapability(capability.clone()))
    } else {
        value
    })
}
fn construct_builtin(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if is_error_builtin(builtin) {
        return construct_error(&builtin, arguments);
    }
    if let Some(result) = crate::functions_dynamic::construct_builtin(builtin, arguments) {
        return result;
    }
    if let Some(result) = construct_typed_builtin(builtin, arguments) {
        return result;
    }
    match builtin {
        crate::ops::Builtin::Array => Ok(crate::builtins::array(arguments)),
        crate::ops::Builtin::ArrayBuffer => construct_array_buffer(arguments),
        crate::ops::Builtin::SharedArrayBuffer => construct_shared_array_buffer(arguments),
        crate::ops::Builtin::DataView => construct_data_view(arguments),
        crate::ops::Builtin::Object => Ok(crate::builtins::object(arguments)),
        crate::ops::Builtin::Number => construct_number(arguments),
        crate::ops::Builtin::Boolean => construct_boolean(arguments),
        crate::ops::Builtin::String => construct_string(arguments),
        crate::ops::Builtin::Promise => construct_promise(arguments),
        crate::ops::Builtin::Proxy => crate::proxy::proxy_new(arguments),
        crate::ops::Builtin::Map => crate::collections::map::map_new(arguments),
        crate::ops::Builtin::Set => crate::collections::set::set_new(arguments),
        crate::ops::Builtin::WeakMap => crate::collections::map::weak_map_new(arguments),
        crate::ops::Builtin::WeakSet => crate::collections::set::weak_set_new(arguments),
        crate::ops::Builtin::WeakRef => construct_weak_ref(arguments),
        crate::ops::Builtin::Date => crate::date::execute(builtin, None, arguments)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        crate::ops::Builtin::DisposableStack => crate::disposable_stack::construct(),
        crate::ops::Builtin::AsyncDisposableStack => crate::disposable_stack::construct_async(),
        crate::ops::Builtin::FinalizationRegistry => {
            crate::finalization_registry::construct(arguments)
        }
        crate::ops::Builtin::RegExp => construct_regexp(arguments),
        _ if crate::intl::is_constructor(builtin) => crate::intl::execute(builtin, arguments, None)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        _ => Err(crate::vm::not_callable()),
    }
}

fn construct_weak_ref(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(target) = arguments
        .first()
        .filter(|value| {
            crate::value::is_object(value)
                || matches!(value, crate::value::Value::String(text) if crate::conversion::is_symbol(value) && !text.starts_with("Symbol.for."))
        })
    else {
        return Err(crate::value::error::throw_type_error(
            "WeakRef target must be an object",
        ));
    };
    Ok(Value::Object(Rc::new(ObjectData::new(vec![
        ("\0weakref".to_string(), target.clone()),
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::WeakRefPrototype),
        ),
    ]))))
}
fn construct_shared_array_buffer(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Value::ArrayBuffer(mut buffer) = construct_array_buffer(arguments)? else {
        return Err(crate::vm::not_callable());
    };
    Rc::make_mut(&mut buffer).shared = true;
    Ok(Value::ArrayBuffer(buffer))
}

fn construct_typed_builtin(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    use crate::ops::Builtin::*;
    Some(match builtin {
        Float64Array => construct_float64_array(arguments),
        Float32Array => construct_float32_array(arguments),
        Int8Array => construct_int8_array(arguments),
        Int16Array => construct_int16_array(arguments),
        Int32Array => construct_int32_array(arguments),
        Uint8Array => construct_uint8_array(arguments),
        Uint16Array => construct_uint16_array(arguments),
        Uint32Array => construct_uint32_array(arguments),
        Uint8ClampedArray => construct_uint8_clamped_array(arguments),
        BigInt64Array => construct_bigint64_array(arguments),
        BigUint64Array => construct_biguint64_array(arguments),
        _ => return None,
    })
}

fn construct_regexp(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let source = arguments
        .first()
        .map_or_else(|| Ok(String::new()), crate::conversion::to_string)?;
    let flags = arguments
        .get(1)
        .map_or_else(|| Ok(String::new()), crate::conversion::to_string)?;
    validate_regexp_pattern(&source, &flags)?;
    let last_index = Value::BindingCell(Rc::new(RefCell::new(Value::Number(0.0))));
    let mut entries = vec![
        ("\0regexp".to_string(), Value::Boolean(true)),
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::RegExpPrototype),
        ),
        ("source".to_string(), Value::String(source.clone())),
        (
            crate::builtins::descriptor_key("source"),
            regexp_data_descriptor(false, true, Value::String(source)),
        ),
        ("flags".to_string(), Value::String(flags.clone())),
        (
            crate::builtins::descriptor_key("flags"),
            regexp_data_descriptor(false, true, Value::String(flags.clone())),
        ),
        ("lastIndex".to_string(), last_index),
        (
            crate::builtins::descriptor_key("lastIndex"),
            regexp_data_descriptor(true, false, Value::Number(0.0)),
        ),
    ];
    entries.extend(regexp_flag_entries(&flags));
    Ok(Value::Object(Rc::new(ObjectData::new(entries))))
}

fn validate_regexp_pattern(source: &str, flags: &str) -> Result<(), crate::execute::VmError> {
    crate::regexp::validate_flags(flags)
        .map_err(|error| crate::value::error::throw_syntax_error(&error))?;
    crate::regexp::validate_literal(source)
        .map_err(|error| crate::value::error::throw_syntax_error(&error))?;
    crate::regexp::validate_pattern(source)
        .map_err(|error| crate::value::error::throw_syntax_error(&error))?;
    if flags.contains('u') {
        crate::regexp::validate_unicode(source, flags)
            .map_err(|error| crate::value::error::throw_syntax_error(&error))?;
    }
    Ok(())
}

fn regexp_data_descriptor(writable: bool, configurable: bool, value: Value) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(writable)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(configurable)),
    ])))
}

fn regexp_flag_entries(flags: &str) -> Vec<(String, Value)> {
    let mut entries = Vec::new();
    for (flag, enabled) in [
        ("global", flags.contains('g')),
        ("ignoreCase", flags.contains('i')),
        ("multiline", flags.contains('m')),
        ("dotAll", flags.contains('s')),
        ("unicode", flags.contains('u')),
        ("unicodeSets", flags.contains('v')),
        ("sticky", flags.contains('y')),
    ] {
        let descriptor = regexp_data_descriptor(false, true, Value::Boolean(enabled));
        entries.push((flag.to_string(), Value::Boolean(enabled)));
        entries.push((crate::builtins::descriptor_key(flag), descriptor));
    }
    entries
}

fn is_error_builtin(builtin: crate::ops::Builtin) -> bool {
    matches!(
        builtin,
        crate::ops::Builtin::TypeError
            | crate::ops::Builtin::Error
            | crate::ops::Builtin::RangeError
            | crate::ops::Builtin::ReferenceError
            | crate::ops::Builtin::SyntaxError
            | crate::ops::Builtin::EvalError
            | crate::ops::Builtin::URIError
            | crate::ops::Builtin::AggregateError
            | crate::ops::Builtin::SuppressedError
    )
}

fn construct_promise(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let executor = arguments.first().ok_or_else(crate::vm::not_callable)?;
    crate::promise::construct_promise(executor)
}

fn construct_array_buffer(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let length = arguments.first().map_or(0.0, |value| {
        crate::intl::tolocale::value::to_number(Some(value))
    });
    let length = to_index(length)?;
    if length > MAX_HOST_BUFFER_BYTES {
        return Err(range_error("ArrayBuffer allocation is too large"));
    }
    let buffer = match arguments.get(1) {
        Some(options) if crate::value::is_object(options) => {
            resizable_array_buffer(length, options)?
        }
        None => crate::value::ArrayBufferData::new(length),
        _ => crate::value::ArrayBufferData::new(length),
    };
    Ok(Value::ArrayBuffer(Rc::new(buffer)))
}

fn resizable_array_buffer(
    length: usize,
    options: &Value,
) -> Result<crate::value::ArrayBufferData, crate::execute::VmError> {
    let maximum = crate::execute::get_property_result(options, "maxByteLength")?;
    if matches!(maximum, Value::Undefined) {
        return Ok(crate::value::ArrayBufferData::new(length));
    }
    let maximum = to_index(crate::conversion::to_number(&maximum)?)?;
    if maximum > MAX_HOST_BUFFER_BYTES {
        return Err(range_error("ArrayBuffer allocation is too large"));
    }
    if maximum < length {
        return Err(range_error("maxByteLength is smaller than byteLength"));
    }
    Ok(crate::value::ArrayBufferData::new_resizable(
        length, maximum,
    ))
}

fn view_length(buffer: &crate::value::ArrayBufferData, fixed_length: usize) -> usize {
    if buffer.max_byte_length.is_some() {
        usize::MAX
    } else {
        fixed_length
    }
}

fn construct_data_view(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(Value::ArrayBuffer(buffer)) = arguments.first() else {
        return Err(type_error("DataView buffer must be an ArrayBuffer"));
    };
    let offset = match arguments.get(1) {
        Some(value) => crate::conversion::to_number(value)?,
        None => 0.0,
    };
    let offset = to_index(offset)?;
    if *buffer.detached.borrow() {
        return Err(type_error("Cannot use a detached ArrayBuffer"));
    }
    let buffer_length = buffer.byte_length();
    if offset > buffer_length {
        return Err(range_error("Invalid DataView byte offset"));
    }
    let available = buffer_length - offset;
    let length = match arguments.get(2) {
        Some(Value::Undefined) | None => view_length(buffer, available),
        Some(value) => {
            let length = to_index(crate::conversion::to_number(value)?)?;
            if *buffer.detached.borrow() {
                return Err(type_error("Cannot use a detached ArrayBuffer"));
            }
            length
        }
    };
    if length != usize::MAX && length > buffer_length - offset {
        return Err(range_error("Invalid DataView byte length"));
    }
    Ok(Value::DataView(Rc::new(crate::value::DataViewData::new(
        buffer.clone(),
        offset,
        length,
    ))))
}

include!("construct_typed_length.rs");
include!("construct_typed_low.rs");
include!("construct_typed_high.rs");
include!("construct_typed_bigint.rs");

fn range_error(message: &str) -> crate::execute::VmError {
    crate::execute::VmError::Thrown(crate::builtins::error(
        crate::ops::Builtin::RangeError,
        &[Value::String(message.to_string())],
    ))
}

fn construct_number(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = crate::vm::explicit_number(arguments.first())?;
    Ok(boxed_primitive(
        Value::Number(value),
        crate::ops::Builtin::Number,
    ))
}

fn construct_boolean(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = crate::execute::execute_builtin_with_receiver(
        crate::ops::Builtin::Boolean,
        arguments,
        None,
    )?;
    Ok(boxed_primitive(value, crate::ops::Builtin::Boolean))
}

fn construct_string(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = crate::execute::execute_builtin_with_receiver(
        crate::ops::Builtin::String,
        arguments,
        None,
    )?;
    Ok(boxed_primitive(value, crate::ops::Builtin::String))
}

fn boxed_primitive(value: Value, constructor: crate::ops::Builtin) -> Value {
    let mut properties = vec![("_value".to_string(), value)];
    if let Some(prototype) = crate::builtin_meta::instance_prototype(constructor) {
        properties.push(("\0prototype".to_string(), Value::Builtin(prototype)));
    }
    Value::Object(std::rc::Rc::new(ObjectData::new(properties)))
}

fn construct_error(
    builtin: &crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if *builtin == crate::ops::Builtin::SuppressedError {
        return crate::builtins::suppressed_error(arguments);
    }
    if *builtin == crate::ops::Builtin::AggregateError {
        return construct_aggregate_error(arguments);
    }

    let name = match builtin {
        crate::ops::Builtin::RangeError => "RangeError",
        crate::ops::Builtin::ReferenceError => "ReferenceError",
        crate::ops::Builtin::SyntaxError => "SyntaxError",
        crate::ops::Builtin::EvalError => "EvalError",
        crate::ops::Builtin::URIError => "URIError",
        crate::ops::Builtin::AggregateError => "AggregateError",
        crate::ops::Builtin::TypeError => "TypeError",
        _ => "Error",
    };

    let mut properties = vec![
        ("name".to_string(), Value::String(name.to_string())),
        (
            crate::builtins::ERROR_SLOT.to_string(),
            Value::Boolean(true),
        ),
        (
            "\0prototype".to_string(),
            Value::Builtin(
                crate::builtin_meta::instance_prototype(*builtin)
                    .unwrap_or(crate::ops::Builtin::ErrorPrototype),
            ),
        ),
    ];
    if let Some(message) = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
    {
        properties.push((
            "message".to_string(),
            Value::String(crate::conversion::to_string(message)?),
        ));
    }

    if let Some(cause_source) = arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined))
    {
        let options = to_object(cause_source)?;
        if crate::with_scope::has_property(&options, "cause")? {
            let cause = crate::execute::get_property_result(&options, "cause")?;
            properties.push(("cause".to_string(), cause));
        }
    }

    for key in ["name", "message", "cause"] {
        if let Some((_, value)) = properties.iter().rev().find(|(current, _)| current == key) {
            properties.push((
                crate::builtins::descriptor_key(key),
                non_enumerable_descriptor(value),
            ));
        }
    }

    Ok(Value::Object(std::rc::Rc::new(ObjectData::new(properties))))
}

fn non_enumerable_descriptor(value: &Value) -> Value {
    Value::Object(std::rc::Rc::new(ObjectData::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}

fn to_object(value: &Value) -> Result<Value, crate::execute::VmError> {
    match value {
        Value::Object(_)
        | Value::Array(_)
        | Value::ObjectAlias(_)
        | Value::Function(_)
        | Value::BoundFunction(_)
        | Value::Builtin(_)
        | Value::Proxy(_)
        | Value::Promise(_)
        | Value::Map(_)
        | Value::Set(_)
        | Value::ArrayBuffer(_)
        | Value::DataView(_)
        | Value::Float32Array(_)
        | Value::Float64Array(_)
        | Value::Int8Array(_)
        | Value::Int16Array(_)
        | Value::Int32Array(_)
        | Value::Uint8Array(_)
        | Value::Uint8ClampedArray(_)
        | Value::Uint16Array(_)
        | Value::Uint32Array(_)
        | Value::BigInt64Array(_)
        | Value::BigUint64Array(_)
        | Value::Iterator(_)
        | Value::Generator(_)
        | Value::HostCapability(_) => Ok(value.clone()),

        Value::Number(value) => Ok(boxed_primitive(
            Value::Number(*value),
            crate::ops::Builtin::Number,
        )),
        Value::Boolean(value) => Ok(boxed_primitive(
            Value::Boolean(*value),
            crate::ops::Builtin::Boolean,
        )),
        Value::String(value) => Ok(boxed_primitive(
            Value::String(value.clone()),
            crate::ops::Builtin::String,
        )),
        Value::StringUnits(value) => Ok(boxed_primitive(
            Value::StringUnits(value.clone()),
            crate::ops::Builtin::String,
        )),
        Value::BigInt(value) => Ok(boxed_primitive(
            Value::BigInt(value.clone()),
            crate::ops::Builtin::BigInt,
        )),
        Value::BindingCell(_) | Value::Undefined | Value::Null => Err(
            crate::value::error::throw_type_error("Cannot convert undefined or null to object"),
        ),
    }
}

fn construct_function(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if !crate::functions::is_constructible(function) {
        return Err(crate::vm::not_callable());
    }
    if is_default_derived_constructor(function) {
        let super_constructor = derived_constructor(function)?;
        return construct_with_new_target(&super_constructor, target, arguments);
    }
    if derived_constructor(function).is_ok() {
        let _context = crate::super_scope::Guard::install(function, &Value::Undefined);
        let (result, final_this) =
            crate::functions::execute_construct(function, &Value::Undefined, target, arguments)?;
        if crate::value::is_object(&result) {
            return Ok(result);
        }
        if crate::value::is_object(&final_this) {
            return Ok(final_this);
        }
        return Err(crate::value::error::throw_reference_error(
            "Derived constructor did not initialize this",
        ));
    }
    let object = initialize_instance_fields(function, constructor_receiver(target))?;
    let (result, final_this) =
        crate::functions::execute_construct(function, &object, target, arguments)?;
    if crate::value::is_object(&result) {
        Ok(result)
    } else if crate::value::is_object(&final_this) {
        Ok(final_this)
    } else {
        Ok(object)
    }
}

pub(crate) fn initialize_instance_fields(
    function: &crate::value::FunctionValue,
    receiver: Value,
) -> Result<Value, crate::execute::VmError> {
    initialize_instance_fields_impl(function, receiver)
}
include!("construct_instance_fields.rs");
pub(crate) fn derived_constructor(
    function: &crate::value::FunctionValue,
) -> Result<Value, crate::execute::VmError> {
    function
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))
        .ok_or_else(|| crate::value::error::throw_reference_error("super is unavailable"))
}

include!("construct_tail.rs");
