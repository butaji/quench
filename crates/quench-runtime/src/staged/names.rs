//! Name, environment and iteration access as staged rows: the environment local reads and
//! writes, the arguments length, the global-name store and `typeof`, and the iteration and
//! spread steps. Each is one definition every tier runs; what runs guest code or the host's
//! binding rules (the generic iterator protocol, a store's TDZ and constant checks) is a
//! `Frame` callout the frame answers or declines (`Frame::vm_step`).
use super::callout::calling;
use super::{CallOut, Constant, GlobalBinding, Operation, NumericFrame, NumericStage, Register, Step};
use crate::typed_ir::Type;
#[cfg(not(stencil_build))]
use crate::bytecode::{Op, WideInstruction};
use crate::Value;

/// An iteration or spread step the host makes by the interpreter's own definition
/// (`Vm::call_out`): the protocol runs guest code (`@@iterator`, `next`, `return`), so a
/// frame answers it with a callout; and the test of a receiver's constant fields, whose
/// answer is VM state (a cache entry), which a frame answers the same way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protocol {
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
    /// Whether the operand, a base class constructor's fresh receiver at the run of leading
    /// constant fields its body defines first, already holds that run as its defines would
    /// leave it (`ConstantFieldsHeld`): its exact shape is the one the run's first define,
    /// through cache site `site`, found already holding its name (`Vm::constant_fields_held`).
    /// It reads one cache entry and a shape, runs no guest code and raises nothing.
    ConstantFieldsHeld { site: u16 },
    #[cfg(not(stencil_build))]
    /// A literal, template, closure or object-construction step (`protocol_literals`).
    Literal(super::protocol_literals::LiteralStep),
    #[cfg(not(stencil_build))]
    /// A class, private-name or function-naming step (`protocol_classes`).
    Class(super::protocol_classes::ClassStep),
    #[cfg(not(stencil_build))]
    /// A by-name, environment, string or iteration-cleanup step (`protocol_values`).
    Value(super::protocol_values::ValueStep),
    #[cfg(not(stencil_build))]
    /// An activation-variable step: a local, a `var` binding, TDZ, an argument (`protocol_owned`).
    Owned(super::protocol_owned::OwnedVarStep),
    #[cfg(not(stencil_build))]
    /// A Wasm GC, reference, table, memory-init, atomic or exception step (`protocol_wasm`).
    Wasm(super::protocol_wasm::WasmHostStep),
}





