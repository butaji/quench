//! Calls of a Wasm function as data, and the check of an indirect call's target written once
//! over `WasmTable`: the element exists, it is initialized, its signature is the expected one,
//! in that order. The interpreter runs it on `ConcreteTable`; the Wasm tier keeps the index in
//! native code and leaves the check to the interpreter's callout, the same rule.

use super::wasm::{WasmStage, Width};
use crate::WasmTrap;
use crate::bytecode::{Class, Op, WideInstruction};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Call {
    /// `result = function(window)`: a function the program defines.
    Known,
    /// `a = table[c]`, checked against the signature `imm`: the reference of an indirect call.
    IndirectTarget,
    /// `a = reference to function imm`.
    RefFunc,
    /// `result = b(window)`: a call of a reference an earlier step produced.
    Reference,
}

impl Call {
    pub(crate) fn of(op: Op) -> Option<Self> {
        match op.class() {
            Class::CallKnown => Some(Self::Known),
            Class::WasmIndirectTarget => Some(Self::IndirectTarget),
            Class::WasmRefFunc => Some(Self::RefFunc),
            Class::Call => Some(Self::Reference),
            _ => None,
        }
    }

    /// The function a call names statically.
    pub(crate) fn callee(self, instruction: WideInstruction) -> Option<usize> {
        (self == Self::Known).then(|| usize::from(instruction.known_function_index()))
    }

    /// Whether the call reads a table's references: it needs the program's callouts.
    pub(crate) const fn needs_callouts(self) -> bool {
        matches!(self, Self::IndirectTarget | Self::RefFunc)
    }
}

/// A table of function references and the signature of each.
pub(crate) trait WasmTable: WasmStage {
    /// The table's element count: an `I64` word.
    fn table_length(&mut self) -> Self::Word;
    /// The reference at `index`, which the rule has checked.
    fn table_element(&mut self, index: Self::Word) -> Self::Word;
    /// The flag that `reference` is the null reference.
    fn reference_is_null(&mut self, reference: Self::Word) -> Self::Word;
    /// The flag that `reference`'s function has the signature type `signature`.
    fn reference_matches(&mut self, reference: Self::Word, signature: u32) -> Self::Word;
}

/// The reference at `index` of the table, which an indirect call of `signature` calls: an
/// index past the table traps, then a null element, then a function of another signature.
/// The one place these checks and their order are written.
#[inline(always)]
pub(crate) fn indirect_target<S: WasmTable>(
    stage: &mut S,
    index: S::Word,
    signature: u32,
) -> Result<S::Word, S::Exit> {
    let length = stage.table_length();
    let outside = stage.operate(Binary::GreaterEqualUnsigned, Width::I64, index, length);
    stage.trap_when(outside, WasmTrap::UndefinedElement)?;
    let reference = stage.table_element(index);
    let null = stage.reference_is_null(reference);
    stage.trap_when(null, WasmTrap::UninitializedElement)?;
    let matches = stage.reference_matches(reference, signature);
    let mismatch = stage.operate_unary(Unary::EqualZero, Width::I32, matches);
    stage.trap_when(mismatch, WasmTrap::IndirectCallTypeMismatch)?;
    Ok(reference)
}
