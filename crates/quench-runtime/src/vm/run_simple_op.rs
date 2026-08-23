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
        TemplateObject { .. } => crate::templates::execute_tagged_template(registers, op)?,
        MakeObject { .. } => run_make_object(registers, op)?,
        MakeBuiltin { .. } => run_make_builtin(registers, op),
        RequireObjectCoercible { .. }
        | GetIterator { .. }
        | IteratorStep { .. }
        | IteratorRest { .. } => crate::collections::iterator::execute(registers, op)?,
        ValidateClassHeritage { .. } => run_class_heritage(registers, op)?,
        GetClassPrototype { .. } => run_class_prototype(registers, op)?,
        CheckSuperThis => crate::super_scope::check_initialized_this()?,
        CaptureSuperBase { dst } => crate::super_scope::execute_capture_base(registers, *dst)?,
        AppendInstanceField(_) => crate::classes::append_instance_field(registers, op)?,
        DeleteProperty { .. } => run_delete_property(registers, op)?,
        MakeFunction { .. } | MakeFunctionWithKind { .. } => {
            crate::functions::write_op(registers, op)
        }
        DynamicImport { .. } => run_dynamic_import(registers, op)?,
        Call { .. } => run_call(registers, op)?,
        OptionalCall { .. } => run_optional_call(registers, op)?,
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

fn run_dynamic_import(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    let Op::DynamicImport {
        dst,
        specifier,
        deferred,
    } = op
    else {
        return Ok(());
    };
    let specifier_value = crate::execute::read_register(registers, *specifier)?;
    let specifier = match crate::conversion::to_string(&specifier_value) {
        Ok(specifier) => specifier,
        Err(crate::execute::VmError::Thrown(reason)) => {
            write_value(registers, *dst, rejected_promise(reason));
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let value = crate::module_bindings::resolve_dynamic_import(&specifier, *deferred)
        .unwrap_or_else(|| Value::String(specifier));
    let value = if let Some(thrown) = crate::module_bindings::take_pending_throw() {
        rejected_promise(thrown)
    } else {
        value
    };
    write_value(registers, *dst, value);
    Ok(())
}

fn rejected_promise(reason: Value) -> Value {
    let promise = std::rc::Rc::new(crate::value::PromiseData::default());
    crate::promise::reject_promise(&promise, reason);
    Value::Promise(promise)
}

fn run_class_prototype(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    let Op::GetClassPrototype {
        dst,
        constructor_dst,
        heritage,
    } = op
    else {
        return Ok(());
    };
    let heritage = crate::execute::read_register(registers, *heritage)?;
    let prototype = class_heritage_prototype(&heritage)?;
    write_value(registers, *dst, prototype);
    write_value(
        registers,
        *constructor_dst,
        class_constructor_parent(&heritage),
    );
    Ok(())
}

fn class_constructor_parent(heritage: &Value) -> Value {
    if matches!(heritage, Value::Null) {
        Value::Builtin(crate::ops::Builtin::FunctionPrototype)
    } else {
        heritage.clone()
    }
}

fn class_heritage_prototype(heritage: &Value) -> Result<Value, VmError> {
    if matches!(heritage, Value::Null) {
        return Ok(Value::Null);
    }
    let prototype = crate::execute::get_property_result(heritage, "prototype")?;
    if matches!(prototype, Value::Null) || crate::value::is_object(&prototype) {
        return Ok(prototype);
    }
    Err(crate::value::error::throw_type_error(
        "Class extends value does not have a valid prototype",
    ))
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
