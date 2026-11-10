//! The arrays a region's loops index in place (`LoopArray`), whose views each loop's
//! entering edge proves once (`Lowering::hoist_arrays`).

use super::*;

/// An array base a local loop indexes and never redefines: its loop's entry proves the
/// array's view once, writable where the loop stores through it, and only the element
/// store's general path, which may grow the backing, loads the view again.
#[derive(Clone, Copy)]
pub(super) struct LoopArray {
    pub(super) header: usize,
    pub(super) back: usize,
    pub(super) register: Register,
    pub(super) writable: bool,
}

/// The register whose value `register` holds at `pc` by a copy earlier in its block (`Move`,
/// the copy not written again before `pc`), else `register` itself: an access through a copy
/// of a loop's array indexes the array the loop hoisted.
fn copied_from(instructions: &[WideInstruction], leader: impl Fn(usize) -> bool, pc: usize, register: Register) -> Register {
    let mut at = pc;
    while at > 0 && !leader(at) {
        at -= 1;
        let instruction = instructions[at];
        if instruction.written_registers().any(|written| written == register) {
            return if instruction.op().class().eq(Class::Move) { instruction.register_b() } else { register };
        }
    }
    register
}

/// Each loop's array bases, by the register (`copied_from`) the loop does not redefine.
pub(super) fn loop_arrays(unit: &super::super::lane_unit::Unit, loops: &[(usize, usize)], leaders: &CompoundBitSet) -> Vec<LoopArray> {
    let mut arrays = Vec::new();
    for &(header, back) in loops {
        let body = &unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|candidate| writes(*candidate)).collect();
        let first = arrays.len();
        for (pc, instruction) in (header..).zip(body) {
            let (base, stores) = match instruction.op().class() {
                Class::GetIndex => (instruction.operand_b().register_index(), false),
                Class::SetIndex => (Some(instruction.register_b()), true),
                _ => continue,
            };
            // The loop's sole entering edge proves the view of any base its body does not
            // redefine, an inlined frame's argument as well as the caller's own register.
            let Some(register) = base.map(|base| copied_from(&unit.instructions, |at| leaders.contains(at), pc, base))
                .filter(|register| !written.contains(register)) else { continue };
            match arrays[first..].iter_mut().find(|array: &&mut LoopArray| array.register == register) {
                Some(array) => array.writable |= stores,
                None => arrays.push(LoopArray { header, back, register, writable: stores }),
            }
        }
    }
    arrays
}

impl Lowering<'_, '_> {
    /// The hoisted array `register` names at `pc` (`copied_from`), of the innermost loop
    /// around `pc` that proved it, if any.
    pub(super) fn loop_array(&self, pc: usize, register: Option<Register>) -> Option<LoopArray> {
        let register = copied_from(&self.unit.instructions, |at| self.blocks[at].is_some(), pc, register?);
        self.loop_arrays.iter().copied()
            .filter(|array| array.register == register && (array.header..=array.back).contains(&pc))
            .min_by_key(|array| array.back - array.header)
    }
}
