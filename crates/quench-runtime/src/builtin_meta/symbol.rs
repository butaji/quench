//! Symbol method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::SymbolFor => Some("Symbol.for"),
        Builtin::SymbolKeyFor => Some("Symbol.keyFor"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::SymbolFor | Builtin::SymbolKeyFor => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::SymbolFor => Some("for"),
        Builtin::SymbolKeyFor => Some("keyFor"),
        _ => None,
    }
}
