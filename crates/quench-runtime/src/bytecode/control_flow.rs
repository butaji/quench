use super::{ControlFlowLayout, Function, Instr, WideInstruction};

pub(super) fn instruction_at(function: &Function, packed: Instr) -> Option<WideInstruction> {
    if packed.is_wide() {
        function.wide.get(packed.wide_index()).copied()
    } else {
        Some(packed.as_wide())
    }
}

/// The PCs of a branch table's `Jump` entries: its cases, then the default.
pub(crate) fn branch_table_entries(
    pc: usize,
    instruction: WideInstruction,
) -> std::ops::RangeInclusive<usize> {
    pc + 1..=pc + 1 + instruction.imm() as usize
}

/// Visit the ordinary instruction successors from the opcode's generated control-flow
/// layout. Exception handlers are separate edges owned by `Function::for_each_successor`.
pub(crate) fn for_each_instruction_successor(
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
                    if entry_instruction.op() != super::Op::Jump {
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
pub(crate) enum ControlEdge {
    Normal,
    Handler,
}

impl Function {
    /// The instructions control can reach from `instruction` at `pc`, normally or through a
    /// handler covering it. `pc + 1` stands for the function's end after its last instruction.
    pub(crate) fn for_each_successor(
        &self,
        pc: usize,
        instruction: WideInstruction,
        mut successor: impl FnMut(usize, ControlEdge),
    ) {
        for_each_instruction_successor(pc, instruction, |target| {
            successor(target, ControlEdge::Normal)
        });
        for handler in &self.handlers {
            if pc as u32 >= handler.start && (pc as u32) < handler.end {
                handler_successors(handler, &mut successor);
            }
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

/// Every instruction's successors (`Function::for_each_successor`), listed once for a
/// pass over the whole function. The handler edges come from one sweep over the handlers
/// in order of their starts, so a function with many try statements costs its code plus
/// its handlers times their nesting, not its whole handler table at every pc.
pub(crate) struct Successors {
    /// Where each pc's successors begin in `targets`, with one entry past the last pc.
    starts: Vec<u32>,
    targets: Vec<u32>,
    edges: Vec<ControlEdge>,
}

impl Successors {
    pub(crate) fn of(function: &Function, instructions: &[WideInstruction]) -> Self {
        let handlers = &function.handlers;
        let mut order = (0..handlers.len()).collect::<Vec<_>>();
        order.sort_by_key(|&index| handlers[index].start);
        let mut pending = order.into_iter().peekable();
        // The handlers covering the current pc, in table order as `for_each_successor` lists them.
        let mut active = Vec::<usize>::new();
        let mut table = Self {
            starts: Vec::with_capacity(instructions.len() + 1),
            targets: Vec::with_capacity(instructions.len()),
            edges: Vec::with_capacity(instructions.len()),
        };
        for (pc, &instruction) in instructions.iter().enumerate() {
            let at = pc as u32;
            active.retain(|&index| handlers[index].end > at);
            while let Some(index) = pending.next_if(|&index| handlers[index].start <= at) {
                if handlers[index].end > at {
                    active.insert(active.partition_point(|&other| other < index), index);
                }
            }
            table.starts.push(pc_index(table.targets.len()));
            let mut push = |target, edge| {
                table.targets.push(pc_index(target));
                table.edges.push(edge);
            };
            for_each_instruction_successor(pc, instruction, |target| push(target, ControlEdge::Normal));
            for &index in &active {
                handler_successors(&handlers[index], &mut push);
            }
        }
        table.starts.push(pc_index(table.targets.len()));
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

    /// Every pc's successor targets, in pc order.
    pub(crate) fn targets(&self) -> &[u32] {
        &self.targets
    }
}

fn pc_index(pc: usize) -> u32 {
    u32::try_from(pc).expect("a function's pcs fit u32")
}
