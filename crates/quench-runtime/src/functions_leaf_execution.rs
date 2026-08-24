const LEAF_REGISTERS: usize = 48;
const LEAF_FACT_SLOTS: usize = 256;

#[derive(Clone)]
struct LeafFact {
    function: std::rc::Weak<crate::value::FunctionValue>,
    rejection: Option<LeafReject>,
}

#[derive(Clone, Copy)]
enum LeafReject { Length, Opcode, Register, Call, Control, Depth }

impl LeafReject {
    fn event(self) -> crate::execution_trace::Event {
        use crate::execution_trace::Event::*;
        match self {
            Self::Length => LeafRejectLength,
            Self::Opcode => LeafRejectOpcode,
            Self::Register => LeafRejectRegister,
            Self::Call => LeafRejectCall,
            Self::Control => LeafRejectControl,
            Self::Depth => LeafRejectDepth,
        }
    }
}

thread_local! {
    static LEAF_FACTS: std::cell::RefCell<Vec<Option<LeafFact>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn execute_proven_leaf(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    crate::execution_trace::event(crate::execution_trace::Event::LeafAttempt);
    let code = function.code.code()?;
    if let Err(rejection) = proven_leaf(function, code) {
        crate::execution_trace::event(crate::execution_trace::Event::LeafReject);
        crate::execution_trace::event(rejection.event());
        return None;
    }
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    let mut registers: [crate::value::Value; LEAF_REGISTERS] =
        std::array::from_fn(|_| crate::value::Value::Undefined);
    Some(run_leaf(
        function,
        receiver,
        arguments,
        code,
        &mut registers,
    ))
}

fn proven_leaf(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    code: crate::machine::CodeView<'_>,
) -> Result<(), LeafReject> {
    let index = (std::rc::Rc::as_ptr(function) as usize >> 4) & (LEAF_FACT_SLOTS - 1);
    let cached = LEAF_FACTS.with(|facts| facts.borrow().get(index).and_then(Clone::clone));
    if let Some(cached) = cached.filter(|cached| {
        cached
            .function
            .upgrade()
            .is_some_and(|value| std::rc::Rc::ptr_eq(&value, function))
    }) {
        return cached.rejection.map_or(Ok(()), Err);
    }
    let rejection = validate_leaf(code).err();
    LEAF_FACTS.with(|facts| {
        let mut facts = facts.borrow_mut();
        if facts.is_empty() {
            facts.resize_with(LEAF_FACT_SLOTS, || None);
        }
        facts[index] = Some(LeafFact {
            function: std::rc::Rc::downgrade(function),
            rejection,
        });
    });
    rejection.map_or(Ok(()), Err)
}

fn validate_leaf(code: crate::machine::CodeView<'_>) -> Result<(), LeafReject> {
    validate_leaf_depth(code, 0)
}

fn validate_leaf_depth(code: crate::machine::CodeView<'_>, depth: u8) -> Result<(), LeafReject> {
    (depth < 4).then_some(()).ok_or(LeafReject::Depth)?;
    (code.len() <= 16).then_some(()).ok_or(LeafReject::Length)?;
    for pc in 0..code.len() {
        let op = code.instruction(pc).ok_or(LeafReject::Opcode)?;
        matches!(
            op.opcode,
            crate::ir::Opcode::LoadConst
                | crate::ir::Opcode::Move
                | crate::ir::Opcode::LoadLocalChecked
                | crate::ir::Opcode::GetN
                | crate::ir::Opcode::SetN
                | crate::ir::Opcode::AGetI
                | crate::ir::Opcode::Binary
                | crate::ir::Opcode::CallN
                | crate::ir::Opcode::Return
                | crate::ir::Opcode::Slow
        )
        .then_some(()).ok_or(LeafReject::Opcode)?;
        leaf_registers(op)
            .into_iter()
            .all(|register| usize::from(register) < LEAF_REGISTERS)
            .then_some(()).ok_or(LeafReject::Register)?;
        if op.opcode == crate::ir::Opcode::CallN {
            (op.flags <= 1).then_some(()).ok_or(LeafReject::Call)?;
        }
        if op.opcode == crate::ir::Opcode::Slow {
            validate_leaf_control(code.cold(op).ok_or(LeafReject::Control)?, depth)?;
        }
    }
    Ok(())
}

fn validate_leaf_control(op: &crate::ops::Op, depth: u8) -> Result<(), LeafReject> {
    let registers = match op {
        crate::ops::Op::Conditional {
            dst,
            condition,
            consequent,
            alternate,
        } => {
            validate_leaf_depth(consequent.code().ok_or(LeafReject::Control)?, depth + 1)?;
            validate_leaf_depth(alternate.code().ok_or(LeafReject::Control)?, depth + 1)?;
            [*dst, *condition]
        }
        crate::ops::Op::Branch {
            condition,
            then_ops,
            else_ops,
        } => {
            validate_leaf_depth(then_ops.code().ok_or(LeafReject::Control)?, depth + 1)?;
            validate_leaf_depth(else_ops.code().ok_or(LeafReject::Control)?, depth + 1)?;
            [*condition, 0]
        }
        crate::ops::Op::Loop {
            label,
            init,
            test,
            body,
            update,
            post_test,
            dst,
            per_iteration,
        } if label.is_none() && !post_test && per_iteration.is_empty() => {
            validate_leaf_depth(init.code().ok_or(LeafReject::Control)?, depth + 1)?;
            validate_leaf_depth(test.code().ok_or(LeafReject::Control)?, depth + 1)?;
            validate_leaf_depth(body.code().ok_or(LeafReject::Control)?, depth + 1)?;
            validate_leaf_depth(update.code().ok_or(LeafReject::Control)?, depth + 1)?;
            [*dst, 0]
        }
        _ => return Err(LeafReject::Control),
    };
    registers
        .into_iter()
        .all(|register| usize::from(register) < LEAF_REGISTERS)
        .then_some(()).ok_or(LeafReject::Register)
}

fn leaf_registers(op: crate::ir::Instruction) -> [u16; 3] {
    use crate::ir::Opcode::*;
    match op.opcode {
        LoadConst | LoadLocalChecked | Return => [op.a, 0, 0],
        Move => [op.a, op.b, 0],
        GetN | SetN => [op.a, op.b, 0],
        AGetI | Binary => [op.a, op.b, op.c],
        CallN if op.flags == 0 => [op.a, op.b, 0],
        CallN => [op.a, op.b, op.c],
        Slow => [0, 0, 0],
        _ => [u16::MAX; 3],
    }
}

fn run_leaf(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
    code: crate::machine::CodeView<'_>,
    registers: &mut [crate::value::Value; LEAF_REGISTERS],
) -> Result<crate::value::Value, crate::execute::VmError> {
    run_leaf_fragment(function, receiver, arguments, code, registers)?
        .ok_or(crate::execute::VmError::MissingReturn)
}

fn run_leaf_fragment(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
    code: crate::machine::CodeView<'_>,
    registers: &mut [crate::value::Value; LEAF_REGISTERS],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    for pc in 0..code.len() {
        let op = code
            .instruction(pc)
            .ok_or(crate::execute::VmError::MissingReturn)?;
        if let Some(value) = run_leaf_op(function, receiver, arguments, code, pc, op, registers)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn run_leaf_op(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
    code: crate::machine::CodeView<'_>,
    pc: usize,
    op: crate::ir::Instruction,
    registers: &mut [crate::value::Value; LEAF_REGISTERS],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    use crate::ir::Opcode::*;
    let value = match op.opcode {
        LoadConst => code.constant_at(pc).map(|(_, value)| value.into()),
        Move => Some(registers[usize::from(op.b)].clone()),
        LoadLocalChecked => Some(leaf_local(function, receiver, arguments, code, pc, op.b)?),
        GetN => Some(leaf_get_named(code, pc, &registers[usize::from(op.b)])?),
        SetN => {
            leaf_set_named(code, pc, op, registers)?;
            return Ok(None);
        }
        AGetI => Some(leaf_get_index(
            &registers[usize::from(op.b)],
            &registers[usize::from(op.c)],
        )?),
        Binary => Some(crate::vm::vm_arithmetic::evaluate_binary(
            &registers[usize::from(op.b)],
            &registers[usize::from(op.c)],
            crate::ir::compact_binary_operator(op.flags)
                .ok_or(crate::execute::VmError::MissingReturn)?,
        )?),
        CallN => Some(leaf_call(code, pc, op, registers)?),
        Slow => return leaf_control(function, receiver, arguments, code, op, registers),
        Return => return Ok(Some(registers[usize::from(op.a)].clone())),
        _ => None,
    };
    registers[usize::from(op.a)] = value.ok_or(crate::execute::VmError::MissingReturn)?;
    Ok(None)
}

fn leaf_conditional(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
    code: crate::machine::CodeView<'_>,
    op: crate::ir::Instruction,
    registers: &mut [crate::value::Value; LEAF_REGISTERS],
) -> Result<(u16, crate::value::Value), crate::execute::VmError> {
    let crate::ops::Op::Conditional {
        dst,
        condition,
        consequent,
        alternate,
    } = code
        .cold(op)
        .ok_or(crate::execute::VmError::MissingReturn)?
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let branch = if crate::vm::is_truthy(&registers[usize::from(*condition)]) {
        consequent
    } else {
        alternate
    };
    let value = run_leaf(
        function,
        receiver,
        arguments,
        branch
            .code()
            .ok_or(crate::execute::VmError::MissingReturn)?,
        registers,
    )?;
    Ok((*dst, value))
}

fn leaf_control(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
    code: crate::machine::CodeView<'_>,
    op: crate::ir::Instruction,
    registers: &mut [crate::value::Value; LEAF_REGISTERS],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    match code.cold(op) {
        Some(crate::ops::Op::Conditional { .. }) => {
            let (dst, value) = leaf_conditional(function, receiver, arguments, code, op, registers)?;
            registers[usize::from(dst)] = value;
            Ok(None)
        }
        Some(crate::ops::Op::Branch {
            condition,
            then_ops,
            else_ops,
        }) => {
            let selected = if crate::vm::is_truthy(&registers[usize::from(*condition)]) {
                then_ops
            } else {
                else_ops
            };
            run_leaf_fragment(
                function,
                receiver,
                arguments,
                selected.code().ok_or(crate::execute::VmError::MissingReturn)?,
                registers,
            )
        }
        Some(crate::ops::Op::Loop {
            init,
            test,
            body,
            update,
            dst,
            ..
        }) => {
            if let Some(value) = run_leaf_fragment(
                function,
                receiver,
                arguments,
                init.code().ok_or(crate::execute::VmError::MissingReturn)?,
                registers,
            )? {
                return Ok(Some(value));
            }
            loop {
                let condition = run_leaf(
                    function,
                    receiver,
                    arguments,
                    test.code().ok_or(crate::execute::VmError::MissingReturn)?,
                    registers,
                )?;
                if !crate::vm::is_truthy(&condition) {
                    break;
                }
                registers[usize::from(*dst)] = crate::value::Value::Undefined;
                if let Some(value) = run_leaf_fragment(
                    function,
                    receiver,
                    arguments,
                    body.code().ok_or(crate::execute::VmError::MissingReturn)?,
                    registers,
                )? {
                    return Ok(Some(value));
                }
                if let Some(value) = run_leaf_fragment(
                    function,
                    receiver,
                    arguments,
                    update.code().ok_or(crate::execute::VmError::MissingReturn)?,
                    registers,
                )? {
                    return Ok(Some(value));
                }
            }
            Ok(None)
        }
        _ => Err(crate::execute::VmError::MissingReturn),
    }
}

fn leaf_local(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
    code: crate::machine::CodeView<'_>,
    pc: usize,
    slot: u16,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let base = u16::try_from(function.captures.len()).unwrap_or(u16::MAX);
    if slot < base {
        if function.captures.is_uninitialized(slot) {
            let name = code
                .metadata_at(pc)
                .and_then(|meta| meta.name.as_deref())
                .unwrap_or("binding");
            return Err(crate::value::error::throw_reference_error(&format!(
                "Cannot access '{name}' before initialization"
            )));
        }
        return Ok(function.captures.get(slot));
    }
    let offset = usize::from(slot - base);
    if offset < usize::from(function.params) {
        return Ok(arguments
            .get(offset)
            .cloned()
            .unwrap_or(crate::value::Value::Undefined));
    }
    if offset == usize::from(function.params) + 1 {
        return Ok(receiver.clone());
    }
    Ok(crate::value::Value::Undefined)
}

fn leaf_get_named(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    object: &crate::value::Value,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let metadata = code
        .metadata_at(pc)
        .ok_or(crate::execute::VmError::MissingReturn)?;
    let key = metadata
        .name
        .as_deref()
        .ok_or(crate::execute::VmError::MissingReturn)?;
    crate::vm::get_named_property_result(object, key, &metadata.named_cache)
}

fn leaf_set_named(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    op: crate::ir::Instruction,
    registers: &mut [crate::value::Value; LEAF_REGISTERS],
) -> Result<(), crate::execute::VmError> {
    let metadata = code
        .metadata_at(pc)
        .ok_or(crate::execute::VmError::MissingReturn)?;
    let key = metadata
        .name
        .as_deref()
        .ok_or(crate::execute::VmError::MissingReturn)?;
    let target = registers[usize::from(op.a)].clone();
    let value = registers[usize::from(op.b)].clone();
    if let crate::value::Value::Object(object) = &target {
        if !object.has_replacement() {
            if let Some((layout, slot)) = crate::machine::unpack_named_cache(metadata.named_cache.get()) {
                if object.semantic_layout_id() == layout {
                    if let Some((_, crate::value::Value::BindingCell(cell))) =
                        object.hot_properties().get(slot as usize)
                    {
                        *cell.borrow_mut() = value;
                        return Ok(());
                    }
                }
            }
        }
    }
    let mut temporary = crate::register_file::RegisterFile::from_values(vec![target, value]);
    crate::properties::execute_set_named_cached(
        &mut temporary,
        0,
        key,
        1,
        op.flags != 0,
        &metadata.named_cache,
    )?;
    registers[usize::from(op.a)] = crate::execute::read_register(&temporary, 0)?;
    Ok(())
}

fn leaf_get_index(
    object: &crate::value::Value,
    key: &crate::value::Value,
) -> Result<crate::value::Value, crate::execute::VmError> {
    if let (crate::value::Value::Array(array), Some(index)) = (object, leaf_array_index(key)) {
        if array.is_packed_ordinary() {
            if let Some(value) = array.dense_value_at(index) {
                return Ok(value);
            }
        }
    }
    let key = crate::properties::dynamic_property_key(key)?;
    crate::execute::get_property_result(object, &key)
}

fn leaf_array_index(value: &crate::value::Value) -> Option<usize> {
    let crate::value::Value::Number(value) = value else {
        return None;
    };
    (*value >= 0.0 && value.fract() == 0.0 && *value <= usize::MAX as f64)
        .then_some(*value as usize)
}

fn leaf_call(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    op: crate::ir::Instruction,
    registers: &[crate::value::Value; LEAF_REGISTERS],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let receiver = crate::locals::resolved_replacement(registers[usize::from(op.b)].clone());
    let (callee, argument) = if op.flags == 0 {
        (leaf_get_named(code, pc, &receiver)?, None)
    } else {
        let argument =
            op.a.checked_sub(1)
                .ok_or(crate::execute::VmError::MissingReturn)?;
        (
            registers[usize::from(op.c)].clone(),
            Some(registers[usize::from(argument)].clone()),
        )
    };
    let arguments = argument
        .as_ref()
        .map(std::slice::from_ref)
        .unwrap_or_default();
    crate::functions::execute_target(&callee, &receiver, arguments)
}
