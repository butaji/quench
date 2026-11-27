//! Runtime glue of `suspend`: the decode of a suspension row from an instruction, and the opcode
//! map `suspend_row` (both name the runtime's opcodes, which the moved types cannot).
#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Op, WideInstruction};
use crate::staged::Operation;

/// The suspension row of `op` (`SuspendRow`); `None` for an opcode that suspends nothing.
#[inline(always)]
pub const fn suspend_row(op: Op) -> Option<SuspendRow> {
    match op {
        Op::Await => Some(SuspendRow::Await),
        Op::Yield => Some(SuspendRow::Yield),
        Op::YieldStar => Some(SuspendRow::YieldStar),
        _ => None,
    }
}

/// The decode of an `Operation`'s suspension row from an instruction.
pub trait OperationSuspendExt<W, P>: Sized {
    /// The suspension row of `instruction` (`decode_flow`); `None` for any other opcode.
    fn decode_suspend(instruction: WideInstruction) -> Option<Self>;
}

impl<W, P> OperationSuspendExt<W, P> for Operation<W, P> {
    fn decode_suspend(instruction: WideInstruction) -> Option<Self> {
        let kind = match suspend_row(instruction.op())? {
            SuspendRow::Await => Suspension::Await,
            SuspendRow::Yield => Suspension::Yield { conversion: YieldConversion::Resolve },
            SuspendRow::YieldStar => {
                let (state, next) = instruction.register_pair();
                Suspension::YieldStar { iterator: instruction.register_c(), state, next }
            }
        };
        Some(Self::Suspend { kind, destination: instruction.result_register(), value: instruction.register_b() })
    }
}