/// Run a name, environment or iteration `operation` on `frame`; `Unresolved` where the frame
/// has neither the row's own reading nor a host for its callout.
#[inline(always)]
pub fn step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::LoadEnvLocal { destination, slot } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_ENV_LOCAL]) } { load_environment(stage, frame, destination, slot) } else { unreachable!() },
        Operation::StoreEnvLocal { source, slot, result, initialize } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::STORE_ENV_LOCAL]) } { store_environment(stage, frame, (source, slot, initialize), result) } else { unreachable!() },
        Operation::ArgumentsLength { destination, slot } => if const { super::op_kind::admits(KIND, &[super::op_kind::ARGUMENTS_LENGTH]) } { arguments_length(stage, frame, destination, slot) } else { unreachable!() },
        Operation::ArgumentGet { destination, index, slot } => if const { super::op_kind::admits(KIND, &[super::op_kind::ARGUMENT_GET]) } { argument_get(stage, frame, destination, index, slot) } else { unreachable!() },
        Operation::GlobalTypeof { destination, atom, site } => if const { super::op_kind::admits(KIND, &[super::op_kind::GLOBAL_TYPEOF]) } { global_typeof(stage, frame, destination, (atom, site)) } else { unreachable!() },
        Operation::SetFunctionName { function, name, site } => if const { super::op_kind::admits(KIND, &[super::op_kind::SET_FUNCTION_NAME]) } { set_function_name(stage, frame, function, (name, site)) } else { unreachable!() },
        Operation::GlobalWrite { source, atom, site, initialize } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::GLOBAL_WRITE]) } { global_write(stage, frame, source, (atom, site, initialize)) } else { unreachable!() },
        Operation::GlobalLexicalLoad { destination, declaration, site } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::GLOBAL_LEXICAL_LOAD]) } { calling(stage, frame, CallOut::GlobalLexicalLoad { declaration, site }, Some(destination)) } else { unreachable!() },
        Operation::GlobalLexicalStore { source, declaration, site, initialize } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::GLOBAL_LEXICAL_STORE]) } { calling(stage, frame, CallOut::GlobalLexicalStore { source, declaration, site, initialize }, None) } else { unreachable!() },
        Operation::LoadName { destination, atom, site, receiver, proof } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_NAME]) } { load_name(stage, frame, (destination, receiver), (atom, site), proof) } else { unreachable!() },
        // A frame's in-place rest read answers the value, which the row defines as every other
        // names row defines its own (`row_writes_result` holds for `RestLength` and `RestGet`).
        Operation::Protocol { step: Protocol::RestView { get, fixed, .. }, destination: Some(destination), operands }
            if let Some((value, fact)) = frame.rest_view(stage, get, fixed, operands[0]) => if const { super::op_kind::admits(KIND, &[super::op_kind::PROTOCOL]) } {
                frame.write(stage, destination, value, fact);
                Step::Value(value)
            } else { unreachable!() },
        Operation::Protocol { step, destination, operands } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::PROTOCOL]) } { calling(stage, frame, CallOut::Protocol { step, operands }, destination) } else { unreachable!() },
        Operation::ForInNext { destination, state } => if const { super::op_kind::admits(KIND, &[super::op_kind::FOR_IN_NEXT]) } { for_in_next(stage, frame, destination, state) } else { unreachable!() },
        Operation::EnumeratedGet { destination, state, base, key } => if const { super::op_kind::admits(KIND, &[super::op_kind::ENUMERATED_GET]) } { enumerated_get(stage, frame, destination, [state, base, key]) } else { unreachable!() },
        Operation::CloneEnv { plan } => if const { super::op_kind::admits(KIND, &[super::op_kind::CLONE_ENV]) } { calling(stage, frame, CallOut::CloneEnv { plan }, None) } else { unreachable!() },
        Operation::IteratorCleanupPush { iterator, done } => if const { super::op_kind::admits(KIND, &[super::op_kind::ITERATOR_CLEANUP_PUSH]) } { iterator_cleanup_push(stage, frame, iterator, done) } else { unreachable!() },
        Operation::IteratorCleanupPop => if const { super::op_kind::admits(KIND, &[super::op_kind::ITERATOR_CLEANUP_POP]) } { iterator_cleanup_pop(stage, frame) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::LOAD_ENV_LOCAL, super::op_kind::STORE_ENV_LOCAL, super::op_kind::ARGUMENTS_LENGTH, super::op_kind::ARGUMENT_GET, super::op_kind::GLOBAL_TYPEOF, super::op_kind::SET_FUNCTION_NAME, super::op_kind::GLOBAL_WRITE, super::op_kind::GLOBAL_LEXICAL_LOAD, super::op_kind::GLOBAL_LEXICAL_STORE, super::op_kind::LOAD_NAME, super::op_kind::PROTOCOL, super::op_kind::FOR_IN_NEXT, super::op_kind::ENUMERATED_GET, super::op_kind::CLONE_ENV, super::op_kind::ITERATOR_CLEANUP_PUSH, super::op_kind::ITERATOR_CLEANUP_POP]) } { delegated_step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
    }
}

/// `step_of` for any operation.
#[inline(always)]
pub fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// The spread, super and capture operations, which have modules of their own.
#[inline(always)]
fn delegated_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::SpreadCall { .. } | Operation::SuperCall { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::SPREAD_CALL, super::op_kind::SUPER_CALL]) } { super::spread::step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        Operation::LoadCapture { .. } | Operation::StoreCapture { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_CAPTURE, super::op_kind::STORE_CAPTURE]) } { super::captures::step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::SPREAD_CALL, super::op_kind::SUPER_CALL, super::op_kind::LOAD_CAPTURE, super::op_kind::STORE_CAPTURE]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `delegated_step_of` for any operation.
#[inline(always)]
fn delegated_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    delegated_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `IteratorCleanupPush`: the frame's own cleanup entry, which defines no register; a frame with no
/// cleanup stack leaves it to the general step.
#[inline(always)]
fn iterator_cleanup_push<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, iterator: Register,
    done: Register) -> Step<S::Word, F::Control> {
    match frame.iterator_push(stage, iterator, done) {
        Some(()) => Step::Value(stage.constant(0)),
        None => Step::Unresolved,
    }
}

