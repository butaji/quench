//! One row per builtin function that a method table installs: the native, the object that
//! holds it, its property name, its `length` and its flags. The rows are written in the
//! child modules (`base`, `collections`, `objects`, `typed`, `promises`), one table per domain,
//! and concatenated into `NATIVE_ROWS`. The installation tables (`ARRAY_PROTOTYPE_METHODS`,
//! `STRING_METHODS`, `MATH_FUNCTIONS`, ...) and the lengths of these natives
//! (`native_lengths::ROW_LENGTHS`) are derived from the rows, so each fact about one of these
//! builtins is stated once.
//!
//! The `Native` enum is declared by `natives!` in `heap/cell.rs`, whose order fixes the discriminants
//! that tables index and snapshots serialize; the rows name its variants and change none of them.
//! The flag predicates (`is_math_function`, `is_pure_string_method`) read the rows' flags, so each
//! flag is stated once, in its row.

use crate::heap::Native;

/// The object whose own property a row's function is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// `Array.prototype`.
    ArrayPrototype,
    /// The `Array` constructor.
    ArrayConstructor,
    /// `String.prototype`.
    StringPrototype,
    /// The `Math` namespace.
    Math,
    /// `Number`: its own `isNaN`, `isFinite`, `isInteger`, `isSafeInteger`, `parseInt`, `parseFloat`.
    NumberConstructor,
    /// `Number.prototype`.
    NumberPrototype,
    /// The global object: `parseInt`, `parseFloat`, `isNaN`, `isFinite`.
    GlobalObject,
    /// `Map.prototype`.
    MapPrototype,
    /// `Set.prototype`.
    SetPrototype,
    /// `WeakMap.prototype`.
    WeakMapPrototype,
    /// `WeakSet.prototype`.
    WeakSetPrototype,
    /// `DataView.prototype`.
    DataViewPrototype,
    /// The `String` constructor: `fromCharCode`, `fromCodePoint`, `raw`.
    StringConstructor,
    /// The `Object` constructor's statics.
    ObjectConstructor,
    /// `Object.prototype`.
    ObjectPrototype,
    /// `Function.prototype`.
    FunctionPrototype,
    /// The `Reflect` namespace.
    Reflect,
    /// The `JSON` namespace.
    Json,
    /// The `Proxy` constructor: `revocable`.
    ProxyConstructor,
    /// The `Symbol` constructor: `for`, `keyFor`.
    SymbolConstructor,
    /// `Symbol.prototype`.
    SymbolPrototype,
    /// `Boolean.prototype`.
    BooleanPrototype,
    /// `Date.prototype`.
    DatePrototype,
    /// The `Date` constructor: `now`, `parse`, `UTC`.
    DateConstructor,
    /// `%TypedArray%.prototype`.
    TypedArrayPrototype,
    /// `%TypedArray%`: `from`, `of`.
    TypedArrayConstructor,
    /// `Uint8Array.prototype`'s own base64 and hex methods.
    Uint8ArrayPrototype,
    /// `Uint8Array`'s `fromBase64` and `fromHex`.
    Uint8ArrayConstructor,
    /// `Promise.prototype`.
    PromisePrototype,
    /// The `Promise` constructor's statics.
    PromiseConstructor,
}

/// A function of `Math` (`Native::is_math_function`).
pub(super) const MATH: u8 = 1 << 0;
/// A pure `String.prototype` method (`Native::is_pure_string_method`).
pub(super) const PURE_STRING: u8 = 1 << 1;
/// A method a compiled lane runs inline by name (`lane_sites`): `charCodeAt`, `charAt`, `push`, `pop`, and the fixed-width DataView getters and setters.
pub(super) const LANE_METHOD: u8 = 1 << 2;

/// A row's `length`: stated, or shared with an Array native. The `%TypedArray%.prototype` callbacks
/// run the Array method of the same name, which the spec gives the same length: `SameAs` names it.
#[derive(Clone, Copy)]
pub(super) enum Length {
    /// The `length` the specification gives the function.
    Fixed(u8),
    /// The length of the Array native this row's function runs.
    SameAs(Native),
}

/// A builtin function as its holder installs it.
#[derive(Clone, Copy)]
pub(super) struct NativeRow {
    pub holder: Holder,
    /// The property name, and the function's `name`.
    pub(super) name: &'static str,
    pub native: Native,
    /// The function's `length`.
    pub(super) length: Length,
    /// `MATH`, `PURE_STRING`, `LANE_METHOD`.
    pub(super) flags: u8,
}

/// A row's `length` as its macro token states it: a number or a `[ArrayNative]` shared length.
macro_rules! length_of {
    ([$twin:ident]) => {
        Length::SameAs(Native::$twin)
    };
    ($fixed:expr) => {
        Length::Fixed(($fixed) as u8)
    };
}

