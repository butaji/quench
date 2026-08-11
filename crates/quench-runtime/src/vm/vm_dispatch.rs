fn run_op(
    registers: &mut Vec<Value>,
    op: &Op,
    _context: &VmContext,
) -> Result<Option<crate::completion::Completion>, VmError> {
    if is_global_declaration_op(op) {
        crate::vm::begin_global_declaration_batch();
    } else {
        crate::vm::flush_global_declaration_batch(registers);
    }
    if let Some(result) = run_simple_op(registers, op)? {
        return Ok(result.map(crate::completion::Completion::Return));
    }
    if let Some(result) = run_control_op(registers, op)? {
        return Ok(Some(result));
    }
    run_dispatch_op(registers, op).map(|value| value.map(crate::completion::Completion::Return))
}

include!("run_simple_op.rs");

fn run_global_declaration_op(registers: &mut Vec<Value>, op: &Op) -> Result<bool, VmError> {
    if !is_global_declaration_op(op) {
        return Ok(false);
    }
    crate::global_environment::execute(registers, op)?;
    Ok(true)
}

fn is_global_declaration_op(op: &Op) -> bool {
    matches!(
        op,
        Op::CheckGlobalFunction { .. }
            | Op::CheckGlobalVar { .. }
            | Op::CreateGlobalFunction { .. }
            | Op::CreateGlobalVar { .. }
    )
}

fn run_eval(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    let Op::Eval {
        dst,
        source,
        strict,
        global,
        direct,
        bindings,
        forbidden_var_names,
    } = op
    else {
        return Ok(());
    };
    crate::reflect::execute_eval(
        registers,
        crate::reflect::EvalExecution {
            dst: *dst,
            source: *source,
            strict: *strict,
            global: *global,
            direct: *direct,
            bindings,
            forbidden_var_names,
        },
    )
}

fn run_property_op(registers: &mut Vec<Value>, op: &Op) -> Result<bool, VmError> {
    use Op::*;
    if !matches!(
        op,
        GetProperty { .. }
            | GetPrivate { .. }
            | GetSuperProperty { .. }
            | GetSuperPropertyDynamic { .. }
            | ResolveGlobal { .. }
            | GetPropertyDynamic { .. }
            | HasPropertyDynamic { .. }
            | ToPropertyKey { .. }
            | SetProperty { .. }
            | SetPrototype { .. }
            | SetPrivate { .. }
            | DefinePrivate { .. }
            | SetPropertyDynamic { .. }
            | SetSuperProperty { .. }
            | SetSuperPropertyDynamic { .. }
            | DefineProperty { .. }
            | CopyDataProperties { .. }
            | ResolveName { .. }
            | ResolveNameOrUndefined { .. }
            | SetName { .. }
            | CheckStrictName { .. }
            | SetFunctionName { .. }
            | SetFunctionNameDynamic { .. }
    ) {
        return Ok(false);
    }
    run_get_set_property(registers, op)?;
    Ok(true)
}

fn run_class_heritage(registers: &[Value], op: &Op) -> Result<(), VmError> {
    let Op::ValidateClassHeritage { src } = op else {
        return Ok(());
    };
    crate::classes::validate_heritage(&crate::execute::read_register(registers, *src)?)
}

