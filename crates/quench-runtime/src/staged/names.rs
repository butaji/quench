//! Name, environment and iteration access as staged rows: the environment local reads and
//! writes, the arguments length, the global-name store and `typeof`, and the iteration and
//! spread steps. Each is one definition every tier runs; what runs guest code or the host's
//! binding rules (the generic iterator protocol, a store's TDZ and constant checks) is a
//! `Frame` callout the frame answers or declines.
use super::{Operation, NumericFrame, NumericStage, Register, Step};
use crate::bytecode::{Op, WideInstruction};

/// An iteration or spread step the host makes by the interpreter's own definition
/// (`Vm::value_step`): the protocol runs guest code (`@@iterator`, `next`, `return`), so a
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
            slot: slot()?, result: instruction.optional_register_b() },
        Op::ArgumentsLength => Operation::ArgumentsLength { destination: destination(), slot: slot()? },
        Op::LoadNameTypeof => Operation::GlobalTypeof { destination: destination(), atom: instruction.atom_index(), site: instruction.cache_site_index() },
        Op::StoreName => Operation::GlobalWrite { source: instruction.register_a(), atom: instruction.atom_index(),
            site: instruction.cache_site_index(),
            initialize: instruction.boolean_field(crate::bytecode::InstructionField::B)? },
        Op::ForInNext => Operation::ForInNext { destination: destination(), state: instruction.register_b() },
        _ => return decode_protocol(instruction),
    })
}

/// The iteration and spread steps of `decode`, with the registers `Vm::value_step` reads.
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
/// declines the callout or the read leaves at its miss edge.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::LoadEnvLocal { destination, slot } => load_environment(stage, frame, destination, slot),
        Operation::StoreEnvLocal { source, slot, result } => store_environment(stage, frame, (source, slot), result),
        Operation::ArgumentsLength { destination, slot } => {
            let length = frame.arguments_length(stage, slot);
            defined(stage, frame, destination, length)
        }
        Operation::GlobalTypeof { destination, atom, site } => {
            let typeof_name = frame.global_typeof(stage, destination, atom, site);
            completed(stage, typeof_name)
        }
        Operation::GlobalWrite { source, atom, site, initialize } => {
            let value = frame.tagged(stage, source);
            match frame.global_write(stage, atom, site, value, initialize) {
                Some(()) => Step::Value(value),
                None => Step::Unresolved,
            }
        }
        Operation::Protocol { step, destination, operands } => protocol(stage, frame, step, destination, operands),
        Operation::ForInNext { destination, state } => {
            let state = frame.tagged(stage, state);
            match frame.for_in_next(stage, destination, state) {
                Some(exhausted) => Step::Control(frame.branch(stage, exhausted)),
                None => Step::Unresolved,
            }
        }
        _ => Step::Unresolved,
    }
}

/// A callout's outcome that defines no word: control falls through with a placeholder word.
#[inline(always)]
fn completed<S: NumericStage, C>(stage: &mut S, done: Option<()>) -> Step<S::Word, C> {
    match done {
        Some(()) => Step::Value(stage.constant(0)),
        None => Step::Unresolved,
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
/// shared row (`environment::environment_slot`).
#[inline(always)]
fn load_environment<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    slot: u16) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let read = frame.environment(stage)
        .and_then(|(storage, fact)| super::environment::environment_slot(stage, storage, slot, miss).map(|value| (value, fact)));
    defined(stage, frame, destination, read)
}

/// `StoreEnvLocal`: `source` to the environment slot, and to `result` where the instruction has one.
#[inline(always)]
fn store_environment<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, (source, slot): (Register, u16),
    result: Option<Register>) -> Step<S::Word, F::Control> {
    let value = frame.tagged(stage, source);
    if frame.environment_write(stage, slot, value).is_none() { return Step::Unresolved; }
    if let Some(result) = result {
        let fact = frame.fact(source);
        frame.write(stage, result, value, fact);
    }
    Step::Value(value)
}

/// An iteration or spread step: its operand words go to the frame's callout, and the word it
/// defines, if any, to `destination`.
#[inline(always)]
fn protocol<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, step: Protocol,
    destination: Option<Register>, operands: [Option<Register>; 2]) -> Step<S::Word, F::Control> {
    let words = operands.map(|register| register.map(|register| frame.tagged(stage, register)));
    match frame.protocol(stage, step, words) {
        Some(Some((value, fact))) => {
            if let Some(destination) = destination { frame.write(stage, destination, value, fact); }
            Step::Value(value)
        }
        Some(None) => Step::Value(stage.constant(0)),
        None => Step::Unresolved,
    }
}

#[cfg(test)]
mod tests;
