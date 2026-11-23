//! Calls of a Wasm function as data, and the check of an indirect call's target written once
//! over `WasmTable`: the element exists, it is initialized, its signature is the expected one,
//! in that order, from the signature id the element carries. The interpreter runs it on
//! `ConcreteTable`; the Wasm tier emits it over the table's mirror (`Emitter`).

use super::wasm::{WasmStage, Width};
use crate::WasmTrap;
#[cfg(not(stencil_build))]
use crate::bytecode::{Class, Op, WideInstruction};
#[cfg(not(stencil_build))]
use crate::wasm::CanonicalSignature;
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Call {
    /// `result = function(window)`: a function the program defines.
    Known,
    /// `a = table[c]`, checked against the signature `imm`: the reference of an indirect call.
    IndirectTarget,
    /// `a = reference to function imm`.
    RefFunc,
    /// `result = b(window)`: a call of a reference an earlier step produced.
    Reference,
}

#[cfg(not(stencil_build))]
impl Call {
    pub const fn of(op: Op) -> Option<Self> {
        match op.class() {
            Class::CallKnown => Some(Self::Known),
            Class::WasmIndirectTarget => Some(Self::IndirectTarget),
            Class::WasmRefFunc => Some(Self::RefFunc),
            Class::Call => Some(Self::Reference),
            _ => None,
        }
    }

    /// The one opcode of an instance read (`IndirectTarget`, `RefFunc`), for a tier that rebuilds its
    /// instruction from a record; `None` for a call, which several opcodes make.
    pub const fn instance_read_op(self) -> Option<Op> {
        match self {
            Self::IndirectTarget => Some(Op::WasmIndirectTarget),
            Self::RefFunc => Some(Op::WasmRefFunc),
            Self::Known | Self::Reference => None,
        }
    }

    /// The function a call names statically.
    pub(crate) fn callee(self, instruction: WideInstruction) -> Option<usize> {
        (self == Self::Known).then(|| usize::from(instruction.known_function_index()))
    }
}

impl Call {
    /// Whether the call reads a table's references: it needs the program's callouts.
    pub const fn needs_callouts(self) -> bool {
        matches!(self, Self::IndirectTarget | Self::RefFunc)
    }
}

/// A table of function references and the canonical signature of each function
/// (`wasm::CanonicalSignature`), which an element carries beside its reference (`TableTarget`).
pub trait WasmTable: WasmStage {
    /// The flag that `extent` elements from `index` pass the table's end: the shared range check
    /// (`wasm::memory::checked_range`) of tables, memories and bulk operations.
    fn table_range_exceeds(&mut self, index: Self::Word, extent: Self::Word) -> Self::Word;
    /// The signature word of the element at `index`, which the rule has checked.
    fn element_signature(&mut self, index: Self::Word) -> Self::Word;
    /// The reference at `index`, which the rule has checked: a stage may answer any word the call
    /// that follows resolves (the tier's is the element's row).
    fn table_element(&mut self, index: Self::Word) -> Self::Word;
    /// The element row at `index`, which the rule has checked: the row the call that follows
    /// reads its target from (the interpreter's `TableTarget`, the tier's row word).
    type Row: Copy;
    fn element_row(&mut self, index: Self::Word) -> Self::Row;
    /// The flag that `reference`, whose element holds the signature word `signature`, satisfies
    /// the `expected` signature word: equal words do, and any other is the machine's to relate
    /// (a subtype, a host function).
    fn signature_accepts(
        &mut self,
        reference: Self::Word,
        signature: Self::Word,
        expected: Self::Word,
    ) -> Self::Word;
    /// The flag of `otherwise` where `signature` differs from `expected`, else 1 (accepted):
    /// the equal words, the common case, decide with one compare before any other check.
    fn unless_equal(
        &mut self,
        signature: Self::Word,
        expected: Self::Word,
        otherwise: impl FnOnce(&mut Self) -> Result<Self::Word, Self::Exit>,
    ) -> Result<Self::Word, Self::Exit>;
}

