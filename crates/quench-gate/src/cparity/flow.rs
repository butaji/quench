//! The loops of a region's machine code, and the hottest of them.
//!
//! A backward jump closes a loop only if its own instruction is reachable from its target:
//! a backward jump inside an exit stub, whose target never flows back to it, is not a loop.
//! The cold code a region lays out after its hot path is therefore not counted as loop body.

use crate::cparity::asm::Instruction;

const BITS_PER_WORD: usize = u64::BITS as usize;

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
        Self(vec![0; len.div_ceil(BITS_PER_WORD)])
    }

    /// Adds `index`; whether it was new.
    fn insert(&mut self, index: usize) -> bool {
        let word = &mut self.0[index / BITS_PER_WORD];
        let bit = 1u64 << (index % BITS_PER_WORD);
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

/// Every loop of `code`, by the position of the jump that closes it. A branch to its own
/// instruction is a trap or a spin (`b .`), not a loop body: it has no instruction to iterate.
pub fn loops(code: &[Instruction]) -> Vec<Loop> {
    (0..code.len())
        .filter_map(|branch| {
            let header = target_index(code, branch)?;
            (header < branch && reaches(code, header, branch)).then_some(Loop { header, branch })
        })
        .collect()
}

/// The loops that hold no other loop.
fn innermost(loops: &[Loop]) -> impl Iterator<Item = &Loop> {
    loops.iter().filter(|inner| !loops.iter().any(|other| other != *inner && inner.header <= other.header && other.branch <= inner.branch))
}

/// One instruction's address path through a loop: from its header to the back edge along the
/// hot path — the fall-through of every conditional branch (the path its checks expect) and the
/// target of every unconditional jump — so the cold blocks a region keeps between the header and
/// the back edge (each guard's miss stub) are not counted as per-iteration work. Each instruction
/// is counted once; the walk stops at the back edge or where the path leaves the loop.
/// The fewest instructions from `from` to `to` inside a loop: a guard's miss stub and a call's
/// cold path cost more than the check they guard, which is what tells the two apart without
/// weights.
fn distance(code: &[Instruction], found: &Loop, from: usize, to: usize) -> usize {
    let mut visited = Visited::new(code.len());
    let mut pending = std::collections::VecDeque::from([(from, 0usize)]);
    while let Some((at, steps)) = pending.pop_front() {
        if at == to {
            return steps;
        }
        if !visited.insert(at) {
            continue;
        }
        for next in successors(code, at) {
            if (found.header..=found.branch).contains(&next) {
                pending.push_back((next, steps + 1));
            }
        }
    }
    usize::MAX
}

fn iteration(code: &[Instruction], found: &Loop) -> Vec<(usize, Instruction)> {
    let mut visited = Visited::new(code.len());
    let mut path = Vec::new();
    let mut at = found.header;
    while visited.insert(at) {
        path.push((at, code[at].clone()));
        if at == found.branch {
            break;
        }
        let fall_through = code[at].falls_through().then_some(at + 1).filter(|next| *next <= found.branch);
        let target = target_index(code, at).filter(|next| (found.header..=found.branch).contains(next));
        let next = if code[at].is_conditional_jump() {
            // Two ways on: the loop keeps the one that reaches the back edge sooner, where the
            // other is a guard's miss stub, a callout or another cold path.
            match (fall_through, target) {
                (Some(left), Some(right)) => Some(if distance(code, found, left, found.branch) <= distance(code, found, right, found.branch) { left } else { right }),
                (left, right) => left.or(right),
            }
        } else if code[at].falls_through() {
            fall_through
        } else {
            target
        };
        match next {
            Some(next) => at = next,
            None => break,
        }
    }
    path
}



/// The instructions of an iteration of the hottest loop of `code`: the smallest innermost loop
/// (alignment padding not counted), which is the specialized version a region runs once its
/// entry checks pass, where the larger ones are the general versions it falls back to.
pub fn hottest(code: &[Instruction]) -> Option<Vec<Instruction>> {
    let all = loops(code);
    let found = innermost(&all).min_by_key(|found| found.branch - found.header)?;
    let path: Vec<Instruction> = iteration(code, found).into_iter().map(|(_, instruction)| instruction).collect();
    Some(path)
}

/// The instructions of the largest innermost loop of `code`: a C twin's work loop, where the
/// smallest can be a generator's.
pub fn largest_innermost(code: &[Instruction]) -> Option<Vec<Instruction>> {
    let all = loops(code);
    let found = innermost(&all).max_by_key(|found| found.branch - found.header)?;
    Some(iteration(code, found).into_iter().map(|(_, instruction)| instruction).collect())
}

/// The instructions one call runs through a scalar entry at `entry` (a byte offset): from the
/// entry, the fall-through of every conditional branch (the path its checks expect) and the
/// target of every unconditional jump, up to the return. A function without a loop has no
/// iteration to measure; this path is its per-call cost.
pub fn call_path(code: &[Instruction], entry: u64) -> Option<Vec<Instruction>> {
    let mut at = index_of(code, entry)?;
    let mut visited = Visited::new(code.len());
    let mut path = Vec::new();
    while visited.insert(at) {
        let instruction = &code[at];
        path.push(instruction.clone());
        if !instruction.falls_through() && !instruction.is_conditional_jump() {
            match target_index(code, at) {
                Some(next) => at = next,
                None => break,
            }
        } else if at + 1 < code.len() {
            at += 1;
        } else {
            break;
        }
    }
    Some(path)
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

    #[test]
    fn a_branch_to_its_own_instruction_is_not_a_loop() {
        // `b .` (and x86's `jmp .`) spin in place: no instruction iterates.
        let code = parse("  0:\tb      0x0\n  4:\tret\n");
        assert!(loops(&code).is_empty());
        assert!(hottest(&code).is_none());
    }

    #[test]
    fn a_call_path_takes_fall_throughs_and_jumps_up_to_the_return() {
        let code = parse(
            "  0:	mov    %rax,%rbx
  3:	cmp    $0x2,%rdi
  7:	jl     0x20
  9:	jmp    0x10
  b:	ud2
 10:	add    $0x1,%rax
 14:	ret
 20:	mov    %rdi,%rax
 23:	ret
",
        );
        let path = call_path(&code, 0x3).expect("an entry");
        let addresses: Vec<u64> = path.iter().map(|instruction| instruction.address).collect();
        assert_eq!(addresses, [0x3, 0x7, 0x9, 0x10, 0x14]);
        assert!(call_path(&code, 0x5).is_none());
    }
}
