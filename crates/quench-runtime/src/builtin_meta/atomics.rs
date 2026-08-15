use crate::ops::Builtin;

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    Some(match builtin {
        Builtin::AtomicsAdd => "Atomics.add",
        Builtin::AtomicsAnd => "Atomics.and",
        Builtin::AtomicsOr => "Atomics.or",
        Builtin::AtomicsSub => "Atomics.sub",
        _ => return None,
    })
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::AtomicsAdd | Builtin::AtomicsAnd | Builtin::AtomicsOr | Builtin::AtomicsSub => {
            Some(3.0)
        }
        _ => None,
    }
}

pub const fn short_name(builtin: Builtin) -> Option<&'static str> {
    Some(match builtin {
        Builtin::AtomicsAdd => "add",
        Builtin::AtomicsAnd => "and",
        Builtin::AtomicsOr => "or",
        Builtin::AtomicsSub => "sub",
        _ => return None,
    })
}