/// The reference at `index` of the table, which an indirect call of the `expected` signature word
/// calls: an index past the table traps, then a null element, then a function of another
/// signature. The one place these checks and their order are written. A signature equal to the
/// expected one is accepted first: the expected signature is never null, so the null check runs
/// only where they differ, and traps in the same order. `null` is the canonical null signature's
/// word, which the caller names (a stencil's cold path never reaches it, see `unless_equal`).
/// The result is the reference and the element's row, read once after the checks.
#[inline(always)]
pub(crate) fn indirect_target_word<S: WasmTable>(
    stage: &mut S,
    index: S::Word,
    expected: S::Word,
    null: S::Word,
) -> Result<(S::Word, S::Row), S::Exit> {
    let one = stage.constant(Width::I64, 1);
    let outside = stage.table_range_exceeds(index, one);
    stage.trap_when(outside, WasmTrap::UndefinedElement)?;
    let signature = stage.element_signature(index);
    let reference = stage.table_element(index);
    let accepted = stage.unless_equal(signature, expected, #[inline(always)] |stage| {
        let uninitialized = stage.operate(Binary::Equal, Width::I64, signature, null);
        stage.trap_when(uninitialized, WasmTrap::UninitializedElement)?;
        Ok(stage.signature_accepts(reference, signature, expected))
    })?;
    let mismatch = stage.operate_unary(Unary::EqualZero, Width::I32, accepted);
    stage.trap_when(mismatch, WasmTrap::IndirectCallTypeMismatch)?;
    let row = stage.element_row(index);
    Ok((reference, row))
}

/// `indirect_target_word` over the canonical signature the program's type names.
#[cfg(not(stencil_build))]
#[inline(always)]
pub fn indirect_target<S: WasmTable>(
    stage: &mut S,
    index: S::Word,
    expected: CanonicalSignature,
) -> Result<(S::Word, S::Row), S::Exit> {
    debug_assert_ne!(expected, CanonicalSignature::NULL);
    let word = stage.constant(Width::I64, expected.word() as i64);
    let null = stage.constant(Width::I64, CanonicalSignature::NULL.word() as i64);
    indirect_target_word(stage, index, word, null)
}

/// The protocol of a call a compiled frame makes into code it does not hold: the callee's entry,
/// the VM's callouts, the allocating step. Arguments and results are addresses the caller names
/// (the window, the destination); a call's status is the only word the protocol reads.
#[cfg(stencil_build)]
pub(crate) trait WasmCall: WasmStage {
    /// Calls the compiled entry `entry` (a Wasm function's `WasmEntry`) with the arguments at
    /// `window` and the result at `destination`; returns the status, 0 when it returned.
    fn enter(&mut self, entry: usize, window: usize, destination: usize) -> Self::Word;
    /// Calls the VM's defined-function callout at `callout` for the function `index`.
    fn call_defined(&mut self, callout: usize, index: u64, window: usize, destination: usize) -> Self::Word;
    /// Calls the VM's reference callout at `callout` (an indirect call's slow path).
    fn call_reference(
        &mut self,
        callout: usize,
        reference: (u64, u64, u64, u64),
        window: usize,
        destination: usize,
    ) -> Self::Word;
    /// Calls the table callout at `callout` with the request and the operand words.
    fn table_callout(&mut self, callout: usize, request: u64, operands: [u64; 4], out: usize) -> Self::Word;
    /// Calls the allocating step at `callout`; its result, when it has one, is written to `out`.
    fn allocate(&mut self, callout: usize, packed: u64, meta: u64, frame: usize, descriptor: usize, out: usize) -> Self::Word;
    /// After a call that may have run the VM: the frame's memory length and pins are the
    /// context's, the length read at `length_at`.
    fn reload_memory(&mut self, length_at: usize);
    /// After a call that cannot have grown the memory: the frame's pins are its header's, as
    /// `reload_memory` leaves them, with the length the header already holds.
    fn restore_memory(&mut self);
    /// Leaves with `status` where it is nonzero.
    fn status_leaves(&mut self, status: Self::Word) -> Result<(), Self::Exit>;
    /// The word a callee wrote at `at`.
    fn read_result(&mut self, at: usize) -> Self::Word;
    /// Writes `word` to the frame or result address `at`.
    fn write_result(&mut self, at: usize, word: Self::Word);
}

