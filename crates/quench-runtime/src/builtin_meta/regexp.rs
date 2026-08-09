//! RegExp method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::RegExpTest => Some("RegExp.prototype.test"),
        Builtin::RegExpExec => Some("RegExp.prototype.exec"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::RegExpTest | Builtin::RegExpExec => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::RegExpTest => Some("test"),
        Builtin::RegExpExec => Some("exec"),
        _ => None,
    }
}
