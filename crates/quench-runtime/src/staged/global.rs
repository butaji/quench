//! The global-name read: what its binding time leaves known, and the frame's reading of each case.
use super::{Frame, Register, Stage};
use crate::typed_ir::Type;
use crate::Value;

/// What is known of a global name's value when its read is staged: a static immediate, a
/// value held by a dependency (a Stable fact), a whole-program value the read checks once,
/// or nothing, so the lane's own lookup answers.
#[derive(Clone, Copy)]
pub(crate) enum GlobalBinding {
    Dynamic,
    Constant(Value),
    Held { value: Value, ty: Type },
    Frozen { value: Value, ty: Type },
}

/// A frame's reading of a global-name read; a frame with no such callout declines it.
pub(crate) trait GlobalFrame<S: Stage>: Frame<S> {
    /// Define `destination` as the immutable global's `value`.
    #[inline(always)]
    fn global_constant(&mut self, _stage: &mut S, _destination: Register, _value: Value) -> Option<()> { None }

    /// Define `destination` as the `value` of type `ty` a dependency holds.
    #[inline(always)]
    fn global_held(&mut self, _stage: &mut S, _destination: Register, _value: Value, _ty: Type) -> Option<()> { None }

    /// Read the global once, require the closed program's `value`, and define `destination`
    /// as it; a changed binding leaves at the proof's edge.
    #[inline(always)]
    fn global_frozen(&mut self, _stage: &mut S, _destination: Register, _atom: u32, _site: u16, _value: Value, _ty: Type,
        _proof: S::Proof) -> Option<()> { None }

    /// The lane's own global-name read (`LaneHelper::GlobalGet`).
    #[inline(always)]
    fn global_read(&mut self, _stage: &mut S, _destination: Register, _atom: u32, _site: u16, _proof: S::Proof) -> Option<()> { None }
}
