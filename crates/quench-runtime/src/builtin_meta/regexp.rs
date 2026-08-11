//! RegExp method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::RegExpPrototypeToString => Some("RegExp.prototype.toString"),
        Builtin::RegExpEscape => Some("escape"),
        Builtin::RegExpTest => Some("RegExp.prototype.test"),
        Builtin::RegExpExec => Some("RegExp.prototype.exec"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::RegExpPrototypeToString => Some(0.0),
        Builtin::RegExpEscape | Builtin::RegExpTest | Builtin::RegExpExec => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::RegExpPrototypeToString => Some("toString"),
        Builtin::RegExpEscape => Some("escape"),
        Builtin::RegExpTest => Some("test"),
        Builtin::RegExpExec => Some("exec"),
        _ => None,
    }
}
