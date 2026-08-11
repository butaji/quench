//! Array method metadata.

use crate::ops::Builtin;

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::ArrayFrom => Some("from"),
        Builtin::ArrayIterator => Some("values"),
        Builtin::ArrayKeys => Some("keys"),
        Builtin::ArrayEntries => Some("entries"),
        Builtin::ArrayShift => Some("shift"),
        Builtin::ArrayReverse => Some("reverse"),
        Builtin::ArrayFindLast => Some("findLast"),
        Builtin::ArrayFindLastIndex => Some("findLastIndex"),
        Builtin::ArrayPop => Some("pop"),
        Builtin::ArrayUnshift => Some("unshift"),
        Builtin::ArrayFill => Some("fill"),
        Builtin::ArrayCopyWithin => Some("copyWithin"),
        Builtin::ArrayToSorted => Some("toSorted"),
        _ => None,
    }
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::ArrayFrom => Some(1.0),
        Builtin::ArrayIterator
        | Builtin::ArrayKeys
        | Builtin::ArrayEntries
        | Builtin::ArrayShift => Some(0.0),
        Builtin::ArrayReverse => Some(0.0),
        Builtin::ArrayPop => Some(0.0),
        Builtin::ArrayUnshift => Some(1.0),
        Builtin::ArrayFill => Some(1.0),
        Builtin::ArrayCopyWithin => Some(2.0),
        Builtin::ArrayFindLast | Builtin::ArrayFindLastIndex => Some(1.0),
        Builtin::ArrayToSorted => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(_b: Builtin) -> Option<&'static str> {
    None
}
