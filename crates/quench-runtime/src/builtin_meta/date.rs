//! Date method metadata.

use crate::ops::Builtin;

/// Returns the builtin for a DatePrototype property key.
pub fn date_prop(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "getYear" => Some(DateGetYear),
        "setYear" => Some(DateSetYear),
        "toLocaleString" => Some(DateToLocaleString),
        "toLocaleDateString" => Some(DateToLocaleDateString),
        "toLocaleTimeString" => Some(DateToLocaleTimeString),
        _ => None,
    }
}

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::DateGetYear => Some("Date.prototype.getYear"),
        Builtin::DateSetYear => Some("Date.prototype.setYear"),
        Builtin::DateToLocaleString => Some("Date.prototype.toLocaleString"),
        Builtin::DateToLocaleDateString => Some("Date.prototype.toLocaleDateString"),
        Builtin::DateToLocaleTimeString => Some("Date.prototype.toLocaleTimeString"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::DateSetYear => Some(1.0),
        Builtin::DateGetYear
        | Builtin::DateToLocaleString
        | Builtin::DateToLocaleDateString
        | Builtin::DateToLocaleTimeString => Some(0.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::DateGetYear => Some("getYear"),
        Builtin::DateSetYear => Some("setYear"),
        Builtin::DateToLocaleString => Some("toLocaleString"),
        Builtin::DateToLocaleDateString => Some("toLocaleDateString"),
        Builtin::DateToLocaleTimeString => Some("toLocaleTimeString"),
        _ => None,
    }
}
