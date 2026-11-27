use crate::staged::OperationOperationExt;
use super::*;
use crate::bytecode::ImmediateLayout;
use crate::staged::{CallOut, DecodeContext};

type Row = Operation<u64, ()>;

fn decoded(op: Op, imm: u32) -> Option<Row> {
    Row::decode(WideInstruction::new(op, 4, 5, 6, imm), DecodeContext::EMPTY)
}

#[test]
fn a_spread_call_names_its_callee_receiver_and_one_argument_register() {
    let imm = ImmediateLayout::call_immediate(7, 1, false, false);
    for (op, passing) in [(Op::CallSpread, Passing::Iterable), (Op::CallArray, Passing::Array), (Op::CallRest, Passing::Rest)] {
        let (call, result) = decoded(op, imm).and_then(|row| row.call_out()).expect("a spread call is a call-out row");
        let arguments = Arguments::Window { first: 7, count: 1 };
        assert_eq!(call, CallOut::Spread { callee: 5, receiver: 6, arguments, passing }, "{op:?}");
        assert_eq!(result, Some(4));
        let operands = call.operands();
        assert_eq!((operands.fixed, operands.list), ([Some(crate::bytecode::Operand::register(5)), Some(crate::bytecode::Operand::register(6))], Some(arguments)));
        assert_eq!(operands.registers(&[]).collect::<Vec<_>>(), [5, 6, 7]);
    }
    let eval = ImmediateLayout::call_immediate(7, 1, true, false);
    assert!(decoded(Op::CallSpread, eval).is_none(), "an eval is the host's");
}

#[test]
fn a_super_construction_passes_a_window_of_values_or_an_array() {
    let values = decoded(Op::Construct, ImmediateLayout::construct_immediate(2, true, false)).and_then(|row| row.call_out());
    let (call, _) = values.expect("a super construction is a call-out row");
    assert_eq!(call, CallOut::SuperCall { callee: 5, arguments: Arguments::Window { first: 6, count: 2 }, passing: Passing::Values });
    assert_eq!(call.operands().registers(&[]).collect::<Vec<_>>(), [5, 6, 7]);
    let array = decoded(Op::Construct, ImmediateLayout::construct_immediate(0, true, true)).and_then(|row| row.call_out());
    let (call, _) = array.expect("an applied super construction is a call-out row");
    assert_eq!(call, CallOut::SuperCall { callee: 5, arguments: Arguments::Window { first: 6, count: 1 }, passing: Passing::Array });
    assert!(decoded(Op::Construct, ImmediateLayout::construct_immediate(2, false, false)).is_some_and(|row| row.call_out().is_none()),
        "an ordinary construction is a call, not a call-out");
}