/// `IteratorCleanupPop`: the frame's own top cleanup entry, which defines no register; an empty
/// stack leaves it to the general step, which raises.
#[inline(always)]
fn iterator_cleanup_pop<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F) -> Step<S::Word, F::Control> {
    match frame.iterator_pop(stage) {
        Some(()) => Step::Value(stage.constant(0)),
        None => Step::Unresolved,
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

/// `ArgumentGet`: the frame's own kept element at `index`, else the VM's read of the binding.
#[inline(always)]
fn argument_get<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    index: Register, slot: u16) -> Step<S::Word, F::Control> {
    let word = frame.tagged(stage, index);
    match frame.argument(stage, slot, word) {
        Some(read) => defined(stage, frame, destination, Some(read)),
        None => calling(stage, frame, CallOut::ArgumentGet { index, slot }, Some(destination)),
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

/// `SetFunctionName`: the frame's own naming of the function in `function`, which defines no register;
/// where the frame holds no naming, the row leaves to the interpreter.
#[inline(always)]
fn set_function_name<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, function: Register,
    (name, site): (u32, u16)) -> Step<S::Word, F::Control> {
    let word = frame.tagged(stage, function);
    match frame.set_function_name(stage, word, name, site) {
        Some(()) => Step::Value(word),
        None => Step::Unresolved,
    }
}

/// `GlobalWrite`: the frame's own store to the global name, else the VM's.
#[inline(always)]
fn global_write<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, source: Register,
    (atom, site, initialize): (u32, u16, bool)) -> Step<S::Word, F::Control> {
    let value = frame.tagged(stage, source);
    // A window whose names resolve through its scope stores to the `with` object that has the name first.
    if frame.scoped_store(stage, atom, value).is_some() {
        return Step::Value(value);
    }
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
        GlobalBinding::Frozen { value, ty } => frame.global_frozen(stage, destination, value, ty),
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
    // A window whose names resolve through its scope reads the `with` object or dynamic binding that has the name first.
    if frame.scoped_name(stage, destination, receiver, atom).is_some() {
        return Step::Value(stage.constant(0));
    }
    match (global_row(stage, frame, destination, (atom, site), proof), receiver) {
        (None, None) => calling(stage, frame, CallOut::LoadName { atom, site }, Some(destination)),
        (None, Some(_)) => Step::Unresolved,
        (Some(()), receiver) => {
            if let Some(receiver) = receiver {
                let undefined = Value::UNDEFINED;
                let (word, fact) = frame.constant(stage, Constant { value: undefined, ty: Type::UNDEFINED });
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
    // A window whose names resolve through its scope stores to the `with` object that has the local's name first.
    if frame.scoped_local_store(stage, slot, value).is_some() {
        if let Some(result) = result {
            let fact = frame.fact(source);
            frame.write(stage, result, value, fact);
        }
        return Step::Value(value);
    }
    if frame.environment_write(stage, slot, value, initialize).is_none() {
        return calling(stage, frame, CallOut::StoreEnvLocal { source, slot, initialize }, result);
    }
    if let Some(result) = result {
        let fact = frame.fact(source);
        frame.write(stage, result, value, fact);
    }
    Step::Value(value)
}

/// `ForInNext`: the VM's next key, or `Value::DELETED` where none is left, which takes the jump. An
/// exhausted step writes nothing: the destination keeps the word it held, as the general step leaves it.
#[inline(always)]
fn for_in_next<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    state: Register) -> Step<S::Word, F::Control> {
    match frame.vm_step(stage, CallOut::ForInNext { state }, Some(destination)) {
        Some((key, fact)) => {
            let exhausted = stage.eq_imm(key, Value::DELETED.to_bits());
            let kept = frame.tagged(stage, destination);
            let word = stage.select(exhausted, kept, key);
            frame.write(stage, destination, word, fact);
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
    if let Some((arrays, epoch)) = frame.heap_records(stage) {
        let (state_word, base_word, key_word) = (frame.tagged(stage, state), frame.tagged(stage, base), frame.tagged(stage, key));
        let miss = frame.miss();
        if let Some(value) = super::iteration::for_in_keyed_get(stage, arrays, state_word, base_word, key_word, epoch, miss) {
            return Step::Value(value);
        }
    }
    calling(stage, frame, CallOut::EnumeratedGet { base, key }, Some(destination))
}

#[cfg(test)]
mod tests;

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
