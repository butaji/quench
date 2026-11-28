//! Register and local rows shared by the staged operations: the wasm operands, this and the local reads and writes.

use super::*;

/// The Wasm scalar word of `register` in `rep`: a raw word through `Frame::raw_read` where `rep` is
/// `RawBits64`, anything else through `Frame::wasm_read`.
#[inline(always)]
pub fn wasm_operand<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, register: Register, rep: WasmRep) -> Option<S::Word> {
    if rep == WasmRep::RawBits64 { frame.raw_read(stage, register) } else { frame.wasm_read(stage, register, rep) }
}

/// Define `register` with the Wasm scalar `word` in `rep`: a raw word through `Frame::raw_write` where
/// `rep` is `RawBits64`, anything else through `Frame::wasm_write`. Returns the tagged word.
#[inline(always)]
pub fn wasm_result<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, register: Register, word: S::Word, rep: WasmRep) -> Option<S::Word> {
    if rep == WasmRep::RawBits64 { frame.raw_write(stage, register, word) } else { frame.wasm_write(stage, register, word, rep) }
}

/// A conditional jump on the register's truthiness, taken when it is `taken`; unresolved where
/// the frame cannot tell its truthiness.
#[inline(always)]
pub fn jump_if<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, register: Register, taken: bool) -> Step<S::Word, F::Control> {
    let Some(condition) = frame.truthiness(stage, register) else { return Step::Unresolved };
    let condition = if taken { condition } else { stage.not(condition) };
    Step::Control(frame.branch(stage, condition))
}

/// `destination` defined with the logical negation of `source`'s truthiness.
#[inline(always)]
pub fn logical_not<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let Some(truthy) = frame.truthiness(stage, source) else { return Step::Unresolved };
    let falsy = stage.not(truthy);
    let (value, fact) = frame.boolean(stage, falsy);
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}

/// The activation's `this` word where it holds one; an unbound `this` leaves at `miss`.
#[inline(always)]
pub fn bound_this<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss) -> Option<S::Word> {
    let this = frame.this(stage);
    let bound = stage.ne_imm(this, crate::Value::DELETED.to_bits());
    stage.guard(bound, miss)?;
    Some(this)
}

/// `destination` defined with the activation's `this`, where it holds one (not the deleted
/// marker); `miss` where it does not.
#[inline(always)]
pub fn load_this<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, miss: S::Miss) -> Step<S::Word, F::Control> {
    if let Some((this, fact)) = frame.this_value(stage) {
        frame.write(stage, destination, this, fact);
        return Step::Value(this);
    }
    match bound_this(stage, frame, miss) {
        Some(this) => {
            frame.define_this(stage, destination, this);
            Step::Value(this)
        }
        None => Step::Unresolved,
    }
}

/// `destination` defined from local slot `slot`; `None` where the frame cannot read it.
#[inline(always)]
pub fn read_local<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, slot: u16, miss: Option<S::Miss>) -> Option<S::Word> {
    let (value, fact) = frame.load_local(stage, slot, miss)?;
    frame.write(stage, destination, value, fact);
    Some(value)
}

/// `destination` defined from the `arguments` binding of local slot `slot` where the frame answers it
/// (`Frame::arguments_binding`): the marker a forwarding read passes on, or the object a keeping activation
/// makes. `None` where the frame does not, and the slot's own read decides.
#[inline(always)]
pub fn read_arguments_binding<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, slot: u16) -> Option<S::Word> {
    let (value, fact) = frame.arguments_binding(stage, slot)?;
    frame.write(stage, destination, value, fact);
    Some(value)
}

/// `source`'s tagged word written to local slot `slot`; `None` where the frame cannot write it.
#[inline(always)]
pub fn write_local<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, source: Register, slot: u16, miss: Option<S::Miss>) -> Option<S::Word> {
    let value = frame.tagged(stage, source);
    frame.store_local(stage, slot, value, miss).then_some(value)
}

/// `destination` defined from local slot `slot` where its binding is initialized (not the deleted
/// marker, which a binding in its temporal dead zone holds); `None` where the frame cannot read the
/// slot, and `miss` where the binding is uninitialized: the host's read raises the `ReferenceError`.
#[inline(always)]
pub fn read_initialized_local<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let (value, fact) = frame.load_local(stage, slot, Some(miss))?;
    let initialized = stage.ne_imm(value, crate::Value::DELETED.to_bits());
    stage.guard(initialized, miss)?;
    frame.write(stage, destination, value, fact);
    Some(value)
}

/// `source`'s tagged word written to local slot `slot` where its binding is initialized: an assignment
/// that is no initialization (`let x; x = 1`) of a binding still in its temporal dead zone is the
/// host's `ReferenceError`, which `miss` leaves to.
#[inline(always)]
pub fn write_initialized_local<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, source: Register, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let (current, _) = frame.load_local(stage, slot, Some(miss))?;
    let initialized = stage.ne_imm(current, crate::Value::DELETED.to_bits());
    stage.guard(initialized, miss)?;
    write_local(stage, frame, source, slot, Some(miss))
}
