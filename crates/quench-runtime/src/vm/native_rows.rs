//! One row per builtin function that a method table installs: the native, the object that
//! holds it, its property name, its `length` and its flags. The installation tables
//! (`ARRAY_PROTOTYPE_METHODS`, `ARRAY_STATIC_METHODS`, `STRING_METHODS`, `MATH_FUNCTIONS`) and
//! the lengths of these natives (`native_lengths::ROW_LENGTHS`) are derived from the rows, so
//! each fact about one of these builtins is stated once, here.
//!
//! The `Native` enum itself stays in `heap/cell.rs`, whose order fixes the discriminants that
//! tables index and snapshots serialize; the rows name its variants and change none of them.

use crate::heap::Native;

/// The object whose own property a row's function is.
#[derive(Clone, Copy)]
pub(super) enum Holder {
    /// `Array.prototype`.
    ArrayPrototype,
    /// The `Array` constructor.
    ArrayConstructor,
    /// `String.prototype`.
    StringPrototype,
    /// The `Math` namespace.
    Math,
}

/// A function of `Math` (`Native::is_math_function`).
pub(super) const MATH: u8 = 1 << 0;
/// A pure `String.prototype` method (`Native::is_pure_string_method`).
pub(super) const PURE_STRING: u8 = 1 << 1;

/// A builtin function as its holder installs it.
#[derive(Clone, Copy)]
pub(super) struct NativeRow {
    pub(super) holder: Holder,
    /// The property name, and the function's `name`.
    pub(super) name: &'static str,
    pub(super) native: Native,
    /// The function's `length`, as the specification gives it.
    pub(super) length: u8,
    /// `MATH`, `PURE_STRING`.
    pub(super) flags: u8,
}

/// `(Holder, "name", NativeVariant, length, FLAG...)` rows to `NATIVE_ROWS`. Rows of one holder
/// keep their order, which is the order its properties are installed in.
macro_rules! natives {
    ($( ($holder:ident, $name:literal, $native:ident, $length:literal $(, $flag:ident)*) ),* $(,)?) => {
        /// Every row, grouped by holder, each holder's rows in installation order.
        pub(super) const NATIVE_ROWS: &[NativeRow] = &[
            $( NativeRow {
                holder: Holder::$holder,
                name: $name,
                native: Native::$native,
                length: $length,
                flags: 0 $(| $flag)*,
            }, )*
        ];
    };
}

