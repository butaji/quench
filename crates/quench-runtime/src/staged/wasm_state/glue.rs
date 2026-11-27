//! Runtime glue of `wasm_state`: the decode of an instance-state operation from its record. The
//! record's fields and the fill's constant index come from the runtime's `Fields` and
//! `DecodeContext`, so the decode cannot move with the operation.
#![allow(unused_imports)]
use super::*;
use crate::bytecode::Op;
use crate::staged::{DecodeContext, Fields, Operation};

/// The decode of a `WasmStateOperation` from a record.
pub trait WasmStateOperationWasmStateExt: Sized {
    /// The operation `instruction` denotes, from its record's fields; a fill's constant is the
    /// record's constant index, read through `context` (or the lane's own constant).
    fn decode<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self>;
}

impl WasmStateOperationWasmStateExt for WasmStateOperation {
    fn decode<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        Some(match instruction.op() {
            Op::WasmGlobalGet => Self::GlobalGet {
                destination: instruction.result_register(),
                binding: instruction.register_b(),
            },
            Op::WasmGlobalSet => Self::GlobalSet {
                source: instruction.register_a(),
                binding: instruction.register_b(),
            },
            Op::WasmInstanceBinding => Self::InstanceBinding {
                destination: instruction.result_register(),
                slot: instruction.wide().imm(),
            },
            Op::WasmFillRegisters => {
                let window = instruction.wide().register_window();
                Self::Fill {
                    constant: instruction.constant(context)?,
                    base: window.base,
                    count: window.count,
                }
            }
            _ => return None,
        })
    }
}

/// The decode of an `Operation`'s instance-state row from a record.
pub trait OperationWasmStateExt<W, P>: Sized {
    /// The instance-state operation `instruction` denotes (`Decoder::WasmState`).
    fn decode_wasm_state<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self>;
}

impl<W, P> OperationWasmStateExt<W, P> for Operation<W, P> {
    fn decode_wasm_state<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        WasmStateOperation::decode(instruction, context).map(Self::WasmState)
    }
}
