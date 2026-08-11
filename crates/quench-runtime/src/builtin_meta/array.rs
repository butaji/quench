//! Array method metadata.

use crate::ops::Builtin;

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::ArrayIterator => Some("values"),
        _ => None,
    }
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::ArrayIterator => Some(0.0),
        _ => None,
    }
}

pub const fn short_name(_b: Builtin) -> Option<&'static str> {
    None
}
