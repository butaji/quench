// The length-word and mark-bit encodings that the native records and the stencils share. ONE
// definition: the runtime's `heap::array_elements` and `heap::thin_slice` re-export these, and the
// stencil crate mounts this file (`stencils/layout.rs`) through `staged/layout_offsets.rs`. Plain
// constants only, so the file compiles in both builds.
#![allow(dead_code)]

/// The conservative `others` marker native code stores for uncounted writes.
pub const OTHERS_UNKNOWN: u32 = u32::MAX;
#[cfg_attr(feature = "test-support", quench_macros::test_visible)]
/// The length word's flag that marks a narrow run of values.
pub(crate) const NARROW_LEN: u32 = 1 << 31;
/// The length word's flag that marks a run of doubles.
pub const DOUBLE_LEN: u32 = 1 << 30;
/// Both length flags: an encoded run.
pub const ENCODED_LEN: u32 = NARROW_LEN | DOUBLE_LEN;
/// A slice header's top bit: the mark its owner may set on a non-empty slice.
pub(crate) const MARK_BIT: usize = 1 << (usize::BITS - 1);
