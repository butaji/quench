use crate::{
    facts::ProgramDb,
    ops::Op,
    value::{ObjectData, Value},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
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
    match target {
        Value::Builtin(builtin) => {
            let value = construct_builtin(*builtin, arguments)?;
            with_new_target_prototype(value, target, new_target)
        }
        Value::Function(function) => construct_function(function, new_target, arguments),
        Value::BoundFunction(bound) => construct_bound(bound, target, arguments),
        _ => Err(crate::vm::not_callable()),
    }
}
fn with_new_target_prototype(
    value: Value,
    target: &Value,
    new_target: &Value,
) -> Result<Value, crate::execute::VmError> {
    if crate::builtins::same_value(Some(target), Some(new_target)) {
        return Ok(value);
    }
    let prototype = crate::execute::get_property_result(new_target, "prototype")?;
    let prototype = if crate::value::is_object(&prototype) {
        Some(prototype)
    } else {
        realm_default_prototype(target, new_target)
    };
    Ok(prototype.map_or(value.clone(), |prototype| {
        crate::builtins::set_property(value, "\0prototype", prototype)
    }))
}
include!("construct_realm.rs");
fn construct_bound(
    bound: &crate::value::BoundFunctionValue,
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Value::Builtin(builtin) = &bound.target else {
        return Err(crate::vm::not_callable());
    };
    if crate::builtin_meta::constructor_name(*builtin).is_none() {
        return Err(crate::vm::not_callable());
    }
    let mut combined = bound.arguments.clone();
    combined.extend_from_slice(arguments);
    let value = construct_bound_in_realm(bound, *builtin, &combined)?;
    let value = if let Value::HostCapability(capability) = &bound.receiver {
        crate::builtins::set_property(value, "\0realm", Value::HostCapability(capability.clone()))
    } else {
        value
    };
    Ok(crate::builtins::set_property(
        value,
        "constructor",
        target.clone(),
    ))
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
        crate::ops::Builtin::RegExp => construct_regexp(arguments),
        _ if is_intl_constructor(builtin) => crate::intl::execute(builtin, arguments, None)
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

fn is_intl_constructor(builtin: crate::ops::Builtin) -> bool {
    use crate::ops::Builtin;
    matches!(
        builtin,
        Builtin::IntlNumberFormat
            | Builtin::IntlDateTimeFormat
            | Builtin::IntlCollator
            | Builtin::IntlPluralRules
            | Builtin::IntlListFormat
            | Builtin::IntlRelativeTimeFormat
            | Builtin::IntlSegmenter
            | Builtin::IntlDisplayNames
            | Builtin::IntlLocale
    )
}

fn construct_regexp(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let source = arguments
        .first()
        .map_or_else(|| Ok(String::new()), crate::conversion::to_string)?;
    let flags = arguments
        .get(1)
        .map_or_else(|| Ok(String::new()), crate::conversion::to_string)?;
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
    let buffer = match arguments.get(1) {
        Some(options) => resizable_array_buffer(length, options)?,
        None => crate::value::ArrayBufferData::new(length),
    };
    Ok(Value::ArrayBuffer(Rc::new(buffer)))
}

fn resizable_array_buffer(
    length: usize,
    options: &Value,
) -> Result<crate::value::ArrayBufferData, crate::execute::VmError> {
    let maximum = crate::execute::get_property(options, "maxByteLength");
    let maximum = to_index(crate::intl::tolocale::value::to_number(Some(&maximum)))?;
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
    if *buffer.detached.borrow() {
        return Err(type_error("Cannot use a detached ArrayBuffer"));
    }
    let offset = arguments.get(1).map_or(0.0, |value| {
        crate::intl::tolocale::value::to_number(Some(value))
    });
    let offset = to_index(offset)?;
    let buffer_length = buffer.byte_length();
    if offset > buffer_length {
        return Err(range_error("Invalid DataView byte offset"));
    }
    let available = buffer_length - offset;
    let length = match arguments.get(2) {
        Some(value) => to_index(crate::intl::tolocale::value::to_number(Some(value)))?,
        None => available,
    };
    if length > available {
        return Err(range_error("Invalid DataView byte length"));
    }
    Ok(Value::DataView(Rc::new(crate::value::DataViewData::new(
        buffer.clone(),
        offset,
        length,
    ))))
}

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
    let value = match arguments.first() {
        Some(argument) => crate::conversion::to_number(argument)?,
        None => 0.0,
    };
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
    let mut properties = vec![
        ("_value".to_string(), value),
        ("constructor".to_string(), Value::Builtin(constructor)),
    ];
    if let Some(prototype) = crate::builtin_meta::instance_prototype(constructor) {
        properties.push(("\0prototype".to_string(), Value::Builtin(prototype)));
    }
    Value::Object(std::rc::Rc::new(ObjectData::new(properties)))
}

fn construct_error(
    builtin: &crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    Ok(crate::builtins::error(*builtin, arguments))
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
        .find_map(|(name, value)| (name == "\0derived_constructor").then(|| value.clone()))
        .ok_or_else(|| crate::value::error::throw_reference_error("super is unavailable"))
}

include!("construct_tail.rs");
