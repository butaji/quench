//! The loop tier covers every opcode: natively, or by leaving to the interpreter's definition.
use super::{admit_region, lowered_natively};
use crate::bytecode::{Op, WideInstruction};

#[test]
fn osr_covers_every_opcode_natively_or_by_its_interpreter_definition() {
    // The tier has no whitelist: an instruction it does not lower leaves at its PC and
    // the interpreter runs it, so no opcode rejects a loop whose iteration can bypass it.
    for opcode in 0..Op::COUNT as u16 {
        // SAFETY: `Op` is a contiguous `repr(u16)` enum of `Op::COUNT` variants.
        let op = unsafe { std::mem::transmute::<u16, Op>(opcode) };
        if lowered_natively(op) {
            continue;
        }
        let region = admit_region(
            vec![
                WideInstruction::new(Op::JumpFalse, 0, 0, 0, 2),
                WideInstruction::new(op, 1, 2, 3, 0),
                WideInstruction::new(Op::Jump, 0, 0, 0, 0),
            ],
            4, &[], &[], None, None, 0, &Default::default(),
        );
        assert!(region.is_ok(), "{op:?} rejects its loop: {region:?}", region = region.err());
    }
    // A loop that leaves at its header runs no native code.
    let always_exits = admit_region(
        vec![WideInstruction::new(Op::Construct, 1, 2, 3, 0), WideInstruction::new(Op::Jump, 0, 0, 0, 0)],
        4, &[], &[], None, None, 0, &Default::default(),
    );
    assert!(always_exits.is_err());
}
