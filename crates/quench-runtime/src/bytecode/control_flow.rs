// The derived-row design reads successors from the row; this relation stays for the callers
// that still spell them while the compiler path is replaced.
#![allow(dead_code)]
use super::{ControlFlowLayout, Function, Instr, WideInstruction};

/// `Function::widened` of a packed instruction not yet validated: `None` where its wide index
/// is past the wide table.
pub(super) fn instruction_at(function: &Function, packed: Instr) -> Option<WideInstruction> {
    (!packed.is_wide() || packed.wide_index() < function.wide.len()).then(|| function.widened(packed))
}

/// Whether every edge into `pc` is the fall-through of the instruction at `pc - 1`: no jump,
/// branch table entry or exception handler targets it, so a record pair fused from `pc - 1`
/// and `pc` leaves nothing else arriving at the second expecting the first's result.
pub fn entered_only_by_fallthrough(function: &Function, pc: usize) -> bool {
    let Some(previous) = pc.checked_sub(1) else {
        return false;
    };
    if function.handlers.iter().any(|handler| handler.target as usize == pc) {
        return false;
    }
    !function.instructions().enumerate().any(|(at, instruction)| {
        at != previous && {
            let mut enters = false;
            for_each_instruction_successor(at, instruction, |successor| enters |= successor == pc);
            enters
        }
    })
}

/// The PCs of a branch table's `Jump` entries: its cases, then the default.
pub fn branch_table_entries(
    pc: usize,
    instruction: WideInstruction,
) -> std::ops::RangeInclusive<usize> {
    pc + 1..=pc + 1 + instruction.imm() as usize
}

/// Visit the ordinary instruction successors from the opcode's generated control-flow
/// layout. Exception handlers are separate edges owned by `Function::for_each_successor`.
pub fn for_each_instruction_successor(
    pc: usize,
    instruction: WideInstruction,
    mut successor: impl FnMut(usize),
) {
    match instruction.op().control_flow_layout() {
        ControlFlowLayout::Jump => successor(instruction.jump_target() as usize),
        ControlFlowLayout::ConditionalJump => {
            successor(pc + 1);
            successor(instruction.jump_target() as usize);
        }
        ControlFlowLayout::BranchTable => {
            branch_table_entries(pc, instruction).for_each(successor)
        }
        ControlFlowLayout::Terminal => {}
        ControlFlowLayout::Call | ControlFlowLayout::Fallthrough
            if instruction.returns_from_frame() => {}
        ControlFlowLayout::Call | ControlFlowLayout::Fallthrough => successor(pc + 1),
    }
}

/// The PCs of `instructions` control reaches from `starts` by ordinary successors.
pub fn reachable_from(instructions: &[WideInstruction], starts: impl IntoIterator<Item = usize>) -> crate::bitset::BitSet {
    let mut reached = crate::bitset::BitSet::new(instructions.len());
    let mut pending: Vec<usize> = starts.into_iter().collect();
    while let Some(pc) = pending.pop() {
        if pc >= instructions.len() || reached.contains(pc) {
            continue;
        }
        reached.insert(pc);
        for_each_instruction_successor(pc, instructions[pc], |successor| pending.push(successor));
    }
    reached
}

