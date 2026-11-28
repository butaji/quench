//! What proves an element access's index within its array: the solver's length proof (`Lowering::proven_below_length`),
//! or else the loop's entry. An index a loop's entry bounds (`Versioned`, `Ranged`) is compared with the array's length once
//! on the entering edge (`loop_versions`), so the access in the body needs no test.

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt, OperationOperationRecordsExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::run::{Model, Point};
use quench_runtime::bytecode::{Register, WideInstruction};
use quench_runtime::staged::Operation;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// What proves the index of an element access within its array.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IndexProof {
    /// Non-negative and below the array's length where it is accessed.
    Proven,
    /// Non-negative and below `bound`, a register of the access's loop whose value the loop's entry
    /// may compare with the length of the array held in `array` once, instead of every access.
    Versioned { array: Register, bound: Register },
    /// Non-negative and at most `max` wherever it is accessed (a mask, a range of the index), so the
    /// loop's entry may compare `max` with the length of the array held in `array` once.
    Ranged { array: Register, max: i64 },
    /// Tested at the access.
    Check,
}

/// The highest index a range proof versions: a loop whose array is shorter than its ceiling fails
/// the entry's comparison and runs interpreted, so a wider ceiling repays no guard.
const RANGED_CEILING: i64 = 1 << 16;

impl Point {
    /// What proves the index of the element access at `pc` within `0..length` of its array: the access reads or writes
    /// a hoisted view and its index is non-negative, and below the array's length (`Proven`, by `below`: the solve's proof,
    /// `Lowering::proven_below_length`), or below a register the solve names as its bound (`Versioned`, by `bound_of`:
    /// `Lowering::bound_below`), or within a range (`Ranged`).
    pub(crate) fn index_proof(&self, model: &Model<'_>, pc: usize, instruction: WideInstruction,
        below: &dyn Fn(Register, Register) -> bool, bound_of: &dyn Fn(Register) -> Option<Register>) -> IndexProof {
        let (array, index) = match Operation::<u64, ()>::of(instruction) {
            Some(Operation::ElementRead { base, key, .. }) => (Some(base), key.register_index()),
            Some(Operation::ElementWrite { base, key, .. }) => (Some(base), Some(key)),
            _ => return IndexProof::Check,
        };
        let Some((array, index)) = array.zip(index) else { return IndexProof::Check };
        let non_negative = self.int_range(index).is_some_and(|range| range.min >= 0);
        if !(model.dense)(pc) || !non_negative { return IndexProof::Check; }
        if below(index, array) { return IndexProof::Proven; }
        let bound = bound_of(index);
        let ceiling = self.int_range(index).map(|range| range.max).filter(|max| *max < RANGED_CEILING);
        match bound {
            Some(bound) if bound != array => IndexProof::Versioned { array, bound },
            _ => match ceiling {
                Some(max) => IndexProof::Ranged { array, max },
                None => IndexProof::Check,
            },
        }
    }
}
