//! The row families and the operator lists the library builds from, which the build script and the lib
//! read (`wasm_library.rs`). `ROW_FAMILIES` is the kind table's families (`wasm_kinds.rs`,
//! `row_kind_table!`); the operator lists are the families' columns: the SIMD operators' and the memory
//! loads' families, the I64 and float counts and the comparison bounds. The stencil crate does not
//! include this file, so none of these compiles there.

use crate::wasm_kinds::*;
use crate::wasm_kinds::row_kind_table;

#[macro_use]
#[path = "../../quench-runtime/src/staged/wasm_lists.rs"]
mod wasm_lists;

/// The runtime's const string equality (`quench_runtime::property_names::same_text`).
#[path = "../../quench-runtime/src/const_text.rs"]
mod const_text;
use const_text::same_text;

/// A SIMD operator's family, the list's column: the row kind its operator runs in.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum SimdFamily {
    Unary, Binary, Splat, Extract, Replace, MoreBinary, MoreCompare, MoreFloat, MoreShift, MoreTest, MoreUnary, MoreWiden, MoreWide, MoreConvert,
}

/// A vector load's family: a scalar access, the whole vector, or a vector load that splats, widens or zeroes.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum LoadFamily {
    Scalar, Plain, Vector,
}

/// How many of `table`'s entries hold `code`.
const fn count_of(table: &[u8], code: u8) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < table.len() {
        if table[index] == code {
            count += 1;
        }
        index += 1;
    }
    count
}

/// The indices of `table`'s entries that hold `code`, in order: the tags of one family.
const fn tags_of<const N: usize>(table: &[u8], code: u8) -> [u32; N] {
    let mut tags = [0u32; N];
    let mut index = 0;
    let mut filled = 0;
    while index < table.len() {
        if table[index] == code {
            tags[filled] = index as u32;
            filled += 1;
        }
        index += 1;
    }
    tags
}

/// One row family: operators at one width, each at every shape.
#[derive(Clone, Copy)]
pub struct RowFamily {
    pub ops: RowOps,
    pub width: u8,
    pub shapes: &'static [u8],
}