/// `(Holder, "name", NativeVariant, length, FLAG...)` rows to a table's `ROWS`. A length is a
/// number, a named constant, or `[ArrayNative]` for a shared length. Rows of one holder keep
/// their order, which is the order its properties are installed in.
macro_rules! row_table {
    ($( ($holder:ident, $name:literal, $native:ident, $length:tt $(, $flag:ident)*) ),* $(,)?) => {
        /// This table's rows, in installation order.
        pub(super) const ROWS: &[NativeRow] = &[
            $( NativeRow {
                holder: Holder::$holder,
                name: $name,
                native: Native::$native,
                length: length_of!($length),
                flags: 0 $(| $flag)*,
            }, )*
        ];
    };
}

mod base;
mod collections;
mod objects;
mod promises;
mod typed;

/// The rows of every table, one table per module, in table order.
const TABLES: [&[NativeRow]; 5] = [base::ROWS, collections::ROWS, objects::ROWS, typed::ROWS, promises::ROWS];
/// The number of rows in `TABLES`.
const ROW_COUNT: usize = row_count(&TABLES);
/// Every row, in table order: each holder's rows keep their installation order.
pub(super) const NATIVE_ROWS: [NativeRow; ROW_COUNT] = concat_rows(&TABLES);

/// The number of rows in `tables`.
const fn row_count(tables: &[&[NativeRow]]) -> usize {
    let mut count = 0;
    let mut table = 0;
    while table < tables.len() {
        count += tables[table].len();
        table += 1;
    }
    count
}

/// The rows of `tables`, concatenated in order; `N` is `row_count(tables)`.
const fn concat_rows<const N: usize>(tables: &[&[NativeRow]]) -> [NativeRow; N] {
    let mut rows = [FILLER_ROW; N];
    let mut filled = 0;
    let mut table = 0;
    while table < tables.len() {
        let part = tables[table];
        let mut index = 0;
        while index < part.len() {
            rows[filled] = part[index];
            filled += 1;
            index += 1;
        }
        table += 1;
    }
    assert!(filled == N, "concat_rows: N is not the row count");
    rows
}

/// The placeholder `concat_rows` starts from; every slot is overwritten.
const FILLER_ROW: NativeRow = NativeRow {
    holder: Holder::Math,
    name: "",
    native: Native::Print,
    length: Length::Fixed(0),
    flags: 0,
};

/// `row`'s `length`: a fixed one as written, a shared one as the Array native's row states it.
pub(super) const fn resolved_length(row: NativeRow) -> u8 {
    match row.length {
        Length::Fixed(length) => length,
        Length::SameAs(native) => match fixed_length(native) {
            Some(length) => length,
            None => panic!("a shared length names a native with no fixed length"),
        },
    }
}

/// The fixed `length` of the first row that names `native`, if one does.
const fn fixed_length(native: Native) -> Option<u8> {
    let mut index = 0;
    while index < NATIVE_ROWS.len() {
        let row = NATIVE_ROWS[index];
        if row.native as u16 == native as u16
            && let Length::Fixed(length) = row.length
        {
            return Some(length);
        }
        index += 1;
    }
    None
}

/// The name `holder` installs `native` under, if it has a row for it.
pub(super) fn method_name(holder: Holder, native: Native) -> Option<&'static str> {
    NATIVE_ROWS.iter().find(|row| row.holder == holder && row.native == native).map(|row| row.name)
}

/// The name of some row that names `native`: a native's `name` as its holder installs it.
pub(super) fn row_name(native: Native) -> Option<&'static str> {
    NATIVE_ROWS.iter().find(|row| row.native == native).map(|row| row.name)
}

/// The Array native a `%TypedArray%.prototype` callback runs, if `native` is one.
pub(super) fn shared_array_native(native: Native) -> Option<Native> {
    NATIVE_ROWS.iter().find_map(|row| match row.length {
        Length::SameAs(twin) if row.native == native => Some(twin),
        _ => None,
    })
}

/// The rows a compiled lane runs inline by name (`LANE_METHOD`).
pub fn lane_method(name: &str) -> Option<NativeRow> {
    NATIVE_ROWS.iter().copied().find(|row| row.flags & LANE_METHOD != 0 && row.name == name)
}

/// The first row named `name` of a collection prototype that installs a native a compiled loop calls in
/// place (`crate::vm::engine_calls::in_place_in_loop`).
pub fn in_place_method(name: &str) -> Option<NativeRow> {
    NATIVE_ROWS.iter().copied().find(|row| row.name == name && crate::vm::engine_calls::in_place_in_loop(row.native)
        && matches!(row.holder, Holder::MapPrototype | Holder::SetPrototype | Holder::WeakMapPrototype | Holder::WeakSetPrototype))
}

/// The number of rows `holder` holds.
const fn holder_rows(holder: Holder) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < NATIVE_ROWS.len() {
        if NATIVE_ROWS[index].holder as u8 == holder as u8 {
            count += 1;
        }
        index += 1;
    }
    count
}

