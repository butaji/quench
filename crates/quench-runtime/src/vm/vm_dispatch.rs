fn run_op(
    registers: &mut Vec<Value>,
    op: &Op,
    _context: &VmContext,
) -> Result<Option<Value>, VmError> {
    if let Some(result) = run_simple_op(registers, op)? {
        return Ok(result);
    }
    if let Some(result) = run_control_op(registers, op)? {
        return Ok(result);
    }
    run_dispatch_op(registers, op)
}

fn run_simple_op(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Option<Value>>, VmError> {
    use Op::*;
    match op {
        Const { dst, value } => write_value(registers, *dst, value.into()),
        StoreLocal { slot, src } => crate::locals::store(registers, *slot, *src)?,
        LoadLocal { dst, slot } => copy_register(registers, *dst, *slot)?,
        MakeArray { .. } => run_make_array(registers, op)?,
        MakeObject { .. } => run_make_object(registers, op)?,
        MakeBuiltin { dst, builtin } => write_value(
            registers,
            *dst,
            match builtin {
                Builtin::HostCapability(kind) => current_host_capability(*kind),
                _ => Value::Builtin(*builtin),
            },
        ),
        GetProperty { .. }
        | GetPropertyDynamic { .. }
        | SetProperty { .. }
        | SetPropertyDynamic { .. } => run_get_set_property(registers, op)?,
        DeleteProperty { .. } => run_delete_property(registers, op)?,
        MakeFunction { .. } | MakeFunctionWithKind { .. } => {
            crate::functions::write_op(registers, op)
        }
        Call { .. } => run_call(registers, op)?,
        Await { .. } => run_await(registers, op)?,
        Unary { dst, operator, src } => {
            vm_arithmetic::execute_unary(registers, *dst, *operator, *src)?
        }
        Binary { .. } => run_binary(registers, op)?,
        _ => return Ok(None),
    }
    Ok(Some(None))
}

fn run_control_op(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Option<Value>>, VmError> {
    use Op::*;
    match op {
        ForIn { .. } => run_for_in(registers, op).map(Some),
        ForOf { .. } => crate::loops::execute_for_of(registers, op).map(Some),
        Branch { .. } => run_branch(registers, op).map(Some),
        Try { .. } => run_try(registers, op).map(Some),
        Loop { .. } | Switch { .. } | Conditional { .. } => {
            run_loop_or_special(registers, op).map(Some)
        }
        Return { .. } | Throw { .. } => run_terminal(registers, op).map(Some).map(Some),
        Break { label } => Err(VmError::Break(label.clone())),
        Continue { label } => Err(VmError::Continue(label.clone())),
        _ => Ok(None),
    }
}

fn run_dispatch_op(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    use Op::*;
    match op {
        CallMethod { .. } | Construct { .. } => run_method_or_construct(registers, op)?,
        _ => {}
    }
    Ok(None)
}

fn run_make_array(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::MakeArray { dst, elements } = op {
        execute_array(registers, *dst, elements)?;
    }
    Ok(())
}

fn run_make_object(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::MakeObject { dst, properties } = op {
        execute_object(registers, *dst, properties)?;
        if GLOBAL_OBJECT.with(|global| global.borrow().is_none()) {
            if let Value::Object(object) = read_register(registers, *dst)? {
                GLOBAL_OBJECT.with(|global| global.replace(Some(object)));
            }
        }
    }
    Ok(())
}

fn run_call(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::Call {
        dst,
        callee,
        args,
        spreads,
    } = op
    {
        vm_ops::execute_call(registers, *dst, *callee, args, spreads)?;
    }
    Ok(())
}

fn run_await(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::Await { dst, src } = op {
        vm_ops::execute_await(registers, *dst, *src)?;
    }
    Ok(())
}

fn run_binary(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::Binary {
        dst,
        operator,
        lhs,
        rhs,
    } = op
    {
        vm_arithmetic::execute_binary(registers, *dst, *operator, *lhs, *rhs)?;
    }
    Ok(())
}

fn run_get_set_property(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    use Op::*;
    match op {
        GetProperty { .. } => crate::properties::execute_get(registers, op)?,
        GetPropertyDynamic { .. } => crate::properties::execute_get_dynamic(registers, op)?,
        SetProperty { .. } | SetPropertyDynamic { .. } => {
            crate::properties::execute_set_property(registers, op)?
        }
        _ => {}
    }
    Ok(())
}

fn run_delete_property(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    crate::properties::execute_delete_property(registers, op)
}

fn run_for_in(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    crate::loops::execute_for_in(registers, op)
}

fn run_method_or_construct(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    use Op::*;
    match op {
        CallMethod { .. } => crate::methods::execute(registers, op)?,
        Construct { .. } => crate::construct::execute(registers, op)?,
        _ => {}
    }
    Ok(())
}

fn run_loop_or_special(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    use Op::*;
    match op {
        Loop { .. } => crate::loops::execute(registers, op),
        Switch { .. } => crate::switch::execute(registers, op),
        Conditional { .. } => {
            crate::conditional::execute(registers, op)?;
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn run_terminal(registers: &[Value], op: &Op) -> Result<Value, VmError> {
    execute_terminal(op, registers)
}

fn run_branch(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    crate::branch::execute_or_continue(registers, op)
}

fn run_try(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    crate::exceptions::execute(registers, op)
}

fn execute_terminal(op: &Op, registers: &[Value]) -> Result<Value, VmError> {
    match op {
        Op::Return { src } => read_register(registers, *src),
        Op::Throw { src } => Err(VmError::Thrown(read_register(registers, *src)?)),
        _ => Err(VmError::MissingReturn),
    }
}

fn render_thrown(value: &Value) -> String {
    if let Value::Object(properties) = value {
        let name = property_string(properties, "name");
        let message = property_string(properties, "message");
        match (name, message) {
            (Some(name), Some(message)) => format!("{name}: {message}"),
            (Some(name), None) => name,
            (None, Some(message)) => message,
            (None, None) => "[object Object]".to_string(),
        }
    } else {
        to_string(Some(value))
    }
}

fn property_string(properties: &[(String, Value)], key: &str) -> Option<String> {
    properties
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| to_string(Some(value)))
}

fn execute_array(registers: &mut Vec<Value>, dst: u16, elements: &[u16]) -> Result<(), VmError> {
    let values = elements
        .iter()
        .map(|index| read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    write_value(registers, dst, Value::Array(Rc::new(values)));
    Ok(())
}

fn execute_object(
    registers: &mut Vec<Value>,
    dst: u16,
    properties: &[(String, u16)],
) -> Result<(), VmError> {
    let values = properties
        .iter()
        .map(|(key, index)| Ok((key.clone(), read_register(registers, *index)?)))
        .collect::<Result<Vec<_>, VmError>>()?;
    write_value(registers, dst, Value::Object(Rc::new(values)));
    Ok(())
}

fn builtin_property(builtin: crate::ops::Builtin, key: &str) -> Value {
    if builtin == Builtin::Object && key == "hasOwn" {
        return Value::Builtin(Builtin::ObjectHasOwnProperty);
    }
    if key == "BYTES_PER_ELEMENT" {
        if let Some(size) = typed_array_element_size(builtin) {
            return Value::Number(size);
        }
    }
    let value = crate::builtins::property(builtin, key);
    if let Value::Builtin(symbol) = value {
        if let Some(name) = crate::intl::tolocale::symbol::name(symbol) {
            return Value::String(name.to_string());
        }
    }
    value
}

fn typed_array_element_size(builtin: Builtin) -> Option<f64> {
    Some(match builtin {
        Builtin::Float64Array | Builtin::Float64ArrayPrototype => {
            crate::value::Float64ArrayData::BYTES_PER_ELEMENT as f64
        }
        Builtin::Float32Array | Builtin::Float32ArrayPrototype => {
            crate::value::Float32ArrayData::BYTES_PER_ELEMENT as f64
        }
        Builtin::Int8Array | Builtin::Int8ArrayPrototype => {
            crate::value::Int8ArrayData::BYTES_PER_ELEMENT as f64
        }
        Builtin::Int16Array | Builtin::Int16ArrayPrototype => {
            crate::value::Int16ArrayData::BYTES_PER_ELEMENT as f64
        }
        Builtin::Uint16Array | Builtin::Uint16ArrayPrototype => {
            crate::value::Uint16ArrayData::BYTES_PER_ELEMENT as f64
        }
        Builtin::Int32Array | Builtin::Int32ArrayPrototype => {
            crate::value::Int32ArrayData::BYTES_PER_ELEMENT as f64
        }
        Builtin::Uint8Array | Builtin::Uint8ArrayPrototype => {
            crate::value::Uint8ArrayData::BYTES_PER_ELEMENT as f64
        }
        Builtin::Uint32Array | Builtin::Uint32ArrayPrototype => {
            crate::value::Uint32ArrayData::BYTES_PER_ELEMENT as f64
        }
        Builtin::Uint8ClampedArray | Builtin::Uint8ClampedArrayPrototype => {
            crate::value::Uint8ClampedArrayData::BYTES_PER_ELEMENT as f64
        }
        _ => return None,
    })
}

fn string_property(value: &str, key: &str) -> Value {
    use crate::ops::Builtin::*;
    match key {
        "length" => return Value::Number(value.chars().count() as f64),
        "toLocaleLowerCase" => return Value::Builtin(StringToLocaleLowerCase),
        "toLocaleUpperCase" => return Value::Builtin(StringToLocaleUpperCase),
        _ => {}
    }
    if let Some(method) = crate::strings::property_method(key) {
        return Value::Builtin(method);
    }
    key.parse::<usize>()
        .ok()
        .and_then(|index| value.chars().nth(index))
        .map(|character| Value::String(character.to_string()))
        .unwrap_or(Value::Undefined)
}

fn number_property(_value: f64, key: &str) -> Value {
    use crate::ops::Builtin::*;
    match key {
        "toLocaleString" => Value::Builtin(NumberToLocaleString),
        "toString" => Value::Builtin(NumberToString),
        "valueOf" => Value::Builtin(NumberValueOf),
        "toFixed" => Value::Builtin(NumberToFixed),
        "toPrecision" => Value::Builtin(NumberToPrecision),
        "toExponential" => Value::Builtin(NumberToExponential),
        _ => Value::Undefined,
    }
}

fn boolean_property(value: bool, key: &str) -> Value {
    match key {
        "toString" => Value::String(value.to_string()),
        "valueOf" => Value::Boolean(value),
        _ => Value::Undefined,
    }
}