fn run_local_op(registers: &mut Vec<Value>, op: &Op) -> Result<bool, VmError> {
    use Op::*;
    match op {
        StoreLocal { slot, src } => crate::locals::store(registers, *slot, *src)?,
        LoadCurrentGlobal { dst } => {
            write_value(registers, *dst, crate::vm::current_global_object())
        }
        MarkUninitialized { slot } => crate::locals::mark_uninitialized(*slot),
        CheckInitialized { slot, name } => crate::locals::check_initialized(*slot, name)?,
        DeleteEvalBinding { dst, name, slot } => write_value(
            registers,
            *dst,
            Value::Boolean(crate::locals::delete_named(name, *slot)),
        ),
        DeleteName { dst, name, strict } => crate::with_scope::execute_delete_name(registers, *dst, name, *strict)?,
        LoadLocal { dst, slot } => crate::locals::load(registers, *dst, *slot)?,
        LoadBinding { dst, slot, name } => {
            crate::locals::load_binding(registers, *dst, *slot, name)?
        }
        ResolveBindingTarget { dst, name } => crate::locals::resolve_target(registers, *dst, name)?,
        InitializeResolvedBinding {
            target,
            slot,
            name,
            src,
        } => crate::locals::initialize_resolved(registers, *target, *slot, name, *src)?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn run_make_builtin(registers: &mut Vec<Value>, op: &Op) {
    let Op::MakeBuiltin { dst, builtin } = op else {
        return;
    };
    let value = match builtin {
        Builtin::HostCapability(kind) => current_host_capability(*kind),
        _ => Value::Builtin(*builtin),
    };
    write_value(registers, *dst, value);
}

fn run_control_op(
    registers: &mut Vec<Value>,
    op: &Op,
) -> Result<Option<crate::completion::Completion>, VmError> {
    use crate::completion::Completion;
    use Op::*;
    match op {
        ForIn { .. } => crate::loops::execute_for_in(registers, op).map(Some),
        ForOf { .. } => crate::loops::execute_for_of(registers, op).map(Some),
        Branch { .. } => crate::branch::execute(registers, op).map(Some),
        Label { .. } => crate::statement_control::execute_label(registers, op).map(Some),
        With { .. } => crate::with_scope::execute(registers, op).map(Some),
        PrivateScope { .. } => crate::private_environment::execute_scope(registers, op).map(Some),
        Try { .. } => crate::exceptions::execute(registers, op).map(Some),
        IteratorBinding { .. } => crate::collections::iterator::execute_binding(registers, op)
            .map(Some),
        Loop { .. } => crate::loops::execute(registers, op).map(Some),
        Switch { .. } => crate::switch::execute(registers, op).map(Some),
        Conditional { .. } => run_conditional(registers, op).map(return_completion),
        Return { src } => read_register(registers, *src)
            .map(Completion::Return)
            .map(Some),
        Throw { src } => read_register(registers, *src)
            .map(Completion::Throw)
            .map(Some),
        Break { label } => Ok(Some(Completion::Break(label.clone()))),
        Continue { label } => Ok(Some(Completion::Continue(label.clone()))),
        TailCall { .. } => run_tail_call(registers, op).map(Some),
        Await { .. } => run_await_completion(registers, op),
        Yield { src } => read_register(registers, *src)
            .map(Completion::Yield)
            .map(Some),
        _ => Ok(None),
    }
}

fn return_completion(value: Option<Value>) -> Option<crate::completion::Completion> {
    value.map(crate::completion::Completion::Return)
}

fn run_dispatch_op(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    use Op::*;
    match op {
        CallMethod { .. }
        | CallSuperMethod { .. }
        | CallSuperConstructor { .. }
        | Construct { .. } => {
            run_method_or_construct(registers, op)?
        }
        _ => {}
    }
    Ok(None)
}

fn run_make_array(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    match op {
        Op::MakeArray { dst, elements } => execute_array(registers, *dst, elements)?,
        Op::BuildArray { dst, elements } => execute_array_plan(registers, *dst, elements)?,
        _ => {}
    }
    Ok(())
}

fn run_make_object(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::MakeObject { dst, properties } = op {
        execute_object(registers, *dst, properties)?;
        if let Value::Object(object) = read_register(registers, *dst)? {
            realm::initialize_current_global(object);
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

fn run_tail_call(
    registers: &[Value],
    op: &Op,
) -> Result<crate::completion::Completion, VmError> {
    let Op::TailCall {
        callee,
        args,
        spreads,
    } = op
    else {
        return Ok(crate::completion::Completion::Normal);
    };
    vm_ops::prepare_tail_call(registers, *callee, args, spreads)
        .map(crate::completion::Completion::TailCall)
}

fn run_await_completion(
    registers: &mut Vec<Value>,
    op: &Op,
) -> Result<Option<crate::completion::Completion>, VmError> {
    use crate::completion::Completion;
    if let Op::Await { dst, src } = op {
        return match vm_ops::execute_await(registers, *dst, *src) {
            Ok(()) => Ok(None),
            Err(VmError::Thrown(value)) => Ok(Some(Completion::Throw(value))),
            Err(VmError::Suspended(promise)) => Ok(Some(Completion::Suspend(promise))),
            Err(error) => Err(error),
        };
    }
    Ok(None)
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
        GetPrivate { .. } => crate::private_slots::execute_get(registers, op)?,
        GetSuperProperty { .. } | GetSuperPropertyDynamic { .. } => {
            crate::super_scope::execute_get(registers, op)?
        }
        ResolveGlobal { .. } => crate::with_scope::execute_resolve_global(registers, op)?,
        GetPropertyDynamic { .. } => crate::properties::execute_get_dynamic(registers, op)?,
        HasPropertyDynamic { .. } => crate::with_scope::execute_has_property(registers, op)?,
        ResolveName { .. } | SetName { .. } | CheckStrictName { .. } => {
            crate::with_scope::execute_name(registers, op)?
        }
        SetFunctionName { .. } => crate::properties::execute_set_function_name(registers, op)?,
        SetFunctionNameDynamic { .. } => {
            crate::properties::execute_set_function_name_dynamic(registers, op)?
        }
        ResolveNameOrUndefined { dst, name } => {
            write_value(registers, *dst, crate::locals::resolve_name_or_undefined(name)?)
        }
        ToPropertyKey { dst, src } => to_property_key(registers, *dst, *src)?,
        SetProperty { .. } | SetPropertyDynamic { .. } => {
            crate::properties::execute_set_property(registers, op)?
        }
        SetPrototype { .. } => crate::properties::execute_set_prototype(registers, op)?,
        SetSuperProperty { .. } | SetSuperPropertyDynamic { .. } => {
            crate::super_scope::execute_set(registers, op)?
        }
        SetPrivate { .. } => crate::private_slots::execute_set(registers, op)?,
        DefinePrivate { .. } => crate::private_slots::execute_define(registers, op)?,
        DefineProperty { .. } => crate::property_define::execute(registers, op)?,
        CopyDataProperties { .. } => {
            crate::properties::execute_copy_data_properties(registers, op)?
        }
        _ => {}
    }
    Ok(())
}

fn run_delete_property(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    crate::properties::execute_delete_property(registers, op)
}

fn to_property_key(registers: &mut Vec<Value>, dst: u16, src: u16) -> Result<(), VmError> {
    let value = crate::execute::read_register(registers, src)?;
    let key = crate::conversion::to_property_key(&value)?;
    crate::execute::write_value(registers, dst, crate::value::Value::String(key));
    Ok(())
}

fn run_method_or_construct(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    use Op::*;
    match op {
        CallMethod { .. } => crate::methods::execute(registers, op)?,
        CallSuperMethod { .. } => crate::super_scope::execute_call(registers, op)?,
        CallSuperConstructor { .. } => crate::super_scope::execute_constructor(registers, op)?,
        Construct { .. } => crate::construct::execute(registers, op)?,
        _ => {}
    }
    Ok(())
}

fn run_conditional(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    crate::conditional::execute(registers, op)?;
    Ok(None)
}

fn render_thrown(value: &Value) -> String {
    if let Value::Object(properties) = value {
        let name = property_string(properties, "name");
        let message = property_string(properties, "message");
        match (name, message) {
            (Some(name), Some(message)) => format!("{name}: {message}"),
            (Some(name), None) => name,
            (None, Some(message)) if message.is_empty() => constructor_name(value),
            (None, Some(message)) => message,
            (None, None) => "[object Object]".to_string(),
        }
    } else {
        to_string(Some(value))
    }
}

fn constructor_name(value: &Value) -> String {
    match crate::vm::get_property(
        &crate::vm::get_property(value, "constructor"),
        "name",
    ) {
        Value::String(name) if !name.is_empty() => name,
        _ => "[object Object]".to_string(),
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
    write_value(registers, dst, Value::array(values));
    Ok(())
}
include!("vm_array_build.rs");

fn execute_object(
    registers: &mut Vec<Value>,
    dst: u16,
    properties: &[(String, u16)],
) -> Result<(), VmError> {
    let values = properties
        .iter()
        .map(|(key, index)| Ok((key.clone(), read_register(registers, *index)?)))
        .collect::<Result<Vec<_>, VmError>>()?;
    write_value(
        registers,
        dst,
        Value::Object(Rc::new(crate::value::ObjectData::new(values))),
    );
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
    crate::builtins::property(builtin, key)
}

fn typed_array_element_size(builtin: Builtin) -> Option<f64> {
    use Builtin::*;
    Some(match builtin {
        Float64Array | Float64ArrayPrototype => crate::value::Float64ArrayData::BYTES_PER_ELEMENT as f64,
        Float32Array | Float32ArrayPrototype => crate::value::Float32ArrayData::BYTES_PER_ELEMENT as f64,
        Int8Array | Int8ArrayPrototype => crate::value::Int8ArrayData::BYTES_PER_ELEMENT as f64,
        Int16Array | Int16ArrayPrototype => crate::value::Int16ArrayData::BYTES_PER_ELEMENT as f64,
        Uint16Array | Uint16ArrayPrototype => crate::value::Uint16ArrayData::BYTES_PER_ELEMENT as f64,
        Int32Array | Int32ArrayPrototype => crate::value::Int32ArrayData::BYTES_PER_ELEMENT as f64,
        Uint8Array | Uint8ArrayPrototype => crate::value::Uint8ArrayData::BYTES_PER_ELEMENT as f64,
        Uint32Array | Uint32ArrayPrototype => crate::value::Uint32ArrayData::BYTES_PER_ELEMENT as f64,
        Uint8ClampedArray | Uint8ClampedArrayPrototype => {
            crate::value::Uint8ClampedArrayData::BYTES_PER_ELEMENT as f64
        }
        BigInt64Array | BigInt64ArrayPrototype => {
            crate::value::BigInt64ArrayData::BYTES_PER_ELEMENT as f64
        }
        BigUint64Array | BigUint64ArrayPrototype => {
            crate::value::BigUint64ArrayData::BYTES_PER_ELEMENT as f64
        }
        _ => return None,
    })
}

fn string_property(value: &str, key: &str) -> Value {
    use crate::ops::Builtin::*;
    match key {
        "length" => return Value::Number(crate::strings::utf16_len(value) as f64),
        "toLocaleLowerCase" => return Value::Builtin(StringToLocaleLowerCase),
        "toLocaleUpperCase" => return Value::Builtin(StringToLocaleUpperCase),
        _ => {}
    }
    if let Some(method) = crate::strings::property_method(key) {
        return Value::Builtin(method);
    }
    key.parse::<usize>()
        .ok()
        .and_then(|index| crate::strings::char_at_utf16(value, index))
        .map(Value::String)
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
        "toString" => Value::Builtin(Builtin::NumberToString),
        "valueOf" => Value::Boolean(value),
        _ => Value::Undefined,
    }
}
