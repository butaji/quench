//! Webs of a loop region: the definition webs of its registers and the reads and successors they follow.
use super::*;

/// A definition of a tracked register: its value at entry, by its index in
/// `LoopRegion::entry_registers`, or the instruction at a PC offset from the header.
pub(super) type Definition = usize;

/// The region's values as webs: the definitions that reach a common read (an operand, or a
/// register written back where native code leaves) form one web, which native code holds in
/// one SSA variable with one representation. A register reused for unrelated values (an
/// index, then an element) gets a web for each, so one does not widen the other. Definitions
/// are bits of one state (`quench_runtime::webs`): the entry values first, then one per instruction.
pub(super) struct Webs {
    /// The definitions reaching each instruction, by PC offset (`Reaching::reached`: those
    /// reachable from the header before an exact-PC VM exit).
    pub(super) flow: quench_runtime::webs::Reaching,
    /// Per register, the definitions that write it, as a state's bits.
    pub(super) written: std::collections::BTreeMap<Register, Vec<u64>>,
    /// Union-find over definitions.
    pub(super) sets: quench_runtime::webs::DefSets,
    /// Per leave PC and live register, a definition of the web written back there.
    pub(super) leaves: std::collections::BTreeMap<(u32, Register), Definition>,
    /// Whether the value a register holds at entry is read (its web reaches a read).
    pub(super) entry_read: Vec<bool>,
}

impl Webs {
    pub(super) fn find(&self, definition: Definition) -> Definition {
        self.sets.find(definition)
    }

    pub(super) fn union(&mut self, left: Definition, right: Definition) {
        self.sets.union(left, right);
    }

    /// The definitions there are.
    pub(super) fn definitions(&self) -> usize {
        self.sets.len()
    }

    /// Whether control from the header reaches the instruction at `offset`.
    pub(super) fn reachable(&self, offset: usize) -> bool {
        self.flow.reached[offset]
    }

    /// The definitions of `register` reaching the instruction at `offset`, ascending.
    pub(super) fn reaching(&self, offset: usize, register: Register) -> Vec<Definition> {
        let Some(written) = self.written.get(&register) else { return Vec::new() };
        let row = self.flow.row(offset);
        quench_runtime::webs::members(written).filter(|definition| quench_runtime::webs::has(row, *definition)).collect()
    }

    /// The web `register` holds before the instruction at `pc`; a PC no path from the header
    /// reaches reads the entry value.
    pub(super) fn read(&self, region: &LoopRegion, pc: u32, register: Register) -> Definition {
        let entry = || region.entry_registers().position(|number| number == register).unwrap_or(0);
        let definition = self.reaching((pc - region.header) as usize, register).first().copied().unwrap_or_else(entry);
        self.find(definition)
    }


    /// The register a web's definitions write: every definition reaching a common read
    /// writes the register read.
    pub(super) fn register(&self, region: &LoopRegion, web: Definition) -> Register {
        match region.entry_register(web) {
            Some(register) => register,
            None => number_definition(region.instructions[web - region.entry_count()])
                .expect("a web's definitions write Number registers"),
        }
    }

    /// The web of the instruction at `pc`'s result.
    pub(super) fn defined(&self, region: &LoopRegion, pc: u32) -> Definition {
        self.find(region.entry_count() + (pc - region.header) as usize)
    }
}

/// The Number registers an instruction reads.
pub(crate) fn number_reads(instruction: WideInstruction) -> Vec<Register> {
    if exits_before_effects(instruction) || reads_type(instruction) || !instruction.op().reads_values() {
        return Vec::new();
    }
    instruction.value_registers().collect()
}

