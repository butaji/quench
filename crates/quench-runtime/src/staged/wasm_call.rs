//! Calls of a Wasm function as data, and the check of an indirect call's target written once
//! over `WasmTable`: the element exists, it is initialized, its signature is the expected one,
//! in that order, from the signature id the element carries. The interpreter runs it on
//! `ConcreteTable`; the Wasm tier emits it over the table's mirror (`Emitter`).

use super::wasm::{WasmStage, Width};
use crate::WasmTrap;
use crate::bytecode::{Class, Op, WideInstruction};
use crate::wasm::CanonicalSignature;
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

/// A table of function references and the canonical signature of each function
/// (`wasm::CanonicalSignature`), which an element carries beside its reference (`TableTarget`).
pub(crate) trait WasmTable: WasmStage {
    /// The flag that `extent` elements from `index` pass the table's end: the shared range check
    /// (`wasm::memory::checked_range`) of tables, memories and bulk operations.
    fn table_range_exceeds(&mut self, index: Self::Word, extent: Self::Word) -> Self::Word;
    /// The signature word of the element at `index`, which the rule has checked.
    fn element_signature(&mut self, index: Self::Word) -> Self::Word;
    /// The reference at `index`, which the rule has checked: a stage may answer any word the call
    /// that follows resolves (the tier's is the element's row).
    fn table_element(&mut self, index: Self::Word) -> Self::Word;
    /// The flag that `reference`, whose element holds the signature word `signature`, satisfies
    /// the `expected` signature word: equal words do, and any other is the machine's to relate
    /// (a subtype, a host function).
    fn signature_accepts(
        &mut self,
        reference: Self::Word,
        signature: Self::Word,
        expected: Self::Word,
    ) -> Self::Word;
}

/// The reference at `index` of the table, which an indirect call of the `expected` signature
/// calls: an index past the table traps, then a null element, then a function of another
/// signature. The one place these checks and their order are written.
#[inline(always)]
pub(crate) fn indirect_target<S: WasmTable>(
    stage: &mut S,
    index: S::Word,
    expected: CanonicalSignature,
) -> Result<S::Word, S::Exit> {
    let one = stage.constant(Width::I64, 1);
    let outside = stage.table_range_exceeds(index, one);
    stage.trap_when(outside, WasmTrap::UndefinedElement)?;
    let signature = stage.element_signature(index);
    let null = stage.constant(Width::I64, CanonicalSignature::NULL.word() as i64);
    let uninitialized = stage.operate(Binary::Equal, Width::I64, signature, null);
    stage.trap_when(uninitialized, WasmTrap::UninitializedElement)?;
    let reference = stage.table_element(index);
    let expected = stage.constant(Width::I64, expected.word() as i64);
    let accepted = stage.signature_accepts(reference, signature, expected);
    let mismatch = stage.operate_unary(Unary::EqualZero, Width::I32, accepted);
    stage.trap_when(mismatch, WasmTrap::IndirectCallTypeMismatch)?;
    Ok(reference)
}
