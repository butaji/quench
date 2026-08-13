fn run_simple_op(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Option<Value>>, VmError> {
    if run_local_op(registers, op)? {
        return Ok(Some(None));
    }
    if run_property_op(registers, op)? {
        return Ok(Some(None));
    }
    if run_global_declaration_op(registers, op)? {
        return Ok(Some(None));
    }
    run_simple_single_op(registers, op)
}

fn run_simple_single_op(
    registers: &mut Vec<Value>,
    op: &Op,
) -> Result<Option<Option<Value>>, VmError> {
    use Op::*;
    match op {
        Const { dst, value } => write_value(registers, *dst, value.into()),
        MakeArray { .. } | BuildArray { .. } => run_make_array(registers, op)?,
        MakeObject { .. } => run_make_object(registers, op)?,
        MakeBuiltin { .. } => run_make_builtin(registers, op),
        RequireObjectCoercible { .. }
        | GetIterator { .. }
        | IteratorStep { .. }
        | IteratorRest { .. } => crate::collections::iterator::execute(registers, op)?,
        ValidateClassHeritage { .. } => run_class_heritage(registers, op)?,
        AppendInstanceField(_) => crate::classes::append_instance_field(registers, op)?,
        DeleteProperty { .. } => run_delete_property(registers, op)?,
        MakeFunction { .. } | MakeFunctionWithKind { .. } => {
            crate::functions::write_op(registers, op)
        }
        Call { .. } => run_call(registers, op)?,
        OptionalCall { .. } => run_optional_call(registers, op)?,
        Eval { .. } => run_eval(registers, op)?,
        DeclareEvalBinding { name, slot } => crate::locals::alias_eval_name(name, *slot),
        DeclareGlobalLexicalBinding {
            name,
            slot,
            immutable,
        } => crate::locals::declare_global_lexical(name, *slot, *immutable),
        ParameterEnd => {}
        Unary { dst, operator, src } => {
            vm_arithmetic::execute_unary(registers, *dst, *operator, *src)?
        }
        Binary { .. } => run_binary(registers, op)?,
        _ => return Ok(None),
    }
    Ok(Some(None))
}

fn run_optional_call(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::OptionalCall {
        dst,
        callee,
        receiver,
        guard_receiver,
        args,
        spreads,
    } = op
    {
        vm_ops::execute_optional_call(
            registers,
            *dst,
            *callee,
            *receiver,
            *guard_receiver,
            args,
            spreads,
        )?;
    }
    Ok(())
}
