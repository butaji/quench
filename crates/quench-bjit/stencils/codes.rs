//! The codes a Number or heap row is named by: the places of its operands, its kind and its heap
//! operation. The one definition `numeric.rs` (the semantics), `library.rs` (which rows exist) and
//! the runtime (which row it plans) all read.

#[macro_use]
#[path = "row_catalog.rs"]
mod row_catalog;
pub use row_catalog::*;
/// The stencils read the site bits through this path (`site_bits.rs`); the lib and the runtime read `numeric::site_bits`.
#[cfg(stencil_build)]
pub use crate::site_bits::*;

/// The catalog's codes: each row's const, and the heap tables (`row_catalog.rs`).
catalog!(row_catalog());

/// The pinned registers the row ABI fixes (`rows.rs`): the int32 pins are r14, r15, rdi and rsi (x22..x25 on
/// AArch64), the double pins xmm0..7 (d0..d7). These two counts are the only places codes the ABI gives; every
/// other place code below follows from them.
pub const INT_PINS: u8 = 4;
pub const DOUBLE_PINS: u8 = 8;

numbered_codes! {
    PLACE_CODE_COUNT:
    MEM,
    MEMINT,
    CONST,
    IMM,
    /// The first pinned int32 register; the `INT_PINS` pins follow it.
    PIN,
    reserve INT_PINS - 1,
    /// The first pinned double register; the `DOUBLE_PINS` pins follow it.
    PINF,
    reserve DOUBLE_PINS - 1,
}
pub const LAST_INT_PIN: u8 = PIN + INT_PINS - 1;

/// The first and last of the Math rows of one Number.
pub const MATH_FIRST: u8 = SQRT;
pub const MATH_LAST: u8 = ROUND;

/// The typed-array kinds, the runtime's enum (`layout::typed_array_catalog`, mounted once for both builds).
use crate::layout::typed_array_catalog::TypedArrayKind;

/// The planned typed array kinds: each typed row family (`TYPED_INDEX_ROW`, `TYPED_STORE_ROW`) has one row per kind.
pub const TYPED_KINDS: u8 = TypedArrayKind::COUNT as u8;
/// The last row of each typed family.
pub const TYPED_INDEX_LAST: u8 = TYPED_INDEX_ROW + TYPED_KINDS - 1;
pub const TYPED_STORE_LAST: u8 = TYPED_STORE_ROW + TYPED_KINDS - 1;
/// The first and last staged codes: one heap-space run after the parse-int row.
pub const STAGED_FIRST: u8 = STAGED_REMAINDER;
pub const STAGED_LAST: u8 = STAGED_STRICT_NE;
/// The discriminants of the kinds with no native element (BigInt64, BigUint64, Float16), which have no row.
pub const NON_NATIVE_KINDS: [u8; non_native_count()] = non_native_kinds();

const fn non_native_count() -> usize {
    let mut count = 0;
    let mut kind = 0;
    while kind < TypedArrayKind::COUNT {
        if !TypedArrayKind::ALL[kind].has_native_element() {
            count += 1;
        }
        kind += 1;
    }
    count
}

const fn non_native_kinds() -> [u8; non_native_count()] {
    let mut kinds = [0; non_native_count()];
    let mut kind = 0;
    let mut at = 0;
    while kind < TypedArrayKind::COUNT {
        if !TypedArrayKind::ALL[kind].has_native_element() {
            kinds[at] = kind as u8;
            at += 1;
        }
        kind += 1;
    }
    kinds
}

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

