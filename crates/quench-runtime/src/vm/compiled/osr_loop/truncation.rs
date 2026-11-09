//! Truncation: arithmetic every read of which takes ToInt32 runs as wrapping `i32`
//! arithmetic, which is exact modulo 2^32 whenever the Number result is an exact integer
//! (LLVM's Float2Int and demanded-bits reasoning, over the region's webs).
use super::{Arithmetic, Definition, LoopRegion, Range, Webs, arithmetic};
use crate::bytecode::WideInstruction;
use crate::vm::operations::NumberArithmetic;
use std::collections::BTreeSet;

/// The largest product magnitude a truncated product takes as its wrapped `i32`: within
/// 2^52 it is an exact Number, and so is its sum with any `i32`.
pub(super) const EXACT_PRODUCT: i64 = 1 << 52;

/// Whether `operator`'s wrapped `i32` result, of the exact `range`, is ToInt32 of its Number
/// result: a sum's always is, and a product's when the product is an exact Number, since
/// reduction modulo 2^32 commutes with exact integer arithmetic.
pub(super) fn wraps_exactly(operator: NumberArithmetic, range: Option<Range>) -> bool {
    operator != NumberArithmetic::Multiply || range.is_some_and(|(low, high)| low.abs().max(high.abs()) <= EXACT_PRODUCT)
}

/// The `candidates` only ToInt32 reads, as the greatest fixpoint: an operand of a sum or
/// difference counts while that result is one.
pub(super) fn truncated_webs(
    region: &LoopRegion,
    webs: &Webs,
    readers: &[Vec<usize>],
    candidates: impl Iterator<Item = Definition>,
) -> BTreeSet<Definition> {
    let sums = |instruction: WideInstruction| {
        instruction.op().is_binary_arithmetic()
            .then(|| arithmetic(instruction.op(), instruction.binary_operator()))
            .flatten()
    };
    let mut truncated: BTreeSet<Definition> = candidates.collect();
    loop {
        let before = truncated.len();
        let kept: BTreeSet<Definition> = truncated
            .iter()
            .copied()
            .filter(|web| {
                readers[*web].iter().all(|offset| {
                    let pc = region.header + *offset as u32;
                    match sums(region.instructions[*offset]) {
                        Some(Arithmetic::Int32(_)) => true,
                        Some(Arithmetic::Float(NumberArithmetic::Add | NumberArithmetic::Subtract)) => {
                            truncated.contains(&webs.defined(region, pc))
                        }
                        _ => false,
                    }
                })
            })
            .collect();
        truncated = kept;
        if truncated.len() == before {
            return truncated;
        }
    }
}
