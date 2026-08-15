fn run_local_op(registers: &mut Vec<Value>, op: &Op) -> Result<bool, VmError> {
    use Op::*;
    match op {
        StoreLocal { slot, src } => crate::locals::store(registers, *slot, *src)?,
        StoreFunctionName { slot, src, strict } => {
            crate::locals::store_function_name(registers, *slot, *src, *strict)?
        }
        LoadCurrentGlobal { dst } => write_value(registers, *dst, crate::vm::current_global_object()),
        MarkUninitialized { slot } => crate::locals::mark_uninitialized(*slot),
        MarkImmutable { slot } => crate::locals::mark_immutable(*slot),
        CheckInitialized { slot, name } => crate::locals::check_initialized(*slot, name)?,
        InitializeLocal { slot } => crate::locals::initialize(*slot),
        LoadParameter { dst, slot } => crate::locals::load_parameter(registers, *dst, *slot)?,
        DeleteEvalBinding { dst, name, slot } => write_value(
            registers,
            *dst,
            Value::Boolean(crate::locals::delete_named(name, *slot)),
        ),
        DeleteName { dst, name, strict } => crate::with_scope::execute_delete_name(registers, *dst, name, *strict)?,
        LoadLocal { dst, slot } => crate::locals::load(registers, *dst, *slot)?,
        LoadBinding {
            dst,
            slot,
            name,
            dynamic,
        } => crate::locals::load_binding(registers, *dst, *slot, name, *dynamic)?,
        ResolveBindingTarget { dst, name } => crate::locals::resolve_target(registers, *dst, name)?,
        InitializeResolvedBinding { target, slot, name, src } => {
            crate::locals::initialize_resolved(registers, *target, *slot, name, *src)?
        }
        SetResolvedLocalBinding { target, slot, name, strict, src } => {
            crate::locals::set_resolved_local(registers, *target, *slot, name, *strict, *src)?
        }
        LoadResolvedLocalBinding { dst, target, slot, name } => {
            crate::locals::load_resolved_local(registers, *dst, *target, *slot, name)?
        }
        SetResolvedBinding { target, name, src, strict } => {
            crate::with_scope::set_resolved(registers, *target, name, *src, *strict)?
        }
        _ => return Ok(false),
    }
    Ok(true)
}
