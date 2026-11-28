use super::*;

/// The unit's instructions, origins and frames as a plan lays them out.
pub(super) struct Emitter {
    pub(super) unit: Unit,
}

impl Emitter {
    /// Emits `plan` at the unit's end. The caller's (`frame` `None`) keeps its registers and
    /// its own `Return`s; an inlined callee's are renamed to its `base`, its `this` is
    /// `receiver`, and its completions move to `result` and jump to `continuation`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit(
        &mut self,
        plan: &Plan,
        frame: Option<usize>,
        base: Register,
        receiver: Register,
        result: Register,
        continuation: u32,
    ) -> Option<()> {
        // Unit PCs of each instruction of the function, and past its last.
        let root = frame.is_none();
        if root && plan.tails.iter().any(Option::is_some) {
            self.unit.instructions.push(super::super::meaning_compiled::jump_record(SELF_TAIL_HEADER));
            self.unit.origins.push(Origin::Caller(0));
        }
        let mut flat = Vec::with_capacity(plan.code.len() + 1);
        let mut at = self.unit.instructions.len() as u32;
        for length in plan.lengths(root) {
            flat.push(at);
            at += length;
        }
        flat.push(at);
        let target_of = |target: u32| flat.get(target as usize).copied();
        let register = |register: Register| base + register;
        for (pc, (instruction, calls)) in plan.code.iter().copied().zip(&plan.calls).enumerate() {
            let origin = match frame {
                None => Origin::Caller(pc as u32),
                Some(frame) => Origin::Inlined { frame, pc: pc as u32 },
            };
            if !calls.is_empty() {
                // A returning call's callee completes what makes the call: an inlined callee
                // completes the call that inlined it, so its result and continuation pass
                // through; the caller uses its original final Return so SourcePc recovery
                // can never execute an already-completed Call again.
                let returns = instruction.returns_from_frame();
                let (call_result, call_continuation) = match (returns, root) {
                    (true, false) => (result, continuation),
                    (true, true) => {
                        let terminal = terminal_return(&plan.code)?;
                        (register(plan.code[terminal].register_a()), flat[terminal])
                    }
                    (false, _) => (register(instruction.result_register()), flat[pc + 1]),
                };
                let keeps = plan.keeps_call(pc, root);
                for (alternative, call) in calls.iter().enumerate() {
                    let index = self.unit.frames.len();
                    let marker = self.unit.instructions.len();
                    self.unit.markers.push(marker);
                    let callee_base = Register::try_from(call.base).ok()?;
                    let next = (alternative + 1 < calls.len() + usize::from(keeps)).then(|| marker + 1 + call.plan.length(false) as usize);
                    let frame_record = InlinedCall {
                        recovery: call.recovery,
                        parent: frame,
                        call: pc as u32,
                        target: call.target,
                        callee_register: register(instruction.register_b()),
                        receiver: register(instruction.register_c()),
                        window: register(ImmediateLayout::call_window_base(instruction.imm())),
                        arguments: ImmediateLayout::argument_count(instruction.imm()), result: (!returns).then_some(call_result),
                        base: callee_base,
                        registers: call.registers,
                        alternative: next,
                    };
                    self.unit.frames.push(frame_record);
                    // The marker: the call's guards and bindings (`Lowering::inline_call`); a
                    // callee other than this frame's goes to the next alternative, and past the
                    // last leaves at the call itself, before any of its effects.
                    self.unit.instructions.push(super::super::meaning_compiled::nop_record());
                    self.unit.origins.push(origin);
                    self.emit(&call.plan, Some(index), callee_base, frame_record.receiver, call_result, call_continuation)?;
                }
                self.unit.instructions.extend(keeps.then_some(instruction));
                self.unit.origins.extend(keeps.then_some(origin));
                continue;
            }
            if root {
                if let Some(tail) = plan.tail(pc) {
                    self.unit.instructions.extend_from_slice(tail);
                    self.unit.origins.extend(std::iter::repeat(origin).take(tail.len()));
                    continue;
                }
                // A caller instruction keeps its registers; a jump goes to its target's unit PC.
                let retargeted = if instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::JumpTarget {
                    WideInstruction::new(instruction.op(), instruction.a(), instruction.b(), instruction.c(), target_of(instruction.imm())?)
                } else {
                    instruction
                };
                self.unit.instructions.push(retargeted);
                self.unit.origins.push(origin);
                continue;
            }
            self.emit_inlined(instruction, origin, &register, receiver, result, continuation, &target_of)?;
        }
        Some(())
    }

    /// An inlined callee's instruction as its caller's unit holds it: registers renamed to the
    /// callee's `base`, its `this` read from `receiver`, and its completions moved to `result`
    /// and jumped to `continuation`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_inlined(&mut self, instruction: WideInstruction, origin: Origin, register: &dyn Fn(Register) -> Register,
        receiver: Register, result: Register, continuation: u32, target_of: &dyn Fn(u32) -> Option<u32>) -> Option<()> {
        if inlined_returning(instruction) {
            return self.emit_returning(instruction, origin, register, receiver, result, continuation, target_of);
        }
        let rewritten = match () {
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Return { .. })) => {
                self.unit.instructions.push(super::super::meaning_compiled::move_record(result, register(instruction.register_a())));
                self.unit.origins.push(origin);
                super::super::meaning_compiled::jump_record(continuation)
            }
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::LoadThis { .. })) => super::super::meaning_compiled::move_record(register(instruction.result_register()), receiver),
            // Exit-only operations (`inlinable`): their registers, renamed for the analyses;
            // the lowering leaves at each, and recovery runs the callee's own instruction.
            _ if Operation::<u64, ()>::is_throw(instruction)
                || matches!(Operation::<u64, ()>::decode_load_name(instruction, ()), Some(Operation::LoadName { receiver: None, .. })) =>
                WideInstruction::new(instruction.op(), register(instruction.a()), instruction.b(), instruction.c(), instruction.imm()),
            _ if super::super::meaning_compiled::is_construct(instruction) => super::super::meaning_compiled::construct_record(register(instruction.a()), register(instruction.b()),
                register(instruction.c()), instruction.imm()),
            _ if site_field_read(instruction) => {
                let flags = instruction.result_flags();
                super::super::meaning_compiled::get_field_record(flags | register(instruction.result_register()), instruction.b(), instruction.c(), instruction.imm())
            }
            _ => renamed(instruction, &register, receiver, &target_of)?,
        };
        self.unit.instructions.push(rewritten);
        self.unit.origins.push(origin);
        Some(())
    }

    /// A callee's instruction that computes the returned value, then moves it to `result` and
    /// jumps to `continuation` (`inlined_returning`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_returning(&mut self, instruction: WideInstruction, origin: Origin, register: &dyn Fn(Register) -> Register,
        receiver: Register, result: Register, continuation: u32, target_of: &dyn Fn(u32) -> Option<u32>) -> Option<()> {
        let computed = if site_field_read(instruction) {
            // A nested lookup keeps its site, whose base the lowering renames (`site_field_read`).
            WideInstruction::new(instruction.op(), register(instruction.result_register()), instruction.b(), instruction.c(), instruction.imm())
        } else {
            let plain = WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm());
            renamed(plain, &register, receiver, &target_of)?
        };
        self.unit.instructions.push(computed);
        self.unit.instructions.push(super::super::meaning_compiled::move_record(result, register(instruction.result_register())));
        self.unit.instructions.push(super::super::meaning_compiled::jump_record(continuation));
        self.unit.origins.extend([origin; 3]);
        Some(())
    }
}

