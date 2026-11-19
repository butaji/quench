use super::*;

fn sum(result: Register, left: Register, right: Register) -> Instr {
    Instr::new(
        Op::Binary,
        result,
        Operand::register(left).0,
        Operand::register(right).0,
        BinaryOperator::Addition as u32,
    )
}

fn fields(code: &[Instr]) -> Vec<(Op, u16, u16, u16, u32)> {
    code.iter()
        .map(|instruction| (instruction.op(), instruction.a(), instruction.b(), instruction.c(), instruction.imm()))
        .collect()
}

/// `code` after `forward_move_sources`, as one function of `registers` registers.
fn forwarded(code: Vec<Instr>, registers: u16) -> Vec<Instr> {
    let mut program = crate::Engine::specialize_unspecialized("", "copies-forward.js").unwrap();
    let function = &mut program.functions[0];
    function.code = code;
    function.registers = registers;
    forward_move_sources(function, &[] as &[MethodSiteSpec], &[], &[], &mut None);
    std::mem::take(&mut function.code)
}

/// `code` after `coalesce_distant_producer_moves` runs to a fixed point.
fn coalesced(code: Vec<Instr>, registers: u16) -> Vec<Instr> {
    let mut program = crate::Engine::specialize_unspecialized("", "copies-coalesce.js").unwrap();
    let function = &mut program.functions[0];
    function.code = code;
    function.registers = registers;
    while coalesce_distant_producer_moves(function, &[] as &[MethodSiteSpec], &[], &[], &mut None) {}
    std::mem::take(&mut function.code)
}

#[test]
fn forwards_a_move_to_a_reader_past_the_old_window() {
    let mut code = vec![Instr::new(Op::Move, 5, 3, 0, 0)];
    code.extend((0..12).map(|index| Instr::new(Op::LoadConst, 10 + index, 0, 0, 0)));
    code.extend([sum(7, 5, 6), Instr::new(Op::Return, 7, 0, 0, 0)]);
    let mut expected = code.clone();
    expected[0] = Instr::new(NOP, 0, 0, 0, 0);
    expected[13] = sum(7, 3, 6);
    assert_eq!(fields(&forwarded(code, 24)), fields(&expected));
}

#[test]
fn stops_forwarding_at_a_redefinition_of_the_source() {
    let code = vec![
        Instr::new(Op::Move, 5, 3, 0, 0),
        Instr::new(Op::LoadConst, 3, 0, 0, 0),
        sum(7, 5, 6),
        Instr::new(Op::Return, 7, 0, 0, 0),
    ];
    let expected = code.clone();
    assert_eq!(fields(&forwarded(code, 8)), fields(&expected));
}

#[test]
fn stops_forwarding_at_a_block_exit() {
    let code = vec![
        Instr::new(Op::Move, 5, 3, 0, 0),
        Instr::new(Op::JumpFalse, 6, 0, 0, 3),
        sum(7, 5, 6),
        Instr::new(Op::Return, 7, 0, 0, 0),
    ];
    let expected = code.clone();
    assert_eq!(fields(&forwarded(code, 8)), fields(&expected));
}

#[test]
fn never_rewrites_a_read_inside_a_call_window() {
    let window = crate::bytecode::ImmediateLayout::call_immediate(4, 3, false, false);
    let code = vec![
        Instr::new(Op::Move, 5, 3, 0, 0),
        Instr::new(Op::Call, 7, 0, 0, window),
        Instr::new(Op::Return, 7, 0, 0, 0),
    ];
    let expected = code.clone();
    assert_eq!(fields(&forwarded(code, 8)), fields(&expected));
}

#[test]
fn removes_a_move_whose_destination_is_redefined_after_its_reads() {
    let code = vec![
        Instr::new(Op::Move, 5, 3, 0, 0),
        sum(8, 5, 6),
        Instr::new(Op::LoadConst, 5, 0, 0, 0),
        Instr::new(Op::Return, 8, 0, 0, 0),
    ];
    let mut expected = code.clone();
    expected[0] = Instr::new(NOP, 0, 0, 0, 0);
    expected[1] = sum(8, 3, 6);
    assert_eq!(fields(&forwarded(code, 9)), fields(&expected));
}

#[test]
fn retargets_a_distant_producer_across_an_unrelated_instruction() {
    let code = vec![
        Instr::new(Op::LoadConst, 3, 0, 0, 0),
        Instr::new(Op::LoadConst, 8, 0, 0, 0),
        Instr::new(Op::Move, 5, 3, 0, 0),
        Instr::new(Op::Return, 5, 0, 0, 0),
    ];
    let expected = vec![
        Instr::new(Op::LoadConst, 5, 0, 0, 0),
        Instr::new(Op::LoadConst, 8, 0, 0, 0),
        Instr::new(NOP, 0, 0, 0, 0),
        Instr::new(Op::Return, 5, 0, 0, 0),
    ];
    assert_eq!(fields(&coalesced(code, 9)), fields(&expected));
}
