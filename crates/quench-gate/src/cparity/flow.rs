//! The loops of a region's machine code, and the hottest of them.
//!
//! A backward jump closes a loop only if its own instruction is reachable from its target:
//! a backward jump inside an exit stub, whose target never flows back to it, is not a loop.
//! The cold code a region lays out after its hot path is therefore not counted as loop body.

use crate::cparity::asm::Instruction;

const WORD_BITS: usize = u64::BITS as usize;

/// A loop as the indices of its target (header) and of the backward jump that closes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loop {
    pub header: usize,
    pub branch: usize,
}

/// A set of instruction indices, one bit each.
struct Visited(Vec<u64>);

impl Visited {
    fn new(len: usize) -> Self {
        Self(vec![0; len.div_ceil(WORD_BITS)])
    }

    /// Adds `index`; whether it was new.
    fn insert(&mut self, index: usize) -> bool {
        let word = &mut self.0[index / WORD_BITS];
        let bit = 1u64 << (index % WORD_BITS);
        let new = *word & bit == 0;
        *word |= bit;
        new
    }
}

fn index_of(code: &[Instruction], address: u64) -> Option<usize> {
    code.binary_search_by_key(&address, |instruction| instruction.address).ok()
}

fn target_index(code: &[Instruction], at: usize) -> Option<usize> {
    code[at].target().and_then(|address| index_of(code, address))
}

fn successors(code: &[Instruction], at: usize) -> impl Iterator<Item = usize> {
    let next = (code[at].falls_through() && at + 1 < code.len()).then_some(at + 1);
    next.into_iter().chain(target_index(code, at))
}

fn reaches(code: &[Instruction], from: usize, to: usize) -> bool {
    let mut visited = Visited::new(code.len());
    visited.insert(from);
    let mut pending = vec![from];
    while let Some(at) = pending.pop() {
        if at == to {
            return true;
        }
        pending.extend(successors(code, at).filter(|next| visited.insert(*next)));
    }
    false
}

/// Every loop of `code`, by the position of the jump that closes it.
pub fn loops(code: &[Instruction]) -> Vec<Loop> {
    (0..code.len())
        .filter_map(|branch| {
            let header = target_index(code, branch)?;
            (header <= branch && reaches(code, header, branch)).then_some(Loop { header, branch })
        })
        .collect()
}

/// The loops that hold no other loop.
fn innermost(loops: &[Loop]) -> impl Iterator<Item = &Loop> {
    loops.iter().filter(|inner| !loops.iter().any(|other| other != *inner && inner.header <= other.header && other.branch <= inner.branch))
}

/// The instructions of an iteration of the hottest loop of `code`: the smallest innermost loop
/// (alignment padding not counted), which is the specialized version a region runs once its
/// entry checks pass, where the larger ones are the general versions it falls back to.
pub fn hottest(code: &[Instruction]) -> Option<&[Instruction]> {
    let all = loops(code);
    innermost(&all)
        .map(|found| &code[found.header..=found.branch])
        .min_by_key(|body| body.iter().filter(|instruction| !instruction.is_nop()).count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cparity::asm::parse;

    #[test]
    fn the_smallest_innermost_loop_is_the_hottest_and_exit_stubs_are_not_loops() {
        let code = parse(
            "  0:\tmov    %rax,%rbx\n  3:\tadd    $0x1,%rax\n  7:\tcmp    %rax,%rcx\n  a:\tjl     0x3\n  c:\tjmp    0x20\n 11:\tmov    $0x9,%edx\n 16:\tjmp    0x20\n 1b:\tjmp    0x11\n 20:\tret\n",
        );
        let found = loops(&code);
        assert_eq!(found, [Loop { header: 1, branch: 3 }]);
        let body = hottest(&code).expect("one loop");
        assert_eq!(body.len(), 3);
    }
}
