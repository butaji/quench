//! The opcode round trip (a merge gate, `tools/handoff-gate.sh` G4): every opcode's catalog row and its decode agree.
//! An integration test, since the core's own test binary cannot link the engine's layer entries.

#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The catalog's staged column and `Operation::decode` agree: every opcode that always decodes
/// has a decoder, one decoded by a tier's proof (`Decoder::Proved`) decodes through
/// `decode_load_name`, and no opcode without a decoder decodes at all.
#[test]
fn every_staged_opcode_decodes_to_one_operation() {
    use quench_runtime::bytecode::{Decoder, Op, WideInstruction};
    use quench_runtime::staged::Operation;
    use quench_runtime::Value;
    use quench_runtime::staged::Constant;
    let constants = [Constant::untyped(Value::number(1.0))];
    let context = quench_runtime::staged::DecodeContext::of_constants(&constants);
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

