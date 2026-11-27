//! Loop-invariant receivers: the exact shape every field access of a loop expects of a register
//! the loop never writes, proved once on the loop's entering edge and held in each of its blocks.
//!
//! The proof on the edge is a guard, which leaves at the loop's header, before it runs. Its blocks
//! hold the shape as the shape a call kept (`kept_shapes`): the code depends on the shape's
//! departures (`Vm::shape_is_stable`), so a block reads the invalidation word once at its first
//! use of the register and tests no shape (a loop that only reads has even that read hoisted to
//! its entering edge, `hoist_stable`). A register whose shape is not stable, that its accesses
//! expect no one shape of, or that the loop writes keeps the guard of each block.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// A register whose object a loop's entering edge proved of an exact shape, for the blocks
/// `header..=end`.
#[derive(Clone, Copy)]
pub(crate) struct LoopShape {
    header: usize,
    end: usize,
    register: Register,
    /// The register's resolved word at the entering edge, which holds the object across the loop (the register is not written).
    word: ir::Value,
    shape: u32,
}

/// The shape each register's accesses (`accesses`: the register and the shape its access expects,
/// `None` for one that expects no single own slot) all expect, for the registers `written` leaves.
fn agreed_shapes(accesses: impl Iterator<Item = (Register, Option<u32>)>, written: impl Iterator<Item = Register>) -> Vec<(Register, u32)> {
    let mut expected: BTreeMap<Register, Option<u32>> = BTreeMap::new();
    for (register, shape) in accesses {
        let held = expected.entry(register).or_insert(shape);
        if *held != shape { *held = None; }
    }
    for register in written { expected.remove(&register); }
    expected.into_iter().filter_map(|(register, shape)| Some((register, shape?))).collect()
}

impl Lowering<'_, '_> {
    /// The exact shape the loop-invariant register of each access in `header..=end` expects, for
    /// the registers every access agrees on and the loop never writes.
    fn invariant_receiver_shapes(&self, header: usize, end: usize) -> Vec<(Register, u32)> {
        let instructions = &self.unit.instructions[header..=end];
        let accesses = (header..=end).zip(instructions).filter_map(|(pc, instruction)|
            self.field_owner(pc, *instruction).filter(|owner| *owner < usize::from(self.unit.registers))
                .map(|owner| (owner as Register, self.access_shape(*instruction))));
        agreed_shapes(accesses, instructions.iter().flat_map(|instruction| writes(*instruction)))
    }

    /// The exact shape of the own slot `instruction`'s field access reads or stores, where it has
    /// one native read or store of a direct object's own slot.
    fn access_shape(&self, instruction: WideInstruction) -> Option<u32> {
        // A get names its site in its field lookup, a store in its cache site operand.
        let site = match instruction.op() {
            _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) => match instruction.field_lookup() {
                Some(FieldLookup::Atom { cache_site, .. }) => cache_site,
                _ => return None,
            },
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) => instruction.cache_site_index(),
            _ => return None,
        };
        let [load] = site_loads(self.fields, site)?.as_slice() else { return None };
        (load.holder.is_none() && load.valid() && !load.function_shape && !load.guessed()).then_some(load.shape)
    }

    /// On the entering edge of the loop at `header`, the proof that each invariant register
    /// its accesses agree on holds an object of the shape they expect, where the VM holds the
    /// shape stable. A register of another shape leaves at the header, before the loop runs.
    pub(crate) fn hoist_receivers(&mut self, state: &mut State, header: usize) {
        let Some(online) = self.vm.online.filter(|_| self.entry.is_some() && self.division().folds(BindingTime::Site)) else { return };
        let Some(end) = self.loop_end(header).filter(|end| self.enters_only_at_header(header, *end)) else { return };
        for (register, shape) in self.invariant_receiver_shapes(header, end) {
            if !(online.shape_stable)(shape) { continue; }
            let exit = self.guard_exit_for(state, header as u32, ExitFact::Shape);
            let proof = self.object_header(state, Some(register), false, exit);
            let word = self.register_word(state, register);
            headers::prove_header_shape(&mut self.builder, state, word, proof, shape, exit);
            state.values[usize::from(register)] = RegisterFact { tagged: state.values[usize::from(register)].tagged,
                ..RegisterFact::definition(Type::direct_object(shape), Representation::Tagged) };
            state.proofs.kept_shapes.insert(word, shape);
            let held = LoopShape { header, end, register, word, shape };
            if !self.loop_shapes.iter().any(|known| known.header == header && known.register == register) { self.loop_shapes.push(held); }
        }
    }

    /// At the block leader `pc`, the shapes the loops around it proved on their entering edges
    /// (`hoist_receivers`), kept for the registers' next uses (`kept_shapes`).
    pub(crate) fn seed_loop_shapes(&self, state: &mut State, pc: usize) {
        for held in self.loop_shapes.iter().filter(|held| (held.header..=held.end).contains(&pc)) {
            let fact = state.values[usize::from(held.register)];
            if fact.value.ty.object_shape().is_none() && fact.value.representation == Representation::Tagged && fact.tagged.current() {
                state.proofs.kept_shapes.insert(held.word, held.shape);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_register_keeps_the_one_shape_all_its_accesses_expect_and_the_loop_never_writes() {
        let accesses = [(1, Some(7)), (2, Some(7)), (1, Some(7)), (2, Some(8)), (3, None), (4, Some(9)), (5, Some(2)), (5, None)];
        assert_eq!(agreed_shapes(accesses.into_iter(), [4].into_iter()), [(1, 7)],
            "2 expects two shapes, 3 and 5 one with no own slot, 4 is written");
    }
}