/// `function` (`function_id`), each call of which its call plan decides (`plans`, by function and PC): a site inlines the
/// callees `targets` names where its plan's arms all inline, spliced in transitively under the growth cap
/// (`tier_policy::INLINE_GROWTH_PERCENT` of the root's own size, spent in program order). A unit with no inlined call
/// is the function itself.
pub(crate) fn unit<'f>(
    program: &quench_runtime::ResidualProgram,
    graph: Option<&CallGraph>,
    function_id: u32,
    function: &Function,
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
    plans: &dyn Fn(u32, u32) -> Option<CallPlan>,
    body: &dyn Fn(u32) -> Option<&'f Function>,
) -> Option<Unit> {
    let caller_registers = function.registers;
    let root = decoded(function);
    let budget = root.len() * INLINE_GROWTH_PERCENT / PERCENT as usize;
    let mut planner = Planner { program, graph, targets, plans, body, root: function_id, registers: u32::from(caller_registers), budget };
    let mut plan = planner.plan(function_id, root, function.strict, 0);
    // The self tail calls of the root become unit instructions (`self_tail`); their argument temporaries sit above
    // the planned registers, and the undefined value is the program's `undefined` constant, absent from a program that has none.
    let undefined = program.constants.iter().position(|constant| matches!(constant, Constant::Undefined)).and_then(|index| u32::try_from(index).ok());
    let tails: Vec<Option<Vec<WideInstruction>>> = match undefined {
        Some(undefined) => (0..plan.code.len()).map(|pc| plan.calls[pc].is_empty().then(|| self_tail(function_id, function, plans,
            &plan.code, pc, planner.registers, undefined)).flatten()).collect(),
        None => Vec::new(),
    };
    let temporaries = if tails.iter().any(Option::is_some) { u32::from(function.params) } else { 0 };
    plan.tails = tails;
    let registers = u16::try_from(planner.registers + temporaries).ok()?;
    let mut emitter = Emitter {
        unit: Unit {
            instructions: Vec::with_capacity(plan.length(true) as usize),
            origins: Vec::with_capacity(plan.length(true) as usize),
            frames: Vec::new(),
            markers: Vec::new(),
            caller_registers,
            registers,
            end: plan.code.len() as u32,
        },
    };
    emitter.emit(&plan, None, 0, 0, 0, 0)?;
    Some(emitter.unit)
}

#[cfg(test)]
#[path = "lane_unit/tests.rs"]
pub(crate) mod tests;

