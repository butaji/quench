//! The liveness solver over instruction transfer masks, at the width a function's registers
//! need: most functions use fewer than 64 registers, so their transfers and live sets take
//! half the space. A function's tables hold several masks per instruction while it is
//! solved, which for a function of a few hundred thousand instructions (an emitted data
//! initializer, mandreel's `global_init`) is tens of megabytes at a load's peak.

use super::{RegisterSet, register_bit};
use crate::bytecode::{ControlEdge, ControlFlowLayout, Function, Op};
use crate::compile::CompileScratch;
use std::ops::{BitAnd, BitOr, BitOrAssign, Not};

/// A set of registers, one bit each, at a width that holds every register of the function.
pub(crate) trait LiveMask:
    Copy + Eq + Default + BitOr<Output = Self> + BitAnd<Output = Self> + Not<Output = Self> + BitOrAssign + Into<RegisterSet>
{
    /// `set` at this width; a function whose registers fit it has no wider register.
    fn narrow(set: RegisterSet) -> Self;
}

impl LiveMask for u64 {
    fn narrow(set: RegisterSet) -> Self {
        u64::try_from(set & RegisterSet::from(u64::MAX)).unwrap_or_default()
    }
}

impl LiveMask for u128 {
    fn narrow(set: RegisterSet) -> Self {
        (set & RegisterSet::from(u128::MAX)).as_u128().unwrap_or_default()
    }
}

impl LiveMask for RegisterSet {
    fn narrow(set: RegisterSet) -> Self {
        set
    }
}

/// An instruction index in a successor table.
pub(crate) trait Pc: Copy {
    fn index(self) -> usize;
}

impl Pc for usize {
    fn index(self) -> usize {
        self
    }
}

impl Pc for u32 {
    fn index(self) -> usize {
        self as usize
    }
}

/// The registers live before each instruction of `function` and after its last, under
/// `transfers` (each instruction's uses and definitions).
pub(crate) fn analyze_transfers<M: LiveMask>(function: &Function, transfers: &[(M, M)]) -> Vec<M> {
    analyze_successors(function, transfers, &crate::bytecode::Successors::of_code(function))
}

/// `analyze_successors` of wide masks, solved at the narrowest of one word, two words or a
/// whole set that holds the function's registers: most fit a word, and a set's eight words
/// cost a function of 65 to 128 registers four times a pair's on every transfer.
pub(crate) fn analyze_wide(
    function: &Function, transfers: &[(RegisterSet, RegisterSet)], successors: &crate::bytecode::Successors,
) -> Vec<RegisterSet> {
    analyze_wide_with(function, transfers, successors, &mut CompileScratch::default())
}

/// `analyze_wide`, narrowing its transfers into `scratch`'s buffers.
pub(crate) fn analyze_wide_with(
    function: &Function, transfers: &[(RegisterSet, RegisterSet)], successors: &crate::bytecode::Successors,
    scratch: &mut CompileScratch,
) -> Vec<RegisterSet> {
    match usize::from(function.registers) {
        registers if registers <= u64::BITS as usize => {
            let mut narrowed = std::mem::take(&mut scratch.narrowed_word);
            let live = analyze_narrowed::<u64>(function, transfers, successors, &mut narrowed);
            scratch.narrowed_word = narrowed;
            live
        }
        registers if registers <= u128::BITS as usize => {
            let mut narrowed = std::mem::take(&mut scratch.narrowed_pair);
            let live = analyze_narrowed::<u128>(function, transfers, successors, &mut narrowed);
            scratch.narrowed_pair = narrowed;
            live
        }
        _ => analyze_successors(function, transfers, successors),
    }
}

/// `analyze_successors` of `transfers` at the width `M`, which holds every register of the
/// function, widened back to sets. `narrowed` is the buffer the narrowed transfers are written to.
fn analyze_narrowed<M: LiveMask>(
    function: &Function, transfers: &[(RegisterSet, RegisterSet)], successors: &crate::bytecode::Successors,
    narrowed: &mut Vec<(M, M)>,
) -> Vec<RegisterSet> {
    narrowed.clear();
    narrowed.extend(transfers.iter().map(|&(read, written)| (M::narrow(read), M::narrow(written))));
    analyze_successors(function, narrowed.as_slice(), successors).into_iter().map(Into::into).collect()
}