pub(super) fn is_bounded(function: &Function) -> bool {
    let code = &function.code;
    let mut reachable = crate::bitset::BitSet::new(code.len());
    let mut work = vec![0usize];
    work.extend(function.handlers.iter().flat_map(|handler| {
        std::iter::once(handler.target as usize)
            .chain(handler.return_target.map(|target| target as usize))
    }));
    while let Some(pc) = work.pop() {
        if pc >= code.len() || reachable.contains(pc) {
            if pc >= code.len() {
                return false;
            }
            continue;
        }
        reachable.insert(pc);
        let Some(instruction) = instruction_at(function, code[pc]) else {
            return false;
        };
        match instruction.op().control_flow_layout() {
            ControlFlowLayout::Jump => work.push(instruction.jump_target() as usize),
            ControlFlowLayout::ConditionalJump => {
                work.push(instruction.jump_target() as usize);
                work.push(pc + 1);
            }
            ControlFlowLayout::BranchTable => {
                for entry in branch_table_entries(pc, instruction) {
                    let Some(entry_instruction) = code
                        .get(entry)
                        .and_then(|packed| instruction_at(function, *packed))
                    else {
                        return false;
                    };
                    if !entry_instruction.op().traits().has(super::Traits::UNCONDITIONAL_JUMP) {
                        return false;
                    }
                    work.push(entry);
                }
            }
            ControlFlowLayout::Terminal => {}
            ControlFlowLayout::Call | ControlFlowLayout::Fallthrough
                if instruction.returns_from_frame() => {}
            ControlFlowLayout::Call | ControlFlowLayout::Fallthrough => work.push(pc + 1),
        }
    }
    true
}

/// How control reaches a successor: in order, or by an abrupt completion that a
/// handler covering the instruction catches (or a `finally` return resumes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlEdge {
    Normal,
    Handler,
}

impl super::Handler {
    /// Visits each of the ascending `pcs` with the indices of the handlers covering it, in table
    /// order. One sweep over the handlers in order of their starts, so a function with many try
    /// statements costs its pcs plus its handlers times their nesting, not its whole handler
    /// table at every pc.
    pub(crate) fn for_each_covering(
        handlers: &[Self],
        pcs: impl IntoIterator<Item = usize>,
        mut visit: impl FnMut(usize, &[usize]),
    ) {
        if handlers.is_empty() {
            return pcs.into_iter().for_each(|pc| visit(pc, &[]));
        }
        let mut order = (0..handlers.len()).collect::<Vec<_>>();
        order.sort_by_key(|&index| handlers[index].start);
        let mut pending = order.into_iter().peekable();
        let mut active = Vec::<usize>::new();
        for pc in pcs {
            let at = pc_index(pc);
            active.retain(|&index| handlers[index].end > at);
            while let Some(index) = pending.next_if(|&index| handlers[index].start <= at) {
                if handlers[index].end > at {
                    active.insert(active.partition_point(|&other| other < index), index);
                }
            }
            visit(pc, &active);
        }
    }
}

/// The successors a handler covering an instruction adds: its target, and the return
/// target a `finally` resumes.
fn handler_successors(handler: &super::Handler, successor: &mut impl FnMut(usize, ControlEdge)) {
    successor(handler.target as usize, ControlEdge::Handler);
    if let Some(target) = handler.return_target {
        successor(target as usize, ControlEdge::Handler);
    }
}

/// Every instruction's successors, normally or through a handler covering it, listed once
/// for a pass over the whole function (`Handler::for_each_covering`). `pc + 1` stands for the
/// function's end after its last instruction.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct Successors {
    /// Where each pc's successors begin in `targets`, with one entry past the last pc.
    starts: Vec<u32>,
    targets: Vec<u32>,
    edges: Vec<ControlEdge>,
}

impl Successors {
    pub(crate) fn of(function: &Function, instructions: &[WideInstruction]) -> Self {
        let mut table = Self::default();
        table.fill(function, instructions.len(), |pc| instructions[pc]);
        table
    }

    /// Refills this table with the successors of `function`'s code as it stands, as `of_code`
    /// computes them, reusing the table's buffers.
    pub(crate) fn refill(&mut self, function: &Function) {
        self.fill(function, function.code.len(), |pc| function.instruction_at(pc));
    }