/// `unit` under the call plans `targets` names: each target one arm of a plan that inlines whole (`Miss::None`), so a
/// fixture names its callees as the one source the planner reads.
#[cfg(test)]
pub(crate) fn unit_planned<'f>(program: &quench_runtime::ResidualProgram, graph: Option<&CallGraph>, function_id: u32, function: &Function,
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>, body: &dyn Fn(u32) -> Option<&'f Function>) -> Option<Unit> {
    let plans = |caller: u32, pc: u32| {
        let arms: Box<[quench_runtime::compile::call_plan::Arm]> = targets(caller, pc).into_iter()
            .map(|target| quench_runtime::compile::call_plan::Arm { class: None, function: target.function }).collect();
        (!arms.is_empty()).then(|| CallPlan { form: quench_runtime::compile::call_plan::CallForm::Inline, arms,
            miss: quench_runtime::compile::call_plan::Miss::None, tail: false, method: None, argc: 0, forwarding: None })
    };
    unit(program, graph, function_id, function, targets, &plans, body)
}

/// The call graph of `program`, which the tests that plan a unit of it give `unit` (`tier_policy::inlines`).
#[cfg(test)]
pub(crate) fn test_graph(program: &quench_runtime::ResidualProgram) -> CallGraph {
    CallGraph::of(program, &quench_runtime::compile::aot_facts::analyze(program))
}

/// Whether an inlined callee's instruction computes its result and returns it in one
/// (`RETURN_REGISTER` alone, of a value-producing instruction), which inlining splits into
/// the computation and the `Return` it makes.
pub(crate) fn inlined_returning(instruction: WideInstruction) -> bool {
    instruction.result_flags() == quench_runtime::bytecode::RETURN_REGISTER
        && !quench_runtime::staged::returns(instruction.op())
        && instruction.op().field_layout(InstructionField::A) == FieldLayout::ResultRegister
        && instruction.defined_register().is_some()
}


/// `instruction` with each register `register` renames, a `this` field base made the
/// `this` register, and a jump target `target` renames; `None` for a field it cannot
/// rename, or a register with flags.
pub(crate) fn renamed(
    instruction: WideInstruction,
    register: &dyn Fn(Register) -> Register,
    this: Register,
    target: &dyn Fn(u32) -> Option<u32>,
) -> Option<WideInstruction> {
    let op = instruction.op();
    let rename = |field: InstructionField, raw: u16| -> Option<u16> {
        Some(match op.field_layout(field) {
            FieldLayout::Unused
            | FieldLayout::BooleanFlag
            | FieldLayout::BinaryOperator
            | FieldLayout::CacheSiteIndex
            | FieldLayout::FieldLookupCacheSiteIndex => raw,
            FieldLayout::ResultRegister
            | FieldLayout::Register
            | FieldLayout::WriteRegister
            | FieldLayout::ReadWriteRegister => {
                (raw <= Operand::PAYLOAD_MASK).then_some(())?;
                register(raw)
            }
            FieldLayout::Operand | FieldLayout::NumericIndexOperand => {
                let operand = Operand(raw);
                match operand.kind()? {
                    OperandKind::Register => Operand::register(register(operand.payload())).0,
                    OperandKind::Constant => raw,
                    OperandKind::Field | OperandKind::Local => return None,
                }
            }
            FieldLayout::FieldBase => {
                let base = FieldBase(raw);
                if base == FieldBase::THIS {
                    FieldBase::register(this).0
                } else {
                    FieldBase::register(register(base.register_index()?)).0
                }
            }
            _ => return None,
        })
    };
    let a = rename(InstructionField::A, instruction.a())?;
    let b = rename(InstructionField::B, instruction.b())?;
    let c = rename(InstructionField::C, instruction.c())?;
    let imm = match op.immediate_role() {
        ImmediateRole::JumpTarget => target(instruction.imm())?,
        // A call's argument window renames as one block: each of its registers to the one
        // after the base's rename.
        _ if quench_runtime::staged::calls(op) => {
            let imm = instruction.imm();
            let base = ImmediateLayout::call_window_base(imm);
            let count = ImmediateLayout::argument_count(imm);
            let renamed_base = register(base);
            (0..count).all(|offset| register(base + offset) == renamed_base + offset).then_some(())?;
            ImmediateLayout::call_immediate(renamed_base, count, ImmediateLayout::direct_eval(imm), ImmediateLayout::parameter_eval(imm))
        }
        _ => instruction.imm(),
    };
    Some(WideInstruction::new(op, a, b, c, imm))
}

/// The function's full decoded code as a plain lane unit, whose PCs are the function's.
pub(crate) fn whole_function(function: &Function) -> Unit {
    let instructions: Vec<_> = function.instructions().collect();
    let count = instructions.len();
    Unit {
        instructions,
        origins: (0..count).map(|pc| Origin::Caller(pc as u32)).collect(),
        frames: Vec::new(), markers: Vec::new(),
        caller_registers: function.registers, registers: function.registers,
        end: count as u32,
    }
}
