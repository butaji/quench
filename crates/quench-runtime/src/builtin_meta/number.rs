//! Number method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IsFinite => Some("Number.isFinite"),
        Builtin::IsNaN => Some("Number.isNaN"),
        Builtin::ParseFloat => Some("Number.parseFloat"),
        Builtin::ParseInt => Some("Number.parseInt"),
        Builtin::NumberIsInteger => Some("Number.isInteger"),
        Builtin::NumberIsSafeInteger => Some("Number.isSafeInteger"),
        Builtin::NumberToString => Some("Number.prototype.toString"),
        Builtin::BooleanToString => Some("Boolean.prototype.toString"),
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
        Builtin::IsFinite
        | Builtin::IsNaN
        | Builtin::ParseFloat
        | Builtin::NumberIsInteger
        | Builtin::NumberIsSafeInteger => Some(1.0),
        Builtin::ParseInt => Some(2.0),
        Builtin::BooleanToString | Builtin::NumberValueOf | Builtin::NumberToLocaleString => {
            Some(0.0)
        }
        Builtin::NumberToString
        | Builtin::NumberToFixed
        | Builtin::NumberToPrecision
        | Builtin::NumberToExponential => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IsFinite => Some("isFinite"),
        Builtin::IsNaN => Some("isNaN"),
        Builtin::ParseFloat => Some("parseFloat"),
        Builtin::ParseInt => Some("parseInt"),
        Builtin::NumberIsInteger => Some("isInteger"),
        Builtin::NumberIsSafeInteger => Some("isSafeInteger"),
        Builtin::NumberToString | Builtin::BooleanToString => Some("toString"),
        Builtin::NumberValueOf => Some("valueOf"),
        Builtin::NumberToFixed => Some("toFixed"),
        Builtin::NumberToPrecision => Some("toPrecision"),
        Builtin::NumberToExponential => Some("toExponential"),
        Builtin::NumberToLocaleString => Some("toLocaleString"),
        _ => None,
    }
}
