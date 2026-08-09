//! Function method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::FunctionCall => Some("Function.prototype.call"),
        Builtin::FunctionBind => Some("Function.prototype.bind"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::FunctionCall | Builtin::FunctionBind => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::FunctionCall => Some("call"),
        Builtin::FunctionBind => Some("bind"),
        _ => None,
    }
}
