use super::{Function, Instr, Op, WideInstruction};

pub(super) fn instruction_at(function: &Function, packed: Instr) -> Option<WideInstruction> {
    if packed.is_wide() {
        function.wide.get(packed.wide_index()).copied()
    } else {
        Some(packed.as_wide())
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
        match instruction.op() {
            Op::Return | Op::Throw => {}
            Op::Jump => work.push(instruction.imm() as usize),
            Op::JumpFalse | Op::JumpBinaryFalse => {
                work.push(instruction.imm() as usize);
                work.push(pc + 1);
            }
            _ if instruction.returns_from_frame() => {}
            _ => work.push(pc + 1),
        }
    }
    true
}
