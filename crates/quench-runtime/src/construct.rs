use std::{collections::HashMap, rc::Rc};

use crate::{facts::ProgramDb, ops::Op, value::Value};

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
    match target {
        Value::Builtin(builtin) => construct_builtin(*builtin, arguments),
        Value::Function(function) => construct_function(function, target, arguments),
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
    Value::Object(Rc::new(vec![
        ("source".to_string(), Value::String(source)),
        ("flags".to_string(), Value::String(flags)),
        ("lastIndex".to_string(), Value::Number(0.0)),
    ]))
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
    Ok(Value::Object(std::rc::Rc::new(vec![(
        "_value".to_string(),
        Value::Number(value),
    )])))
}

fn construct_boolean(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = crate::execute::execute_builtin_with_receiver(
        crate::ops::Builtin::Boolean,
        arguments,
        None,
    )?;
    Ok(Value::Object(std::rc::Rc::new(vec![(
        "_value".to_string(),
        value,
    )])))
}

fn construct_string(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = crate::execute::execute_builtin_with_receiver(
        crate::ops::Builtin::String,
        arguments,
        None,
    )?;
    Ok(Value::Object(std::rc::Rc::new(vec![(
        "_value".to_string(),
        value,
    )])))
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
    if let Some(super_constructor) = derived_constructor(function) {
        return construct_value(&super_constructor, arguments);
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

fn initialize_instance_fields(
    function: &crate::value::FunctionValue,
    mut receiver: Value,
) -> Result<Value, crate::execute::VmError> {
    for field in function.instance_fields.borrow().iter() {
        receiver = initialize_instance_field(field, receiver)?;
    }
    Ok(receiver)
}

fn initialize_instance_field(
    field: &crate::value::InstanceFieldPlan,
    receiver: Value,
) -> Result<Value, crate::execute::VmError> {
    let key = match &field.key {
        crate::value::InstanceFieldKey::Static(key) => key.to_string(),
        crate::value::InstanceFieldKey::Dynamic(key) => crate::conversion::to_property_key(key)?,
    };
    let value = match &field.initializer {
        crate::value::InstanceFieldInitializer::Undefined => Value::Undefined,
        crate::value::InstanceFieldInitializer::Callable(initializer) => {
            crate::functions::execute(initializer, &receiver, &[])?
        }
    };
    define_instance_field(receiver, &key, value)
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

fn derived_constructor(function: &crate::value::FunctionValue) -> Option<Value> {
    function
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0derived_constructor").then(|| value.clone()))
}

fn constructor_receiver(target: &Value) -> Value {
    let prototype = crate::execute::get_property(target, "prototype");
    Value::Object(std::rc::Rc::new(vec![(
        "\0prototype".to_string(),
        prototype,
    )]))
}
