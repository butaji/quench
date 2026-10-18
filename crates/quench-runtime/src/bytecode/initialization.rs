//! Definite initialization of lexical local slots. `InitializeTdz` opens a slot's temporal
//! dead zone and an initializing store closes it; a slot whose every read is reached only
//! with its dead zone closed, on every path and every handler edge, observes no TDZ and can
//! be read and written as a plain frame local.

use super::{
    ControlEdge, FieldLayout, Function, ImmediateRole, InstructionField, Op, Operand,
    OperandKind, Superinstruction, WideInstruction,
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

/// Dense candidate bitsets, one row per program point.
struct Rows {
    words: usize,
    bits: Vec<u64>,
}

impl Rows {
    fn new(points: usize, words: usize) -> Self {
        Self { words, bits: vec![0; points * words] }
    }

    fn row(&self, point: usize) -> &[u64] {
        &self.bits[point * self.words..(point + 1) * self.words]
    }

    /// Joins `state` into `point`'s row; true when the row grew.
    fn join(&mut self, point: usize, state: &[u64]) -> bool {
        let row = &mut self.bits[point * self.words..(point + 1) * self.words];
        let mut grew = false;
        for (word, incoming) in row.iter_mut().zip(state) {
            let next = *word | incoming;
            grew |= next != *word;
            *word = next;
        }
        grew
    }
}

/// What a candidate slot holds when the function starts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryState {
    /// Frame setup fills every local slot with `undefined`, so only `InitializeTdz` makes a
    /// slot maybe-uninitialized.
    Initialized,
    /// Nothing is written before the code runs: a register that replaces a slot starts with
    /// whatever its pooled frame held.
    Uninitialized,
}

/// Of `candidates`, the slots that no read can reach while maybe-uninitialized: from entry
/// (`EntryState::Uninitialized`) or inside a dead zone opened by `InitializeTdz`.
pub(crate) fn initialized_before_reads(
    function: &Function,
    superinstructions: &[Superinstruction],
    candidates: &[bool],
    entry: EntryState,
) -> Vec<bool> {
    let mut proven = candidates.to_vec();
    let dense: Vec<Option<usize>> = {
        let mut next = 0;
        candidates
            .iter()
            .map(|&candidate| {
                candidate.then(|| {
                    next += 1;
                    next - 1
                })
            })
            .collect()
    };
    let count = dense.iter().flatten().count();
    if count == 0 {
        return proven;
    }
    let words = count.div_ceil(u64::BITS as usize);
    let decoded: Vec<WideInstruction> = function.instructions().collect();
    // One row past the last instruction stands for the function's end.
    let mut maybe_uninitialized = Rows::new(decoded.len() + 1, words);
    if entry == EntryState::Uninitialized {
        let all = vec![u64::MAX; words];
        maybe_uninitialized.join(0, &all);
    }
    let mut state = vec![0; words];
    let mut abrupt = vec![0; words];
    let mut changed = true;
    while changed {
        changed = false;
        for (pc, &instruction) in decoded.iter().enumerate() {
            state.copy_from_slice(maybe_uninitialized.row(pc));
            abrupt.copy_from_slice(&state);
            let mut apply = |slot: usize, access: Access, state: &mut [u64], abrupt: &mut [u64]| {
                let Some(Some(index)) = dense.get(slot).copied() else {
                    return;
                };
                let (word, bit) = (index / u64::BITS as usize, 1 << (index % u64::BITS as usize));
                match access {
                    Access::Uninitialize => state[word] |= bit,
                    Access::Initialize => state[word] &= !bit,
                    Access::Read if state[word] & bit != 0 => proven[slot] = false,
                    Access::Read => {}
                    Access::Opaque => proven[slot] = false,
                }
                abrupt[word] |= state[word];
            };
            match instruction.op().immediate_role() {
                ImmediateRole::SuperinstructionIndex => {
                    for member in superinstructions[instruction.superinstruction_index()].code {
                        if member.is_wide() {
                            // Members are packed without a wide table; an undecodable member
                            // may touch any slot.
                            for slot in 0..candidates.len() {
                                apply(slot, Access::Opaque, &mut state, &mut abrupt);
                            }
                        } else {
                            accesses(member.as_wide(), entry, |slot, access| {
                                apply(slot, access, &mut state, &mut abrupt)
                            });
                        }
                    }
                }
                _ => accesses(instruction, entry, |slot, access| {
                    apply(slot, access, &mut state, &mut abrupt)
                }),
            }
            function.for_each_successor(pc, instruction, |successor, edge| {
                let incoming = match edge {
                    ControlEdge::Normal => &state,
                    ControlEdge::Handler => &abrupt,
                };
                if successor <= decoded.len() {
                    changed |= maybe_uninitialized.join(successor, incoming);
                }
            });
        }
    }
    proven
}

/// The local slots `instruction` touches, in execution order.
fn accesses(instruction: WideInstruction, entry: EntryState, mut access: impl FnMut(usize, Access)) {
    for &field in InstructionField::ALL {
        if matches!(
            instruction.op().field_layout(field),
            FieldLayout::Operand | FieldLayout::NumericIndexOperand
        ) {
            let operand = Operand(instruction.field_value(field));
            if operand.kind() == Some(OperandKind::Local) {
                access(usize::from(operand.payload()), Access::Read);
            }
        }
    }
    if instruction.op().immediate_role() == ImmediateRole::LocalSlot {
        let slot = instruction.local_slot();
        match instruction.op() {
            Op::InitializeTdz => access(slot, Access::Uninitialize),
            Op::LoadLocal | Op::LoadLocalPlain => access(slot, Access::Read),
            // From an uninitialized entry the question is only whether a value was written:
            // a plain slot's assigning store never meets a dead zone (`plain_local_slots`).
            Op::StoreLocalPlain if entry == EntryState::Uninitialized => {
                access(slot, Access::Initialize)
            }
            Op::StoreLocal | Op::StoreLocalPlain => access(
                slot,
                if instruction.boolean_field(InstructionField::C) == Some(true) {
                    Access::Initialize
                } else {
                    Access::Read
                },
            ),
            _ => access(slot, Access::Opaque),
        }
    }
    if let Some(target) = instruction.numeric_local_target() {
        access(usize::from(target), Access::Initialize);
    }
}
