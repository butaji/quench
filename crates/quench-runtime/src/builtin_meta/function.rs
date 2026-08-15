//! Function method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::ThrowTypeError => Some(""),
        Builtin::FunctionCall => Some("Function.prototype.call"),
        Builtin::FunctionApply => Some("Function.prototype.apply"),
        Builtin::FunctionBind => Some("Function.prototype.bind"),
        Builtin::FunctionPrototypeToString => Some("Function.prototype.toString"),
        Builtin::FunctionPrototypeHasInstance => Some("Function.prototype[@@hasInstance]"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::ThrowTypeError => Some(0.0),
        Builtin::FunctionCall | Builtin::FunctionBind | Builtin::FunctionPrototypeHasInstance => {
            Some(1.0)
        }
        Builtin::FunctionApply => Some(2.0),
        Builtin::FunctionPrototypeToString => Some(0.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::ThrowTypeError => Some(""),
        Builtin::FunctionCall => Some("call"),
        Builtin::FunctionApply => Some("apply"),
        Builtin::FunctionBind => Some("bind"),
        Builtin::FunctionPrototypeToString => Some("toString"),
        Builtin::FunctionPrototypeHasInstance => Some("[Symbol.hasInstance]"),
        _ => None,
    }
}