    /// Clears the table and writes the successors of the `length` instructions `instruction`
    /// gives, normally or through a handler covering them (`Handler::for_each_covering`).
    fn fill(&mut self, function: &Function, length: usize, instruction: impl Fn(usize) -> WideInstruction) {
        let handlers = &function.handlers;
        self.starts.clear();
        self.targets.clear();
        self.edges.clear();
        self.starts.reserve(length + 1);
        self.targets.reserve(length);
        self.edges.reserve(length);
        super::Handler::for_each_covering(handlers, 0..length, |pc, covering| {
            self.starts.push(pc_index(self.targets.len()));
            let mut push = |target, edge| {
                self.targets.push(pc_index(target));
                self.edges.push(edge);
            };
            for_each_instruction_successor(pc, instruction(pc), |target| push(target, ControlEdge::Normal));
            for &index in covering {
                handler_successors(&handlers[index], &mut push);
            }
        });
        self.starts.push(pc_index(self.targets.len()));
    }

    /// This table after dropping the instructions `map` (old pc to new, one entry past the
    /// end) gives no pc of their own, as `relocate` renumbers the code: each kept pc's
    /// successors, renumbered, without decoding the code again.
    pub(crate) fn relocated(&self, map: &[usize]) -> Self {
        let kept = map.last().copied().unwrap_or_default();
        let mut table = Self { starts: Vec::with_capacity(kept + 1), targets: Vec::with_capacity(self.targets.len()), edges: Vec::with_capacity(self.edges.len()) };
        for pc in (0..map.len() - 1).filter(|&pc| map[pc] != map[pc + 1]) {
            table.starts.push(pc_index(table.targets.len()));
            for (target, edge) in self.at(pc) {
                table.targets.push(pc_index(map[target]));
                table.edges.push(edge);
            }
        }
        table.starts.push(pc_index(table.targets.len()));
        table
    }

    /// The successor table of `function`'s code as it stands.
    pub(crate) fn of_code(function: &Function) -> Self {
        let mut table = Self::default();
        table.refill(function);
        table
    }

    /// The successors of the instruction at `pc`, with how control reaches each.
    pub(crate) fn at(&self, pc: usize) -> impl Iterator<Item = (usize, ControlEdge)> + '_ {
        let range = self.starts[pc] as usize..self.starts[pc + 1] as usize;
        self.targets[range.clone()].iter().map(|&target| target as usize).zip(self.edges[range].iter().copied())
    }

    /// Where each pc's successors begin in `targets`, with one entry past the last pc.
    pub(crate) fn starts(&self) -> &[u32] {
        &self.starts
    }

    /// How control reaches each of `targets`, in the same order.
    pub(crate) fn edges(&self) -> &[ControlEdge] {
        &self.edges
    }

    /// Every pc's successor targets, in pc order.
    pub(crate) fn targets(&self) -> &[u32] {
        &self.targets
    }
}

fn pc_index(pc: usize) -> u32 {
    u32::try_from(pc).expect("a function's pcs fit u32")
}

impl Function {
    /// Every control transfer of the function to an instruction at or before its source, as
    /// (target, source): jumps, branch table entries (which are jumps), and exception and
    /// `finally` continuations, taken from the last instruction their range covers.
    pub(crate) fn backward_transfers(&self) -> Vec<(usize, usize)> {
        // The jumps are the body's back edges, listed by source; the handler edges below are the
        // transfers `back_edges` does not cover.
        let mut transfers: Vec<(usize, usize)> = self.back_edges().map(|(source, target)| (target, source)).collect();
        for handler in self.handlers.iter() {
            let last = handler.end.saturating_sub(1) as usize;
            for target in std::iter::once(handler.target).chain(handler.return_target) {
                if target as usize <= last {
                    transfers.push((target as usize, last));
                }
            }
        }
        transfers
    }
}

/// Whether the instruction at `pc` may run more than once in one activation. A cycle through
/// `pc` must leave it for an instruction before it, by a transfer from at or after `pc` to at
/// or before it, which `backward` (`Function::backward_transfers`) lists.
pub fn may_repeat(backward: &[(usize, usize)], pc: usize) -> bool {
    backward.iter().any(|&(target, source)| target <= pc && pc <= source)
}
