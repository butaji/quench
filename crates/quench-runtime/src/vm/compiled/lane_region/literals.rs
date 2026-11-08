//! Heap values a lane body makes or names: array literals in one allocation, as the lane
//! makes them, string and BigInt constants, and the operators that take them.

use super::*;

impl Lowering<'_, '_> {
    /// The values of an array literal whose definitions follow its `MakeArray`, one per index
    /// in order with no edge between them, as the lane fuses them
    /// (`dispatch_fast::specialize_array_literals`): written to a stack slot, and the
    /// definitions consumed. Zero, for an array of holes, otherwise.
    pub(super) fn literal_elements(&mut self, state: &mut State, pc: usize, make: WideInstruction) -> ir::Value {
        let (array, length) = (make.result_register(), make.array_length());
        let defines = (1..=length).all(|offset| self.unit.instructions.get(pc + offset).is_some_and(|define|
            define.op() == Op::DefineArrayElement && define.register_b() == array && define.register_a() != array
                && define.imm() as usize == offset - 1 && self.blocks[pc + offset].is_none()));
        if length == 0 || !defines { return self.ins().iconst(types::I64, 0); }
        let word = types::I64.bytes();
        let slot = self.builder.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,
            word * length as u32, word.trailing_zeros() as u8));
        for offset in 1..=length {
            let element = self.unit.instructions[pc + offset].register_a();
            let value = self.tagged(state, element);
            self.builder.ins().stack_store(types::I64, value, slot, (word as usize * (offset - 1)) as i32);
            self.consumed.insert(pc + offset);
        }
        self.native += length;
        self.ins().stack_addr(types::I64, slot, 0)
    }

    /// A constant's value and type: an immediate, or in online code the string or BigInt the
    /// VM made of it (`OnlineFacts::constants`).
    pub(super) fn constant_value(&self, index: usize) -> Option<(Value, Type)> {
        let constant = self.program.constants.get(index)?;
        let value = super::super::facts::immediate(constant).or_else(|| self.vm.online.as_ref()?.constants.get(index).copied())?;
        Some((value, super::super::facts::constant_type(constant)))
    }

    /// The result type of a binary operator a string or BigInt operand takes
    /// (`facts::heap_primitive_binary`), by its operands' facts here.
    pub(super) fn heap_primitive_binary(&self, state: &State, instruction: WideInstruction) -> Option<Type> {
        let operand = |operand| super::super::facts::operand_type(&self.program.constants, operand, |register|
            state.values.get(usize::from(register)).map_or(Type::ANY, |fact| fact.value.ty));
        super::super::facts::heap_primitive_binary(instruction.binary_operator(), operand(instruction.operand_b()), operand(instruction.operand_c()))
            .filter(|_| super::super::facts::generic_operator(instruction))
    }

    /// A binary operator a string or BigInt operand takes, by the interpreter's definition
    /// (`compiled::lane_binary`); an object operand, a throw or a due collection leaves first.
    pub(super) fn binary_by_definition(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, ty: Type) -> Option<bool> {
        let left = self.operand_tagged(state, instruction.operand_b())?;
        let right = self.operand_tagged(state, instruction.operand_c())?;
        let exit = self.guard_exit(state, pc as u32);
        let (payload, operator) = (self.payload, self.ins().iconst(types::I64, i64::from(instruction.binary_operator())));
        let value = self.helper_call(LaneHelper::Binary, &[payload, operator, left, right]);
        let made = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, made, exit);
        self.allocated(state);
        self.define(state, instruction.result_register(), value, RegisterFact::definition(ty, Representation::Tagged));
        Some(true)
    }

    /// A numeric binary operator: by the interpreter's definition where a string or BigInt
    /// operand takes it, natively on Numbers otherwise (`Lowering::binary`).
    pub(super) fn operator(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        match self.heap_primitive_binary(state, instruction).filter(|_| !instruction.returns_from_frame()) {
            Some(ty) => self.binary_by_definition(state, pc, instruction, ty),
            None => self.binary(state, pc, instruction),
        }
    }

    /// Whether `register` holds `ty` at an edge to `target`: proved, or guarded there as a
    /// string or BigInt (`guard_heap_primitive`).
    pub(super) fn proves(&mut self, state: &mut State, register: Register, ty: Type, target: usize) -> bool {
        if state.values[usize::from(register)].value.ty.is_subtype(ty) { return true; }
        let exit = self.guard_exit(state, target as u32);
        self.guard_heap_primitive(state, register, ty, exit)
    }

    /// An element store (`Lowering::set_index`); one keyed by a string names a property, which
    /// the element store (`compiled::lane_index_set`) never makes: it leaves for the interpreter.
    pub(super) fn element_store(&mut self, state: &mut State, pc: u32, instruction: WideInstruction) -> bool {
        let key = state.values[usize::from(instruction.register_c())].value.ty;
        if key == Type::NEVER || !key.is_subtype(Type::language(crate::ValueType::String)) { return self.set_index(state, pc, instruction); }
        self.exit(state, pc);
        false
    }

    /// Guards a string or BigInt fact an edge does not prove (`Lowering::conform`) by the VM's
    /// own `typeof` test (`compiled::lane_typeof_is`); `false` for any other fact.
    pub(super) fn guard_heap_primitive(&mut self, state: &mut State, register: Register, ty: Type, exit: ir::Block) -> bool {
        let Some(name) = [(crate::ValueType::String, "string"), (crate::ValueType::BigInt, "bigint")].into_iter()
            .find(|(kind, _)| ty != Type::NEVER && ty.is_subtype(Type::language(*kind))).map(|(_, name)| name) else { return false };
        let bits = self.tagged(state, register);
        let index = self.ins().iconst(types::I64, crate::bytecode::typeof_name_index(name).expect("a typeof name") as i64);
        let named = self.helper_call(LaneHelper::TypeofIs, &[bits, index]);
        let holds = self.ins().icmp_imm_u(IntCC::NotEqual, named, 0);
        emit_js_guard(&mut self.builder, holds, exit);
        self.define(state, register, bits, RegisterFact::definition(ty, Representation::Tagged));
        true
    }
}

