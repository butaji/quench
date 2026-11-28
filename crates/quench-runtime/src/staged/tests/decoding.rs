#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt, OperationNamesExt};
use super::*;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// The catalog's staged column and `Operation::decode` agree: every opcode that always decodes
/// has a decoder, one decoded by a tier's proof (`Decoder::Proved`) decodes through
/// `decode_load_name`, and no opcode without a decoder decodes at all.
#[test]
fn every_staged_opcode_decodes_to_one_operation() {
    use crate::bytecode::{Decoder, Op, WideInstruction};
    let constants = [Constant::untyped(Value::number(1.0))];
    let context = crate::staged::DecodeContext::of_constants(&constants);
    let always = [Op::Move, Op::LoadConst, Op::LoadLocalPlain, Op::StoreLocalPlain,
        Op::Jump, Op::JumpFalse, Op::JumpTrue, Op::LoadThis, Op::RequireObjectCoercible,
        Op::ToNumeric, Op::ToPropertyKey, Op::LoadEnvLocal, Op::StoreEnvLocal, Op::ArgumentsLength, Op::LoadNameTypeof,
        Op::StoreName, Op::StoreVarBinding, Op::LoadGlobalLexical, Op::StoreGlobalLexical, Op::ForInNext, Op::ForInStart, Op::GetIterator,
        Op::IteratorClose, Op::AppendArray,
        Op::LoadCapture, Op::StoreCapture, Op::JumpBinaryFalse, Op::Return];
    for op in always {
        let instruction = WideInstruction::new(op, 1, 1, 1, 0);
        assert!(op.staged().is_some(), "{op:?}");
        assert!(Operation::<u64, ()>::decode(instruction, context).is_some(), "{op:?} must decode to a staged operation");
    }
    for index in 0..Op::COUNT {
        let op = Op::from_index(index).unwrap();
        let instruction = WideInstruction::new(op, 1, 1, 1, 0);
        let decoded = Operation::<u64, ()>::decode(instruction, context).is_some();
        assert!(op.staged().is_some() || !decoded, "{op:?} decodes though the catalog names no decoder");
        if op.staged() == Some(Decoder::Proved) {
            assert!(!decoded && Operation::<u64, ()>::decode_load_name(instruction, ()).is_some(), "{op:?}");
        }
    }
}

/// A window record carries its register count above its slot (`window_base`); every local access
/// decodes the slot below it (`window_slot`). The plain load and store read the whole immediate as
/// the slot, so in a function with a window plan each took the general step.
#[test]
fn a_window_records_local_access_decodes_its_slot() {
    use crate::bytecode::{Op, WideInstruction};
    let imm = 3 | 40 << u16::BITS;
    let slot = |operation| match operation {
        Some(Operation::LoadLocalPlain { slot, .. } | Operation::StoreLocalPlain { slot, .. } | Operation::LoadLocalChecked { slot, .. }
            | Operation::StoreLocalChecked { slot, .. } | Operation::InitializeLocal { slot }) => slot,
        _ => panic!("a local access decodes"),
    };
    for op in [Op::LoadLocalPlain, Op::StoreLocalPlain, Op::LoadLocal, Op::StoreLocal, Op::InitializeTdz] {
        assert_eq!(slot(Operation::<u64, ()>::decode_local(WideInstruction::new(op, 1, 0, 0, imm))), 3, "{op:?}");
    }
}

/// `await`, `yield` and `yield*` decode to their suspension rows, with the registers the general
/// step reads (`b` the value, `c` the delegate's iterator, the immediate's pair its state and `next`
/// method) and resumes into (`a`), and a frame that answers none of the call-outs leaves the row to
/// the interpreter.
#[test]
fn suspensions_decode_to_rows_a_plain_frame_leaves_unresolved() {
    use crate::bytecode::{Op, WideInstruction};
    let context = crate::staged::DecodeContext::of_constants(&[]);
    let pair = crate::bytecode::ImmediateLayout::register_pair_immediate(4, 5);
    let delegation = Suspension::YieldStar { iterator: 3, state: 4, next: 5 };
    for (op, expected) in [(Op::Await, Suspension::Await), (Op::Yield, Suspension::Yield { conversion: crate::staged::YieldConversion::Resolve }), (Op::YieldStar, delegation)] {
        let imm = if op == Op::YieldStar { pair } else { 0 };
        let row = Operation::<u64, ()>::decode(WideInstruction::new(op, 1, 2, 3, imm), context);
        let Some(Operation::Suspend { kind, destination: 1, value: 2 }) = row else { panic!("{op:?} decodes to a suspension row") };
        assert_eq!(kind, expected, "{op:?}");
        if let Suspension::YieldStar { iterator, state, next } = kind {
            assert_eq!((iterator, state, next), (3, 4, 5), "the delegate's iterator is `c`, its state and method the pair");
        }
        let mut stage = Concrete::pure();
        let mut frame = ConcreteFrame { registers: vec![0; 6], heap: None };
        assert!(matches!(step(&mut stage, &mut frame, Operation::Suspend { kind, destination: 1, value: 2 }), Step::Unresolved), "{op:?}");
    }
}

