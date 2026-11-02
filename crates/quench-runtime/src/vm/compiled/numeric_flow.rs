//! Forward numeric admission propagates register proofs in contiguous word rows.
//! Bytecode order is topological: every accepted edge points to a later PC.

use super::*;
use smallvec::SmallVec;

const WORD_BITS: usize = u64::BITS as usize;
/// The same small-frame capacity as frame promotion: 128 membership bits inline.
const INLINE_WORDS: usize = 2;

pub(super) struct Bits {
    words: SmallVec<[u64; INLINE_WORDS]>,
    count: usize,
}

impl Bits {
    pub(super) fn new(count: usize) -> Self {
        Self {
            words: std::iter::repeat_n(0, count.div_ceil(WORD_BITS)).collect(),
            count,
        }
    }

    pub(super) fn contains(&self, index: usize) -> bool {
        index < self.count && self.words[index / WORD_BITS] & (1 << (index % WORD_BITS)) != 0
    }

    pub(super) fn insert(&mut self, index: usize) -> Option<()> {
        if index >= self.count {
            return None;
        }
        self.words[index / WORD_BITS] |= 1 << (index % WORD_BITS);
        Some(())
    }
}

/// No predecessor has published an incoming state for this PC yet.
const NO_ROW: usize = usize::MAX;

/// Incoming proofs, with one bit per register and flat PC-to-row indices.
/// Only edges actually lowered publish rows, so an early rejection or unreachable
/// suffix cannot allocate the instruction-count × register-count product.
pub(super) struct Incoming {
    words: Vec<u64>,
    rows: Vec<usize>,
    registers: usize,
}

impl Incoming {
    pub(super) fn new(instructions: usize, entry: &Bits) -> Option<Self> {
        let mut rows = vec![NO_ROW; instructions];
        *rows.first_mut()? = 0;
        Some(Self {
            words: entry.words.to_vec(),
            rows,
            registers: entry.count,
        })
    }

    pub(super) fn read(&self, pc: usize, state: &mut Bits) {
        debug_assert_eq!(state.count, self.registers);
        let width = state.words.len();
        let row = self.rows[pc];
        debug_assert_ne!(row, NO_ROW);
        state.words.copy_from_slice(&self.words[row..row + width]);
    }

    pub(super) fn propagate(&mut self, pc: usize, instruction: WideInstruction, state: &Bits) {
        let width = state.words.len();
        crate::bytecode::for_each_instruction_successor(pc, instruction, |successor| {
            let row = &mut self.rows[successor];
            if *row == NO_ROW {
                *row = self.words.len();
                self.words.extend_from_slice(&state.words);
                return;
            }
            for (incoming, outgoing) in self.words[*row..*row + width].iter_mut().zip(&state.words)
            {
                *incoming &= outgoing;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_respects_register_and_inline_boundaries() {
        for count in [
            0,
            1,
            WORD_BITS,
            WORD_BITS + 1,
            INLINE_WORDS * WORD_BITS,
            INLINE_WORDS * WORD_BITS + 1,
        ] {
            let mut bits = Bits::new(count);
            assert_eq!(bits.words.spilled(), count > INLINE_WORDS * WORD_BITS);
            for index in 0..count {
                assert!(!bits.contains(index));
                bits.insert(index).unwrap();
                assert!(bits.contains(index));
            }
            assert!(!bits.contains(count));
            assert_eq!(bits.insert(count), None);
        }
    }

    #[test]
    fn forward_join_keeps_only_proofs_from_every_path() {
        use crate::bytecode::Op;

        // Exercise an inline frame, a partial word, and a frame that spills.
        for count in [1, WORD_BITS + 1, INLINE_WORDS * WORD_BITS + 1] {
            let mut state = Bits::new(count);
            state.insert(0).unwrap();
            let mut incoming = Incoming::new(4, &state).unwrap();
            incoming.propagate(0, WideInstruction::new(Op::JumpFalse, 0, 0, 0, 2), &state);
            incoming.read(1, &mut state);
            state.insert(count - 1).unwrap();
            incoming.propagate(1, WideInstruction::new(Op::Jump, 0, 0, 0, 3), &state);
            incoming.read(2, &mut state);
            assert_eq!(state.contains(count - 1), count == 1);
            incoming.propagate(2, WideInstruction::new(Op::Jump, 0, 0, 0, 3), &state);
            incoming.read(3, &mut state);
            assert!(state.contains(0));
            for index in 1..count {
                assert!(!state.contains(index));
            }
        }
    }

    #[test]
    fn sparse_or_rejected_functions_allocate_only_published_rows() {
        let state = Bits::new(usize::from(u16::MAX));
        let incoming = Incoming::new(usize::from(u16::MAX), &state).unwrap();
        assert_eq!(incoming.words.len(), state.words.len());
        assert!(incoming.rows[1..].iter().all(|&row| row == NO_ROW));
    }

    #[test]
    fn duplicate_edges_and_empty_register_files_are_idempotent() {
        use crate::bytecode::Op;
        for count in [0, WORD_BITS + 1] {
            let mut state = Bits::new(count);
            let mut incoming = Incoming::new(2, &state).unwrap();
            incoming.propagate(0, WideInstruction::new(Op::JumpFalse, 0, 0, 0, 1), &state);
            assert_eq!(incoming.words.len(), 2 * state.words.len());
            incoming.read(1, &mut state);
            assert!(state.words.iter().all(|&word| word == 0));
        }
    }
}
