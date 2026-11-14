//! The prototype-held reads `GetField` records keep as their hints.
use super::*;

/// A `GetField` record's read of `atom` that a receiver's immediate prototype answered
/// (`Vm::holder_hint_hit`, `Vm::string_hint_hit`): a receiver of exact shape `receiver`, which
/// lacks the name (`NO_FIELD_RECEIVER` for a string primitive, whose own names a site that
/// reads `String.prototype` does not read), whose prototype is `holder`, read `answer` under
/// `watch`, which a change to the holder's slot values or any prototype's layout retires.
/// `prototype` is `holder` as an object's header links it, which an object receiver's test compares.
#[derive(Clone)]
pub(super) struct HolderHint {
    pub(super) receiver: u32,
    pub(super) atom: Atom,
    pub(super) prototype: crate::heap::ProtoLink,
    pub(super) holder: Value,
    pub(super) answer: HintAnswer,
    pub(super) watch: crate::heap::PrototypeValueWatch,
}

/// What a `HolderHint`'s read yields: the value its holder keeps, or the own slot of the
/// receiver that the holder's getter returns (`return this.field`), whose shape fixes the slot.
#[derive(Clone, Copy)]
pub(super) enum HintAnswer {
    Held(Value),
    OwnSlot(u16),
}