/// `analyze_transfers` over the function's successor table, `successors`.
pub(crate) fn analyze_successors<M: LiveMask>(
    function: &Function, transfers: &[(M, M)], successors: &crate::bytecode::Successors,
) -> Vec<M> {
    // Legacy f.arguments can observe promoted parameters without a bytecode read.
    // Keep them even at terminal/exception PCs; consumers include GC and native recovery.
    // Without binding-name tables, conservatively retain every promoted parameter
    // of a function that permits legacy reflection.
    let observable = if function.has_restricted_legacy_caller_access() {
        M::default()
    } else {
        M::narrow(function.local_registers.iter().filter(|entry| entry.local < function.params)
            .fold(RegisterSet::default(), |mask, entry| mask | register_bit(entry.register)))
    };
    let mut live = analyze_graph(transfers, successors.starts(), successors.targets(), successors.edges(), |_| observable);
    add_suspended_exception_roots(function, &mut live);
    live
}

/// Solve shared instruction transfers with per-PC recovery requirements.
/// `starts` indexes a compact successor table; the final requirement describes
/// falling off the graph. Rewritten native units add their recovery floors here.
/// `kinds` gives how control reaches each successor (empty: every one in order). An
/// instruction an abrupt completion may leave by a handler reaches the handler's live-in set
/// whatever the instruction writes, since the write is not made on that edge: it is live
/// before every instruction the handler covers.
pub(crate) fn analyze_graph<M: LiveMask, P: Pc>(
    transfers: &[(M, M)], starts: &[P], successors: &[P], kinds: &[ControlEdge], required: impl Fn(usize) -> M,
) -> Vec<M> {
    let length = transfers.len();
    assert_eq!(starts.len(), length + 1);
    let edges = |pc: usize| &successors[starts[pc].index()..starts[pc + 1].index()];
    let has_backward_successor = (0..length).any(|pc| edges(pc).iter().any(|successor| successor.index() <= pc));
    let mut live: Vec<_> = (0..=length).map(&required).collect();
    loop {
        let mut changed = false;
        for pc in (0..length).rev() {
            let (mut output, mut thrown) = (M::default(), M::default());
            for edge in starts[pc].index()..starts[pc + 1].index() {
                let reached = live.get(successors[edge].index()).copied().unwrap_or_default();
                output |= reached;
                if kinds.get(edge) == Some(&ControlEdge::Handler) { thrown |= reached; }
            }
            let (read, written) = transfers[pc];
            let next = required(pc) | read | thrown | (output & !written);
            if live[pc] != next {
                live[pc] = next;
                changed = true;
            }
        }
        if !changed || !has_backward_successor {
            break;
        }
    }
    live
}

/// Whether a frame paused at `op` can resume with an abrupt completion that enters the
/// enclosing handler: a call that throws, or a suspension point (an awaited rejection, a
/// generator's `throw` or `return`).
fn resumes_into_handler(op: Op) -> bool {
    op.control_flow_layout() == ControlFlowLayout::Call || op.suspends()
}

pub(crate) fn add_suspended_exception_roots<M: LiveMask>(function: &Function, live: &mut [M]) {
    // Most functions have no handler for an abrupt resumption to enter.
    if function.handlers.is_empty() {
        return;
    }
    let resumptions = function
        .instructions()
        .enumerate()
        .filter(|(_, instruction)| resumes_into_handler(instruction.op()))
        .map(|(pc, _)| pc);
    crate::bytecode::Handler::for_each_covering(&function.handlers, resumptions, |pc, covering| {
        for index in covering {
            let handler_live = live[function.handlers[*index].target as usize];
            live[pc + 1] |= handler_live;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{Instr, Successors};

    #[test]
    fn a_word_solve_equals_the_wide_solve_it_stands_for() {
        let bit = RegisterSet::bit;
        // A loop that reads a register before writing it: the live sets need a second sweep.
        let transfers = [(bit(0), bit(1)), (bit(1), bit(2)), (RegisterSet::default(), RegisterSet::default())];
        let mut program = crate::Engine::specialize_unspecialized("", "wide-solve.js").unwrap();
        let function = &mut program.functions[0];
        function.code = vec![
            Instr::new(Op::Move, 1, 0, 0, 0),
            Instr::new(Op::Move, 2, 1, 0, 0),
            Instr::new(Op::Jump, 0, 0, 0, 0),
        ];
        for registers in [3, u64::BITS as u16, u64::BITS as u16 + 1] {
            function.registers = registers;
            let successors = Successors::of_code(function);
            assert_eq!(
                analyze_wide(function, &transfers, &successors),
                analyze_successors(function, &transfers, &successors),
                "{registers} registers"
            );
        }
    }
}
