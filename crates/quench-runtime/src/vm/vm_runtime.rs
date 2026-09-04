include!("vm_generator_step.rs");
include!("vm_completion_step.rs");

fn run_ops(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<Value, VmError> {
    completion_result(run_ops_completion(ops, registers, context)?)
}

fn run_ops_completion(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    Ok(run_ops_completion_step(ops, registers, context)?.completion)
}

pub(crate) fn execute_ops_from(
    ops: &[Op],
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<(crate::completion::Completion, usize), VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let step = run_ops_completion_step_from(ops, start, registers, context)?;
    Ok((step.completion, step.next))
}

pub(crate) fn execute_code_from(
    code: crate::machine::CodeView<'_>,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<(crate::completion::Completion, usize), VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let step = run_code_completion_step_from(code, start, registers, context)?;
    Ok((step.completion, step.next))
}

fn run_ops_completion_step(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    run_ops_completion_step_from(ops, 0, registers, context)
}

fn run_ops_completion_step_from(
    ops: &[Op],
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    let executable = crate::machine::ExecutableCode::from_ops(ops.to_vec());
    run_code_completion_step_from(executable.code(), start, registers, context)
}

#[inline]
fn run_code_completion_step_from(
    code: crate::machine::CodeView<'_>,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    let mut pc = start;
    while let Some(instruction) = code.instruction(pc) {
        let _source_offset = crate::vm::source_offset(
            code.metadata_at(pc).and_then(|metadata| metadata.source),
        );
        if !context.consume_execution_budget() {
            return Err(VmError::Interrupted);
        }
        match instruction.opcode {
            crate::ir::Opcode::Jump => {
                pc = usize::from(instruction.a);
                continue;
            }
            crate::ir::Opcode::JumpIfFalse => {
                let truthy = registers
                    .word_truthiness(usize::from(instruction.a))
                    .map_or_else(
                        || read_register(registers, instruction.a).map(|value| is_truthy(&value)),
                        Ok,
                    )?;
                if truthy {
                    pc += 1;
                } else {
                    pc = usize::from(instruction.b);
                }
                continue;
            }
            _ => {}
        }
        #[cfg(not(feature = "execution-trace"))]
        let result = match run_instruction_hot(code, pc, instruction, registers) {
            Some(result) => result,
            None => run_instruction(code, pc, instruction, registers, context),
        };
        #[cfg(feature = "execution-trace")]
        let result = run_instruction(code, pc, instruction, registers, context);
        let result = match result {
            Ok(result) => result,
            Err(error) => return completion_step_after_error(registers, error, pc + 1),
        };
        pc += 1;
        if let Some(completion) =
            result.filter(|value| !matches!(value, crate::completion::Completion::Normal))
        {
            return completion_step_after_transition(registers, completion, pc);
        }
    }
    completion_step_after_transition(registers, crate::completion::Completion::Normal, code.len())
}

/// Inline the representation-only compact operations that cannot suspend or
/// invoke host code.  The traced build deliberately uses the canonical
/// dispatcher so attribution remains complete and deterministic.
#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
fn run_instruction_hot(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
) -> Option<Result<Option<crate::completion::Completion>, VmError>> {
    use crate::ir::Opcode;
    let result = match instruction.opcode {
        Opcode::LoadConst => {
            let result = code
                .constant_at(pc)
                .ok_or(VmError::MissingReturn)
                .map(|(_, value)| write_value(registers, instruction.a, value.into()));
            result.map(|_| None)
        }
        Opcode::Move => {
            let result = if instruction.flags == 1 {
                crate::locals::move_proven_local(
                    registers,
                    instruction.a,
                    instruction.b,
                    instruction.c,
                )
            } else {
                copy_register(registers, instruction.a, instruction.b)
            };
            result.map(|_| None)
        }
        Opcode::LoadLocal => crate::locals::load_proven(registers, instruction.a, instruction.b)
            .map(|_| None),
        Opcode::LoadLocalChecked => {
            let name = code
                .metadata_at(pc)
                .and_then(|metadata| metadata.name.as_deref())
                .unwrap_or("binding");
            crate::locals::load_checked(registers, instruction.a, instruction.b, name)
                .map(|_| None)
        }
        Opcode::StoreLocal => crate::locals::store_proven(registers, instruction.a, instruction.b)
            .map(|_| None),
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div => {
            let operator = match instruction.opcode {
                Opcode::Add => crate::ops::BinaryOp::Add,
                Opcode::Sub => crate::ops::BinaryOp::Subtract,
                Opcode::Mul => crate::ops::BinaryOp::Multiply,
                Opcode::Div => crate::ops::BinaryOp::Divide,
                _ => unreachable!(),
            };
            vm_arithmetic::execute_binary(
                registers,
                instruction.a,
                operator,
                instruction.b,
                instruction.c,
            )
            .map(|_| None)
        }
        Opcode::Binary => crate::ir::compact_binary_operator(instruction.flags)
            .ok_or_else(|| VmError::EvalError("invalid compact binary operator".into()))
            .and_then(|operator| {
                vm_arithmetic::execute_binary(
                    registers,
                    instruction.a,
                    operator,
                    instruction.b,
                    instruction.c,
                )
            })
            .map(|_| None),
        Opcode::AGetI => return run_array_get_hot(registers, instruction),
        Opcode::GetN => return run_length_get_hot(code, pc, registers, instruction),
        Opcode::ASetI => return run_array_set_hot(registers, instruction),
        Opcode::Slow => {
            let Some(crate::ops::Op::RequireObjectCoercible { src }) = code.cold(instruction)
            else {
                return None;
            };
            return (registers.word_is_non_nullish(usize::from(*src)) == Some(true))
                .then_some(Ok(None));
        }
        Opcode::Return => read_register(registers, instruction.a)
            .map(crate::completion::Completion::Return)
            .map(Some),
        _ => return None,
    };
    Some(result)
}

#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
fn run_array_get_hot(
    registers: &mut crate::register_file::RegisterFile,
    instruction: crate::ir::Instruction,
) -> Option<Result<Option<crate::completion::Completion>, VmError>> {
    let index = registers.read_array_index(usize::from(instruction.c));
    let raw_array = registers.read_array(usize::from(instruction.b));
    let array = raw_array.filter(|array| crate::locals::array_word_is_current(array));
    if let Some((array, index)) = array.filter(|array| array.is_plain_dense_access()).zip(index) {
        if let Some(number) = array.dense_number_at(index) {
            registers.write_number(usize::from(instruction.a), number);
            return Some(Ok(None));
        }
        if let Some(value) = array.dense_value_at(index) {
            write_value(registers, instruction.a, value);
            return Some(Ok(None));
        }
        write_value(registers, instruction.a, crate::value::Value::Undefined);
        return Some(Ok(None));
    }
    None
}

#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
fn run_length_get_hot(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    registers: &mut crate::register_file::RegisterFile,
    instruction: crate::ir::Instruction,
) -> Option<Result<Option<crate::completion::Completion>, VmError>> {
    let metadata = code.metadata_at(pc)?;
    if metadata.name.as_deref() != Some("length") {
        return None;
    }
    let array = registers
        .read_array(usize::from(instruction.b))
        .filter(|array| crate::locals::array_word_is_current(array))?;
    if array.is_arguments() {
        registers.write(usize::from(instruction.a), array.arguments_length_value());
    } else {
        registers.write_number(usize::from(instruction.a), array.header_length() as f64);
    }
    Some(Ok(None))
}

#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
fn run_array_set_hot(
    registers: &mut crate::register_file::RegisterFile,
    instruction: crate::ir::Instruction,
) -> Option<Result<Option<crate::completion::Completion>, VmError>> {
    let index = registers.read_array_index(usize::from(instruction.b));
    let number = registers.read_number(usize::from(instruction.c));
    if let Some((index, number)) = index.zip(number) {
        if let Some(array) = registers
            .read_array(usize::from(instruction.a))
            .filter(|array| crate::locals::array_word_is_current(array))
        {
            if array.set_plain_existing_f64(index, number)
                || array.append_preallocated_f64(index, number)
            {
                return Some(Ok(None));
            }
        }
    }
    None
}

#[inline(never)]
fn run_instruction(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<Option<crate::completion::Completion>, VmError> {
    use crate::ir::Opcode;
    let _decode_guard = crate::execution_trace::compact(instruction.opcode);
    crate::execution_trace::compact_site(code, pc);
    crate::execution_trace::operands(instruction);
    match instruction.opcode {
        Opcode::LoadConst => {
            let (_, value) = code.constant_at(pc).ok_or(VmError::MissingReturn)?;
            write_value(registers, instruction.a, value.into());
            Ok(None)
        }
        Opcode::Move => {
            if instruction.flags == 1 {
                crate::locals::move_proven_local(
                    registers,
                    instruction.a,
                    instruction.b,
                    instruction.c,
                )?;
            } else {
                copy_register(registers, instruction.a, instruction.b)?;
            }
            Ok(None)
        }
        Opcode::LoadLocal => {
            crate::locals::load_proven(registers, instruction.a, instruction.b)?;
            Ok(None)
        }
        Opcode::LoadLocalChecked => {
            let name = code
                .metadata_at(pc)
                .and_then(|metadata| metadata.name.as_deref())
                .unwrap_or("binding");
            crate::locals::load_checked(registers, instruction.a, instruction.b, name)?;
            Ok(None)
        }
        Opcode::StoreLocalChecked => {
            let name = code
                .metadata_at(pc)
                .and_then(|metadata| metadata.name.as_deref())
                .unwrap_or("binding");
            crate::locals::check_initialized(instruction.a, name)?;
            crate::locals::store(registers, instruction.a, instruction.b)?;
            Ok(None)
        }
        Opcode::StoreLocal => {
            crate::locals::store_proven(registers, instruction.a, instruction.b)?;
            Ok(None)
        }
        Opcode::InitLocal => {
            crate::locals::store(registers, instruction.a, instruction.b)?;
            crate::locals::initialize(instruction.a);
            Ok(None)
        }
        Opcode::UpdateLocal => {
            crate::locals::update(
                registers,
                instruction.a,
                instruction.b,
                instruction.c,
                instruction.flags != 0,
            )?;
            Ok(None)
        }
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div => {
            let operator = match instruction.opcode {
                Opcode::Add => crate::ops::BinaryOp::Add,
                Opcode::Sub => crate::ops::BinaryOp::Subtract,
                Opcode::Mul => crate::ops::BinaryOp::Multiply,
                Opcode::Div => crate::ops::BinaryOp::Divide,
                _ => unreachable!(),
            };
            vm_arithmetic::execute_binary(
                registers,
                instruction.a,
                operator,
                instruction.b,
                instruction.c,
            )?;
            Ok(None)
        }
        Opcode::Binary => {
            let operator = crate::ir::compact_binary_operator(instruction.flags)
                .ok_or_else(|| VmError::EvalError("invalid compact binary operator".into()))?;
            vm_arithmetic::execute_binary(
                registers,
                instruction.a,
                operator,
                instruction.b,
                instruction.c,
            )?;
            Ok(None)
        }
        Opcode::Call => {
            let argument = (instruction.flags == 1).then_some(instruction.c);
            if instruction.flags <= 1 {
                if let Some(result) = crate::functions::execute_word_leaf_call(
                    registers,
                    instruction.a,
                    instruction.b,
                    argument,
                ) {
                    result?;
                    return Ok(None);
                }
            }
            let argument = [instruction.c];
            let spreads = [false];
            let (arguments, spreads) = if instruction.flags == 0 {
                (&[][..], &[][..])
            } else if instruction.flags == 1 {
                (&argument[..], &spreads[..])
            } else {
                return Err(VmError::EvalError("invalid compact call arity".into()));
            };
            crate::vm::vm_ops::execute_call(
                registers,
                instruction.a,
                instruction.b,
                None,
                arguments,
                spreads,
            )
            .map(Some)
        }
        Opcode::GetProperty | Opcode::AGetI => {
            if instruction.opcode == Opcode::AGetI {
                let index = registers.read_array_index(usize::from(instruction.c));
                let raw_array = registers.read_array(usize::from(instruction.b));
                let array = raw_array.filter(|array| crate::locals::array_word_is_current(array));
                if let Some((array, index)) =
                    array.filter(|array| array.is_packed_ordinary()).zip(index)
                {
                    if let Some(number) = array.dense_number_at(index) {
                        crate::execution_trace::event(
                            crate::execution_trace::Event::PackedArrayGet,
                        );
                        registers.write_number(usize::from(instruction.a), number);
                        return Ok(None);
                    }
                    if let Some(value) = array.dense_value_at(index) {
                        crate::execution_trace::event(
                            crate::execution_trace::Event::PackedArrayGet,
                        );
                        write_value(registers, instruction.a, value);
                        return Ok(None);
                    }
                }
                if let Some(array) = array.filter(|array| !array.is_packed_ordinary()) {
                    crate::execution_trace::packed_kind_miss(array.kind());
                    let object = read_register(registers, instruction.b)?;
                    let key = read_register(registers, instruction.c)?;
                    let key = crate::properties::dynamic_property_key(&key)?;
                    let value = get_property_result(&object, &key)?;
                    write_value(registers, instruction.a, value);
                    return Ok(None);
                }
                let reason = if array.is_none() {
                    crate::execution_trace::packed_kind_reason(if raw_array.is_some() {
                        "stale"
                    } else {
                        "non_array"
                    });
                    None
                } else if index.is_none() {
                    Some("other")
                } else if index.expect("checked index")
                    >= array.expect("checked array").logical_len()
                {
                    Some("oob")
                } else {
                    Some("hole")
                };
                if let Some(reason) = reason {
                    crate::execution_trace::packed_miss(reason);
                }
            }
            let object = read_register(registers, instruction.b)?;
            let key = read_register(registers, instruction.c)?;
            let key = crate::properties::dynamic_property_key(&key)?;
            let value = get_property_result(&object, &key)?;
            write_value(registers, instruction.a, value);
            Ok(None)
        }
        Opcode::GetN => {
            let metadata = code.metadata_at(pc).ok_or(VmError::MissingReturn)?;
            let key = metadata.name.as_deref().ok_or(VmError::MissingReturn)?;
            if instruction.flags == crate::ir::GETN_GLOBAL_FLAG {
                let global = crate::vm::current_global_object();
                let value = crate::vm::get_global_named_property_result(
                    &global,
                    key,
                    &metadata.named_cache,
                )?;
                write_value(registers, instruction.a, value);
                return Ok(None);
            }
            if key == "length" {
                if let Some(array) = registers
                    .read_array(usize::from(instruction.b))
                    .filter(|array| crate::locals::array_word_is_current(array))
                {
                    if array.is_arguments() {
                        registers.write(
                            usize::from(instruction.a),
                            array.arguments_length_value(),
                        );
                        crate::execution_trace::event(crate::execution_trace::Event::NamedPropertyHit);
                        return Ok(None);
                    }
                    registers
                        .write_number(usize::from(instruction.a), array.header_length() as f64);
                    crate::execution_trace::event(crate::execution_trace::Event::NamedPropertyHit);
                    crate::execution_trace::named_property_word("own", "number");
                    return Ok(None);
                }
            }
            let object = registers.read_object(usize::from(instruction.b));
            let global_like = object.as_ref().is_some_and(|object| {
                crate::vm::current_global_identity() == Some(object.identity())
                    || object
                        .hot_properties()
                        .names()
                        .any(|name| name == crate::vm::SCRIPT_GLOBAL_VIEW)
            });
            if let Some(payload) = object.as_ref().filter(|_| !global_like).and_then(|object| {
                get_named_cached_payload(object, &metadata.named_cache)
            }) {
                match payload {
                    NamedCachedPayload::Word(word) => {
                        // SAFETY: the source register owns the containing
                        // object until this complete word copy returns. The
                        // retain-before-replace sequence also handles an
                        // in-place GetN where source and destination match.
                        unsafe { &*word }
                            .copy_to_register(registers, usize::from(instruction.a));
                    }
                    NamedCachedPayload::Cell(cell) => {
                        unsafe { &*cell }.with_word(|word| {
                            registers.write_owned(usize::from(instruction.a), word)
                        });
                    }
                    NamedCachedPayload::Value(value) => {
                        write_value(registers, instruction.a, value)
                    }
                }
                return Ok(None);
            }
            let object = read_register(registers, instruction.b)?;
            let value = get_named_property_result(&object, key, &metadata.named_cache)?;
            write_value(registers, instruction.a, value);
            Ok(None)
        }
        Opcode::SetN => {
            let metadata = code.metadata_at(pc).ok_or(VmError::MissingReturn)?;
            let key = metadata.name.as_deref().ok_or(VmError::MissingReturn)?;
            crate::properties::execute_set_named_cached(
                registers,
                instruction.a,
                key,
                instruction.b,
                instruction.flags != 0,
                &metadata.named_cache,
            )?;
            Ok(None)
        }
        Opcode::CallN => {
            if instruction.flags != 0 {
                crate::methods::execute_registered(registers, instruction, code, pc)
            } else {
                let metadata = code.metadata_at(pc).ok_or(VmError::MissingReturn)?;
                let key = metadata.name.as_deref().ok_or(VmError::MissingReturn)?;
                crate::methods::execute_named(registers, instruction, key, &metadata.named_cache)
            }
        }
        Opcode::ASetI => {
            let index = registers.read_array_index(usize::from(instruction.b));
            let number = registers.read_number(usize::from(instruction.c));
            if let (Some(index), Some(number)) = (index, number) {
                if let Some(target) = registers.read_typed_array(usize::from(instruction.a)) {
                    crate::typed_array_ops::set_numeric_index(target, index, number);
                    crate::execution_trace::event(crate::execution_trace::Event::PackedArraySet);
                    return Ok(None);
                }
            }
            let stored = index.zip(number).is_some_and(|(index, number)| {
                registers
                    .read_array(usize::from(instruction.a))
                    .filter(|array| crate::locals::array_word_is_current(array))
                    .is_some_and(|array| {
                        array.set_existing_f64(index, number)
                            || array.append_preallocated_f64(index, number)
                    })
            });
            if stored {
                crate::execution_trace::event(crate::execution_trace::Event::PackedArraySet);
                return Ok(None);
            }
            crate::execution_trace::packed_miss("other");
            crate::properties::execute_set_property(
                registers,
                &crate::ops::Op::SetPropertyDynamic {
                    object: instruction.a,
                    key: instruction.b,
                    src: instruction.c,
                    strict: instruction.flags != 0,
                },
            )?;
            Ok(None)
        }
        Opcode::Return => read_register(registers, instruction.a)
            .map(crate::completion::Completion::Return)
            .map(Some),
        Opcode::Slow => run_op(
            registers,
            code.cold(instruction)
                .ok_or_else(|| VmError::EvalError("missing cold instruction".into()))?,
            context,
        ),
        _ => Err(VmError::EvalError("unsupported compact instruction".into())),
    }
}

fn error_completion(error: VmError) -> Result<crate::completion::Completion, VmError> {
    crate::completion::Completion::from_vm_error(error)
}

#[cold]
fn completion_step_after_error(
    registers: &mut crate::register_file::RegisterFile,
    error: VmError,
    next: usize,
) -> Result<CompletionStep, VmError> {
    crate::vm::flush_global_declaration_batch(registers);
    error_completion(error).map(|completion| CompletionStep { completion, next })
}

#[cold]
fn completion_step_after_transition(
    registers: &mut crate::register_file::RegisterFile,
    completion: crate::completion::Completion,
    next: usize,
) -> Result<CompletionStep, VmError> {
    crate::vm::flush_global_declaration_batch(registers);
    Ok(CompletionStep { completion, next })
}

pub(crate) fn completion_result(
    completion: crate::completion::Completion,
) -> Result<Value, VmError> {
    completion.into_vm_error()
}

struct GlobalObjectGuard {
    previous: Option<ObjectProperties>,
    restore: bool,
    realm: Option<RealmId>,
}
include!("vm_global.rs");

pub(crate) fn bare_call_receiver(
    function: &crate::value::FunctionValue,
    this_value: &Value,
) -> Value {
    if matches!(
        function.kind,
        FunctionKind::Ordinary | FunctionKind::Method | FunctionKind::Generator
    )
        && matches!(function.strictness, FunctionStrictness::Sloppy)
    {
        let realm = function
            .properties
            .borrow()
            .iter()
            .find_map(|(key, value)| {
                (key == "\0realm")
                    .then(|| crate::vm::realm_id_for_intrinsic_receiver(Some(value)))
                    .flatten()
            })
            .or_else(|| crate::vm::realm_id_for_global_value(&function.captures.get(0)));
        let global = realm
            .and_then(|realm| {
                crate::vm::with_realm(realm, || Some(crate::vm::current_global_object()))
            })
            .flatten()
            .unwrap_or_else(crate::vm::current_global_object);
        return to_object_value_in_realm(this_value, &global);
    }
    this_value.clone()
}

fn to_object_value_in_realm(this_value: &Value, global: &Value) -> Value {
    let Some(realm) = crate::vm::realm_id_for_global_value(global) else {
        return to_object_value(this_value);
    };
    crate::vm::with_realm(realm, || to_object_value(this_value))
        .unwrap_or_else(|| to_object_value(this_value))
}

fn to_object_value(this_value: &Value) -> Value {
    match this_value {
        Value::WeakFunction(function) => to_object_value(&function.value()),
        Value::Object(_)
        | Value::Array(_)
        | Value::Function(_)
        | Value::BoundFunction(_)
        | Value::Builtin(_)
        | Value::ObjectAlias(_)
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
        | Value::HostCapability(_) => this_value.clone(),
        Value::Null | Value::Undefined => crate::vm::current_global_object(),
        Value::Number(_) => boxed_primitive(this_value, crate::ops::Builtin::Number),
        Value::Boolean(_) => boxed_primitive(this_value, crate::ops::Builtin::Boolean),
        Value::String(_) => boxed_primitive(this_value, crate::ops::Builtin::String),
        Value::StringUnits(_) => boxed_primitive(this_value, crate::ops::Builtin::String),
        Value::BigInt(_) => boxed_primitive(this_value, crate::ops::Builtin::BigInt),
        Value::BindingCell(_) => this_value.clone(),
    }
}

fn boxed_primitive(value: &Value, constructor: crate::ops::Builtin) -> Value {
    let prototype = match constructor {
        Builtin::Boolean => Builtin::BooleanPrototype,
        Builtin::String => Builtin::StringPrototype,
        Builtin::BigInt => Builtin::BigIntPrototype,
        Builtin::Number => Builtin::NumberPrototype,
        _ => Builtin::ObjectPrototype,
    };
    let mut properties = vec![
        ("_value".to_string(), value.clone()),
        ("\0prototype".to_string(), crate::vm::realm_intrinsic(prototype)),
    ];
    if constructor != Builtin::Number {
        properties.push((
            "constructor".to_string(),
            crate::vm::realm_intrinsic(constructor),
        ));
    }
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(properties)))
}

