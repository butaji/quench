const LEAF_REGISTERS: usize = 96;
const SMALL_LEAF_REGISTERS: usize = 32;
const LEAF_LOCAL_SLOTS: usize = 128;
const LEAF_FACT_SLOTS: usize = 256;

pub(crate) fn execute_word_leaf_call(
    caller: &mut crate::register_file::RegisterFile,
    destination: u16,
    callee: u16,
    argument: Option<u16>,
) -> Option<Result<(), crate::execute::VmError>> {
    let function = caller.function_ptr(usize::from(callee))?;
    // SAFETY: the caller register owns the function word for the complete
    // call. This lane never overwrites that register before returning.
    let function = unsafe { &*function };
    if function.is_async
        || !matches!(function.kind, crate::ops::FunctionKind::Ordinary)
        || usize::from(function.params) != usize::from(argument.is_some())
    {
        return None;
    }
    if let Some(argument) = argument {
        if crate::functions::execute_cached_word_call(caller, destination, argument, function)
            .is_some()
        {
            return Some(Ok(()));
        }
    }
    let code = function.code.code()?;
    let parameter_base = u16::try_from(function.captures.len()).ok()?;
    let mut frame = crate::register_file::FixedWordFile::<SMALL_LEAF_REGISTERS>::new();
    crate::execution_trace::event(crate::execution_trace::Event::LeafAttempt);
    for pc in 0..code.len() {
        let op = code.instruction(pc)?;
        let _decode_guard = crate::execution_trace::leaf_compact(op.opcode);
        match op.opcode {
            crate::ir::Opcode::LoadConst => {
                let (_, constant) = code.constant_at(pc)?;
                let crate::ops::Constant::Number(number) = constant else {
                    return None;
                };
                frame.write_number(usize::from(op.a), *number)?;
            }
            crate::ir::Opcode::LoadLocal => {
                let parameter = op.b.checked_sub(parameter_base)?;
                if parameter >= function.params {
                    return None;
                }
                let source = argument.filter(|_| parameter == 0)?;
                frame.copy_from(usize::from(op.a), caller, usize::from(source))?;
            }
            crate::ir::Opcode::Add
            | crate::ir::Opcode::Sub
            | crate::ir::Opcode::Mul
            | crate::ir::Opcode::Div
            | crate::ir::Opcode::Binary => {
                let left = frame.number(usize::from(op.b))?;
                let right = frame.number(usize::from(op.c))?;
                let operator = leaf_binary_operator(op)?;
                let crate::value::Value::Number(result) =
                    crate::vm::vm_arithmetic::fast_number_binary(left, right, operator)?
                else {
                    return None;
                };
                frame.write_number(usize::from(op.a), result)?;
            }
            crate::ir::Opcode::Return => {
                frame.copy_to_register(usize::from(op.a), caller, usize::from(destination))?;
                crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
                return Some(Ok(()));
            }
            _ => return None,
        }
    }
    None
}

trait LeafRegisterFile {
    fn read(&self, index: usize) -> Option<crate::value::Value>;
    fn write(&mut self, index: usize, value: crate::value::Value) -> Option<()>;
    fn write_number(&mut self, index: usize, value: f64) -> Option<()>;
    fn copy(&mut self, destination: usize, source: usize) -> Option<()>;
    fn truthiness(&self, index: usize) -> Option<bool>;
    fn number(&self, index: usize) -> Option<f64>;
    fn object(&self, index: usize) -> Option<&crate::value::ObjectData>;
    fn write_slot(&mut self, index: usize, slot: &crate::register_file::SlotWord) -> Option<()>;
    fn write_cell(&mut self, index: usize, cell: &crate::value::BindingCell) -> Option<()>;
    fn store_slot(&self, source: usize, slot: &crate::register_file::SlotWord) -> Option<()>;
    fn load_environment(
        &mut self,
        environment: &crate::environment::Environment,
        destination: usize,
        slot: u16,
    ) -> bool;
    fn copy_from_local(
        &mut self,
        locals: &crate::register_file::LocalWordFile<LEAF_LOCAL_SLOTS>,
        destination: usize,
        slot: u16,
    ) -> Option<()>;
    fn copy_to_local(
        &self,
        locals: &mut crate::register_file::LocalWordFile<LEAF_LOCAL_SLOTS>,
        slot: u16,
        source: usize,
    ) -> Option<()>;
}

