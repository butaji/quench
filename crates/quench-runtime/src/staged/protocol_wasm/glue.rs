//! Runtime glue of `protocol_wasm`: the decode of a host step from its record, the operand registers
//! `Vm::call_out` reads, and the rebuild of a step's instruction (the runtime's opcodes and
//! instructions cannot move with the moved types).
#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Op, Traits, WideInstruction};
use crate::staged::Protocol;

/// A host step's fields, taken from an instruction and rebuilt into one.
pub trait WasmHostFieldsProtocolWasmExt: Sized {
    /// The fields of `instruction`.
    fn of(instruction: WideInstruction) -> Self;
    /// The instruction these fields are.
    fn instruction(self) -> WideInstruction;
}

impl WasmHostFieldsProtocolWasmExt for WasmHostFields {
    #[inline(always)]
    fn of(instruction: WideInstruction) -> Self {
        Self { op: instruction.op() as u16, a: instruction.a(), b: instruction.b(), c: instruction.c(), imm: instruction.imm() }
    }

    #[inline(always)]
    fn instruction(self) -> WideInstruction {
        let op = Op::from_index(usize::from(self.op)).expect("a host step's opcode is a catalog opcode");
        WideInstruction::new(op, self.a, self.b, self.c, self.imm)
    }
}

/// The host step an instruction denotes among this family's opcodes.
pub trait WasmHostStepProtocolWasmExt: Sized {
    /// The step `instruction` denotes among these opcodes; `None` for any other.
    fn of(instruction: WideInstruction) -> Option<Self>;
}

/// Implements `WasmHostStepProtocolWasmExt` over the same opcode list as `host_steps!`.
macro_rules! host_of {
    ($($op:ident),* $(,)?) => {
        impl WasmHostStepProtocolWasmExt for WasmHostStep {
            #[inline(always)]
            fn of(instruction: WideInstruction) -> Option<Self> {
                let fields = WasmHostFields::of(instruction);
                Some(match instruction.op() {
                    $(Op::$op => Self::$op(fields),)*
                    _ => return None,
                })
            }
        }
    };
}

host_of! {
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

/// The registers `Vm::call_out` reads for `instruction`'s (at most two) operands: the first the
/// tier reads as the first operand, the second as the second. Every other register the helper
/// reads, it reads from the running frame.
#[inline(always)]
pub fn operands(instruction: WideInstruction) -> [Option<Register>; 2] {
    let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
    match instruction.op() {
        Op::WasmArrayCopy | Op::WasmArrayFill | Op::WasmArrayInitData | Op::WasmArrayInitElem
        | Op::WasmArraySet | Op::WasmStructSet | Op::WasmArraySetRaw | Op::WasmStructSetRaw | Op::WasmTableFill | Op::WasmTableCopy
        | Op::WasmTableInit | Op::WasmMemoryInit => [Some(a), Some(b)],
        Op::WasmArrayGet | Op::WasmArrayGetRaw | Op::WasmArrayGetS | Op::WasmArrayGetU | Op::WasmArrayNew
        | Op::WasmDescriptorCast | Op::WasmDescriptorTest | Op::WasmExceptionMatch | Op::WasmRefEq
        | Op::WasmTableGet | Op::WasmTableSet | Op::WasmTableGrow | Op::WasmIndirectTarget => [Some(b), Some(c)],
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
#[inline(always)]
const fn defines_nothing(op: Op) -> bool {
    op.traits().has(Traits::DEFINES_NOTHING)
}

/// The step `instruction` denotes among these opcodes, with the register it defines and its (at
/// most two) operand registers, which `Vm::call_out` reads; `None` for any other opcode.
#[inline(always)]
pub fn decode(instruction: WideInstruction) -> Option<(Protocol, Option<Register>, [Option<Register>; 2])> {
    let step = WasmHostStep::of(instruction)?;
    let destination = (!defines_nothing(instruction.op())).then(|| instruction.result_register());
    Some((Protocol::Wasm(step), destination, operands(instruction)))
}

/// The Wasm instruction a step makes, rebuilt from the step's fields, for a tier whose host
/// runs it over its own registers.
pub trait ProtocolProtocolWasmExt: Sized {
    /// The Wasm instruction this step makes, rebuilt from the step's fields, for a tier whose host
    /// runs it over its own registers (a GC value step, `Vm::wasm_gc_value_step`); `None` for a
    /// step that is no Wasm host step.
    fn wasm_instruction(self) -> Option<WideInstruction>;
}

impl ProtocolProtocolWasmExt for Protocol {
    #[inline(always)]
    fn wasm_instruction(self) -> Option<WideInstruction> {
        match self {
            Protocol::Wasm(step) => Some(step.fields().instruction()),
            _ => None,
        }
    }
}
