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

pub const CAPTURE_ROW: u8 = 0;
pub const INDEX_ROW: u8 = 1;
pub const STORE_ROW: u8 = 2;
pub const FIELD_ROW: u8 = 3;
pub const ARRAY_ROW: u8 = 4;
/// The inline-cache rows (`ic_rows.rs`): a read of the receiver's own flat slot, a read of the
/// slot its prototype holds, and a store of a Number into the receiver's own flat slot, whose
/// slot type is `ANY` or `NUMBER` (`IC_SET_ROW`) or `INT32` (`IC_SET_INT_ROW`).
pub const IC_GET_ROW: u8 = 5;
pub const IC_GET_HOLDER_ROW: u8 = 6;
pub const IC_SET_ROW: u8 = 7;
pub const IC_SET_INT_ROW: u8 = 8;

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

/// What a supernode row runs first: a Number row or a heap row (`supernodes.rs`).
pub const FIRST_NUMBER: u8 = 0;
pub const FIRST_HEAP: u8 = 1;