pub fn execute_builtin_with_receiver(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Some(result) = stateful_builtin(builtin, receiver, arguments) {
        return result;
    }
    if builtin == Builtin::Print {
        return execute_print(arguments);
    }
    if is_object_special(builtin) {
        return crate::builtins::object::execute_special(builtin, receiver, arguments);
    }
    if let Some(result) = define_builtin(builtin, arguments) {
        return result;
    }
    if let Some(result) = early_dispatch(builtin, receiver, arguments) {
        return result;
    }
    if is_data_view_builtin(builtin) {
        return execute_data_view_builtin(builtin, receiver, arguments);
    }
    if is_shared_array_buffer_builtin(builtin) {
        return execute_shared_array_buffer_builtin(builtin, receiver, arguments);
    }
    if let Builtin::HostCapability(kind) = builtin {
        return vm_ops::execute_host_capability(kind, receiver, arguments);
    }
    match builtin {
        _ if is_function_builtin(builtin) => {
            crate::functions::function_builtin(builtin, receiver, arguments)
        }
        _ if is_simple_builtin(builtin) => execute_simple_builtin(builtin, arguments, receiver),
        _ => vm_ops::execute_builtin_tail(builtin, arguments, receiver),
    }
}

fn is_shared_array_buffer_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ArrayBufferByteLengthGetter
            | Builtin::ArrayBufferDetachedGetter
            | Builtin::ArrayBufferImmutableGetter
            | Builtin::ArrayBufferMaxByteLengthGetter
            | Builtin::ArrayBufferResizableGetter
            | Builtin::SharedArrayBufferByteLengthGetter
            | Builtin::SharedArrayBufferGrow
            | Builtin::ArrayBufferSlice
            | Builtin::SharedArrayBufferSlice
            | Builtin::SharedArrayBufferGrowableGetter
            | Builtin::SharedArrayBufferMaxByteLengthGetter
    )
}