/// `holder`'s `(name, native)` pairs, in row order; `N` is `holder_rows(holder)`.
const fn named<const N: usize>(holder: Holder) -> [(&'static str, Native); N] {
    let mut pairs = [("", Native::Print); N];
    let mut filled = 0;
    let mut index = 0;
    while index < NATIVE_ROWS.len() {
        let row = NATIVE_ROWS[index];
        if row.holder as u8 == holder as u8 {
            pairs[filled] = (row.name, row.native);
            filled += 1;
        }
        index += 1;
    }
    assert!(filled == N, "named: N is not the holder's row count");
    pairs
}

/// `holder`'s `(name, native, length)` triples, in row order; `N` is `holder_rows(holder)`.
const fn named_with_lengths<const N: usize>(holder: Holder) -> [(&'static str, Native, f64); N] {
    let mut triples = [("", Native::Print, 0.0); N];
    let mut filled = 0;
    let mut index = 0;
    while index < NATIVE_ROWS.len() {
        let row = NATIVE_ROWS[index];
        if row.holder as u8 == holder as u8 {
            triples[filled] = (row.name, row.native, resolved_length(row) as f64);
            filled += 1;
        }
        index += 1;
    }
    assert!(filled == N, "named_with_lengths: N is not the holder's row count");
    triples
}

const ARRAY_PROTOTYPE_ROWS: [(&str, Native); holder_rows(Holder::ArrayPrototype)] =
    named(Holder::ArrayPrototype);
const ARRAY_CONSTRUCTOR_ROWS: [(&str, Native); holder_rows(Holder::ArrayConstructor)] =
    named(Holder::ArrayConstructor);
const STRING_PROTOTYPE_ROWS: [(&str, Native, f64); holder_rows(Holder::StringPrototype)] =
    named_with_lengths(Holder::StringPrototype);
const MATH_ROWS: [(&str, Native); holder_rows(Holder::Math)] = named(Holder::Math);

/// `Array.prototype`'s methods, in installation order.
pub(super) const ARRAY_PROTOTYPE_METHODS: &[(&str, Native)] = &ARRAY_PROTOTYPE_ROWS;
/// `Array`'s static methods, in installation order.
pub(super) const ARRAY_STATIC_METHODS: &[(&str, Native)] = &ARRAY_CONSTRUCTOR_ROWS;
/// `String.prototype`'s methods with their lengths, in installation order.
pub(super) const STRING_METHODS: &[(&str, Native, f64)] = &STRING_PROTOTYPE_ROWS;
/// `Math`'s functions, in installation order.
pub(super) const MATH_FUNCTIONS: &[(&str, Native)] = &MATH_ROWS;

/// Whether `left` and `right` spell the same text.
const fn same_name(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// Each native's row flags by discriminant: the union of the flags of the rows that name it.
const NATIVE_FLAGS: [u8; Native::COUNT as usize] = {
    let mut flags = [0; Native::COUNT as usize];
    let mut index = 0;
    while index < NATIVE_ROWS.len() {
        flags[NATIVE_ROWS[index].native as usize] |= NATIVE_ROWS[index].flags;
        index += 1;
    }
    flags
};

impl Native {
    /// `String.prototype` methods that, for a string primitive receiver and arguments that
    /// are not objects, run no guest code and have no effect beyond allocating their result.
    /// `split` looks `Symbol.split` up only on an object separator (ES2025), so it is among
    /// them; `match` and `search`, which make a RegExp of a string, are not.
    #[inline]
    pub const fn is_pure_string_method(self) -> bool {
        NATIVE_FLAGS[self as usize] & PURE_STRING != 0
    }

    /// The functions of the `Math` object: the natives of `Math`'s rows (`MATH_FUNCTIONS`).
    pub const fn is_math_function(self) -> bool {
        NATIVE_FLAGS[self as usize] & MATH != 0
    }
}

// Each row flagged `MATH` is a `Math` row, and each alias (a native on several holders) agrees
// with the rows it aliases on length, flags and name.
const _: () = {
    let mut index = 0;
    while index < NATIVE_ROWS.len() {
        let row = NATIVE_ROWS[index];
        let math = row.flags & MATH != 0;
        assert!(math == (row.holder as u8 == Holder::Math as u8), "MATH flags exactly the Math rows");
        // A native on two holders is an alias: its rows agree on name, length and flags.
        let mut later = index + 1;
        while later < NATIVE_ROWS.len() {
            let other = NATIVE_ROWS[later];
            if other.native as u16 == row.native as u16 {
                assert!(
                    resolved_length(other) == resolved_length(row) && other.flags == row.flags,
                    "alias rows disagree on length or flags"
                );
                assert!(same_name(other.name, row.name), "alias rows disagree on name");
            }
            later += 1;
        }
        index += 1;
    }
};