/// After any call that may reach the VM or run another function: the memory is reloaded, then the
/// status is checked (a nonzero status leaves).
#[inline(always)]
#[cfg(stencil_build)]
pub(crate) fn finish_call<S: WasmCall>(stage: &mut S, status: S::Word, length_at: usize) -> Result<(), S::Exit> {
    stage.reload_memory(length_at);
    stage.status_leaves(status)
}

/// A compiled Wasm function's call (`call` row, or `call_keep` when `RELOAD` is false): its entry,
/// then the memory's reload (`finish_call`), or its restore where the callee cannot have grown the
/// memory (`restore_memory`), then the status check. `length_at` is read only when reloading.
#[inline(always)]
#[cfg(stencil_build)]
pub(crate) fn call_entry<S: WasmCall, const RELOAD: bool>(
    stage: &mut S,
    entry: usize,
    window: usize,
    destination: usize,
    length_at: usize,
) -> Result<(), S::Exit> {
    let status = stage.enter(entry, window, destination);
    if RELOAD {
        finish_call(stage, status, length_at)
    } else {
        stage.restore_memory();
        stage.status_leaves(status)
    }
}

/// A call of a defined function through the VM (`call_vm` row), then `finish_call`.
#[inline(always)]
#[cfg(stencil_build)]
pub(crate) fn call_defined<S: WasmCall>(
    stage: &mut S,
    callout: usize,
    index: u64,
    window: usize,
    destination: usize,
    length_at: usize,
) -> Result<(), S::Exit> {
    let status = stage.call_defined(callout, index, window, destination);
    finish_call(stage, status, length_at)
}

/// The indirect call's slow path: the VM resolves the reference, relates its signature and calls
/// it, then `finish_call`.
#[inline(always)]
#[cfg(stencil_build)]
pub(crate) fn call_reference<S: WasmCall>(
    stage: &mut S,
    callout: usize,
    reference: (u64, u64, u64, u64),
    window: usize,
    destination: usize,
    length_at: usize,
) -> Result<(), S::Exit> {
    let status = stage.call_reference(callout, reference, window, destination);
    finish_call(stage, status, length_at)
}

/// A table operation's callout (`callout` row), then `finish_call`; a `wrap` result is an i32, so
/// its word is zero-extended from its low 32 bits.
#[inline(always)]
#[cfg(stencil_build)]
pub(crate) fn call_table_op<S: WasmCall>(
    stage: &mut S,
    callout: usize,
    request: u64,
    operands: [u64; 4],
    out: usize,
    length_at: usize,
    wrap: bool,
) -> Result<(), S::Exit> {
    let status = stage.table_callout(callout, request, operands, out);
    finish_call(stage, status, length_at)?;
    if wrap {
        let word = stage.read_result(out);
        let mask = stage.constant(Width::I64, i64::from(u32::MAX));
        let widened = stage.operate(Binary::And, Width::I64, word, mask);
        stage.write_result(out, widened);
    }
    Ok(())
}

/// The allocating step (`gc_allocate` row): its status is checked, then a step with a result
/// writes it to `destination`. No memory reload: the step runs no interpreter code.
#[inline(always)]
#[cfg(stencil_build)]
pub(crate) fn allocate_step<S: WasmCall>(
    stage: &mut S,
    callout: usize,
    (packed, meta): (u64, u64),
    (frame, descriptor): (usize, usize),
    out: usize,
    destination: Option<usize>,
) -> Result<(), S::Exit> {
    let status = stage.allocate(callout, packed, meta, frame, descriptor, out);
    stage.status_leaves(status)?;
    if let Some(destination) = destination {
        let word = stage.read_result(out);
        stage.write_result(destination, word);
    }
    Ok(())
}
