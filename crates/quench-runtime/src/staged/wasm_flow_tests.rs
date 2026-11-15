//! The control-flow table of `staged::wasm_flow`: which opcode is which flow, and where each
//! leaves to, read from the instruction's own targets.

use super::ConcreteWasm;
use super::machine::MachineStage;
use super::wasm_flow::{Flow, Leave};
use super::wasm_step::Step;
use crate::WasmTrap;
use crate::bytecode::{Op, WideInstruction};

#[test]
fn the_table_names_each_control_opcode() {
    let named = [
        (Op::Jump, Flow::Jump),
        (Op::WasmBranchTable, Flow::Table),
        (Op::WasmSelect, Flow::Select),
        (Op::WasmUnreachable, Flow::Unreachable),
        (Op::Return, Flow::Return),
    ];
    for (op, flow) in named {
        assert_eq!(Flow::of(op), Some(flow), "{op:?}");
        assert_eq!(Step::of(op, 0), Some(Step::Flow(flow)), "{op:?}");
    }
    assert_eq!(Flow::of(Op::WasmI32Add), None);
    assert!(Step::of(Op::Return, 0).is_some_and(Step::returns));
    assert!(!Step::of(Op::Jump, 0).is_some_and(Step::returns));
}

fn jump_to(target: u32) -> WideInstruction {
    WideInstruction::new(Op::Jump, 0, 0, 0, target)
}

#[test]
fn a_jump_leaves_to_its_target_and_a_table_to_its_entries_targets() {
    let target = |entry: usize| jump_to(100 + entry as u32);
    let jump = jump_to(9);
    assert_eq!(Flow::Jump.leave(3, jump, target), Leave::Jump(9));
    let table = WideInstruction::new(Op::WasmBranchTable, 1, 0, 0, 2);
    assert_eq!(
        Flow::Table.leave(4, table, target),
        Leave::Table(vec![105, 106, 107])
    );
}

#[test]
fn the_other_flows_leave_where_their_meaning_says() {
    let none = |_: usize| jump_to(0);
    let instruction = jump_to(0);
    assert_eq!(Flow::Select.leave(0, instruction, none), Leave::Next);
    assert_eq!(Flow::Return.leave(0, instruction, none), Leave::Return);
    assert_eq!(
        Flow::Unreachable.leave(0, instruction, none),
        Leave::Trap(WasmTrap::Unreachable)
    );
}

#[test]
fn a_select_takes_the_first_value_where_the_flag_is_set() {
    assert_eq!(ConcreteWasm.select(1, 7, 9), 7);
    assert_eq!(ConcreteWasm.select(0, 7, 9), 9);
    assert_eq!(ConcreteWasm.select(u64::from(u32::MAX), 7, 9), 7);
}
