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
    let mut reachable = vec![false; code.len()];
    let mut work = vec![0usize];
    work.extend(function.handlers.iter().flat_map(|handler| {
        std::iter::once(handler.target as usize)
            .chain(handler.return_target.map(|target| target as usize))
    }));
    while let Some(pc) = work.pop() {
        if pc >= code.len() || reachable[pc] {
            if pc >= code.len() {
                return false;
            }
            continue;
        }
        reachable[pc] = true;
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
                successor(handler.target as usize, ControlEdge::Handler);
                if let Some(target) = handler.return_target {
                    successor(target as usize, ControlEdge::Handler);
                }
            }
        }
    }
}