impl<const N: usize> LeafRegisterFile for crate::register_file::FixedWordFile<N> {
    fn read(&self, index: usize) -> Option<crate::value::Value> {
        self.read(index)
    }
    fn write(&mut self, index: usize, value: crate::value::Value) -> Option<()> {
        self.write(index, value)
    }
    fn write_number(&mut self, index: usize, value: f64) -> Option<()> {
        self.write_number(index, value)
    }
    fn copy(&mut self, destination: usize, source: usize) -> Option<()> {
        self.copy(destination, source)
    }
    fn truthiness(&self, index: usize) -> Option<bool> {
        self.truthiness(index)
    }
    fn number(&self, index: usize) -> Option<f64> {
        self.number(index)
    }
    fn object(&self, index: usize) -> Option<&crate::value::ObjectData> {
        self.object(index)
    }
    fn write_slot(&mut self, index: usize, slot: &crate::register_file::SlotWord) -> Option<()> {
        slot.copy_to_fixed(self, index)
    }
    fn write_cell(&mut self, index: usize, cell: &crate::value::BindingCell) -> Option<()> {
        cell.with_word(|word| self.write_owned(index, word))
    }
    fn store_slot(&self, source: usize, slot: &crate::register_file::SlotWord) -> Option<()> {
        slot.store_from_fixed(self, source)
    }
    fn load_environment(
        &mut self,
        environment: &crate::environment::Environment,
        destination: usize,
        slot: u16,
    ) -> bool {
        environment.load_into_fixed(self, destination, slot)
    }
    fn copy_from_local(
        &mut self,
        locals: &crate::register_file::LocalWordFile<LEAF_LOCAL_SLOTS>,
        destination: usize,
        slot: u16,
    ) -> Option<()> {
        locals.copy_to_fixed(slot, self, destination)
    }
    fn copy_to_local(
        &self,
        locals: &mut crate::register_file::LocalWordFile<LEAF_LOCAL_SLOTS>,
        slot: u16,
        source: usize,
    ) -> Option<()> {
        locals.copy_from_fixed(slot, self, source)
    }
}

trait LeafLocalFile {
    fn read(&self, slot: u16) -> Option<crate::value::Value>;
    fn write(&mut self, slot: u16, value: crate::value::Value) -> Option<()>;
    fn copy_to(
        &self,
        slot: u16,
        registers: &mut impl LeafRegisterFile,
        destination: usize,
    ) -> Option<()>;
    fn copy_from(
        &mut self,
        slot: u16,
        registers: &impl LeafRegisterFile,
        source: usize,
    ) -> Option<()>;
    fn update_number(&mut self, slot: u16, delta: f64) -> Option<(f64, f64)>;
}

impl LeafLocalFile for crate::register_file::LocalWordFile<LEAF_LOCAL_SLOTS> {
    fn read(&self, slot: u16) -> Option<crate::value::Value> {
        self.read(slot)
    }

    fn write(&mut self, slot: u16, value: crate::value::Value) -> Option<()> {
        self.write(slot, value)
    }

    fn copy_to(
        &self,
        slot: u16,
        registers: &mut impl LeafRegisterFile,
        destination: usize,
    ) -> Option<()> {
        registers.copy_from_local(self, destination, slot)
    }

    fn copy_from(
        &mut self,
        slot: u16,
        registers: &impl LeafRegisterFile,
        source: usize,
    ) -> Option<()> {
        registers.copy_to_local(self, slot, source)
    }

    fn update_number(&mut self, slot: u16, delta: f64) -> Option<(f64, f64)> {
        self.update_number(slot, delta)
    }
}

struct NoLeafLocals;

impl LeafLocalFile for NoLeafLocals {
    fn read(&self, _: u16) -> Option<crate::value::Value> {
        None
    }

    fn write(&mut self, _: u16, _: crate::value::Value) -> Option<()> {
        None
    }
    fn copy_to(&self, _: u16, _: &mut impl LeafRegisterFile, _: usize) -> Option<()> {
        None
    }
    fn copy_from(&mut self, _: u16, _: &impl LeafRegisterFile, _: usize) -> Option<()> {
        None
    }
    fn update_number(&mut self, _: u16, _: f64) -> Option<(f64, f64)> {
        None
    }
}

#[derive(Clone)]
struct LeafFact {
    function: std::rc::Weak<crate::value::FunctionValue>,
    rejection: Option<LeafReject>,
    extended: bool,
}

