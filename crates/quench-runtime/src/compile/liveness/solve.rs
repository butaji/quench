//! The liveness solver over instruction transfer masks, at the width a function's registers
//! need: most functions use fewer than 64 registers, so their transfers and live sets take
//! half the space. A function's tables hold several masks per instruction while it is
//! solved, which for a function of a few hundred thousand instructions (an emitted data
//! initializer, mandreel's `global_init`) is tens of megabytes at a load's peak.

use super::{RegisterSet, register_bit};
use crate::bytecode::{ControlEdge, ControlFlowLayout, Function, Op, Successors};
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

/// The registers live before each instruction of `function` and after its last, under
/// `transfers` (each instruction's uses and definitions).
pub fn analyze_transfers<M: LiveMask>(function: &Function, transfers: &[(M, M)]) -> Vec<M> {
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
pub fn analyze_wide_with(
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
    let mut live = analyze_graph(transfers, successors, |_| observable);
    add_suspended_exception_roots(function, &mut live);
    live
}

/// Solve shared instruction transfers with per-PC recovery requirements, over `successors`
/// (one entry per instruction, its successors and how control reaches each). The final
/// requirement describes falling off the graph. Rewritten native units add their recovery
/// floors here. An instruction an abrupt completion may leave by a handler reaches the
/// handler's live-in set whatever the instruction writes, since the write is not made on that
/// edge: it is live before every instruction the handler covers.
///
/// A backward problem of the one driver, `dataflow::solve`, over basic blocks: the blocks are
/// `Successors::leaders`, so a handler edge leaves from the end of its block. A block's state is
/// the pair (live after its last instruction, live into the handlers it may leave by); blocks
/// are numbered from the function's end, so the driver's lowest-first order is a reverse sweep.
pub fn analyze_graph<M: LiveMask>(
    transfers: &[(M, M)], successors: &Successors, required: impl Fn(usize) -> M,
) -> Vec<M> {
    let length = transfers.len();
    assert_eq!(successors.starts().len(), length + 1);
    let mut graph = LiveGraph::new(transfers, successors, required);
    crate::dataflow::solve(&mut graph);
    graph.live
}

/// `analyze_graph`'s block graph. `heads` holds each block's first instruction in ascending
/// order; block `b` is `heads[heads.len() - 1 - b]`.
struct LiveGraph<'a, M, F> {
    transfers: &'a [(M, M)],
    required: F,
    heads: Vec<usize>,
    /// Each block's predecessors, sorted, at `predecessor_starts[block]..[block + 1]`.
    predecessors: Vec<usize>,
    predecessor_starts: Vec<usize>,
    /// Whether the predecessor at the same index reaches the block by a handler edge.
    handler: Vec<bool>,
    /// What each block's last instruction reaches past the function's end: its output and
    /// thrown halves.
    exits: Vec<(M, M)>,
    /// The live set before each instruction and after the last, recorded by `transfer`.
    live: Vec<M>,
}

impl<'a, M: LiveMask, F: Fn(usize) -> M> LiveGraph<'a, M, F> {
    fn new(transfers: &'a [(M, M)], successors: &Successors, required: F) -> Self {
        let length = transfers.len();
        let leaders = successors.leaders(length);
        let heads: Vec<usize> = (0..length).filter(|&pc| leaders.contains(pc)).collect();
        let count = heads.len();
        let block_of = |pc: usize| count - heads.partition_point(|&start| start <= pc);
        let end_of = |block: usize| heads.get(count - block).copied().unwrap_or(length);
        let mut live: Vec<M> = vec![M::default(); length + 1];
        live[length] = required(length);
        let mut exits = vec![(M::default(), M::default()); count];
        let mut pairs: Vec<(usize, usize, bool)> = Vec::new();
        for block in 0..count {
            let last = end_of(block) - 1;
            for (target, edge) in successors.at(last) {
                let by_handler = edge == ControlEdge::Handler;
                if target < length {
                    pairs.push((block_of(target), block, by_handler));
                } else if target == length {
                    exits[block].0 |= live[length];
                    if by_handler { exits[block].1 |= live[length]; }
                }
            }
        }
        pairs.sort_unstable();
        let mut predecessors = Vec::with_capacity(pairs.len());
        let mut handler: Vec<bool> = Vec::with_capacity(pairs.len());
        let mut predecessor_starts = vec![0; count + 1];
        let mut previous: Option<(usize, usize)> = None;
        for (target, block, by_handler) in pairs {
            if previous == Some((target, block)) {
                if let Some(flag) = handler.last_mut() { *flag |= by_handler; }
                continue;
            }
            previous = Some((target, block));
            predecessors.push(block);
            handler.push(by_handler);
            predecessor_starts[target + 1] += 1;
        }
        for block in 0..count {
            predecessor_starts[block + 1] += predecessor_starts[block];
        }
        LiveGraph { transfers, required, heads, predecessors, predecessor_starts, handler, exits, live }
    }

    /// The instructions of `block`, first and one past its last.
    fn span(&self, block: usize) -> (usize, usize) {
        let count = self.heads.len();
        let index = count - 1 - block;
        (self.heads[index], self.heads.get(index + 1).copied().unwrap_or(self.transfers.len()))
    }
}

impl<M: LiveMask, F: Fn(usize) -> M> crate::dataflow::Dataflow for LiveGraph<'_, M, F> {
    /// Live after the block's last instruction, and live into the handlers it may leave by.
    type State = (M, M);

    fn blocks(&self) -> usize {
        self.heads.len()
    }

    fn edges(&self, block: usize) -> &[usize] {
        &self.predecessors[self.predecessor_starts[block]..self.predecessor_starts[block + 1]]
    }

    fn entry_state(&self, block: usize) -> Option<Self::State> {
        Some(self.exits[block])
    }

    fn join(&self, into: &mut Self::State, incoming: &Self::State) -> bool {
        let joined = (into.0 | incoming.0, into.1 | incoming.1);
        let changed = joined != *into;
        *into = joined;
        changed
    }

    fn transfer(&mut self, block: usize, state: &Self::State) -> Self::State {
        let (start, end) = self.span(block);
        let last = end - 1;
        let (read, written) = self.transfers[last];
        let mut live = (self.required)(last) | read | state.1 | (state.0 & !written);
        self.live[last] = live;
        for pc in (start..last).rev() {
            let (read, written) = self.transfers[pc];
            live = (self.required)(pc) | read | (live & !written);
            self.live[pc] = live;
        }
        (live, M::default())
    }

    fn transfer_edge(&self, block: usize, target: usize, out: &Self::State) -> Self::State {
        let (first, last) = (self.predecessor_starts[block], self.predecessor_starts[block + 1]);
        let by_handler = self.predecessors[first..last]
            .binary_search(&target)
            .is_ok_and(|index| self.handler[first + index]);
        (out.0, if by_handler { out.0 } else { M::default() })
    }
}

/// Whether a frame paused at `op` can resume with an abrupt completion that enters the
/// enclosing handler: a call that throws, or a suspension point (an awaited rejection, a
/// generator's `throw` or `return`).
fn resumes_into_handler(op: Op) -> bool {
    op.control_flow_layout() == ControlFlowLayout::Call || op.suspends()
}

pub fn add_suspended_exception_roots<M: LiveMask>(function: &Function, live: &mut [M]) {
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
