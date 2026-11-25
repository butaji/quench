//! Definite initialization of lexical local slots. `InitializeTdz` opens a slot's temporal
//! dead zone and an initializing store closes it; a slot whose every read is reached only
//! with its dead zone closed, on every path and every handler edge, observes no TDZ and can
//! be read and written as a plain frame local.

use crate::bitset::BitSet;

use super::{
    ControlEdge, Function, ImmediateRole, InstructionField, Operand,
    OperandKind, Superinstruction, Traits, WideInstruction,
};

/// How one instruction member touches a local slot.
#[derive(Clone, Copy)]
enum Access {
    /// Opens the dead zone (`InitializeTdz`).
    Uninitialize,
    /// An initializing store, or a numeric local target formed only from one.
    Initialize,
    /// A read, or a store that checks the binding is initialized first.
    Read,
    /// A slot access this analysis does not model; the slot keeps its general path.
    Opaque,
}

/// The basic blocks of a function's control flow, built on the first analysis that needs them
/// and shared by every later one over the same flow: a pass that renames operations without
/// moving control (plain local specialization) keeps them.
pub(crate) type FlowCell = std::cell::OnceCell<BasicBlocks>;

/// Of `candidates`, the slots that no read can reach while maybe-uninitialized. Function
/// entry leaves a slot initialized unless `uninitialized_at_entry` names it, as for a
/// register that replaces a slot and starts with whatever its pooled frame held; after that
/// only `InitializeTdz` makes a slot maybe-uninitialized.
/// The blocks in `flow`, when set, are those of `function`'s current control flow.
pub(crate) fn initialized_before_reads(function: &Function, superinstructions: &[Superinstruction],
    candidates: &BitSet, uninitialized_at_entry: &BitSet, flow: &FlowCell) -> BitSet {
    let mut proven = candidates.clone();
    let dense = dense_indices(candidates);
    let count = dense.iter().flatten().count();
    if count == 0 {
        return proven;
    }
    let words = count.div_ceil(u64::BITS as usize);
    let decoded: Vec<WideInstruction> = function.instructions().collect();
    let blocks = flow.get_or_init(|| BasicBlocks::of(function, &decoded));
    // States are kept per basic block, not per instruction: a row per instruction grows with
    // code length times candidates (lucide-react's 1.2 MB bundle: tens of MiB, TOD-140).
    let mut maybe_uninitialized = crate::bitset::BitRows::new(blocks.count, count);
    let mut entry = vec![0; words];
    let entering = dense.iter().enumerate().filter(|(slot, _)| uninitialized_at_entry.contains(*slot));
    for index in entering.filter_map(|(_, index)| *index) {
        entry[index / u64::BITS as usize] |= 1 << (index % u64::BITS as usize);
    }
    maybe_uninitialized.join(0, &entry);
    let mut state = vec![0; words];
    let mut abrupt = vec![0; words];
    let mut changed = true;
    while changed {
        changed = false;
        for (block, &start) in blocks.starts.iter().enumerate() {
            state.copy_from_slice(maybe_uninitialized.row(block));
            let mut pc = start;
            while let Some(&instruction) = decoded.get(pc) {
                if blocks.interruptible(pc) { abrupt.copy_from_slice(&state); }
                let mut apply = |slot: usize, access: Access, state: &mut [u64], abrupt: &mut [u64]| {
                    let Some(Some(index)) = dense.get(slot).copied() else {
                        return;
                    };
                    let (word, bit) = (index / u64::BITS as usize, 1 << (index % u64::BITS as usize));
                    match access {
                        Access::Uninitialize => state[word] |= bit,
                        Access::Initialize => state[word] &= !bit,
                        Access::Read if state[word] & bit != 0 => proven.remove(slot),
                        Access::Read => {}
                        Access::Opaque => proven.remove(slot),
                    }
                    abrupt[word] |= state[word];
                };
                match instruction.op().immediate_role() {
                    ImmediateRole::SuperinstructionIndex => {
                        for member in superinstructions[instruction.superinstruction_index()].wide {
                            accesses(member, uninitialized_at_entry, |slot, access| {
                                apply(slot, access, &mut state, &mut abrupt)
                            });
                        }
                    }
                    _ => accesses(instruction, uninitialized_at_entry, |slot, access| {
                        apply(slot, access, &mut state, &mut abrupt)
                    }),
                }
                let mut falls_through = false;
                blocks.successors.at(pc).for_each(|(successor, edge)| {
                    if blocks.continues(pc, successor, edge) {
                        falls_through = true;
                        return;
                    }
                    let incoming = match edge {
                        ControlEdge::Normal => &state,
                        ControlEdge::Handler => &abrupt,
                    };
                    // A later block takes its gain in this sweep; only a loop asks for another.
                    if let Some(target) = blocks.at(successor) { changed |= maybe_uninitialized.join(target, incoming) & (target <= block); }
                });
                if !falls_through {
                    break;
                }
                pc += 1;
            }
        }
    }
    proven
}

