//! The bit layout of the packed words a site and an add row carry beside their operands: where each
//! field of a hole's word starts. Shared by the stencils (`codes.rs` re-exports it for them), the
//! lib (`numeric.rs` mounts it, and the runtime's planner reads the fields from there). `build.rs`
//! does not mount it: a build-side row reads no field.

/// Half of a 64-bit word: the bit where a row's second field starts in a word split in two.
const WORD_HALF: u32 = u64::BITS / 2;

/// Bits below the function register's frame offset in a naming row's hole 2, which carries the slot beneath it.
pub const NAME_FUNCTION_AT: u32 = ADD_SLOT_BITS;
/// Bits below the after-shape in an add row's hole 0, which carries the before-shape beneath it.
pub const ADD_AFTER_AT: u32 = WORD_HALF;
/// An add row's hole 2: the slot in its low `ADD_SLOT_BITS`, the prototype link a chained add proves
/// (a 28-bit header field, `OBJECT.proto_mask`) from `ADD_LINK_AT`, and the object register's frame
/// offset from `ADD_OBJECT_AT` (`ADD_OBJECT_BITS` of it: a register file's byte span fits).
pub const ADD_SLOT_BITS: u32 = u16::BITS;
pub const ADD_LINK_AT: u32 = ADD_SLOT_BITS;
pub const ADD_LINK_BITS: u32 = 28;
pub const ADD_OBJECT_AT: u32 = ADD_LINK_AT + ADD_LINK_BITS;
pub const ADD_OBJECT_BITS: u32 = u64::BITS - ADD_OBJECT_AT;
/// Bits below the prototype link in a folded read's hole 1, which carries the receiver's exact shape beneath it.
pub const HELD_LINK_AT: u32 = WORD_HALF;
/// Bits below the object register's frame offset in a site field read's hole 0, which carries
/// the destination's frame offset beneath it.
pub const SITE_OBJECT_AT: u32 = WORD_HALF;
/// Bits below the second cache entry's byte offset in a chained site read's hole 2, which carries
/// the first entry's offset beneath it.
pub const CHAIN_SECOND_AT: u32 = WORD_HALF;

/// A tested field store's hole 3 (`site_rows::site_store_row`): the tag fields past the first tag
/// a non-int32 word must fall outside. A slot whose observed type holds Number admits a double,
/// which lies outside the six tags; an int32-only slot admits none, against every tag field.
pub const STORE_ADMITS_DOUBLES: u64 = 6;
pub const STORE_ADMITS_INT32: u64 = 1 << 16;