/// SSA operands and their semantic requirements. Entry-pinned heap views have
/// their own guard plan; other receivers read a guest register here.
pub(super) fn value_reads(region: &LoopRegion, offset: usize) -> Vec<(Register, Type)> {
    let instruction = region.instructions[offset];
    let pc = region.header + offset as u32;
    let strict_number_test = quench_runtime::staged::compare_jump(instruction.op())
        && quench_runtime::vm::operations::equality_operator(instruction.binary_operator_field()).is_some_and(|(strict, _)| strict)
        && [instruction.operand_b(), instruction.operand_c()].iter().any(|operand| operand.kind() == Some(OperandKind::Constant));
    let expected = if is_move_row(instruction) || is_truthiness_row(instruction)
        || region.identity_branches.contains_key(&pc) || strict_number_test {
        Type::ANY
    } else {
        Type::NUMBER
    };
    let mut reads: Vec<_> = number_reads(instruction).into_iter().map(|register| (register, expected)).collect();
    if reads_type(instruction) {
        reads.push((instruction.register_b(), Type::ANY));
    }
    if let Some(register) = object_reads(instruction) {
        let dynamic = match instruction.op() {
            opcode if quench_runtime::staged::reads_named_field(opcode) => region.dynamic_field_reads.contains_key(&(offset as u32)),
            opcode if quench_runtime::staged::reads_element(opcode) => region.dynamic_arrays.contains(&register)
                || !region.arrays.iter().any(|array| array.register == register),
            _ if instruction.field_store_register().is_some() => region.dynamic_field_stores.contains_key(&(offset as u32)),
            _ => region.dynamic_arrays.contains(&register),
        };
        if dynamic { reads.push((register, Type::ANY)); }
    }
    if admitted_store(instruction) {
        let store = region.field_stores.get(&(offset as u32)).map(|index| &region.field_store_sites[*index]);
        let expected = store.map_or(Type::ANY, |store| region.field_store_type(store));
        reads.push((instruction.register_a(), expected));
    }
    reads
}

/// Whether `instruction` is a register move, by its staged row (`Operation::Move`).
pub(super) fn is_move_row(instruction: WideInstruction) -> bool {
    matches!(Operation::<(), ()>::decode(instruction, DecodeContext::EMPTY), Some(Operation::Move { .. }))
}

/// Whether `instruction` jumps on its register's truthiness, by its staged row (`Operation::JumpIf`).
pub(super) fn is_truthiness_row(instruction: WideInstruction) -> bool {
    matches!(Operation::<(), ()>::decode(instruction, DecodeContext::EMPTY), Some(Operation::JumpIf { .. }))
}

/// Whether `instruction` reads a capture, by its staged row (`Operation::LoadCapture`, which the
/// row decodes only where the instruction has no result flags).
pub(super) fn is_load_capture_row(instruction: WideInstruction) -> bool {
    matches!(Operation::<(), ()>::decode(instruction, DecodeContext::EMPTY), Some(Operation::LoadCapture { .. }))
}

/// The value register an instruction defines, numeric or tagged.
pub(super) fn number_definition(instruction: WideInstruction) -> Option<Register> {
    instruction.op().defines_loop_result().then(|| instruction.result_register())
}

/// The instructions this tier lowers to native code (`admit`): those of the lane specializer's
/// (`lane_region::Lowering::instruction`) that a loop's entry facts support.
/// Every other one leaves at its exact PC before its effects and the interpreter runs its own
/// definition, so the tier covers every opcode; analysis and emission keep that boundary.
pub(super) fn lowered_natively(instruction: WideInstruction) -> bool {
    let op = instruction.op();
    admitted_store(instruction) || stores_binding(instruction) || if comparison_jump(instruction) {
        // A conditional jump only on a relation or a bitwise and, so one on `in` or
        // `instanceof` is an exit.
        quench_runtime::vm::operations::NumberRelation::from_operator(instruction.binary_operator_field())
            .is_some() || bitwise_condition(instruction).is_some()
    } else {
        op.lowers_in_loop()
    }
}

/// The registers an allocation of `instructions` defines, which also hold the Numbers a
/// register reuse writes to them.
pub(super) fn boxed(instructions: &[WideInstruction]) -> std::collections::BTreeSet<Register> {
    instructions.iter().filter(|instruction| instruction.op().allocates()).map(|instruction| instruction.result_register()).collect()
}

/// A `typeof` of its operand: the type-name index or a fused test (`bytecode::typeof_test`).
pub(super) fn reads_type(instruction: WideInstruction) -> bool {
    quench_runtime::staged::computes_unary(instruction.op()) && (instruction.unary_operator() == quench_runtime::bytecode::UNARY_TYPEOF_INDEX
        || quench_runtime::bytecode::typeof_test(instruction.unary_operator()).is_some())
}

pub(super) fn exits_before_effects(instruction: WideInstruction) -> bool { !lowered_natively(instruction) }

/// The register a native run defines at an instruction: an exit's definition is the VM's.
pub(super) fn native_definition(at: WideInstruction) -> Option<Register> { at.defined_register().filter(|_| lowered_natively(at)) }

