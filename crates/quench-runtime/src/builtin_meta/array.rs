//! Array method metadata.

use crate::ops::Builtin;

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::ArrayIsArray => Some("isArray"),
        Builtin::ArrayFrom => Some("from"),
        Builtin::ArrayFromAsync => Some("fromAsync"),
        Builtin::TypedArrayFrom => Some("from"),
        Builtin::TypedArrayOf => Some("of"),
        Builtin::Uint8ArrayFromBase64 => Some("fromBase64"),
        Builtin::Uint8ArrayFromHex => Some("fromHex"),
        Builtin::Uint8ArraySetFromBase64 => Some("setFromBase64"),
        Builtin::Uint8ArraySetFromHex => Some("setFromHex"),
        Builtin::Uint8ArrayToBase64 => Some("toBase64"),
        Builtin::Uint8ArrayToHex => Some("toHex"),
        Builtin::Uint8ArraySubarray => Some("subarray"),
        Builtin::ArrayMap => Some("map"),
        Builtin::ArrayFilter => Some("filter"),
        Builtin::ArraySome => Some("some"),
        Builtin::ArrayEvery => Some("every"),
        Builtin::ArrayFind => Some("find"),
        Builtin::ArrayIterator => Some("values"),
        Builtin::ArrayKeys => Some("keys"),
        Builtin::ArrayEntries => Some("entries"),
        Builtin::ArrayIncludes => Some("includes"),
        Builtin::ArrayIndexOf => Some("indexOf"),
        Builtin::ArrayLastIndexOf => Some("lastIndexOf"),
        Builtin::ArraySlice | Builtin::ArrayBufferSlice => Some("slice"),
        Builtin::ArrayConcat => Some("concat"),
        Builtin::ArrayFlat => Some("flat"),
        Builtin::ArrayFlatMap => Some("flatMap"),
        Builtin::ArrayAt => Some("at"),
        Builtin::ArraySort => Some("sort"),
        Builtin::ArrayForEach => Some("forEach"),
        Builtin::ArrayReduce => Some("reduce"),
        Builtin::ArrayReduceRight => Some("reduceRight"),
        Builtin::ArrayPush => Some("push"),
        Builtin::ArrayShift => Some("shift"),
        Builtin::ArrayReverse => Some("reverse"),
        Builtin::ArrayFindLast => Some("findLast"),
        Builtin::ArrayFindLastIndex => Some("findLastIndex"),
        _ => fn_name_tail(builtin),
    }
}

const fn fn_name_tail(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::ArrayPop => Some("pop"),
        Builtin::ArrayUnshift => Some("unshift"),
        Builtin::ArrayFill => Some("fill"),
        Builtin::ArrayCopyWithin => Some("copyWithin"),
        Builtin::ArrayToSorted => Some("toSorted"),
        Builtin::ArrayToReversed => Some("toReversed"),
        Builtin::ArraySplice => Some("splice"),
        Builtin::ArrayJoin => Some("join"),
        Builtin::ArrayToLocaleString => Some("toLocaleString"),
        _ => None,
    }
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::ArrayIsArray
        | Builtin::ArrayFrom
        | Builtin::ArrayFromAsync
        | Builtin::TypedArrayFrom => Some(1.0),
        Builtin::TypedArrayOf => Some(0.0),
        Builtin::Uint8ArrayFromBase64
        | Builtin::Uint8ArrayFromHex
        | Builtin::Uint8ArraySetFromBase64
        | Builtin::Uint8ArraySetFromHex => Some(1.0),
        Builtin::Uint8ArrayToBase64 | Builtin::Uint8ArrayToHex => Some(0.0),
        Builtin::Uint8ArraySubarray => Some(2.0),
        Builtin::ArrayMap
        | Builtin::ArrayFilter
        | Builtin::ArraySome
        | Builtin::ArrayEvery
        | Builtin::ArrayFind
        | Builtin::ArrayFindLast
        | Builtin::ArrayFindLastIndex
        | Builtin::ArrayIncludes
        | Builtin::ArrayIndexOf
        | Builtin::ArrayLastIndexOf
        | Builtin::ArrayFlatMap
        | Builtin::ArrayAt
        | Builtin::ArraySort
        | Builtin::ArrayForEach
        | Builtin::ArrayReduce
        | Builtin::ArrayReduceRight
        | Builtin::ArrayPush
        | Builtin::ArrayUnshift
        | Builtin::ArrayFill
        | Builtin::ArrayToSorted => Some(1.0),
        Builtin::ArrayIterator
        | Builtin::ArrayKeys
        | Builtin::ArrayEntries
        | Builtin::ArrayShift => Some(0.0),
        Builtin::ArrayFlat
        | Builtin::ArrayReverse
        | Builtin::ArrayPop
        | Builtin::ArrayToReversed
        | Builtin::ArrayJoin
        | Builtin::ArrayToLocaleString => Some(0.0),
        Builtin::ArrayCopyWithin => Some(2.0),
        Builtin::ArraySlice | Builtin::ArrayBufferSlice | Builtin::ArraySplice => Some(2.0),
        Builtin::ArrayConcat => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(_b: Builtin) -> Option<&'static str> {
    None
}
