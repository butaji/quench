//! Name, environment and iteration access as staged rows: the environment local reads and
//! writes, the arguments length, the global-name store and `typeof`, and the iteration and
//! spread steps. Each is one definition every tier runs; what runs guest code or the host's
//! binding rules (the generic iterator protocol, a store's TDZ and constant checks) is a
//! `Frame` callout the frame answers or declines (`Frame::vm_step`).
use super::callout::calling;
use super::{CallOut, Operation, NumericFrame, NumericStage, Register, Step};
use crate::bytecode::{Op, WideInstruction};
use crate::Value;

/// An iteration or spread step the host makes by the interpreter's own definition
/// (`Vm::call_out`): the protocol runs guest code (`@@iterator`, `next`, `return`), so a
/// frame answers it with a callout.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Protocol {
    /// The iterator of the operand (`GetIterator`).
    GetIterator,
    /// The for-in state over the operand's key snapshot (`ForInStart`).
    ForInStart,
    /// Close the iterator in the operand (`IteratorClose`).
    IteratorClose,
    /// Append `length` holes to the array operand, then the optional element operand, spread
    /// where `spread` (`AppendArray`).
    AppendArray { length: usize, spread: bool },
}

/// The staged operation `instruction` denotes among the name, environment and iteration
/// opcodes; `None` for any other opcode.
pub(super) fn decode<W, P>(instruction: WideInstruction) -> Option<Operation<W, P>> {
    let destination = || instruction.result_register();
    let slot = || u16::try_from(instruction.local_slot()).ok();
    Some(match instruction.op() {
        Op::LoadEnvLocal => Operation::LoadEnvLocal { destination: destination(), slot: slot()? },
        Op::StoreEnvLocal => Operation::StoreEnvLocal { source: instruction.register_a(),
            slot: slot()?, result: instruction.optional_register_b(),
            initialize: instruction.boolean_field(crate::bytecode::InstructionField::C).unwrap_or(false) },
        Op::ArgumentsLength => Operation::ArgumentsLength { destination: destination(), slot: slot()? },
        Op::LoadNameTypeof => Operation::GlobalTypeof { destination: destination(), atom: instruction.atom_index(), site: instruction.cache_site_index() },
        Op::StoreName => Operation::GlobalWrite { source: instruction.register_a(), atom: instruction.atom_index(),
            site: instruction.cache_site_index(),
            initialize: instruction.boolean_field(crate::bytecode::InstructionField::B)? },
        Op::ForInNext => Operation::ForInNext { destination: destination(), state: instruction.register_b() },
        _ => return decode_protocol(instruction),
    })
}

/// The iteration and spread steps of `decode`, with the registers `Vm::call_out` reads.
fn decode_protocol<W, P>(instruction: WideInstruction) -> Option<Operation<W, P>> {
    let (step, destination, operands) = match instruction.op() {
        Op::AppendArray => (Protocol::AppendArray { length: instruction.array_length(),
            spread: instruction.boolean_field(crate::bytecode::InstructionField::C)? },
            None, [Some(instruction.register_a()), instruction.optional_register_b()]),
        Op::GetIterator => (Protocol::GetIterator, Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::ForInStart => (Protocol::ForInStart, Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::IteratorClose => (Protocol::IteratorClose, Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        _ => return None,
    };
    Some(Operation::Protocol { step, destination, operands })
}

/// Run a name, environment or iteration `operation` on `frame`; `Unresolved` where the frame
/// has neither the row's own reading nor a host for its callout.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::LoadEnvLocal { destination, slot } => load_environment(stage, frame, destination, slot),
        Operation::StoreEnvLocal { source, slot, result, initialize } =>
            store_environment(stage, frame, (source, slot, initialize), result),
        Operation::ArgumentsLength { destination, slot } => arguments_length(stage, frame, destination, slot),
        Operation::GlobalTypeof { destination, atom, site } => global_typeof(stage, frame, destination, (atom, site)),
        Operation::GlobalWrite { source, atom, site, initialize } =>
            global_write(stage, frame, source, (atom, site, initialize)),
        Operation::Protocol { step, destination, operands } =>
            calling(stage, frame, CallOut::Protocol { step, operands }, destination),
        Operation::ForInNext { destination, state } => for_in_next(stage, frame, destination, state),
        _ => Step::Unresolved,
    }
}

/// `ArgumentsLength`: the frame's own length, else the VM's.
#[inline(always)]
fn arguments_length<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    slot: u16) -> Step<S::Word, F::Control> {
    match frame.arguments_length(stage, slot) {
        Some(length) => defined(stage, frame, destination, Some(length)),
        None => calling(stage, frame, CallOut::ArgumentsLength { slot }, Some(destination)),
    }
}

