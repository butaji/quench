use super::*;
use crate::bytecode::{Op, WideInstruction};

/// Each allocation opcode decodes to its row with the operands the catalog fixes, and
/// no other opcode decodes.
#[test]
fn allocation_opcodes_decode_to_their_rows() {
    let literal = WideInstruction::new(Op::MakeObjectLiteral, 4, 1, 3, 9);
    assert_eq!(Allocation::decode(literal, &[]), Some((4, Allocation::ObjectLiteral {
        site: 9, window: RegisterWindow { base: 1, count: 3 } })));
    let closure = WideInstruction::new(Op::MakeClosure, 2, 0, 0, 7);
    assert_eq!(Allocation::decode(closure, &[]), Some((2, Allocation::Closure { function: 7 })));
    let template = WideInstruction::new(Op::MakeLiteralTemplate, 5, 0, 0, 3);
    assert_eq!(Allocation::decode(template, &[]), Some((5, Allocation::Template { template: 3 })));
    let concat = WideInstruction::new(Op::StringConcat, 6, 2, 2, 0);
    assert_eq!(Allocation::decode(concat, &[]), Some((6, Allocation::Concat {
        window: RegisterWindow { base: 2, count: 2 } })));
    assert_eq!(Allocation::decode(WideInstruction::new(Op::Move, 1, 2, 0, 0), &[]), None);
}

#[test]
fn the_empty_object_pair_and_constant_array_decode_to_rows() {
    let make = |op, b, c, imm| Allocation::decode(WideInstruction::new(op, 3, b, c, imm), &[]);
    assert_eq!(make(Op::MakeObject, 0, 0, 0), Some((3, Allocation::Empty)));
    assert_eq!(make(Op::MakeObject2, 1, 2, 5), Some((3, Allocation::ObjectPair { site: 5, first: 1, second: 2 })));
    assert_eq!(make(Op::MakeConstArray, 2, 0, 5), Some((3, Allocation::ConstArray { constant: 5, count: 2 })));
    assert_eq!(make(Op::MakeArray, 0, 0, 4), Some((3, Allocation::Array { length: 4, numbers: false })));
    assert_eq!(make(Op::CreateRegExpLiteral, 0, 0, 6), Some((3, Allocation::RegExp { site: 6, callee: None })));
    let argument = make(Op::CreateRegExpLiteral, 5 + crate::bytecode::OPTIONAL_REGISTER_BIAS, 0, 6);
    assert_eq!(argument, Some((3, Allocation::RegExp { site: 6, callee: Some(5) })), "field B names the call's callee");
    assert!(matches!(make(Op::StringConcat, 1, 1, 0), Some((3, Allocation::Concat { .. }))));
    assert!(make(Op::Jump, 0, 0, 0).is_none());
}

/// A `MakeArray` whose definitions follow it in order is the `ArrayOf` row over their
/// registers, which need not be contiguous; a missing, misordered or self-referencing
/// definition leaves the array of holes, and a literal past the window does too.
#[test]
fn an_array_literal_decodes_to_its_element_registers() {
    let make = |length| WideInstruction::new(Op::MakeArray, 4, 0, 0, length);
    let define = |value, index| WideInstruction::new(Op::DefineArrayElement, value, 4, 0, index);
    let rows = [define(9, 0), define(2, 1), define(7, 2)];
    let Some((4, Allocation::ArrayOf { elements })) = Allocation::decode(make(3), &rows) else { panic!("fused") };
    assert_eq!(elements.registers(), &[9, 2, 7]);
    let holes = Some((4, Allocation::Array { length: 3, numbers: false }));
    assert_eq!(Allocation::decode(make(3), &rows[..2]), holes, "a definition is missing");
    assert_eq!(Allocation::decode(make(3), &[rows[1], rows[0], rows[2]]), holes, "out of order");
    assert_eq!(Allocation::decode(make(3), &[rows[0], define(4, 1), rows[2]]), holes, "the array is its own element");
    assert_eq!(Allocation::decode(make(3), &[rows[0], WideInstruction::new(Op::DefineArrayElement, 2, 5, 0, 1), rows[2]]), holes,
        "another array's definition");
    assert_eq!(Allocation::decode(make(0), &rows), Some((4, Allocation::Array { length: 0, numbers: false })));
    let long: Vec<_> = (0..=crate::bytecode::ARRAY_LITERAL_WINDOW as u32).map(|index| define(index as u16 + 8, index)).collect();
    assert_eq!(Allocation::decode(make(long.len() as u32), &long),
        Some((4, Allocation::Array { length: long.len() as u32, numbers: false })), "past the window");
}
