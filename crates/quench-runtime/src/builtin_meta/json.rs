//! JSON builtin metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::JsonParse => Some("JSON.parse"),
        Builtin::JsonStringify => Some("JSON.stringify"),
        Builtin::JsonRawJson => Some("JSON.rawJSON"),
        Builtin::JsonIsRawJson => Some("JSON.isRawJSON"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::JsonParse => Some(2.0),
        Builtin::JsonStringify => Some(3.0),
        Builtin::JsonRawJson | Builtin::JsonIsRawJson => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::JsonParse => Some("parse"),
        Builtin::JsonStringify => Some("stringify"),
        Builtin::JsonRawJson => Some("rawJSON"),
        Builtin::JsonIsRawJson => Some("isRawJSON"),
        _ => None,
    }
}