/// Each candidate slot's index among the candidates.
fn dense_indices(candidates: &BitSet) -> Vec<Option<usize>> {
    let mut next = 0;
    (0..candidates.len())
        .map(|slot| {
            candidates.contains(slot).then(|| {
                next += 1;
                next - 1
            })
        })
        .collect()
}

/// The basic blocks of a function's decoded code. A block starts at the entry, at every
/// successor other than an instruction's own sole fall-through, and after every instruction
/// that does not only fall through, so control enters a block only at its start and the
/// states inside it are the forward walk from there.
pub(crate) struct BasicBlocks {
    /// The first pc of each block, ascending; the entry is block 0.
    starts: Vec<usize>,
    /// The block starting at each pc, with one entry past the end for the function's end.
    block_at: Vec<Option<u32>>,
    count: usize,
    /// Every pc's successors, listed once for all the passes over the blocks.
    successors: super::Successors,
}

impl BasicBlocks {
    fn of(function: &Function, decoded: &[WideInstruction]) -> Self {
        let successors = super::Successors::of(function, decoded);
        let end = decoded.len();
        let leader = successors.leaders(end);
        let mut starts = Vec::new();
        let mut block_at = vec![None; end + 1];
        for pc in leader.iter() {
            block_at[pc] = Some(starts.len() as u32);
            starts.push(pc);
        }
        let count = starts.len();
        // The function's end is a join point no instruction follows.
        starts.retain(|&pc| pc < end);
        Self { starts, block_at, count, successors }
    }

    fn at(&self, pc: usize) -> Option<usize> {
        self.block_at.get(pc).copied().flatten().map(|block| block as usize)
    }

    /// Whether a handler can interrupt the instruction at `pc`: only then are the states it
    /// passes through (`abrupt`) read, so only then are they gathered.
    fn interruptible(&self, pc: usize) -> bool {
        self.successors.at(pc).any(|(_, edge)| edge == ControlEdge::Handler)
    }

    /// Whether control from `pc` to `successor` stays inside `pc`'s block.
    fn continues(&self, pc: usize, successor: usize, edge: ControlEdge) -> bool {
        successor == pc + 1 && edge == ControlEdge::Normal && self.at(successor).is_none()
    }
}

/// The local slots `instruction` touches, in execution order.
fn accesses(instruction: WideInstruction, uninitialized_at_entry: &BitSet, mut access: impl FnMut(usize, Access)) {
    for &field in InstructionField::ALL {
        if instruction.op().field_layout(field).holds_operand() {
            let operand = Operand(instruction.field_value(field));
            if operand.kind() == Some(OperandKind::Local) {
                access(usize::from(operand.payload()), Access::Read);
            }
        }
    }
    // A field read fused onto a local (`rewrite::LocalField`) reads that local first.
    if let Some(local) = instruction.local_field_base() {
        access(usize::from(local), Access::Read);
    }
    if instruction.op().immediate_role() == ImmediateRole::LocalSlot {
        let slot = instruction.local_slot();
        let op = instruction.op();
        if op.traits().has(Traits::TDZ_INITIALIZE) {
            access(slot, Access::Uninitialize);
        } else if op.traits().has(Traits::FRAME_LOCAL_LOAD) {
            access(slot, Access::Read);
        // For a slot uninitialized at entry the question is only whether a value was
        // written: a plain slot's assigning store never meets a dead zone
        // (`plain_local_slots`).
        } else if op.traits().has(Traits::PLAIN_LOCAL_STORE) && uninitialized_at_entry.contains(slot) {
            access(slot, Access::Initialize);
        } else if op.traits().has(Traits::FRAME_LOCAL_STORE) {
            access(
                slot,
                if instruction.boolean_field(InstructionField::C) == Some(true) {
                    Access::Initialize
                } else {
                    Access::Read
                },
            );
        } else {
            access(slot, Access::Opaque);
        }
    }
    if let Some(target) = instruction.numeric_local_target() {
        access(usize::from(target), Access::Initialize);
    }
}
