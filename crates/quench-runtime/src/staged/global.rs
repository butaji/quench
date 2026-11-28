//! The global-name read: what its binding time leaves known, and the frame's reading of each case.
use super::region::Region;
use super::{Frame, Register, Stage};
use crate::typed_ir::Type;
use crate::Value;

/// What is known of a global name's value when its read is staged: a static immediate, a
/// value held by a dependency (a Stable fact), a whole-program value the read checks once,
/// a global lexical binding whose word lives at an address a dependency holds, or nothing,
/// so the lane's own lookup answers.
#[derive(Clone, Copy)]
pub enum GlobalBinding {
    Dynamic,
    /// The lane's own read, of a value a dependency keeps of type `ty` (a slot the code stores).
    Typed { ty: Type },
    Constant(Value),
    Held { value: Value, ty: Type },
    Frozen { value: Value, ty: Type },
    /// A global lexical binding of the active realm's declarative record, read live from its word at `address`: the record
    /// stays where it is until a new declaration changes the global layout, a Stable fact the code holds.
    Lexical { address: u64 },
}

/// The word of the global lexical binding at `address` (`GlobalBinding::Lexical`), read live; `None`, leaving at `miss`, where it holds
/// the deleted marker, a binding in its temporal dead zone, whose read the host raises.
#[inline(always)]
pub fn lexical_read<S: Stage>(stage: &mut S, address: S::Word, miss: S::Miss) -> Option<S::Word> {
    let word = stage.load_u64(Region::Cells.mutable(), address, 0);
    let initialized = stage.ne_imm(word, Value::DELETED.to_bits());
    stage.guard(initialized, miss)?;
    Some(word)
}

/// A frame's reading of a global-name read; a frame with no such callout declines it.
pub trait GlobalFrame<S: Stage>: Frame<S> {
    /// What is known of the global the read under `proof` names: the tier's facts about it (an
    /// immutable value, a cell a code dependency holds, a frozen program's value); `Dynamic`
    /// where nothing is, which the interpreter's frames always answer.
    #[inline(always)]
    fn global_binding(&mut self, _proof: S::Proof) -> GlobalBinding { GlobalBinding::Dynamic }

    /// Define `destination` as the immutable global's `value`.
    #[inline(always)]
    fn global_constant(&mut self, _stage: &mut S, _destination: Register, _value: Value) -> Option<()> { None }

    /// Define `destination` as the word of the global lexical binding at `address`, which is initialized; `None` calls out.
    #[inline(always)]
    fn global_lexical(&mut self, _stage: &mut S, _destination: Register, _address: u64, _proof: S::Proof) -> Option<()> { None }

    /// Define `destination` as the `value` of type `ty` a dependency holds.
    #[inline(always)]
    fn global_held(&mut self, _stage: &mut S, _destination: Register, _value: Value, _ty: Type) -> Option<()> { None }

    /// Define `destination` as the whole-program `value` of type `ty` the closed world fixes before any read of it: no read
    /// runs, so no check does either.
    #[inline(always)]
    fn global_frozen(&mut self, _stage: &mut S, _destination: Register, _value: Value, _ty: Type) -> Option<()> { None }

    /// The lane's own global-name read (`LaneHelper::GlobalGet`), whose value has type `ty`.
    #[inline(always)]
    fn global_read(&mut self, _stage: &mut S, _destination: Register, _atom: u32, _site: u16, _ty: Type, _proof: S::Proof) -> Option<()> { None }

    /// The global-name store of register `source`, whose word is `value`, to `atom` through
    /// cache `site`, an initialization where `initialize`: a direct write to the global's slot
    /// where the frame holds it (`GlobalSlot`) and the value's type fits it; `None` calls out.
    #[inline(always)]
    fn global_write(&mut self, _stage: &mut S, _source: Register, _atom: u32, _site: u16, _value: S::Word, _initialize: bool) -> Option<()> { None }

    /// Define `destination` with the value of the global `atom` that a `typeof` names (its load,
    /// `LoadNameTypeof`, never throws); `None` calls out, where the VM answers (`undefined`
    /// where the name is unresolvable).
    #[inline(always)]
    fn global_typeof(&mut self, _stage: &mut S, _destination: Register, _atom: u32, _site: u16) -> Option<()> { None }
}
