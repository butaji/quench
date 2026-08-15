use crate::ops::Builtin;

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    Some(match builtin {
        Builtin::AtomicsAdd => "Atomics.add",
        Builtin::AtomicsAnd => "Atomics.and",
        Builtin::AtomicsOr => "Atomics.or",
        Builtin::AtomicsSub => "Atomics.sub",
        Builtin::AtomicsXor => "Atomics.xor",
        Builtin::AtomicsCompareExchange => "Atomics.compareExchange",
        _ => return None,
    })
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::AtomicsAdd
        | Builtin::AtomicsAnd
        | Builtin::AtomicsOr
        | Builtin::AtomicsSub
        | Builtin::AtomicsXor => Some(3.0),
        Builtin::AtomicsCompareExchange => Some(4.0),
        _ => None,
    }
}

pub const fn short_name(builtin: Builtin) -> Option<&'static str> {
    Some(match builtin {
        Builtin::AtomicsAdd => "add",
        Builtin::AtomicsAnd => "and",
        Builtin::AtomicsOr => "or",
        Builtin::AtomicsSub => "sub",
        Builtin::AtomicsXor => "xor",
        Builtin::AtomicsCompareExchange => "compareExchange",
        _ => return None,
    })
}
