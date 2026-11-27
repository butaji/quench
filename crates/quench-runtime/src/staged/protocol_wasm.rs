//! The Wasm GC, reference, table, memory-init, atomic and exception opcodes as staged call-out rows
//! (`Operation::Protocol`): each is one definition every tier runs, and its step is the interpreter's
//! own helper (`vm/dispatch_wasm`), which the host makes (`Vm::call_out` -> `Vm::wasm_host_step`).
//! The decode of a step from its record, and the rebuild of the instruction, are in `protocol_wasm/glue.rs`.
use crate::bytecode::Register;

/// The fields of the instruction a host step runs: its opcode's index (`Op as u16`), its three
/// register fields and its immediate. The host rebuilds the instruction from them, so its helper
/// reads what the interpreter's handler reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WasmHostFields {
    pub op: u16,
    pub a: Register,
    pub b: Register,
    pub c: Register,
    pub imm: u32,
}

macro_rules! host_steps {
    ($($op:ident),* $(,)?) => {
        /// A Wasm heap, table, memory, atomic or exception step the host makes by the interpreter's
        /// own definition, one variant per opcode, carrying the instruction's fields.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum WasmHostStep {
            $($op(WasmHostFields),)*
        }

        impl WasmHostStep {
            /// The fields of the step.
            #[inline(always)]
            pub fn fields(self) -> WasmHostFields {
                match self {
                    $(Self::$op(fields) => fields,)*
                }
            }
        }
    };
}

// The opcodes of this family. `protocol_wasm/glue.rs` lists the same names, which map each to its
// variant when it decodes an instruction; the two lists change together.
host_steps! {
    WasmArrayCopy, WasmArrayFill, WasmArrayGet, WasmArrayGetS, WasmArrayGetU,
    WasmArrayInitData, WasmArrayInitElem, WasmArrayLen, WasmArrayNew, WasmArrayNewData,
    WasmArrayNewDefault, WasmArrayNewElem, WasmArrayNewFixed, WasmArraySet, WasmArrayGetRaw, WasmArraySetRaw,
    WasmAtomicAccess, WasmAtomicFence,
    WasmDescriptorCast, WasmDescriptorTest,
    WasmExceptionMatch, WasmExceptionNew, WasmExceptionPayload,
    WasmExternalConversion, WasmI31, WasmIndirectTarget,
    WasmMemoryInit,
    WasmRefAsNonNull, WasmRefCast, WasmRefEq, WasmRefFunc, WasmRefGetDesc, WasmRefIsNull, WasmRefTest,
    WasmStructGet, WasmStructGetS, WasmStructGetU, WasmStructNew, WasmStructNewDefault,
    WasmStructNewDefaultDesc, WasmStructNewDesc, WasmStructSet, WasmStructGetRaw, WasmStructSetRaw,
    WasmTableCopy, WasmTableFill, WasmTableGet, WasmTableGrow, WasmTableInit, WasmTableSet,
    WasmTableSize, WasmThrowRef,
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
