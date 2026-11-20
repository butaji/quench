//! The Wasm GC, reference, table, memory-init, atomic and exception opcodes as staged call-out
//! rows (`Operation::Protocol`): each is one definition every tier runs, and its step is the
//! interpreter's own helper (`vm/dispatch_wasm`), which the host makes (`Vm::call_out` ->
//! `Vm::wasm_host_step`).
use super::Protocol;
use crate::bytecode::{Op, Register, WideInstruction};

/// The fields of the instruction a host step runs: its opcode, its three register fields and its
/// immediate. The host rebuilds the instruction from them, so its helper reads what the
/// interpreter's handler reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct WasmHostFields {
    op: Op,
    a: Register,
    b: Register,
    c: Register,
    imm: u32,
}

impl WasmHostFields {
    fn of(instruction: WideInstruction) -> Self {
        Self { op: instruction.op(), a: instruction.a(), b: instruction.b(), c: instruction.c(), imm: instruction.imm() }
    }

    /// The instruction these fields are.
    pub(crate) fn instruction(self) -> WideInstruction {
        WideInstruction::new(self.op, self.a, self.b, self.c, self.imm)
    }
}

macro_rules! host_steps {
    ($($op:ident),* $(,)?) => {
        /// A Wasm heap, table, memory, atomic or exception step the host makes by the interpreter's
        /// own definition, one variant per opcode, carrying the instruction's fields.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub(crate) enum WasmHostStep {
            $($op(WasmHostFields),)*
        }

        impl WasmHostStep {
            /// The step `instruction` denotes among these opcodes; `None` for any other.
            fn of(instruction: WideInstruction) -> Option<Self> {
                let fields = WasmHostFields::of(instruction);
                Some(match instruction.op() {
                    $(Op::$op => Self::$op(fields),)*
                    _ => return None,
                })
            }

            /// The fields of the step.
            pub(crate) fn fields(self) -> WasmHostFields {
                match self {
                    $(Self::$op(fields) => fields,)*
                }
            }
        }
    };
}

host_steps! {
    WasmArrayCopy, WasmArrayFill, WasmArrayGet, WasmArrayGetS, WasmArrayGetU,
    WasmArrayInitData, WasmArrayInitElem, WasmArrayLen, WasmArrayNew, WasmArrayNewData,
    WasmArrayNewDefault, WasmArrayNewElem, WasmArrayNewFixed, WasmArraySet, WasmArrayGetRaw, WasmArraySetRaw,
    WasmAtomicAccess, WasmAtomicFence,
    WasmDescriptorCast, WasmDescriptorTest,
    WasmExceptionMatch, WasmExceptionNew, WasmExceptionPayload,
    WasmExternalConversion, WasmI31,
    WasmMemoryInit,
    WasmRefAsNonNull, WasmRefCast, WasmRefEq, WasmRefGetDesc, WasmRefIsNull, WasmRefTest,
    WasmStructGet, WasmStructGetS, WasmStructGetU, WasmStructNew, WasmStructNewDefault,
    WasmStructNewDefaultDesc, WasmStructNewDesc, WasmStructSet, WasmStructGetRaw, WasmStructSetRaw,
    WasmTableCopy, WasmTableFill, WasmTableGet, WasmTableGrow, WasmTableInit, WasmTableSet,
    WasmTableSize, WasmThrowRef,
}

/// The registers `Vm::call_out` reads for `instruction`'s (at most two) operands: the first the
/// tier reads as the first operand, the second as the second. Every other register the helper
/// reads, it reads from the running frame.
pub(crate) fn operands(instruction: WideInstruction) -> [Option<Register>; 2] {
    let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
    match instruction.op() {
        Op::WasmArrayCopy | Op::WasmArrayFill | Op::WasmArrayInitData | Op::WasmArrayInitElem
        | Op::WasmArraySet | Op::WasmStructSet | Op::WasmArraySetRaw | Op::WasmStructSetRaw | Op::WasmTableFill | Op::WasmTableCopy
        | Op::WasmTableInit | Op::WasmMemoryInit => [Some(a), Some(b)],
        Op::WasmArrayGet | Op::WasmArrayGetRaw | Op::WasmArrayGetS | Op::WasmArrayGetU | Op::WasmArrayNew
        | Op::WasmDescriptorCast | Op::WasmDescriptorTest | Op::WasmExceptionMatch | Op::WasmRefEq
        | Op::WasmTableGet | Op::WasmTableSet | Op::WasmTableGrow => [Some(b), Some(c)],
        Op::WasmThrowRef => [Some(a), None],
        Op::WasmArrayLen | Op::WasmArrayNewDefault | Op::WasmExceptionPayload | Op::WasmExternalConversion
        | Op::WasmI31 | Op::WasmRefAsNonNull | Op::WasmRefCast | Op::WasmRefGetDesc | Op::WasmRefIsNull
        | Op::WasmRefTest | Op::WasmStructGet | Op::WasmStructGetRaw | Op::WasmStructGetS | Op::WasmStructGetU
        | Op::WasmTableSize => [Some(b), None],
        _ => [None, None],
    }
}

/// Whether `instruction`'s opcode defines no register: a write, a fill, a copy or an
/// initialization, which the interpreter's handler writes nothing for.
fn defines_nothing(op: Op) -> bool {
    matches!(
        op,
        Op::WasmArrayCopy | Op::WasmArrayFill | Op::WasmArrayInitData | Op::WasmArrayInitElem
            | Op::WasmArraySet | Op::WasmStructSet | Op::WasmArraySetRaw | Op::WasmStructSetRaw | Op::WasmTableSet | Op::WasmTableFill
            | Op::WasmTableCopy | Op::WasmTableInit | Op::WasmMemoryInit | Op::WasmAtomicFence
            | Op::WasmThrowRef
    )
}

#[cfg(not(stencil_build))]
/// The step `instruction` denotes among these opcodes, with the register it defines and its (at
/// most two) operand registers, which `Vm::call_out` reads; `None` for any other opcode.
#[inline(always)]
pub(super) fn decode(instruction: WideInstruction) -> Option<(Protocol, Option<Register>, [Option<Register>; 2])> {
    let step = WasmHostStep::of(instruction)?;
    let destination = (!defines_nothing(instruction.op())).then(|| instruction.result_register());
    Some((Protocol::Wasm(step), destination, operands(instruction)))
}
