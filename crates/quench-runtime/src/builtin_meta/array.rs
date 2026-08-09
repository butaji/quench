//! Array method metadata.

use crate::ops::Builtin;

pub const fn fn_name(_b: Builtin) -> Option<&'static str> {
    None
}

pub const fn fn_len(_b: Builtin) -> Option<f64> {
    None
}

pub const fn short_name(_b: Builtin) -> Option<&'static str> {
    None
}
