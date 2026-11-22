//! Wasm exception operations as one semantics: a catch's tag test, the payload read at an index,
//! and `throw_ref`, with their checks, their order, their traps and their rejections, written once
//! over `WasmException`. The interpreter runs them on the VM's heap (`ExceptionStage`,
//! `vm/wasm_exception.rs`), and the u-jit's callouts run the same rules. Building an exception and
//! reading a tag's declarations stay with the VM (`Vm::wasm_exception_new`).

use super::machine::MachineStage;
use super::wasm::Width;
use crate::WasmTrap;
use crate::bytecode::Op;
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};

/// The exception operations a Wasm function's instruction can be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exception {
    /// `exception.new`: a tag and its payload, from a window of registers, the tag first.
    New,
    /// A catch's test of an exception against a tag.
    Match,
    /// The payload field of an exception at an index, which a catch reads.
    Payload,
    /// `throw_ref`: the exception a Wasm function raises.
    ThrowRef,
}

impl Exception {
    /// The exception operation `op` is, if it is one.
    pub const fn of(op: Op) -> Option<Self> {
        Some(match op {
            Op::WasmExceptionNew => Self::New,
            Op::WasmExceptionMatch => Self::Match,
            Op::WasmExceptionPayload => Self::Payload,
            Op::WasmThrowRef => Self::ThrowRef,
            _ => return None,
        })
    }
}

/// The type a payload read carries in its instruction's `c` field: a payload's Wasm type. A numeric
/// type is decided by its code alone; a reference's word is its Value's bits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayloadType {
    I32,
    I64,
    F32,
    F64,
    Reference,
}

impl PayloadType {
    /// Every payload type, in code order.
    const ALL: [Self; 5] = [Self::I32, Self::I64, Self::F32, Self::F64, Self::Reference];

    /// The code of the type, as the instruction's field carries it.
    pub(crate) const fn code(self) -> u16 {
        self as u16
    }

    /// The type a field's code names.
    pub fn of_code(code: u16) -> Option<Self> {
        Self::ALL.get(usize::from(code)).copied()
    }

    /// The payload type of a Wasm value type, or `None` for a vector, which a payload never holds.
    pub(crate) fn of(ty: crate::WasmType) -> Option<Self> {
        Some(match ty {
            crate::WasmType::I32 => Self::I32,
            crate::WasmType::I64 => Self::I64,
            crate::WasmType::F32 => Self::F32,
            crate::WasmType::F64 => Self::F64,
            crate::WasmType::Reference { .. } => Self::Reference,
            crate::WasmType::V128 => return None,
        })
    }

    /// The numeric Wasm type of a numeric payload; `None` for a reference.
    pub(crate) fn numeric(self) -> Option<crate::WasmType> {
        Some(match self {
            Self::I32 => crate::WasmType::I32,
            Self::I64 => crate::WasmType::I64,
            Self::F32 => crate::WasmType::F32,
            Self::F64 => crate::WasmType::F64,
            Self::Reference => return None,
        })
    }
}

