use super::*;
use crate::compile::CompileScratch;

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

/// A handler covering `start..end` that enters `target`.
fn handler(start: u32, end: u32, target: u32) -> crate::bytecode::Handler {
    crate::bytecode::Handler { start, end, target, slot: None, return_target: None, return_slot: None, with_depth: 0 }
}

/// `code` after `forward_move_sources`, as one function of `registers` registers under `handlers`.
fn forwarded_in(code: Vec<Instr>, registers: u16, handlers: Vec<crate::bytecode::Handler>) -> Vec<Instr> {
    let mut program = crate::Engine::specialize_unspecialized("", "copies-forward.js").unwrap();
    let function = &mut program.functions[0];
    function.code = code;
    function.registers = registers;
    function.handlers = handlers.into();
    forward_move_sources(function, &[] as &[MethodSiteSpec], &[], &[], &mut None, &mut CompileScratch::default());
    std::mem::take(&mut function.code)
}

fn forwarded(code: Vec<Instr>, registers: u16) -> Vec<Instr> {
    forwarded_in(code, registers, Vec::new())
}

/// `code` after `coalesce_distant_producer_moves` runs to a fixed point under `handlers`.
fn coalesced_in(code: Vec<Instr>, registers: u16, handlers: Vec<crate::bytecode::Handler>) -> Vec<Instr> {
    let mut program = crate::Engine::specialize_unspecialized("", "copies-coalesce.js").unwrap();
    let function = &mut program.functions[0];
    function.code = code;
    function.registers = registers;
    function.handlers = handlers.into();
    while coalesce_distant_producer_moves(function, &[] as &[MethodSiteSpec], &[], &[], &mut None, &mut CompileScratch::default()) {}
    std::mem::take(&mut function.code)
}

fn coalesced(code: Vec<Instr>, registers: u16) -> Vec<Instr> {
    coalesced_in(code, registers, Vec::new())
}

/// The two operands of a binary instruction, as registers.
fn operands(instruction: Instr) -> (Option<Register>, Option<Register>) {
    (instruction.operand_b().register_index(), instruction.operand_c().register_index())
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

#[test]
fn forwards_both_operand_copies_into_one_reader() {
    // The second copy forwards too: the first rewrite leaves the reader's masks current.
    let code = vec![
        Instr::new(Op::LoadConst, 1, 0, 0, 0),
        Instr::new(Op::LoadConst, 2, 0, 0, 0),
        Instr::new(Op::Move, 3, 1, 0, 0),
        Instr::new(Op::Move, 4, 2, 0, 0),
        sum(5, 3, 4),
        Instr::new(Op::Return, 5, 0, 0, 0),
    ];
    let forwarded = forwarded(code, 6);
    assert_eq!((forwarded[2].op(), forwarded[3].op()), (Op::Nop, Op::Nop));
    assert_eq!(operands(forwarded[4]), (Some(1), Some(2)));
}

#[test]
fn forwards_a_copy_into_every_reader_of_its_span() {
    let code = vec![
        Instr::new(Op::LoadConst, 1, 0, 0, 0),
        Instr::new(Op::Move, 3, 1, 0, 0),
        sum(4, 3, 1),
        sum(5, 4, 3),
        Instr::new(Op::Return, 5, 0, 0, 0),
    ];
    let forwarded = forwarded(code, 6);
    assert_eq!(forwarded[1].op(), Op::Nop);
    assert_eq!(operands(forwarded[2]), (Some(1), Some(1)));
    assert_eq!(operands(forwarded[3]), (Some(4), Some(1)));
}

#[test]
fn reads_the_source_before_its_redefinition_and_keeps_the_copy() {
    // The copy's value is still live at the redefinition of its source, so the copy stays; the
    // reader before the redefinition reads the source, and the one after it reads the copy.
    let code = vec![
        Instr::new(Op::LoadConst, 1, 0, 0, 0),
        Instr::new(Op::Move, 3, 1, 0, 0),
        sum(4, 3, 3),
        Instr::new(Op::LoadConst, 1, 0, 0, 0),
        sum(5, 4, 3),
        sum(5, 5, 1),
        Instr::new(Op::Return, 5, 0, 0, 0),
    ];
    let forwarded = forwarded(code, 6);
    assert_eq!(forwarded[1].op(), Op::Move);
    assert_eq!(operands(forwarded[2]), (Some(1), Some(1)));
    assert_eq!(operands(forwarded[4]), (Some(4), Some(3)));
}

#[test]
fn forwards_inside_a_handler_whose_catch_does_not_read_the_copy() {
    let code = vec![
        Instr::new(Op::LoadConst, 1, 0, 0, 0),
        Instr::new(Op::Move, 3, 1, 0, 0),
        sum(4, 3, 1),
        Instr::new(Op::Return, 4, 0, 0, 0),
        Instr::new(Op::Return, 0, 0, 0, 0),
    ];
    let mut expected = code.clone();
    expected[1] = Instr::new(NOP, 0, 0, 0, 0);
    expected[2] = sum(4, 1, 1);
    assert_eq!(fields(&forwarded_in(code, 6, vec![handler(1, 3, 4)])), fields(&expected));
}

#[test]
fn keeps_a_copy_inside_a_handler_whose_catch_reads_it() {
    let code = vec![
        Instr::new(Op::LoadConst, 1, 0, 0, 0),
        Instr::new(Op::Move, 3, 1, 0, 0),
        sum(4, 3, 1),
        Instr::new(Op::Return, 4, 0, 0, 0),
        Instr::new(Op::Return, 3, 0, 0, 0),
    ];
    let expected = code.clone();
    assert_eq!(fields(&forwarded_in(code, 6, vec![handler(1, 3, 4)])), fields(&expected));
}

#[test]
fn retargets_a_producer_inside_a_handler_whose_catch_does_not_read_either_register() {
    // Every instruction from the producer to the move lies under the same handler (0..4).
    let code = vec![
        Instr::new(Op::LoadConst, 3, 0, 0, 0),
        Instr::new(Op::LoadConst, 8, 0, 0, 0),
        Instr::new(Op::Move, 5, 3, 0, 0),
        Instr::new(Op::Return, 5, 0, 0, 0),
        Instr::new(Op::Return, 0, 0, 0, 0),
    ];
    let expected = vec![
        Instr::new(Op::LoadConst, 5, 0, 0, 0),
        Instr::new(Op::LoadConst, 8, 0, 0, 0),
        Instr::new(NOP, 0, 0, 0, 0),
        Instr::new(Op::Return, 5, 0, 0, 0),
        Instr::new(Op::Return, 0, 0, 0, 0),
    ];
    assert_eq!(fields(&coalesced_in(code, 9, vec![handler(0, 4, 4)])), fields(&expected));
}

#[test]
fn keeps_a_producer_inside_a_handler_whose_catch_reads_its_result() {
    let code = vec![
        Instr::new(Op::LoadConst, 3, 0, 0, 0),
        Instr::new(Op::LoadConst, 8, 0, 0, 0),
        Instr::new(Op::Move, 5, 3, 0, 0),
        Instr::new(Op::Return, 5, 0, 0, 0),
        Instr::new(Op::Return, 3, 0, 0, 0),
    ];
    let expected = code.clone();
    assert_eq!(fields(&coalesced_in(code, 9, vec![handler(0, 4, 4)])), fields(&expected));
}