/// The operators a family's rows take.
#[derive(Clone, Copy)]
pub enum RowOps {
    /// The operators `first..end`.
    Span(u32, u32),
    /// The operators `first..end` except those listed.
    Except(u32, u32, &'static [u32]),
    /// The operators listed.
    List(&'static [u32]),
    /// Every scalar access (`ACCESSES`), by its index.
    Accesses,
    /// Every scalar access that does not sign-extend (`ACCESSES`), by its index.
    UnsignedAccesses,
}

/// A family of a kind's rows: the operators `ops` at `width`, each at every shape in `shapes`.
pub const fn row_family(ops: RowOps, width: u8, shapes: &'static [u8]) -> RowFamily {
    RowFamily { ops, width, shapes }
}

/// The families callback: each kind with its row families, in table order.
macro_rules! row_families {
    ($($kind:ident => [$($family:expr),* $(,)?]),* $(,)?) => {
        /// Every kind with its row families, in table order. Each family says what the library builds for
        /// its kind: the stencil symbol of every row is `w_<kind>_<op>_<width>_<shape>`, or `call_w_...` for
        /// a row that calls (`wasm_library.rs`, `wasm_name`).
        pub const ROW_FAMILIES: &[(u8, &[RowFamily])] = &[$(($kind, &[$($family),*])),*];
    };
}
row_kind_table!(row_families);

pub const W64: u8 = IntWidth::W64 as u8;

pub const REGISTER: u8 = OperandShape::Register as u8;

pub const STORAGE_BITS64: u32 = StorageCode::Bits64 as u32;

/// The position of `name` in `names`, in a constant: the comparison bounds are the positions of two
/// named operators in their list.
const fn position_of(names: &[&str], name: &str) -> u32 {
    let mut index = 0;
    while index < names.len() {
        if same_text(names[index], name) {
            return index as u32;
        }
        index += 1;
    }
    panic!("the named operator is not in its list")
}


// The counts and bounds of the operator lists (`wasm_lists.rs`), counted from the list's entries:
// the runtime's families expand the same lists, and `wasm_codes_check.rs` asserts each count equals
// its family's.
macro_rules! i64_binary_counts {
    ($aliases:tt, $arguments:tt; $($name:ident, $i32:ident, $i64:ident [$($machine:tt)+] => $body:expr;)+ @i64 $($wide:ident, $wide_wasm:ident [$($wide_machine:tt)+] => $wide_body:expr;)+) => {
        /// The i32 operators' count: the shared operators, which the i64 family starts with.
        pub const SHARED_BINARY: u32 = [$(stringify!($name)),+].len() as u32;
        pub const BINARY_COUNT: u32 = [$(stringify!($name),)+ $(stringify!($wide),)+].len() as u32;
        /// The comparisons, `Equal` through `GreaterEqualUnsigned`.
        pub const FIRST_COMPARISON: u32 = position_of(&[$(stringify!($name),)+ $(stringify!($wide),)+], "Equal");
        pub const LAST_COMPARISON: u32 = position_of(&[$(stringify!($name),)+ $(stringify!($wide),)+], "GreaterEqualUnsigned");
    };
}
macro_rules! i64_unary_counts {
    ($signed:ident, $argument:ident; $($name:ident, $i32:ident, $i64:ident => $body:expr;)+ @i64 $($wide:ident, $wide_wasm:ident => $wide_body:expr;)+) => {
        /// The unary operators both widths have; the i64 family adds the i64-only `ExtendSigned32`.
        pub const SHARED_UNARY: u32 = [$(stringify!($name)),+].len() as u32;
        pub const UNARY_COUNT: u32 = [$(stringify!($name),)+ $(stringify!($wide),)+].len() as u32;
    };
}
macro_rules! float_binary_counts {
    ($aliases:tt, $arguments:tt; $($name:ident, $f32:ident, $f64:ident => $body:expr;)+) => {
        /// `F64BinaryOperator`'s selectors, `Add` through `GreaterEqual`.
        pub const FLOAT_BINARY_COUNT: u32 = [$(stringify!($name)),+].len() as u32;
    };
}
macro_rules! float_unary_counts {
    ($aliases:tt, $argument:ident; $($name:ident, $f32:ident, $f64:ident => $body:expr;)+) => {
        /// `F64UnaryOperator`'s selectors.
        pub const FLOAT_UNARY_COUNT: u32 = [$(stringify!($name)),+].len() as u32;
    };
}
macro_rules! conversion_counts {
    ($value:ident; $($name:ident, $wasm:ident, $source:ident => $result:ident, $body:expr;)+) => {
        /// `ScalarConversionOperator`'s selectors.
        pub const CONVERT_COUNT: u32 = [$(stringify!($name)),+].len() as u32;
    };
}

i64_binary_list!(i64_binary_counts);
i64_unary_list!(i64_unary_counts);
float_binary_list!(float_binary_counts);
float_unary_list!(float_unary_counts);
scalar_conversion_list!(conversion_counts);

/// The rows that once compiled with a constant-pool load: none now, since their float bounds and
/// masks are built from integer immediates (`wasm_rows_convert.rs`). Kept so the library and its
/// tests name one list.
pub const CONVERT_NEEDS_CONSTANTS: [u32; 0] = [];
pub const FLOAT_BINARY_NEEDS_CONSTANTS: [u32; 0] = [];

/// The SIMD operators' families, from the list (`wasm_lists.rs`): each operator's family, the column the
/// row families are built from.
macro_rules! simd_family_table {
    ($($name:ident $([$lane:ident])? : $width:expr, $left:ident, $right:expr, $result:ident, $family:ident => |$a:ident, $b:ident, $l:ident| $body:expr;)*) => {
        /// Each SIMD operator's family code, by tag.
        pub const SIMD_FAMILY_OF: &[u8] = &[$(SimdFamily::$family as u8,)*];
    };
}
simd_operator_list!(simd_family_table);

/// The `Unary` family's operator tags, in tag order.
pub const SIMD_UNARY_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::Unary as u8) }>(SIMD_FAMILY_OF, SimdFamily::Unary as u8);
/// The `Binary` family's operator tags, in tag order.
pub const SIMD_BINARY_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::Binary as u8) }>(SIMD_FAMILY_OF, SimdFamily::Binary as u8);
/// The `Splat` family's operator tags, in tag order.
pub const SIMD_SPLAT_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::Splat as u8) }>(SIMD_FAMILY_OF, SimdFamily::Splat as u8);
/// The `Extract` family's operator tags, in tag order.
pub const SIMD_EXTRACT_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::Extract as u8) }>(SIMD_FAMILY_OF, SimdFamily::Extract as u8);
/// The `Replace` family's operator tags, in tag order.
pub const SIMD_REPLACE_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::Replace as u8) }>(SIMD_FAMILY_OF, SimdFamily::Replace as u8);
/// The `MoreBinary` family's operator tags, in tag order.
pub const SIMD_MORE_BINARY_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreBinary as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreBinary as u8);
/// The `MoreCompare` family's operator tags, in tag order.
pub const SIMD_MORE_COMPARE_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreCompare as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreCompare as u8);
/// The `MoreFloat` family's operator tags, in tag order.
pub const SIMD_MORE_FLOAT_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreFloat as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreFloat as u8);
/// The `MoreShift` family's operator tags, in tag order.
pub const SIMD_MORE_SHIFT_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreShift as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreShift as u8);
/// The `MoreTest` family's operator tags, in tag order.
pub const SIMD_MORE_TEST_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreTest as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreTest as u8);
/// The `MoreUnary` family's operator tags, in tag order.
pub const SIMD_MORE_UNARY_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreUnary as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreUnary as u8);
/// The `MoreWiden` family's operator tags, in tag order.
pub const SIMD_MORE_WIDEN_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreWiden as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreWiden as u8);
/// The `MoreWide` family's operator tags, in tag order.
pub const SIMD_MORE_WIDE_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreWide as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreWide as u8);
/// The `MoreConvert` family's operator tags, in tag order.
pub const SIMD_MORE_CONVERT_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreConvert as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreConvert as u8);

/// The memory loads' families, from the list: each load's family, by tag.
macro_rules! memory_load_family_table {
    ($($name:ident $family:ident),* $(,)?) => {
        /// Each memory load's family code, by tag.
        pub const MEMORY_LOAD_FAMILY_OF: &[u8] = &[$(LoadFamily::$family as u8,)*];
    };
}
memory_load_list!(memory_load_family_table);

/// The vector loads a `SIMD_MORE_LOAD` row computes (the splatting, widening and zeroing loads), by tag.
pub const SIMD_MORE_LOAD_OPS: &[u32] = &tags_of::<{ count_of(MEMORY_LOAD_FAMILY_OF, LoadFamily::Vector as u8) }>(MEMORY_LOAD_FAMILY_OF, LoadFamily::Vector as u8);

