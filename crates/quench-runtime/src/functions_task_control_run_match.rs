fn task_state_arm(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    if code.len() != 6 {
        return false;
    }
    let ops: [_; 6] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && ops[1].opcode == Move
        && ops[2].opcode == Move
        && is_local_load(ops[3])
        && (ops[4].opcode, ops[4].a, ops[4].b) == (SetN, ops[2].a, ops[3].a)
        && code.metadata_at(4).and_then(|meta| meta.name.as_deref()) == Some("state")
}
