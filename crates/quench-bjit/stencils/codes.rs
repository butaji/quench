//! The codes a Number or heap row is named by: the places of its operands, its kind and its heap
//! operation. The one definition `numeric.rs` (the semantics), `library.rs` (which rows exist) and
//! the runtime (which row it plans) all read.

#![allow(dead_code)]

pub const MEM: u8 = 0;
pub const MEMINT: u8 = 1;
pub const CONST: u8 = 2;
pub const IMM: u8 = 3;
pub const PIN: u8 = 4;
pub const PINF: u8 = 8;
/// Pinned int32 registers, then pinned double registers.
pub const INT_PINS: u8 = 4;
pub const DOUBLE_PINS: u8 = 8;
pub const LAST_INT_PIN: u8 = PIN + INT_PINS - 1;

pub const MOVE: u8 = 0;
pub const COPY: u8 = 1;
pub const ADD: u8 = 2;
pub const SUB: u8 = 3;
pub const MUL: u8 = 4;
pub const DIV: u8 = 5;
pub const AND: u8 = 6;
pub const OR: u8 = 7;
pub const XOR: u8 = 8;
pub const SHL: u8 = 9;
pub const SHR: u8 = 10;
pub const USHR: u8 = 11;
pub const LT: u8 = 12;
pub const LE: u8 = 13;
pub const GT: u8 = 14;
pub const GE: u8 = 15;
pub const EQ: u8 = 16;
pub const NE: u8 = 17;
pub const JUMP: u8 = 18;
pub const JUMP_POLL: u8 = 19;
pub const GUARD: u8 = 20;
pub const TAG_INIT: u8 = 21;
pub const SPILL: u8 = 22;
pub const JUMP_WATCH: u8 = 23;
/// Polls the interrupt flag and continues: the guard a conditional branch back passes first.
pub const POLL: u8 = 24;
/// Branches to the taken successor when the first operand is truthy, or falsy (`JUMP_FALSE`).
pub const JUMP_TRUE: u8 = 25;
pub const JUMP_FALSE: u8 = 26;
/// The destination takes the logical negation of the first operand's truthiness, or the Number
/// negation of the first operand.
pub const NOT: u8 = 27;
pub const NEG: u8 = 28;

pub const CAPTURE_ROW: u8 = 0;
pub const INDEX_ROW: u8 = 1;
pub const STORE_ROW: u8 = 2;
pub const FIELD_ROW: u8 = 3;
pub const ARRAY_ROW: u8 = 4;
pub const SITE_ROW: u8 = 5;
/// The running activation's `this`, a local slot read, a local slot written.
pub const THIS_ROW: u8 = 6;
pub const LOAD_LOCAL_ROW: u8 = 7;
pub const STORE_LOCAL_ROW: u8 = 8;
/// The frame slot at hole 1 holds exactly the word at hole 2: the proof a call's callee is the one
/// a site was stitched for.
pub const IDENTITY_ROW: u8 = 9;
pub const LENGTH_ROW: u8 = 10;
pub const CAPTURE_SET_ROW: u8 = 11;
pub const SITE_STORE_ROW: u8 = 12;
/// The site rows of the activation's receiver (`this`), which the payload holds.
pub const THIS_SITE_ROW: u8 = 13;
pub const THIS_STORE_ROW: u8 = 14;
/// A prototype-held field folded to the value its holder held when the code was compiled, over the
/// receiver (shape and prototype link) it stands on; the receiver's variant (`entry`) takes it from the payload.
pub const HELD_ROW: u8 = 15;
/// Leaves unless the invalidation word at the address in hole 1 is zero.
pub const LIVE_ROW: u8 = 16;
/// A fast arm of an instruction whose full operation a helper step makes (`staged::iteration`): the
/// frame slot at hole 1 holds a direct object or array record (`RequireIteratorResult`), or a direct
/// string, which is copied to the slot at hole 0 (`ToPropertyKey`). A hit continues at TAKEN, past the
/// helper step laid out after the row; a miss leaves through EXIT0 into it.
pub const OBJECT_ROW: u8 = 17;
pub const KEY_ROW: u8 = 18;
/// Bits below the prototype link in a folded read's hole 1, which carries the receiver's exact shape beneath it.
pub const HELD_LINK_AT: u32 = 32;
/// Bits below the object register's frame offset in a site field read's hole 0, which carries
/// the destination's frame offset beneath it.
pub const SITE_OBJECT_AT: u32 = 32;
/// Bits below the second cache entry's byte offset in a chained site read's hole 2, which carries
/// the first entry's offset beneath it.
pub const CHAIN_SECOND_AT: u32 = 32;

/// The kind families the library is built from.
pub const fn is_copy(kind: u8) -> bool {
    kind <= COPY
}
pub const fn is_arith(kind: u8) -> bool {
    ADD <= kind && kind <= MUL
}
pub const fn is_bitwise(kind: u8) -> bool {
    AND <= kind && kind <= USHR
}
pub const fn is_compare(kind: u8) -> bool {
    LT <= kind && kind <= NE
}
/// Whether a destination holds an int32 only: a pinned register or an int slot.
pub const fn holds_int(place: u8) -> bool {
    matches!(place, PIN..=LAST_INT_PIN | MEMINT)
}
