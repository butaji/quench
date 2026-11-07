//! Array literals a lane body makes in one allocation, as the lane does.

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
}
