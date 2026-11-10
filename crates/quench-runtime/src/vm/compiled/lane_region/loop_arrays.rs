//! The arrays a region's loops index in place (`LoopArray`), whose views each loop's
//! entering edge proves once (`Lowering::hoist_arrays`).

use super::*;

/// An array base a local loop indexes and never redefines: its loop's entry proves the
/// array's view once, writable where the loop stores through it, and only the element
/// store's general path, which may grow the backing, loads the view again. `aliases` are
/// the loop-local temporaries that hold this array at one use (`resolved_base`), so an
/// access through one indexes the array the loop hoisted.
#[derive(Clone, Copy)]
pub(super) struct LoopArray {
    pub(super) header: usize,
    pub(super) back: usize,
    pub(super) register: Register,
    pub(super) writable: bool,
    pub(super) aliases: [Option<(usize, Register)>; ALIASES],
}

/// The loop-local temporaries one hoisted array records (a use each): a store through one
/// indexes the hoisted array. Past these, a base resolves the long way, which stays correct.
const ALIASES: usize = 4;

impl LoopArray {
    /// Whether `register` at `pc` holds this array by a recorded copy.
    fn aliases(&self, pc: usize, register: Register) -> bool {
        self.aliases
            .iter()
            .flatten()
            .any(|(at, alias)| *at == pc && *alias == register)
    }

    fn push_alias(&mut self, pc: usize, register: Register) {
        if let Some(slot) = self.aliases.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some((pc, register));
        }
    }
}

/// The register whose value `register` holds at `pc` by a copy earlier in its block (`Move`,
/// the copy not written again before `pc`), else `register` itself: an access through a copy
/// of a loop's array indexes the array the loop hoisted.
fn copied_from(
    instructions: &[WideInstruction],
    leader: impl Fn(usize) -> bool,
    pc: usize,
    register: Register,
) -> Register {
    let mut at = pc;
    while at > 0 && !leader(at) {
        at -= 1;
        let instruction = instructions[at];
        if instruction
            .written_registers()
            .any(|written| written == register)
        {
            return if instruction.op().class().eq(Class::Move) {
                instruction.register_b()
            } else {
                register
            };
        }
    }
    register
}

/// The loop's block leaders in order: the indices of `blocks` and of every `index`.
fn loop_blocks(leaders: &CompoundBitSet, header: usize, back: usize) -> Vec<usize> {
    (header..=back).filter(|pc| leaders.contains(*pc)).collect()
}

/// The block of `blocks` that holds `pc`.
fn block_of(blocks: &[usize], pc: usize) -> usize {
    blocks
        .binary_search(&pc)
        .unwrap_or_else(|insert| insert - 1)
}

/// Each loop block's successors, by block index: its branch target and its fallthrough,
/// where they stay in the loop.
fn successors(instructions: &[WideInstruction], blocks: &[usize], back: usize) -> Vec<Vec<usize>> {
    blocks
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let end = blocks.get(index + 1).map_or(back, |next| next - 1);
            let layout = instructions[end].op().control_flow_layout();
            let mut next = Vec::new();
            if let Some(target) = layout
                .jumps()
                .then(|| usize::try_from(instructions[end].jump_target()).ok())
                .flatten()
            {
                next.extend(
                    blocks
                        .binary_search(&target)
                        .ok()
                        .filter(|_| target <= back),
                );
            }
            if layout.falls_through() && end < back {
                next.extend(blocks.binary_search(&(end + 1)).ok());
            }
            next
        })
        .collect()
}

/// Each block's predecessors, by block index.
fn predecessors(successors: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut predecessors = vec![Vec::new(); successors.len()];
    for (from, next) in successors.iter().enumerate() {
        for &to in next {
            predecessors[to].push(from);
        }
    }
    predecessors
}

/// The intersection of two sorted block lists.
fn intersect(left: &[usize], right: &[usize]) -> Vec<usize> {
    left.iter()
        .copied()
        .filter(|block| right.binary_search(block).is_ok())
        .collect()
}

/// Each loop block's dominators within the loop, by block index: its entry is index zero,
/// the header.
fn dominators(successors: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let count = successors.len();
    let predecessors = predecessors(successors);
    let mut dominators: Vec<Vec<usize>> = (0..count)
        .map(|block| {
            if block == 0 {
                vec![0]
            } else {
                (0..count).collect()
            }
        })
        .collect();
    loop {
        let mut changed = false;
        for block in 1..count {
            let mut next: Option<Vec<usize>> = None;
            for &predecessor in &predecessors[block] {
                next = Some(match next {
                    None => dominators[predecessor].clone(),
                    Some(known) => intersect(&known, &dominators[predecessor]),
                });
            }
            let mut next = next.unwrap_or_default();
            if next.binary_search(&block).is_err() {
                next.push(block);
                next.sort_unstable();
            }
            if next != dominators[block] {
                dominators[block] = next;
                changed = true;
            }
        }
        if !changed {
            return dominators;
        }
    }
}