/// The primitives of an exception heap the rules run on. A reference is its bits, a tag or an
/// exception is a reference, and a flag is 0 or 1. The rules hold every check, its order and its
/// trap or rejection; a stage answers the primitives only.
pub(crate) trait WasmException: MachineStage {
    /// The flag that `value` is a tag.
    fn is_tag(&mut self, value: Self::Word) -> Self::Word;
    /// The flag that `value` is null.
    fn is_null(&mut self, value: Self::Word) -> Self::Word;
    /// The flag that `value` is a Wasm exception: a cell of a tag and its payload. A JavaScript
    /// exception a Wasm handler caught by reference is not one.
    fn is_wasm_exception(&mut self, value: Self::Word) -> Self::Word;
    /// The tag of the Wasm exception `value`; 0 where `value` is no Wasm exception.
    fn tag_of(&mut self, value: Self::Word) -> Self::Word;
    /// The `WebAssembly.JSTag` of the realm, whose one payload is the thrown JavaScript value.
    fn js_tag(&mut self) -> Self::Word;
    /// The flag that payload `index` is present in `exception`. A JavaScript exception has one
    /// payload, the thrown value itself, at index 0.
    fn payload_present(&mut self, exception: Self::Word, index: u32) -> Self::Word;
    /// The payload word at `index` of `exception`, where present; undefined where it is absent, as
    /// `throw_ref` reads a `WebAssembly.JSTag` exception's value.
    fn payload_word(&mut self, exception: Self::Word, index: u32) -> Self::Word;
    /// Continues where `flag` is 0; where it is 1, rejects with `reason`, a validation failure.
    fn reject_when(&mut self, flag: Self::Word, reason: &'static str) -> Result<(), Self::Exit>;
    /// The exit that raises `value`: a Wasm exception where `wasm` is 1, else a JavaScript throw of
    /// `value`.
    fn raise(&mut self, value: Self::Word, wasm: Self::Word) -> Self::Exit;
}

/// The flag of the negation of `flag`.
#[inline(always)]
fn negated<S: MachineStage>(stage: &mut S, flag: S::Word) -> S::Word {
    stage.operate_unary(Unary::EqualZero, Width::I32, flag)
}

/// `new`'s check of its tag operand: it must name a tag.
pub(crate) fn new_tag<S: WasmException>(stage: &mut S, tag: S::Word) -> Result<(), S::Exit> {
    let is_tag = stage.is_tag(tag);
    let invalid = negated(stage, is_tag);
    stage.reject_when(invalid, "invalid Wasm exception tag")
}

/// `match`: whether `exception` is caught by the catch tag `expected`. A Wasm exception is caught
/// by its own tag; a JavaScript exception is caught by `WebAssembly.JSTag` alone. The expected tag
/// must be a tag.
pub(crate) fn matches<S: WasmException>(stage: &mut S, exception: S::Word, expected: S::Word) -> Result<S::Word, S::Exit> {
    let is_tag = stage.is_tag(expected);
    let invalid = negated(stage, is_tag);
    stage.reject_when(invalid, "invalid Wasm catch tag")?;
    let wasm = stage.is_wasm_exception(exception);
    let own = stage.tag_of(exception);
    let own = stage.operate(Binary::Equal, Width::I64, own, expected);
    let js_tag = stage.js_tag();
    let js = stage.operate(Binary::Equal, Width::I64, js_tag, expected);
    Ok(stage.select(wasm, own, js))
}

/// `payload`: the word of `exception` at `index`. An index past the exception's payload is a
/// validation failure of the module.
pub fn payload<S: WasmException>(stage: &mut S, exception: S::Word, index: u32) -> Result<S::Word, S::Exit> {
    let present = stage.payload_present(exception, index);
    let absent = negated(stage, present);
    stage.reject_when(absent, "Wasm exception payload index out of bounds")?;
    Ok(stage.payload_word(exception, index))
}

/// `throw_ref`: the exit `exception` raises. A null reference traps. An exception of
/// `WebAssembly.JSTag` throws its payload, which is the JavaScript value; any other Wasm exception
/// stays one; a JavaScript exception rethrows as itself.
pub(crate) fn throw_ref<S: WasmException>(stage: &mut S, exception: S::Word) -> S::Exit {
    let null = stage.is_null(exception);
    if let Err(exit) = stage.trap_when(null, WasmTrap::NullExceptionReference) {
        return exit;
    }
    let wasm = stage.is_wasm_exception(exception);
    let tag = stage.tag_of(exception);
    let js_tag = stage.js_tag();
    let is_js_tag = stage.operate(Binary::Equal, Width::I64, tag, js_tag);
    let js_exception = stage.operate(Binary::And, Width::I64, wasm, is_js_tag);
    let payload = stage.payload_word(exception, 0);
    let value = stage.select(js_exception, payload, exception);
    let not_js = negated(stage, js_exception);
    let wasm_thrown = stage.operate(Binary::And, Width::I64, wasm, not_js);
    stage.raise(value, wasm_thrown)
}
