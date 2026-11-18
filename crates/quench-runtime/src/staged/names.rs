//! Name, environment and iteration access as staged rows: the environment local reads and
//! writes, the arguments length, the global-name store and `typeof`, and the iteration and
//! spread steps. Each is one definition every tier runs; what runs guest code or the host's
//! binding rules (the generic iterator protocol, a store's TDZ and constant checks) is a
//! `Frame` callout the frame answers or declines (`Frame::vm_step`).
use super::callout::calling;
use super::{CallOut, Constant, GlobalBinding, Operation, NumericFrame, NumericStage, Register, Step};
use crate::typed_ir::Type;
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
    /// `rest.length` (`get` false, `RestLength`) or `rest[key]` (`RestGet`) of a rest view
    /// (`Function::rest_view`): the values the activation kept past its first `fixed`
    /// parameters while the rest register (`rest`, the second operand) holds `undefined`,
    /// else the array there. A key no kept value answers makes the rest array, into `rest`.
    RestView { get: bool, fixed: usize, rest: Register },
}

/// The staged operation `instruction` denotes among the name, environment and iteration
/// opcodes; `None` for any other opcode.
#[inline(always)]
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
        Op::GetEnumerated => Operation::EnumeratedGet { destination: destination(), base: instruction.register_b(),
            key: instruction.register_c(), state: crate::bytecode::ImmediateLayout::register_pair(instruction.imm()).0 },
        _ => return decode_protocol(instruction),
    })
}

impl<W, P> Operation<W, P> {
    /// The name read `instruction` (`LoadName`, or a call's `LoadNameCall`) denotes under the
    /// tier's `proof` of the global it names; `None` for any other opcode.
    pub(crate) fn decode_load_name(instruction: WideInstruction, proof: P) -> Option<Self> {
        let receiver = match instruction.op() {
            Op::LoadName => None,
            Op::LoadNameCall => Some(instruction.register_b()),
            _ => return None,
        };
        Some(Self::LoadName { destination: instruction.result_register(), atom: instruction.atom_index(),
            site: instruction.cache_site_index(), receiver, proof })
    }
}

/// The iteration and spread steps of `decode`, with the registers `Vm::call_out` reads.
#[inline(always)]
fn decode_protocol<W, P>(instruction: WideInstruction) -> Option<Operation<W, P>> {
    let (step, destination, operands) = match instruction.op() {
        Op::AppendArray => (Protocol::AppendArray { length: instruction.array_length(),
            spread: instruction.boolean_field(crate::bytecode::InstructionField::C)? },
            None, [Some(instruction.register_a()), instruction.optional_register_b()]),
        Op::GetIterator => (Protocol::GetIterator, Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::ForInStart => (Protocol::ForInStart, Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::IteratorClose => (Protocol::IteratorClose, Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::RestLength => (Protocol::RestView { get: false, fixed: instruction.array_length(), rest: instruction.register_b() },
            Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::RestGet => (Protocol::RestView { get: true, fixed: instruction.array_length(), rest: instruction.register_c() },
            Some(instruction.result_register()), [Some(instruction.register_b()), Some(instruction.register_c())]),
        _ => return super::spread::decode(instruction),
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
        Operation::LoadName { destination, atom, site, receiver, proof } =>
            load_name(stage, frame, (destination, receiver), (atom, site), proof),
        // A frame's in-place rest read answers the value; its runner defines the destination, as
        // for every names row's value (`lane_row`).
        Operation::Protocol { step: Protocol::RestView { get, fixed, .. }, destination: Some(_), operands }
            if let Some((value, _)) = frame.rest_view(stage, get, fixed, operands[0]) => Step::Value(value),
        Operation::Protocol { step, destination, operands } =>
            calling(stage, frame, CallOut::Protocol { step, operands }, destination),
        Operation::ForInNext { destination, state } => for_in_next(stage, frame, destination, state),
        Operation::EnumeratedGet { destination, state, base, key } => enumerated_get(stage, frame, destination, [state, base, key]),
        _ => delegated_step(stage, frame, operation),
    }
}

/// The spread, super and capture operations, which have modules of their own.
#[inline(always)]
fn delegated_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::SpreadCall { .. } | Operation::SuperCall { .. } => super::spread::step(stage, frame, operation),
        Operation::LoadCapture { .. } | Operation::StoreCapture { .. } => super::captures::step(stage, frame, operation),
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

/// The global-name read, by what the frame's binding time leaves known of it.
#[inline(always)]
fn global_row<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    (atom, site): (u32, u16), proof: S::Proof) -> Option<()> {
    match frame.global_binding(proof) {
        GlobalBinding::Constant(value) => frame.global_constant(stage, destination, value),
        GlobalBinding::Held { value, ty } => frame.global_held(stage, destination, value, ty),
        GlobalBinding::Frozen { value, ty } => frame.global_frozen(stage, destination, atom, site, value, ty, proof),
        GlobalBinding::Lexical { address } => frame.global_lexical(stage, destination, address, proof),
        GlobalBinding::Dynamic => frame.global_read(stage, destination, atom, site, Type::ANY, proof),
        GlobalBinding::Typed { ty } => frame.global_read(stage, destination, atom, site, ty, proof),
    }
}

/// `LoadName`: the global the frame resolves, whose call receiver is `undefined`; else the VM's
/// read, which a call's receiver (a `with` object) leaves to the interpreter.
#[inline(always)]
fn load_name<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    (destination, receiver): (Register, Option<Register>), (atom, site): (u32, u16), proof: S::Proof) -> Step<S::Word, F::Control> {
    match (global_row(stage, frame, destination, (atom, site), proof), receiver) {
        (None, None) => calling(stage, frame, CallOut::LoadName { atom, site }, Some(destination)),
        (None, Some(_)) => Step::Unresolved,
        (Some(()), receiver) => {
            if let Some(receiver) = receiver {
                let undefined = Value::UNDEFINED;
                let (word, fact) = frame.constant(stage, Constant { value: undefined, ty: Type::immediate(undefined) });
                frame.write(stage, receiver, word, fact);
            }
            Step::Value(stage.constant(0))
        }
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
    if frame.environment_write(stage, slot, value, initialize).is_none() {
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

/// `EnumeratedGet`: where the frame reads live records (`Frame::heap_records`), the key's listed
/// slot under the state's memo (`iteration::for_in_keyed_get`); any other read, and a frame that
/// reads none, take the element read the VM makes.
#[inline(always)]
fn enumerated_get<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    [state, base, key]: [Register; 3]) -> Step<S::Word, F::Control> {
    if let Some((arrays, objects, epoch)) = frame.heap_records(stage) {
        let (state_word, base_word, key_word) = (frame.tagged(stage, state), frame.tagged(stage, base), frame.tagged(stage, key));
        let miss = frame.miss();
        if let Some(value) = super::iteration::for_in_keyed_get(stage, (arrays, objects), state_word, base_word, key_word, epoch, miss) {
            return Step::Value(value);
        }
    }
    calling(stage, frame, CallOut::EnumeratedGet { base, key }, Some(destination))
}

#[cfg(test)]
mod tests;