/// Whether the loop at `header` runs an instruction `op` selects: from it to the last
/// backward jump to it.
pub(super) fn loop_runs(unit: &super::super::lane_unit::Unit, header: usize, op: impl Fn(Op) -> bool) -> bool {
    let jump = |instruction: &WideInstruction| instruction.op().control_flow_layout() == crate::bytecode::ControlFlowLayout::Jump;
    let back = unit.instructions.iter().rposition(|instruction| jump(instruction) && instruction.jump_target() as usize == header);
    unit.instructions[header..=back.unwrap_or(header)].iter().any(|instruction| op(instruction.op()))
}

/// The binary operators a string or BigInt takes (`facts::heap_primitive_binary`) by the facts
/// from `entry`, which ask no Number of the function's parameters.
pub(super) fn heap_primitive_pcs(entry: usize, initial: &[Type], instructions: &[WideInstruction], leaders: &CompoundBitSet,
    constants: &[crate::bytecode::Constant], native_calls: bool) -> CompoundBitSet {
    let facts = super::super::facts::analyze(entry, initial, instructions, leaders, constants, &writes, &|_| None, native_calls, &[]);
    let mut pcs = CompoundBitSet::with_capacity(instructions.len());
    for (pc, instruction) in instructions.iter().enumerate().filter(|(pc, instruction)| super::super::facts::generic_operator(**instruction) && facts.reached(*pc)) {
        let operand = |operand| super::super::facts::operand_type(constants, operand, |register|
            facts.before(pc, register, instructions, leaders, constants, &writes, native_calls));
        if super::super::facts::heap_primitive_binary(instruction.binary_operator(), operand(instruction.operand_b()), operand(instruction.operand_c())).is_some() {
            pcs.insert(pc);
        }
    }
    pcs
}
