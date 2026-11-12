//! The one frame's layout: the words of a call in progress that emitted code reads and
//! writes at fixed offsets. Everything else (return linkage, kind) belongs to the lane, which
//! asserts these against the record it lays out (`vm::compiled::ShadowFrame`).

/// The callee's descriptor (`DirectCallee`), null for a frame the lane made.
pub(crate) const CALLEE: usize = 0;
/// The callee's receiver, as value bits.
pub(crate) const THIS: usize = 8;
/// The published body (`LaneMethod`) the frame runs, 0 when none.
pub(crate) const BODY: usize = 16;
/// The address of the frame's first register; its registers, then what it keeps, follow.
pub(crate) const WINDOW: usize = 24;
/// The prefix every frame starts with.
pub(crate) const PREFIX_BYTES: usize = 32;