#[derive(Clone, Copy)]
enum LeafReject {
    Length,
    Opcode(&'static str),
    Register,
    Call,
    Control(&'static str),
    Depth,
}

impl LeafReject {
    fn event(self) -> crate::execution_trace::Event {
        use crate::execution_trace::Event::*;
        match self {
            Self::Length => LeafRejectLength,
            Self::Opcode(_) => LeafRejectOpcode,
            Self::Register => LeafRejectRegister,
            Self::Call => LeafRejectCall,
            Self::Control(_) => LeafRejectControl,
            Self::Depth => LeafRejectDepth,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Length => "Length",
            Self::Opcode(name) => name,
            Self::Register => "Register",
            Self::Call => "Call",
            Self::Control(name) => name,
            Self::Depth => "Depth",
        }
    }
}

thread_local! {
    static LEAF_FACTS: std::cell::RefCell<Vec<Option<LeafFact>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[inline(never)]
pub(crate) fn execute_proven_leaf(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    // Async functions must be lowered through the generator completion path;
    // a leaf hit would let a body throw escape synchronously instead of
    // becoming a rejected Promise.
    if function.is_async {
        return None;
    }
    crate::execution_trace::event(crate::execution_trace::Event::LeafAttempt);
    let code = function.code.code()?;
    let extended = match proven_leaf(function, code) {
        Ok(extended) => extended,
        Err(rejection) => {
            crate::execution_trace::event(crate::execution_trace::Event::LeafReject);
            crate::execution_trace::event(rejection.event());
            crate::execution_trace::leaf_rejection(rejection.name());
            return None;
        }
    };
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    if extended {
        let mut registers = crate::register_file::FixedWordFile::<LEAF_REGISTERS>::new();
        let mut locals = crate::register_file::LocalWordFile::<LEAF_LOCAL_SLOTS>::new();
        let parameter_base = u16::try_from(function.captures.len()).unwrap_or(u16::MAX);
        for offset in 0..function.params {
            let value = arguments
                .get(usize::from(offset))
                .cloned()
                .unwrap_or(crate::value::Value::Undefined);
            locals.write(parameter_base.saturating_add(offset), value)?;
        }
        locals.write(
            parameter_base
                .saturating_add(function.params)
                .saturating_add(1),
            receiver.clone(),
        )?;
        return Some(run_leaf(
            function,
            receiver,
            arguments,
            code,
            &mut registers,
            &mut locals,
        ));
    }
    let mut registers = crate::register_file::FixedWordFile::<SMALL_LEAF_REGISTERS>::new();
    let mut locals = NoLeafLocals;
    Some(run_leaf(
        function,
        receiver,
        arguments,
        code,
        &mut registers,
        &mut locals,
    ))
}

fn proven_leaf(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    code: crate::machine::CodeView<'_>,
) -> Result<bool, LeafReject> {
    let arguments_slot = function.captures.captured_len() as u16 + function.params;
    if function.code.uses_slot(arguments_slot) {
        return Err(LeafReject::Opcode("Arguments"));
    }
    if function.captures.captured_len() != 0
        && (0..code.len()).any(|pc| {
            code.instruction(pc)
                .is_some_and(|op| op.opcode == crate::ir::Opcode::CallN)
        })
    {
        return Err(LeafReject::Opcode("CapturedCall"));
    }
    let index = (std::rc::Rc::as_ptr(function) as usize >> 4) & (LEAF_FACT_SLOTS - 1);
    let cached = LEAF_FACTS.with(|facts| facts.borrow().get(index).and_then(Clone::clone));
    if let Some(cached) = cached.filter(|cached| {
        cached
            .function
            .upgrade()
            .is_some_and(|value| std::rc::Rc::ptr_eq(&value, function))
    }) {
        return cached.rejection.map_or(Ok(cached.extended), Err);
    }
    let arguments_slot = u16::try_from(function.captures.len())
        .unwrap_or(u16::MAX)
        .saturating_add(function.params);
    let result = if function.code.uses_slot(arguments_slot) {
        Err(LeafReject::Control("Arguments"))
    } else {
        validate_leaf(code)
    };
    let rejection = result.as_ref().err().copied();
    let extended = result.as_ref().copied().unwrap_or(false);
    LEAF_FACTS.with(|facts| {
        let mut facts = facts.borrow_mut();
        if facts.is_empty() {
            facts.resize_with(LEAF_FACT_SLOTS, || None);
        }
        facts[index] = Some(LeafFact {
            function: std::rc::Rc::downgrade(function),
            rejection,
            extended,
        });
    });
    result
}

fn validate_leaf(code: crate::machine::CodeView<'_>) -> Result<bool, LeafReject> {
    if validate_leaf_depth(code, 0, 16, SMALL_LEAF_REGISTERS, false).is_ok() {
        return Ok(false);
    }
    validate_leaf_depth(code, 0, 40, LEAF_REGISTERS, true)?;
    Ok(true)
}

fn validate_leaf_depth(
    code: crate::machine::CodeView<'_>,
    depth: u8,
    max_len: usize,
    max_registers: usize,
    allow_locals: bool,
) -> Result<(), LeafReject> {
    (depth < 4).then_some(()).ok_or(LeafReject::Depth)?;
    (code.len() <= max_len)
        .then_some(())
        .ok_or(LeafReject::Length)?;
    for pc in 0..code.len() {
        let op = code
            .instruction(pc)
            .ok_or(LeafReject::Opcode("MissingInstruction"))?;
        if op.opcode == crate::ir::Opcode::Move && op.flags != 0 {
            return Err(LeafReject::Opcode("MoveLocal"));
        }
        matches!(
            op.opcode,
            crate::ir::Opcode::LoadConst
                | crate::ir::Opcode::Move
                | crate::ir::Opcode::LoadLocal
                | crate::ir::Opcode::LoadLocalChecked
                | crate::ir::Opcode::StoreLocal
                | crate::ir::Opcode::StoreLocalChecked
                | crate::ir::Opcode::InitLocal
                | crate::ir::Opcode::UpdateLocal
                | crate::ir::Opcode::GetN
                | crate::ir::Opcode::SetN
                | crate::ir::Opcode::AGetI
                | crate::ir::Opcode::Add
                | crate::ir::Opcode::Sub
                | crate::ir::Opcode::Mul
                | crate::ir::Opcode::Div
                | crate::ir::Opcode::Binary
                | crate::ir::Opcode::Return
                | crate::ir::Opcode::Slow
        )
        .then_some(())
        .ok_or(LeafReject::Opcode(op.opcode.name()))?;
        if matches!(
            op.opcode,
            crate::ir::Opcode::StoreLocal
                | crate::ir::Opcode::StoreLocalChecked
                | crate::ir::Opcode::InitLocal
                | crate::ir::Opcode::UpdateLocal
        ) && !allow_locals
        {
            return Err(LeafReject::Opcode(op.opcode.name()));
        }
        leaf_registers(op)
            .into_iter()
            .all(|register| usize::from(register) < max_registers)
            .then_some(())
            .ok_or(LeafReject::Register)?;
        if matches!(
            op.opcode,
            crate::ir::Opcode::StoreLocal | crate::ir::Opcode::StoreLocalChecked
        ) && usize::from(op.a) >= LEAF_LOCAL_SLOTS
        {
            return Err(LeafReject::Register);
        }
        if matches!(op.opcode, crate::ir::Opcode::UpdateLocal)
            && usize::from(op.c) >= LEAF_LOCAL_SLOTS
        {
            return Err(LeafReject::Register);
        }
        if op.opcode == crate::ir::Opcode::Slow {
            validate_leaf_control(
                code.cold(op).ok_or(LeafReject::Control("MissingColdOp"))?,
                depth,
                max_len,
                max_registers,
                allow_locals,
            )?;
        }
    }
    Ok(())
}

fn validate_leaf_control(
    op: &crate::ops::Op,
    depth: u8,
    max_len: usize,
    max_registers: usize,
    allow_locals: bool,
) -> Result<(), LeafReject> {
    let registers = match op {
        crate::ops::Op::Conditional {
            dst,
            condition,
            consequent,
            alternate,
        } => {
            validate_leaf_depth(
                consequent
                    .code()
                    .ok_or(LeafReject::Control("MissingCode"))?,
                depth + 1,
                max_len,
                max_registers,
                allow_locals,
            )?;
            validate_leaf_depth(
                alternate.code().ok_or(LeafReject::Control("MissingCode"))?,
                depth + 1,
                max_len,
                max_registers,
                allow_locals,
            )?;
            [*dst, *condition]
        }
        crate::ops::Op::Branch {
            condition,
            then_ops,
            else_ops,
        } => {
            validate_leaf_depth(
                then_ops.code().ok_or(LeafReject::Control("MissingCode"))?,
                depth + 1,
                max_len,
                max_registers,
                allow_locals,
            )?;
            validate_leaf_depth(
                else_ops.code().ok_or(LeafReject::Control("MissingCode"))?,
                depth + 1,
                max_len,
                max_registers,
                allow_locals,
            )?;
            [*condition, 0]
        }
        crate::ops::Op::Loop { .. } => return Err(LeafReject::Control("Loop")),
        crate::ops::Op::ResolveBindingTarget { dst, .. } if allow_locals => [*dst, 0],
        crate::ops::Op::InitializeResolvedBinding { slot, src, .. }
            if allow_locals && usize::from(*slot) < LEAF_LOCAL_SLOTS =>
        {
            [*src, 0]
        }
        crate::ops::Op::Unary { operator, .. } => {
            return Err(LeafReject::Control(leaf_unary_name(*operator)));
        }
        op => return Err(LeafReject::Control(op.variant_name())),
    };
    registers
        .into_iter()
        .all(|register| usize::from(register) < max_registers)
        .then_some(())
        .ok_or(LeafReject::Register)
}

fn leaf_unary_name(operator: crate::ops::UnaryOp) -> &'static str {
    use crate::ops::UnaryOp::*;
    match operator {
        Plus => "UnaryPlus",
        Minus => "UnaryMinus",
        Not => "UnaryNot",
        BitwiseNot => "UnaryBitwiseNot",
        Void => "UnaryVoid",
        Typeof => "UnaryTypeof",
        ToString => "UnaryToString",
        ToNumeric => "UnaryToNumeric",
        Delete => "UnaryDelete",
        IsNullish => "UnaryIsNullish",
    }
}

fn leaf_registers(op: crate::ir::Instruction) -> [u16; 3] {
    use crate::ir::Opcode::*;
    match op.opcode {
        LoadConst | LoadLocal | LoadLocalChecked | Return => [op.a, 0, 0],
        StoreLocal | StoreLocalChecked | InitLocal => [op.b, 0, 0],
        UpdateLocal => [op.a, op.b, 0],
        Move => [op.a, op.b, 0],
        GetN | SetN => [op.a, op.b, 0],
        AGetI | Add | Sub | Mul | Div | Binary => [op.a, op.b, op.c],
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
    registers: &mut impl LeafRegisterFile,
    locals: &mut impl LeafLocalFile,
) -> Result<crate::value::Value, crate::execute::VmError> {
    Ok(run_leaf_fragment(function, receiver, arguments, code, registers, locals)?.unwrap_or(
        crate::value::Value::Undefined,
    ))
}

fn run_leaf_fragment(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
    code: crate::machine::CodeView<'_>,
    registers: &mut impl LeafRegisterFile,
    locals: &mut impl LeafLocalFile,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    for pc in 0..code.len() {
        let op = code
            .instruction(pc)
            .ok_or(crate::execute::VmError::MissingReturn)?;
        if let Some(value) = run_leaf_op(
            function, receiver, arguments, code, pc, op, registers, locals,
        )? {
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
    registers: &mut impl LeafRegisterFile,
    locals: &mut impl LeafLocalFile,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    use crate::ir::Opcode::*;
    let _source_offset = crate::vm::source_offset(
        code.metadata_at(pc).and_then(|metadata| metadata.source),
    );
    let _decode_guard = crate::execution_trace::leaf_compact(op.opcode);
    let value = match op.opcode {
        LoadConst => code.constant_at(pc).map(|(_, value)| value.into()),
        Move => {
            registers
                .copy(usize::from(op.a), usize::from(op.b))
                .ok_or(crate::execute::VmError::MissingReturn)?;
            return Ok(None);
        }
        LoadLocal | LoadLocalChecked if leaf_load_capture(function, code, pc, op, registers)? => {
            return Ok(None);
        }
        LoadLocal | LoadLocalChecked
            if locals.copy_to(op.b, registers, usize::from(op.a)).is_some() =>
        {
            return Ok(None);
        }
        LoadLocal | LoadLocalChecked => Some(leaf_local(
            function, receiver, arguments, code, pc, op.b, locals,
        )?),
        StoreLocal | StoreLocalChecked => {
            let capture_count = u16::try_from(function.captures.len()).unwrap_or(u16::MAX);
            if op.a >= capture_count
                && locals
                    .copy_from(op.a, registers, usize::from(op.b))
                    .is_some()
            {
                return Ok(None);
            }
            leaf_store(function, op.a, leaf_register(registers, op.b)?, locals)?;
            return Ok(None);
        }
        InitLocal => {
            leaf_store(function, op.a, leaf_register(registers, op.b)?, locals)?;
            if op.a < function.captures.len() as u16 {
                function.captures.initialize(op.a);
            }
            return Ok(None);
        }
        UpdateLocal => Some(leaf_update_local(
            function, receiver, arguments, code, pc, op, registers, locals,
        )?),
        GetN if leaf_get_named_word(code, pc, op, registers)? => return Ok(None),
        GetN => Some(leaf_get_named_register(code, pc, op.b, registers)?),
        SetN => {
            leaf_set_named(code, pc, op, registers)?;
            return Ok(None);
        }
        AGetI => Some(leaf_get_index(
            &leaf_register(registers, op.b)?,
            &leaf_register(registers, op.c)?,
        )?),
        Add | Sub | Mul | Div | Binary => {
            if let Some(value) = leaf_number_binary(op, registers) {
                registers
                    .write_number(usize::from(op.a), value)
                    .ok_or(crate::execute::VmError::MissingReturn)?;
                return Ok(None);
            } else {
                let left = leaf_register(registers, op.b)?;
                let right = leaf_register(registers, op.c)?;
                Some(crate::vm::vm_arithmetic::evaluate_binary(
                    &left,
                    &right,
                    leaf_binary_operator(op).ok_or(crate::execute::VmError::MissingReturn)?,
                )?)
            }
        }
        CallN => Some(leaf_call(code, pc, op, registers)?),
        Slow => {
            return leaf_control(function, receiver, arguments, code, op, registers, locals);
        }
        Return => return Ok(Some(leaf_register(registers, op.a)?)),
        _ => None,
    };
    registers
        .write(
            usize::from(op.a),
            value.ok_or(crate::execute::VmError::MissingReturn)?,
        )
        .ok_or(crate::execute::VmError::MissingReturn)?;
    Ok(None)
}

fn leaf_load_capture(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    code: crate::machine::CodeView<'_>,
    pc: usize,
    op: crate::ir::Instruction,
    registers: &mut impl LeafRegisterFile,
) -> Result<bool, crate::execute::VmError> {
    if usize::from(op.b) >= function.captures.len() {
        return Ok(false);
    }
    if function.captures.is_uninitialized(op.b) {
        let name = code
            .metadata_at(pc)
            .and_then(|meta| meta.name.as_deref())
            .unwrap_or("binding");
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access '{name}' before initialization"
        )));
    }
    registers
        .load_environment(&function.captures, usize::from(op.a), op.b)
        .then_some(true)
        .ok_or(crate::execute::VmError::MissingReturn)
}

#[inline(always)]
fn leaf_register(
    registers: &impl LeafRegisterFile,
    index: u16,
) -> Result<crate::value::Value, crate::execute::VmError> {
    registers
        .read(usize::from(index))
        .ok_or(crate::execute::VmError::MissingReturn)
}

#[inline(always)]
fn leaf_number_binary(
    op: crate::ir::Instruction,
    registers: &impl LeafRegisterFile,
) -> Option<f64> {
    let operator = leaf_binary_operator(op)?;
    let left = registers.number(usize::from(op.b))?;
    let right = registers.number(usize::from(op.c))?;
    match crate::vm::vm_arithmetic::fast_number_binary(left, right, operator)? {
        crate::value::Value::Number(value) => Some(value),
        _ => None,
    }
}

fn leaf_binary_operator(op: crate::ir::Instruction) -> Option<crate::ops::BinaryOp> {
    Some(match op.opcode {
        crate::ir::Opcode::Add => crate::ops::BinaryOp::Add,
        crate::ir::Opcode::Sub => crate::ops::BinaryOp::Subtract,
        crate::ir::Opcode::Mul => crate::ops::BinaryOp::Multiply,
        crate::ir::Opcode::Div => crate::ops::BinaryOp::Divide,
        crate::ir::Opcode::Binary => crate::ir::compact_binary_operator(op.flags)?,
        _ => return None,
    })
}

#[inline(always)]
fn leaf_truthy(
    registers: &impl LeafRegisterFile,
    index: u16,
) -> Result<bool, crate::execute::VmError> {
    if let Some(value) = registers.truthiness(usize::from(index)) {
        return Ok(value);
    }
    Ok(crate::vm::is_truthy(&leaf_register(registers, index)?))
}

fn leaf_conditional(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
    code: crate::machine::CodeView<'_>,
    op: crate::ir::Instruction,
    registers: &mut impl LeafRegisterFile,
    locals: &mut impl LeafLocalFile,
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
    let branch = if leaf_truthy(registers, *condition)? {
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
        locals,
    )?;
    Ok((*dst, value))
}

fn leaf_control(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
    code: crate::machine::CodeView<'_>,
    op: crate::ir::Instruction,
    registers: &mut impl LeafRegisterFile,
    locals: &mut impl LeafLocalFile,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    match code.cold(op) {
        Some(crate::ops::Op::Conditional { .. }) => {
            let (dst, value) =
                leaf_conditional(function, receiver, arguments, code, op, registers, locals)?;
            registers
                .write(usize::from(dst), value)
                .ok_or(crate::execute::VmError::MissingReturn)?;
            Ok(None)
        }
        Some(crate::ops::Op::Branch {
            condition,
            then_ops,
            else_ops,
        }) => {
            let selected = if leaf_truthy(registers, *condition)? {
                then_ops
            } else {
                else_ops
            };
            run_leaf_fragment(
                function,
                receiver,
                arguments,
                selected
                    .code()
                    .ok_or(crate::execute::VmError::MissingReturn)?,
                registers,
                locals,
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
                locals,
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
                    locals,
                )?;
                if !crate::vm::is_truthy(&condition) {
                    break;
                }
                registers
                    .write(usize::from(*dst), crate::value::Value::Undefined)
                    .ok_or(crate::execute::VmError::MissingReturn)?;
                if let Some(value) = run_leaf_fragment(
                    function,
                    receiver,
                    arguments,
                    body.code().ok_or(crate::execute::VmError::MissingReturn)?,
                    registers,
                    locals,
                )? {
                    return Ok(Some(value));
                }
                if let Some(value) = run_leaf_fragment(
                    function,
                    receiver,
                    arguments,
                    update
                        .code()
                        .ok_or(crate::execute::VmError::MissingReturn)?,
                    registers,
                    locals,
                )? {
                    return Ok(Some(value));
                }
            }
            Ok(None)
        }
        Some(crate::ops::Op::ResolveBindingTarget { dst, .. }) => {
            registers
                .write(usize::from(*dst), crate::value::Value::Undefined)
                .ok_or(crate::execute::VmError::MissingReturn)?;
            Ok(None)
        }
        Some(crate::ops::Op::InitializeResolvedBinding { slot, src, .. }) => {
            leaf_store(function, *slot, leaf_register(registers, *src)?, locals)?;
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
    locals: &impl LeafLocalFile,
) -> Result<crate::value::Value, crate::execute::VmError> {
    if let Some(value) = locals.read(slot) {
        return Ok(value);
    }
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

fn leaf_store(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    slot: u16,
    value: crate::value::Value,
    locals: &mut impl LeafLocalFile,
) -> Result<(), crate::execute::VmError> {
    let capture_count = u16::try_from(function.captures.len()).unwrap_or(u16::MAX);
    if slot < capture_count {
        function.captures.set(slot, value);
        return Ok(());
    }
    locals
        .write(slot, value)
        .ok_or(crate::execute::VmError::MissingReturn)
}

fn leaf_update_local(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
    code: crate::machine::CodeView<'_>,
    pc: usize,
    op: crate::ir::Instruction,
    registers: &mut impl LeafRegisterFile,
    locals: &mut impl LeafLocalFile,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let capture_count = u16::try_from(function.captures.len()).unwrap_or(u16::MAX);
    let decrement = op.flags != 0;
    let delta = 1.0;
    if op.c >= capture_count {
        if let Some((old, updated)) = locals.update_number(op.c, delta) {
            registers
                .write_number(usize::from(op.b), updated)
                .ok_or(crate::execute::VmError::MissingReturn)?;
            return Ok(crate::value::Value::Number(old));
        }
    }
    let old = leaf_local(function, receiver, arguments, code, pc, op.c, locals)?;
    let delta = crate::value::Value::Number(delta);
    let operator = if decrement {
        crate::ops::BinaryOp::NumericSubtract
    } else {
        crate::ops::BinaryOp::NumericAdd
    };
    let updated = crate::vm::vm_arithmetic::evaluate_binary(&old, &delta, operator)?;
    registers
        .write(usize::from(op.b), updated.clone())
        .ok_or(crate::execute::VmError::MissingReturn)?;
    leaf_store(function, op.c, updated, locals)?;
    crate::vm::vm_arithmetic::evaluate_to_numeric(&old)
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

fn leaf_get_named_register(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    register: u16,
    registers: &impl LeafRegisterFile,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let metadata = code
        .metadata_at(pc)
        .ok_or(crate::execute::VmError::MissingReturn)?;
    if let Some(object) = registers.object(usize::from(register)) {
        if let Some(value) = crate::vm::get_named_cached_object(object, &metadata.named_cache) {
            return Ok(value);
        }
    }
    leaf_get_named(code, pc, &leaf_register(registers, register)?)
}

fn leaf_get_named_word(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    op: crate::ir::Instruction,
    registers: &mut impl LeafRegisterFile,
) -> Result<bool, crate::execute::VmError> {
    if op.a == op.b {
        return Ok(false);
    }
    let metadata = code
        .metadata_at(pc)
        .ok_or(crate::execute::VmError::MissingReturn)?;
    let payload = registers
        .object(usize::from(op.b))
        .and_then(|object| crate::vm::get_named_cached_payload(object, &metadata.named_cache));
    let Some(payload) = payload else {
        return Ok(false);
    };
    match payload {
        crate::vm::NamedCachedPayload::Word(slot) => registers
            .write_slot(usize::from(op.a), unsafe { &*slot })
            .ok_or(crate::execute::VmError::MissingReturn)?,
        crate::vm::NamedCachedPayload::Cell(cell) => registers
            .write_cell(usize::from(op.a), unsafe { &*cell })
            .ok_or(crate::execute::VmError::MissingReturn)?,
        crate::vm::NamedCachedPayload::Value(value) => registers
            .write(usize::from(op.a), value)
            .ok_or(crate::execute::VmError::MissingReturn)?,
    }
    Ok(true)
}

fn leaf_set_named(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    op: crate::ir::Instruction,
    registers: &mut impl LeafRegisterFile,
) -> Result<(), crate::execute::VmError> {
    let metadata = code
        .metadata_at(pc)
        .ok_or(crate::execute::VmError::MissingReturn)?;
    let key = metadata
        .name
        .as_deref()
        .ok_or(crate::execute::VmError::MissingReturn)?;
    if let Some(object) = registers.object(usize::from(op.a)) {
        if !object.has_replacement() {
            if let Some((layout, slot)) =
                crate::machine::unpack_named_cache(metadata.named_cache.get())
            {
                if object.semantic_layout_id() == layout {
                    if let Some(word) = object.hot_properties().slot_word(slot as usize) {
                        registers
                            .store_slot(usize::from(op.b), word)
                            .ok_or(crate::execute::VmError::MissingReturn)?;
                        crate::execution_trace::event(
                            crate::execution_trace::Event::NamedPropertySetHit,
                        );
                        return Ok(());
                    }
                }
            }
        }
    }
    let target = leaf_register(registers, op.a)?;
    let value = leaf_register(registers, op.b)?;
    let mut temporary = crate::register_file::RegisterFile::from_values(vec![target, value]);
    crate::properties::execute_set_named_cached(
        &mut temporary,
        0,
        key,
        1,
        op.flags != 0,
        &metadata.named_cache,
    )?;
    registers
        .write(
            usize::from(op.a),
            crate::execute::read_register(&temporary, 0)?,
        )
        .ok_or(crate::execute::VmError::MissingReturn)?;
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
    registers: &impl LeafRegisterFile,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let receiver = crate::locals::resolved_replacement(leaf_register(registers, op.b)?);
    if op.flags == 0 {
        let callee = leaf_get_named(code, pc, &receiver)?;
        return crate::functions::execute_target_with_receiver(&callee, &receiver, &[])
            .map(|(value, _)| value);
    }
    let callee = leaf_register(registers, op.c)?;
    let first =
        op.a.checked_sub(u16::from(op.flags))
            .ok_or(crate::execute::VmError::MissingReturn)?;
    let arguments = [
        leaf_register(registers, first)?,
        (op.flags == 2)
            .then(|| leaf_register(registers, first + 1))
            .transpose()?
            .unwrap_or(crate::value::Value::Undefined),
    ];
    crate::functions::execute_target_with_receiver(
        &callee,
        &receiver,
        &arguments[..usize::from(op.flags)],
    )
    .map(|(value, _)| value)
}