/// Visits the region PCs control continues at from the instruction at `pc`, and the PCs outside
/// it where the lane resumes; as lowered. An instruction that exits before its effects has none.
pub(super) fn for_each_successor(region: &LoopRegion, pc: u32, mut visit: impl FnMut(u32)) {
    let instruction = region.instructions[(pc - region.header) as usize];
    if exits_before_effects(instruction) {
        return;
    }
    quench_runtime::bytecode::for_each_instruction_successor(pc as usize, instruction, |successor| visit(successor as u32));
}

pub(super) fn webs(region: &LoopRegion) -> Webs {
    use std::collections::{BTreeMap, BTreeSet};
    let count = region.instructions.len();
    let entries = region.entry_count();
    let bits = entries + count;
    let words = bits.div_ceil(quench_runtime::bitset::WORD_BITS);
    let inside = |pc: u32| (region.header..=region.backedge).contains(&pc);
    let mut written: BTreeMap<Register, Vec<u64>> = BTreeMap::new();
    let mut seed = vec![0; words];
    for (index, register) in region.entry_registers().enumerate() {
        quench_runtime::webs::set(written.entry(register).or_insert_with(|| vec![0; words]), index);
        quench_runtime::webs::set(&mut seed, index);
    }
    for (offset, instruction) in region.instructions.iter().enumerate() {
        if let Some(register) = number_definition(*instruction) {
            quench_runtime::webs::set(written.entry(register).or_insert_with(|| vec![0; words]), entries + offset);
        }
    }
    // An instruction's definition replaces every other of its register.
    let transfer = |offset: usize, out: &mut [u64]| if let Some(register) = number_definition(region.instructions[offset]) {
        for (word, kill) in out.iter_mut().zip(&written[&register]) { *word &= !kill; }
        quench_runtime::webs::set(out, entries + offset);
    };
    let next = |offset: usize, deliver: &mut dyn FnMut(quench_runtime::webs::Next)| for_each_successor(region, region.header + offset as u32, |pc| {
        deliver(if inside(pc) { quench_runtime::webs::Next::Row((pc - region.header) as usize) } else { quench_runtime::webs::Next::Leave(pc) });
    });
    let flow = quench_runtime::webs::solve(count, bits, &seed, &transfer, &next, false);
    let mut webs = Webs { flow, written, sets: quench_runtime::webs::DefSets::new(bits), leaves: BTreeMap::new(), entry_read: vec![false; entries] };
    let mut reads: Vec<Definition> = Vec::new();
    let unify = |webs: &mut Webs, definitions: Vec<Definition>, reads: &mut Vec<Definition>| -> Option<Definition> {
        let first = webs.sets.join_all(definitions)?;
        reads.push(first);
        Some(first)
    };
    for offset in 0..count {
        if !webs.reachable(offset) {
            continue;
        }
        let pc = region.header + offset as u32;
        let instruction = region.instructions[offset];
        for (register, _) in value_reads(region, offset) {
            let definitions = webs.reaching(offset, register);
            unify(&mut webs, definitions, &mut reads);
        }
        // Native code may leave at any instruction of the region, before its effects, and
        // resumes the lane where a jump leaves the region; each writes back what is live.
        let leave = |webs: &mut Webs, at: u32, after: bool, reads: &mut Vec<Definition>| {
            for register in region.entry_registers().filter(|register| region.is_live(at, *register)) {
                let mut definitions: BTreeSet<Definition> = if after && number_definition(instruction) == Some(register) {
                    BTreeSet::from([entries + offset])
                } else {
                    webs.reaching(offset, register).into_iter().collect()
                };
                if let Some(previous) = webs.leaves.get(&(at, register)) {
                    definitions.insert(*previous);
                }
                if let Some(first) = unify(webs, definitions.into_iter().collect(), reads) {
                    webs.leaves.insert((at, register), first);
                }
            }
        };
        leave(&mut webs, pc, false, &mut reads);
        // A fallthrough exit sees this instruction's completed definition;
        // an exit before its effects sees the incoming web instead.
        for_each_successor(region, pc, |successor| if !inside(successor) {
            leave(&mut webs, successor, true, &mut reads);
        });
    }
    let read_webs: BTreeSet<Definition> = reads.iter().map(|definition| webs.find(*definition)).collect();
    webs.entry_read = (0..entries).map(|entry| read_webs.contains(&webs.find(entry))).collect();
    webs
}
