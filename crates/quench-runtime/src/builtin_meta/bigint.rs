//! BigInt method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::BigIntAsIntN => Some("BigInt.asIntN"),
        Builtin::BigIntAsUintN => Some("BigInt.asUintN"),
        Builtin::BigIntToString => Some("BigInt.prototype.toString"),
        Builtin::BigIntToLocaleString => Some("BigInt.prototype.toLocaleString"),
        Builtin::BigIntValueOf => Some("BigInt.prototype.valueOf"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::BigIntAsIntN | Builtin::BigIntAsUintN => Some(2.0),
        Builtin::BigIntToString | Builtin::BigIntToLocaleString | Builtin::BigIntValueOf => {
            Some(0.0)
        }
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::BigIntAsIntN => Some("asIntN"),
        Builtin::BigIntAsUintN => Some("asUintN"),
        Builtin::BigIntToString => Some("toString"),
        Builtin::BigIntToLocaleString => Some("toLocaleString"),
        Builtin::BigIntValueOf => Some("valueOf"),
        _ => None,
    }
}
