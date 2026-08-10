use std::{collections::HashMap, rc::Rc};

use crate::{
    facts::ProgramDb,
    ops::Op,
    value::{ObjectData, Value},
};

pub(crate) fn reduce(
    expression: &oxc::ast::ast::NewExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let callee =
        crate::reduce::reduce_expression(&expression.callee, ops, facts, next_register, locals)?;
    let args = expression
        .arguments
        .iter()
        .map(|argument| {
            crate::reduce::reduce_expression(
                argument.as_expression()?,
                ops,
                facts,
                next_register,
                locals,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Construct { dst, callee, args });
    Some(dst)
}

pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<(), crate::execute::VmError> {
    let Op::Construct { dst, callee, args } = op else {
        return Err(crate::execute::VmError::NotCallable);
    };
    let arguments = args
        .iter()
        .map(|index| crate::execute::read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    let target = crate::execute::read_register(registers, *callee)?;
    let value = construct_value(&target, &arguments)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

pub(crate) fn construct_value(
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    construct_with_new_target(target, target, arguments)
}

pub(crate) fn construct_super(
    target: &Value,
    new_target: &std::rc::Rc<crate::value::FunctionValue>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let new_target = Value::Function(std::rc::Rc::clone(new_target));
    construct_with_new_target(target, &new_target, arguments)
}

fn construct_with_new_target(
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match target {
        Value::Builtin(builtin) => construct_builtin(*builtin, arguments),
        Value::Function(function) => construct_function(function, new_target, arguments),
        Value::BoundFunction(bound) => construct_bound(bound, target, arguments),
        _ => Err(crate::vm::not_callable()),
    }
}

fn construct_bound(
    bound: &crate::value::BoundFunctionValue,
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Value::Builtin(builtin) = &bound.target else {
        return Err(crate::vm::not_callable());
    };
    let mut combined = bound.arguments.clone();
    combined.extend_from_slice(arguments);
    let value = construct_builtin(*builtin, &combined)?;
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
        crate::ops::Builtin::DataView => construct_data_view(arguments),
        crate::ops::Builtin::Object => Ok(crate::builtins::object(arguments)),
        crate::ops::Builtin::Number => construct_number(arguments),
        crate::ops::Builtin::Boolean => construct_boolean(arguments),
        crate::ops::Builtin::String => construct_string(arguments),
        crate::ops::Builtin::Promise => construct_promise(arguments),
        crate::ops::Builtin::Proxy => crate::proxy::proxy_new(arguments),
        crate::ops::Builtin::Map | crate::ops::Builtin::Set => {
            crate::collections::execute_builtin(builtin, None, arguments)
                .unwrap_or_else(|| Err(crate::vm::not_callable()))
        }
        crate::ops::Builtin::Date => crate::date::execute(builtin, None, arguments)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        crate::ops::Builtin::RegExp => Ok(construct_regexp(arguments)),
        _ if is_intl_constructor(builtin) => crate::intl::execute(builtin, arguments, None)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        _ => Err(crate::vm::not_callable()),
    }
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

fn construct_regexp(arguments: &[Value]) -> Value {
    let source = arguments.first().map_or_else(String::new, |value| {
        crate::intl::tolocale::value::to_string(Some(value))
    });
    let flags = arguments.get(1).map_or_else(String::new, |value| {
        crate::intl::tolocale::value::to_string(Some(value))
    });
    let has = |flag: char| flags.contains(flag);
    let data_descriptor = |writable: bool, configurable: bool, value: Value| {
        Value::Object(Rc::new(ObjectData::new(vec![
            ("value".to_string(), value),
            ("writable".to_string(), Value::Boolean(writable)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(configurable)),
        ])))
    };
    let source_descriptor = data_descriptor(false, false, Value::String(source.clone()));
    let flags_descriptor = data_descriptor(false, false, Value::String(flags.clone()));
    let last_index_descriptor = data_descriptor(true, false, Value::Number(0.0));
    let mut entries = vec![
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::RegExpPrototype),
        ),
        ("source".to_string(), Value::String(source)),
        (crate::builtins::descriptor_key("source"), source_descriptor),
        ("flags".to_string(), Value::String(flags.clone())),
        (crate::builtins::descriptor_key("flags"), flags_descriptor),
        ("lastIndex".to_string(), Value::Number(0.0)),
        (
            crate::builtins::descriptor_key("lastIndex"),
            last_index_descriptor,
        ),
    ];
    for flag in [
        "global",
        "ignoreCase",
        "multiline",
        "dotAll",
        "unicode",
        "sticky",
    ] {
        let enabled = match flag {
            "global" => has('g'),
            "ignoreCase" => has('i'),
            "multiline" => has('m'),
            "dotAll" => has('s'),
            "unicode" => has('u'),
            "sticky" => has('y'),
            _ => false,
        };
        let descriptor = data_descriptor(false, false, Value::Boolean(enabled));
        entries.push((flag.to_string(), Value::Boolean(enabled)));
        entries.push((crate::builtins::descriptor_key(flag), descriptor));
    }
    Value::Object(Rc::new(ObjectData::new(entries)))
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
    let value = arguments.first().map_or(0.0, |argument| {
        crate::intl::tolocale::value::to_number(Some(argument))
    });
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
    Value::Object(std::rc::Rc::new(ObjectData::new(vec![
        ("_value".to_string(), value),
        ("constructor".to_string(), Value::Builtin(constructor)),
    ])))
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
            crate::functions::execute_construct(function, &Value::Undefined, arguments)?;
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
    let (result, final_this) = crate::functions::execute_construct(function, &object, arguments)?;
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
    mut receiver: Value,
) -> Result<Value, crate::execute::VmError> {
    for field in function.instance_fields.borrow().iter() {
        let previous = receiver.clone();
        receiver = initialize_instance_field(function, field, receiver)?;
        crate::locals::replace_value(&previous, &receiver);
    }
    Ok(receiver)
}

fn initialize_instance_field(
    function: &crate::value::FunctionValue,
    field: &crate::value::InstanceFieldPlan,
    receiver: Value,
) -> Result<Value, crate::execute::VmError> {
    let value = match &field.initializer {
        crate::value::InstanceFieldInitializer::Undefined => Value::Undefined,
        crate::value::InstanceFieldInitializer::Callable(initializer) => {
            crate::functions::execute(initializer, &receiver, &[])?
        }
        crate::value::InstanceFieldInitializer::Value(value) => value.clone(),
    };
    match &field.key {
        crate::value::InstanceFieldKey::Private(id) => {
            let name = function.private_environment.resolve(*id).ok_or_else(|| {
                crate::value::error::throw_type_error(
                    "Private field access on an object without the required brand",
                )
            })?;
            crate::private_slots::define(&receiver, name, value)?;
            Ok(receiver)
        }
        crate::value::InstanceFieldKey::Static(key) => {
            define_instance_field(receiver, key.as_ref(), value)
        }
        crate::value::InstanceFieldKey::Dynamic(key) => {
            let key = crate::conversion::to_property_key(key)?;
            define_instance_field(receiver, &key, value)
        }
    }
}

fn define_instance_field(
    receiver: Value,
    key: &str,
    value: Value,
) -> Result<Value, crate::execute::VmError> {
    let descriptor = [
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ];
    crate::builtins::define_own_property(&receiver, key, &descriptor)
}

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

fn is_default_derived_constructor(function: &crate::value::FunctionValue) -> bool {
    function
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == "\0default_derived_constructor")
}

fn constructor_receiver(target: &Value) -> Value {
    let prototype = crate::execute::get_property(target, "prototype");
    Value::Object(std::rc::Rc::new(ObjectData::new(vec![(
        "\0prototype".to_string(),
        prototype,
    )])))
}