/// `GlobalTypeof`: the frame's own `typeof` of the global, else the VM's.
#[inline(always)]
fn global_typeof<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    (atom, site): (u32, u16)) -> Step<S::Word, F::Control> {
    match frame.global_typeof(stage, destination, atom, site) {
        Some(()) => Step::Value(stage.constant(0)),
        None => calling(stage, frame, CallOut::GlobalTypeof { atom, site }, Some(destination)),
    }
}

/// `GlobalWrite`: the frame's own store to the global name, else the VM's.
#[inline(always)]
fn global_write<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, source: Register,
    (atom, site, initialize): (u32, u16, bool)) -> Step<S::Word, F::Control> {
    let value = frame.tagged(stage, source);
    match frame.global_write(stage, source, atom, site, value, initialize) {
        Some(()) => Step::Value(value),
        None => calling(stage, frame, CallOut::GlobalWrite { source, atom, site, initialize }, None),
    }
}

/// Define `destination` with a callout's word and fact.
#[inline(always)]
fn defined<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    read: Option<(S::Word, F::Fact)>) -> Step<S::Word, F::Control> {
    match read {
        Some((value, fact)) => {
            frame.write(stage, destination, value, fact);
            Step::Value(value)
        }
        None => Step::Unresolved,
    }
}

/// `LoadEnvLocal`: the slot of the frame's dense environment storage, read once through the
/// shared row (`environment::environment_slot`); the VM's binding rules where the frame holds none.
#[inline(always)]
fn load_environment<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    slot: u16) -> Step<S::Word, F::Control> {
    let Some((storage, fact)) = frame.environment(stage) else {
        return calling(stage, frame, CallOut::LoadEnvLocal { slot }, Some(destination));
    };
    let miss = frame.miss();
    let read = super::environment::environment_slot(stage, storage, slot, miss).map(|value| (value, fact));
    defined(stage, frame, destination, read)
}

/// `StoreEnvLocal`: `source` to the environment slot, and to `result` where the instruction has
/// one; the VM's binding rules where the frame cannot store.
#[inline(always)]
fn store_environment<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    (source, slot, initialize): (Register, u16, bool), result: Option<Register>) -> Step<S::Word, F::Control> {
    let value = frame.tagged(stage, source);
    if frame.environment_write(stage, slot, value).is_none() {
        return calling(stage, frame, CallOut::StoreEnvLocal { source, slot, initialize }, result);
    }
    if let Some(result) = result {
        let fact = frame.fact(source);
        frame.write(stage, result, value, fact);
    }
    Step::Value(value)
}

/// `ForInNext`: the VM's next key, or `Value::DELETED` where none is left, which takes the jump.
#[inline(always)]
fn for_in_next<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    state: Register) -> Step<S::Word, F::Control> {
    match frame.vm_step(stage, CallOut::ForInNext { state }, Some(destination)) {
        Some((key, fact)) => {
            frame.write(stage, destination, key, fact);
            let exhausted = stage.eq_imm(key, Value::DELETED.to_bits());
            Step::Control(frame.branch(stage, exhausted))
        }
        None => Step::Unresolved,
    }
}

#[cfg(test)]
mod tests;