/// Whether the definition at `definition` dominates `pc`.
fn dominates(dominators: &[Vec<usize>], blocks: &[usize], definition: usize, pc: usize) -> bool {
    let (from, target) = (block_of(blocks, definition), block_of(blocks, pc));
    if from == target {
        return definition <= pc;
    }
    dominators[target].binary_search(&from).is_ok()
}

/// The loop-invariant array `register` holds at `pc` when every write of it in the loop
/// reaches `pc` and the last one is a `Move` from a register the loop does not write: the
/// loop-local temporary a store indexes keeps the array its source holds.
fn resolved_base(
    instructions: &[WideInstruction],
    header: usize,
    back: usize,
    pc: usize,
    register: Register,
    written: &BTreeSet<Register>,
    leaders: &CompoundBitSet,
) -> Option<Register> {
    let definitions: Vec<usize> = (header..=back)
        .filter(|at| writes(instructions[*at]).contains(&register))
        .collect();
    let blocks = loop_blocks(leaders, header, back);
    let dominators = dominators(&successors(instructions, &blocks, back));
    if definitions.is_empty()
        || !definitions
            .iter()
            .all(|definition| dominates(&dominators, &blocks, *definition, pc))
    {
        return None;
    }
    let instruction = instructions[*definitions.iter().max()?];
    if !instruction.op().class().eq(Class::Move) {
        return None;
    }
    let source = instruction.register_b();
    (!written.contains(&source)).then_some(source)
}

/// The array `base` names at `pc`: its loop-invariant source register and the loop-local
/// temporary it came from, where the loop redefines `base` by copies of one array.
fn resolved_array(unit: &super::super::lane_unit::Unit, header: usize, back: usize, pc: usize,
    base: Register, written: &BTreeSet<Register>, leaders: &CompoundBitSet) -> Option<(Register, Option<Register>)> {
    if written.contains(&base) {
        resolved_base(&unit.instructions, header, back, pc, base, written, leaders)
            .map(|register| (register, Some(base)))
    } else {
        Some((copied_from(&unit.instructions, |at| leaders.contains(at), pc, base), None))
    }
}

/// Record a loop's use of `register` as a hoisted array: the innermost loop's entry proves
/// the view once, and a store through it turns the loop's view writable.
fn record_array(arrays: &mut Vec<LoopArray>, first: usize, header: usize, back: usize,
    register: Register, writable: bool, alias: Option<(usize, Register)>) {
    match arrays[first..].iter_mut().find(|array| array.register == register) {
        Some(array) => {
            array.writable |= writable;
            if let Some((pc, alias)) = alias { array.push_alias(pc, alias); }
        }
        None => {
            let mut array = LoopArray { header, back, register, writable, aliases: [None; ALIASES] };
            if let Some((pc, alias)) = alias { array.push_alias(pc, alias); }
            arrays.push(array);
        }
    }
}

/// Each loop's array bases, by the register (`copied_from`, `resolved_base`) the loop does
/// not redefine.
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
            // redefine, an inlined frame's argument as well as the caller's own register; a
            // loop-local copy of one does too, at the use the copy reaches.
            let Some(base) = base else { continue };
            let Some((register, alias)) = resolved_array(unit, header, back, pc, base, &written, leaders) else { continue };
            let alias = alias.filter(|alias| *alias != register).map(|alias| (pc, alias));
            record_array(&mut arrays, first, header, back, register, stores, alias);
        }
    }
    arrays
}

impl Lowering<'_, '_> {
    /// The hoisted array `register` names at `pc` (`copied_from`, `resolved_base`), of the
    /// innermost loop around `pc` that proved it, if any.
    pub(super) fn loop_array(&self, pc: usize, register: Option<Register>) -> Option<LoopArray> {
        let register = copied_from(
            &self.unit.instructions,
            |at| self.blocks[at].is_some(),
            pc,
            register?,
        );
        self.loop_arrays
            .iter()
            .copied()
            .filter(|array| {
                (array.register == register || array.aliases(pc, register))
                    && (array.header..=array.back).contains(&pc)
            })
            .min_by_key(|array| array.back - array.header)
    }
}