fn define_builtin(builtin: Builtin, arguments: &[Value]) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::ObjectDefineProperty => Some(crate::builtins::define_property(arguments)),
        Builtin::ObjectDefineProperties => Some(crate::builtins::define_properties(arguments)),
        _ => None,
    }
}

fn stateful_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::GeneratorNext => Some(crate::generator::next(receiver, arguments)),
        Builtin::AsyncGeneratorNext => Some(crate::generator::async_next(receiver, arguments)),
        Builtin::GeneratorReturn => Some(crate::generator::return_(receiver, arguments)),
        Builtin::AsyncGeneratorReturn => Some(crate::generator::async_return(receiver, arguments)),
        Builtin::GeneratorThrow => Some(crate::generator::throw(receiver, arguments)),
        Builtin::AsyncGeneratorThrow => Some(crate::generator::async_throw(receiver, arguments)),
        Builtin::AsyncIteratorDispose => Some(crate::generator::async_dispose(receiver)),
        Builtin::AsyncIteratorDisposeFulfilled => Some(Ok(Value::Undefined)),
        Builtin::ProxyRevoke => Some(crate::proxy::revoke(receiver)),
        Builtin::Math => Some(Err(not_callable())),
        builtin @ (Builtin::AtomicsAdd
        | Builtin::AtomicsAnd
        | Builtin::AtomicsOr
        | Builtin::AtomicsSub
        | Builtin::AtomicsXor
        | Builtin::AtomicsCompareExchange) => {
            Some(crate::atomics::execute(builtin, receiver, arguments))
        }
        Builtin::AtomicsIsLockFree => Some(crate::atomics::is_lock_free(arguments)),
        Builtin::AtomicsNotify => Some(crate::atomics::notify(arguments)),
        Builtin::AtomicsWait => Some(crate::atomics::wait(arguments)),
        Builtin::AtomicsLoad | Builtin::AtomicsStore => {
            Some(crate::atomics::load_store(builtin, arguments))
        }
        Builtin::AtomicsExchange => Some(crate::atomics::exchange(arguments)),
        Builtin::AtomicsWaitAsync => Some(crate::atomics::wait_async(arguments)),
        Builtin::AtomicsPause => Some(Ok(Value::Undefined)),
        _ => None,
    }
}

fn is_object_special(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ObjectHasOwnProperty
            | Builtin::ObjectHasOwn
            | Builtin::ObjectGetOwnPropertyDescriptor
            | Builtin::ObjectGetOwnPropertyDescriptors
            | Builtin::ObjectGetOwnPropertyNames
            | Builtin::ObjectGetOwnPropertySymbols
            | Builtin::ObjectKeys
            | Builtin::ObjectValues
            | Builtin::ObjectEntries
            | Builtin::ObjectAssign
            | Builtin::ObjectFromEntries
            | Builtin::ObjectGroupBy
            | Builtin::ObjectCreate
            | Builtin::ObjectGetPrototypeOf
            | Builtin::ObjectSetPrototypeOf
            | Builtin::ObjectPropertyIsEnumerable
            | Builtin::ObjectPrototypeIsPrototypeOf
            | Builtin::ObjectPrototypeDefineGetter
            | Builtin::ObjectPrototypeDefineSetter
            | Builtin::ObjectPrototypeLookupGetter
            | Builtin::ObjectPrototypeLookupSetter
    )
}

include!("vm_host.rs");
include!("vm_boolean_value.rs");
include!("vm_builtins.rs");
include!("vm_properties.rs");
include!("vm_dispatch.rs");
