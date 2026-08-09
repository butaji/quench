//! Number method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::NumberToString => Some("Number.prototype.toString"),
        Builtin::NumberValueOf => Some("Number.prototype.valueOf"),
        Builtin::NumberToFixed => Some("Number.prototype.toFixed"),
        Builtin::NumberToPrecision => Some("Number.prototype.toPrecision"),
        Builtin::NumberToExponential => Some("Number.prototype.toExponential"),
        Builtin::NumberToLocaleString => Some("Number.prototype.toLocaleString"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::NumberToString | Builtin::NumberValueOf | Builtin::NumberToLocaleString => {
            Some(0.0)
        }
        Builtin::NumberToFixed | Builtin::NumberToPrecision | Builtin::NumberToExponential => {
            Some(1.0)
        }
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::NumberToString => Some("toString"),
        Builtin::NumberValueOf => Some("valueOf"),
        Builtin::NumberToFixed => Some("toFixed"),
        Builtin::NumberToPrecision => Some("toPrecision"),
        Builtin::NumberToExponential => Some("toExponential"),
        Builtin::NumberToLocaleString => Some("toLocaleString"),
        _ => None,
    }
}