natives! {
    // `Array.prototype`, in installation order.
    (ArrayPrototype, "push", ArrayPush, 1),
    (ArrayPrototype, "pop", ArrayPop, 0),
    (ArrayPrototype, "slice", ArraySlice, 2),
    (ArrayPrototype, "includes", ArrayIncludes, 1),
    (ArrayPrototype, "join", ArrayJoin, 1),
    (ArrayPrototype, "concat", ArrayConcat, 1),
    (ArrayPrototype, "flat", ArrayFlat, 0),
    (ArrayPrototype, "reverse", ArrayReverse, 0),
    (ArrayPrototype, "shift", ArrayShift, 0),
    (ArrayPrototype, "unshift", ArrayUnshift, 1),
    (ArrayPrototype, "splice", ArraySplice, 2),
    (ArrayPrototype, "fill", ArrayFill, 1),
    (ArrayPrototype, "at", ArrayAt, 1),
    (ArrayPrototype, "lastIndexOf", ArrayLastIndexOf, 1),
    (ArrayPrototype, "indexOf", ArrayIndexOf, 1),
    (ArrayPrototype, "copyWithin", ArrayCopyWithin, 2),
    (ArrayPrototype, "with", ArrayWith, 2),
    (ArrayPrototype, "forEach", ArrayForEach, 1),
    (ArrayPrototype, "map", ArrayMap, 1),
    (ArrayPrototype, "filter", ArrayFilter, 1),
    (ArrayPrototype, "some", ArraySome, 1),
    (ArrayPrototype, "every", ArrayEvery, 1),
    (ArrayPrototype, "find", ArrayFind, 1),
    (ArrayPrototype, "findIndex", ArrayFindIndex, 1),
    (ArrayPrototype, "findLast", ArrayFindLast, 1),
    (ArrayPrototype, "findLastIndex", ArrayFindLastIndex, 1),
    (ArrayPrototype, "flatMap", ArrayFlatMap, 1),
    (ArrayPrototype, "reduce", ArrayReduce, 1),
    (ArrayPrototype, "reduceRight", ArrayReduceRight, 1),
    (ArrayPrototype, "toReversed", ArrayToReversed, 0),
    (ArrayPrototype, "toSpliced", ArrayToSpliced, 2),
    (ArrayPrototype, "sort", ArraySort, 1),
    (ArrayPrototype, "toSorted", ArrayToSorted, 1),
    (ArrayPrototype, "toString", ArrayToString, 0),
    (ArrayPrototype, "toLocaleString", ArrayToLocaleString, 0),
    (ArrayPrototype, "keys", ArrayKeys, 0),
    (ArrayPrototype, "values", ArrayValues, 0),
    (ArrayPrototype, "entries", ArrayEntries, 0),
    // `Array`, in installation order.
    (ArrayConstructor, "from", ArrayFrom, 1),
    (ArrayConstructor, "fromAsync", ArrayFromAsync, 1),
    (ArrayConstructor, "of", ArrayOf, 0),
    (ArrayConstructor, "isArray", ArrayIsArray, 1),
    // `String.prototype`, in installation order (the HTML methods and aliases follow, from `string.rs`).
    (StringPrototype, "at", StringAt, 1, PURE_STRING),
    (StringPrototype, "charAt", StringCharAt, 1, PURE_STRING),
    (StringPrototype, "charCodeAt", StringCharCodeAt, 1, PURE_STRING),
    (StringPrototype, "codePointAt", StringCodePointAt, 1, PURE_STRING),
    (StringPrototype, "concat", StringConcat, 1, PURE_STRING),
    (StringPrototype, "endsWith", StringEndsWith, 1, PURE_STRING),
    (StringPrototype, "includes", StringIncludes, 1, PURE_STRING),
    (StringPrototype, "isWellFormed", StringIsWellFormed, 0),
    (StringPrototype, "indexOf", StringIndexOf, 1, PURE_STRING),
    (StringPrototype, "lastIndexOf", StringLastIndexOf, 1, PURE_STRING),
    (StringPrototype, "localeCompare", StringLocaleCompare, 1),
    (StringPrototype, "match", StringMatch, 1),
    (StringPrototype, "matchAll", StringMatchAll, 1),
    (StringPrototype, "normalize", StringNormalize, 0),
    (StringPrototype, "padEnd", StringPadEnd, 1, PURE_STRING),
    (StringPrototype, "padStart", StringPadStart, 1, PURE_STRING),
    (StringPrototype, "repeat", StringRepeat, 1, PURE_STRING),
    (StringPrototype, "replace", StringReplace, 2),
    (StringPrototype, "replaceAll", StringReplaceAll, 2),
    (StringPrototype, "search", StringSearch, 1),
    (StringPrototype, "slice", StringSlice, 2, PURE_STRING),
    (StringPrototype, "split", StringSplit, 2, PURE_STRING),
    (StringPrototype, "startsWith", StringStartsWith, 1, PURE_STRING),
    (StringPrototype, "substr", StringSubstr, 2, PURE_STRING),
    (StringPrototype, "substring", StringSubstring, 2, PURE_STRING),
    (StringPrototype, "toLocaleLowerCase", StringToLocaleLowerCase, 0),
    (StringPrototype, "toLocaleUpperCase", StringToLocaleUpperCase, 0),
    (StringPrototype, "toLowerCase", StringToLowerCase, 0, PURE_STRING),
    (StringPrototype, "toString", StringToString, 0),
    (StringPrototype, "toWellFormed", StringToWellFormed, 0),
    (StringPrototype, "toUpperCase", StringToUpperCase, 0, PURE_STRING),
    (StringPrototype, "trim", StringTrim, 0, PURE_STRING),
    (StringPrototype, "trimEnd", StringTrimEnd, 0, PURE_STRING),
    (StringPrototype, "trimStart", StringTrimStart, 0, PURE_STRING),
    (StringPrototype, "valueOf", StringValueOf, 0),
    // `Math`, in installation order (after its value properties).
    (Math, "abs", MathAbs, 1, MATH),
    (Math, "acos", MathAcos, 1, MATH),
    (Math, "acosh", MathAcosh, 1, MATH),
    (Math, "asin", MathAsin, 1, MATH),
    (Math, "asinh", MathAsinh, 1, MATH),
    (Math, "atan", MathAtan, 1, MATH),
    (Math, "atanh", MathAtanh, 1, MATH),
    (Math, "atan2", MathAtan2, 2, MATH),
    (Math, "ceil", MathCeil, 1, MATH),
    (Math, "cbrt", MathCbrt, 1, MATH),
    (Math, "expm1", MathExpm1, 1, MATH),
    (Math, "clz32", MathClz32, 1, MATH),
    (Math, "cos", MathCos, 1, MATH),
    (Math, "cosh", MathCosh, 1, MATH),
    (Math, "exp", MathExp, 1, MATH),
    (Math, "floor", MathFloor, 1, MATH),
    (Math, "fround", MathFround, 1, MATH),
    (Math, "f16round", MathF16Round, 1, MATH),
    (Math, "hypot", MathHypot, 2, MATH),
    (Math, "imul", MathImul, 2, MATH),
    (Math, "log", MathLog, 1, MATH),
    (Math, "log1p", MathLog1p, 1, MATH),
    (Math, "log2", MathLog2, 1, MATH),
    (Math, "log10", MathLog10, 1, MATH),
    (Math, "max", MathMax, 2, MATH),
    (Math, "min", MathMin, 2, MATH),
    (Math, "pow", MathPow, 2, MATH),
    (Math, "random", MathRandom, 0, MATH),
    (Math, "round", MathRound, 1, MATH),
    (Math, "sign", MathSign, 1, MATH),
    (Math, "sin", MathSin, 1, MATH),
    (Math, "sinh", MathSinh, 1, MATH),
    (Math, "sqrt", MathSqrt, 1, MATH),
    (Math, "sumPrecise", MathSumPrecise, 1, MATH),
    (Math, "tan", MathTan, 1, MATH),
    (Math, "tanh", MathTanh, 1, MATH),
    (Math, "trunc", MathTrunc, 1, MATH),
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
            triples[filled] = (row.name, row.native, row.length as f64);
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

/// Whether some row names `native`.
const fn has_row(native: Native) -> bool {
    let mut index = 0;
    while index < NATIVE_ROWS.len() {
        if NATIVE_ROWS[index].native as u16 == native as u16 {
            return true;
        }
        index += 1;
    }
    false
}

// The rows agree with the predicates `heap/cell.rs` keeps beside the enum: each native has one
// row, a row is flagged `MATH` exactly when `Math` holds it and the native is a Math function,
// `PURE_STRING` exactly when the native is a pure string method, and every native either
// predicate names has a row.
const _: () = {
    let mut index = 0;
    while index < NATIVE_ROWS.len() {
        let row = NATIVE_ROWS[index];
        let math = row.flags & MATH != 0;
        assert!(math == (row.holder as u8 == Holder::Math as u8), "MATH flags exactly the Math rows");
        assert!(math == row.native.is_math_function(), "MATH flag disagrees with is_math_function");
        assert!(
            (row.flags & PURE_STRING != 0) == row.native.is_pure_string_method(),
            "PURE_STRING flag disagrees with is_pure_string_method"
        );
        let mut later = index + 1;
        while later < NATIVE_ROWS.len() {
            assert!(NATIVE_ROWS[later].native as u16 != row.native as u16, "a native has two rows");
            later += 1;
        }
        index += 1;
    }
    let mut discriminant = 0;
    while discriminant < Native::COUNT {
        let native = Native::from_discriminant(discriminant).unwrap();
        if native.is_math_function() || native.is_pure_string_method() {
            assert!(has_row(native), "a Math function or pure string method has no row");
        }
        discriminant += 1;
    }
};
