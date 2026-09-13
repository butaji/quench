//! Array method metadata.

use crate::ops::Builtin;

// Array and typed-array intrinsics are one semantic family.  Keep the
// observable name and the Function#length metadata beside each other so a
// new method cannot drift between the two dispatch tables.
macro_rules! declare_array_method_metadata {
    ($( $builtin:ident => $name:literal, $length:expr ),+ $(,)?) => {
        const fn method_name(builtin: Builtin) -> Option<&'static str> {
            match builtin {
                $(Builtin::$builtin => Some($name),)+
                _ => None,
            }
        }

        const fn method_length(builtin: Builtin) -> Option<f64> {
            match builtin {
                $(Builtin::$builtin => Some($length),)+
                _ => None,
            }
        }
    };
}

declare_array_method_metadata! {
    ArrayMap => "map", 1.0,
    ArrayFilter => "filter", 1.0,
    ArraySome => "some", 1.0,
    ArrayEvery => "every", 1.0,
    TypedArrayEvery => "every", 1.0,
    TypedArraySome => "some", 1.0,
    TypedArrayMap => "map", 1.0,
    TypedArrayFilter => "filter", 1.0,
    TypedArraySlice => "slice", 2.0,
    ArrayFind => "find", 1.0,
    TypedArrayFind => "find", 1.0,
    ArrayFindIndex => "findIndex", 1.0,
    TypedArrayFindIndex => "findIndex", 1.0,
    ArrayIterator => "values", 0.0,
    TypedArrayIterator => "values", 0.0,
    ArrayKeys => "keys", 0.0,
    TypedArrayKeys => "keys", 0.0,
    ArrayEntries => "entries", 0.0,
    TypedArrayEntries => "entries", 0.0,
    ArrayIncludes => "includes", 1.0,
    TypedArrayIncludes => "includes", 1.0,
    ArrayIndexOf => "indexOf", 1.0,
    TypedArrayIndexOf => "indexOf", 1.0,
    ArrayLastIndexOf => "lastIndexOf", 1.0,
    TypedArrayLastIndexOf => "lastIndexOf", 1.0,
    ArraySlice => "slice", 2.0,
    ArrayConcat => "concat", 1.0,
    ArrayFlat => "flat", 0.0,
    ArrayFlatMap => "flatMap", 1.0,
    ArrayAt => "at", 1.0,
    TypedArrayAt => "at", 1.0,
    ArraySort => "sort", 1.0,
    TypedArraySort => "sort", 1.0,
    TypedArrayWith => "with", 2.0,
    ArrayForEach => "forEach", 1.0,
    TypedArrayForEach => "forEach", 1.0,
    ArrayReduce => "reduce", 1.0,
    ArrayReduceRight => "reduceRight", 1.0,
    TypedArrayReduce => "reduce", 1.0,
    TypedArrayReduceRight => "reduceRight", 1.0,
    ArrayPush => "push", 1.0,
    ArrayShift => "shift", 0.0,
    ArrayReverse => "reverse", 0.0,
    TypedArrayReverse => "reverse", 0.0,
    TypedArrayCopyWithin => "copyWithin", 2.0,
    ArrayFindLast => "findLast", 1.0,
    TypedArrayFindLast => "findLast", 1.0,
    ArrayFindLastIndex => "findLastIndex", 1.0,
    TypedArrayFindLastIndex => "findLastIndex", 1.0,
    TypedArrayFill => "fill", 1.0,
    ArrayPop => "pop", 0.0,
    ArrayUnshift => "unshift", 1.0,
    ArrayFill => "fill", 1.0,
    ArrayCopyWithin => "copyWithin", 2.0,
    ArrayToSorted => "toSorted", 1.0,
    TypedArrayToSorted => "toSorted", 1.0,
    ArrayToReversed => "toReversed", 0.0,
    TypedArrayToReversed => "toReversed", 0.0,
    ArrayToSpliced => "toSpliced", 2.0,
    ArrayWith => "with", 2.0,
    ArrayToString => "toString", 0.0,
    ArraySplice => "splice", 2.0,
    ArrayJoin => "join", 1.0,
    TypedArrayJoin => "join", 1.0,
    ArrayToLocaleString => "toLocaleString", 0.0,
    TypedArrayToLocaleString => "toLocaleString", 0.0,
    TypedArraySet => "set", 1.0,
}

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::ArrayIsArray => Some("isArray"),
        Builtin::ArrayFrom => Some("from"),
        Builtin::ArrayFromAsync => Some("fromAsync"),
        Builtin::ArrayOf => Some("of"),
        Builtin::TypedArrayFrom => Some("from"),
        Builtin::TypedArrayOf => Some("of"),
        Builtin::Uint8ArrayFromBase64 => Some("fromBase64"),
        Builtin::Uint8ArrayFromHex => Some("fromHex"),
        Builtin::Uint8ArraySetFromBase64 => Some("setFromBase64"),
        Builtin::Uint8ArraySetFromHex => Some("setFromHex"),
        Builtin::Uint8ArrayToBase64 => Some("toBase64"),
        Builtin::Uint8ArrayToHex => Some("toHex"),
        Builtin::Uint8ArraySubarray => Some("subarray"),
        _ => fn_name_methods(builtin),
    }
}

const fn fn_name_methods(builtin: Builtin) -> Option<&'static str> {
    method_name(builtin)
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::ArrayIsArray
        | Builtin::ArrayFrom
        | Builtin::ArrayFromAsync
        | Builtin::TypedArrayFrom => Some(1.0),
        Builtin::ArrayOf => Some(0.0),
        Builtin::TypedArrayOf => Some(0.0),
        Builtin::Uint8ArrayFromBase64
        | Builtin::Uint8ArrayFromHex
        | Builtin::Uint8ArraySetFromBase64
        | Builtin::Uint8ArraySetFromHex => Some(1.0),
        Builtin::Uint8ArrayToBase64 | Builtin::Uint8ArrayToHex => Some(0.0),
        Builtin::Uint8ArraySubarray => Some(2.0),
        _ => method_length(builtin),
    }
}

pub const fn short_name(_b: Builtin) -> Option<&'static str> {
    None
}
