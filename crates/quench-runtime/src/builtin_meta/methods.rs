//! Method metadata for builtins.

use crate::ops::Builtin;

use super::{
    array, bigint, collections, dataview, date, disposable, error, finalization_registry, function,
    intl, json, math, number, object, promise, reflect, regexp, string, symbol,
};

pub fn function_name(builtin: Builtin) -> Option<&'static str> {
    if matches!(
        builtin,
        Builtin::AtomicsPause
            | Builtin::AtomicsAdd
            | Builtin::AtomicsStore
            | Builtin::AtomicsLoad
            | Builtin::AtomicsAnd
            | Builtin::AtomicsCompareExchange
    ) {
        return Some(match builtin {
            Builtin::AtomicsPause => "Atomics.pause",
            Builtin::AtomicsAdd => "Atomics.add",
            Builtin::AtomicsStore => "Atomics.store",
            Builtin::AtomicsLoad => "Atomics.load",
            Builtin::AtomicsAnd => "Atomics.and",
            _ => "Atomics.compareExchange",
        });
    }
    if let Some(v) = dataview::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = array::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = date::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = disposable::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = finalization_registry::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = function::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = regexp::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = object::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = number::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = error::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = bigint::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = symbol::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = intl::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = reflect::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = math::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = collections::fn_name(builtin) {
        return Some(v);
    }
    json::fn_name(builtin)
        .or_else(|| promise::fn_name(builtin).or_else(|| string::fn_name(builtin)))
}

pub fn function_length(builtin: Builtin) -> Option<f64> {
    if matches!(
        builtin,
        Builtin::AtomicsPause
            | Builtin::AtomicsAdd
            | Builtin::AtomicsStore
            | Builtin::AtomicsLoad
            | Builtin::AtomicsAnd
            | Builtin::AtomicsCompareExchange
    ) {
        return Some(if builtin == Builtin::AtomicsPause {
            0.0
        } else {
            3.0
        });
    }
    if let Some(v) = dataview::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = array::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = date::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = disposable::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = finalization_registry::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = function::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = regexp::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = object::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = number::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = error::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = bigint::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = symbol::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = intl::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = reflect::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = math::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = collections::fn_len(builtin) {
        return Some(v);
    }
    json::fn_len(builtin).or_else(|| promise::fn_len(builtin).or_else(|| string::fn_len(builtin)))
}

pub fn short_name(builtin: Builtin) -> Option<&'static str> {
    if matches!(
        builtin,
        Builtin::AtomicsPause
            | Builtin::AtomicsAdd
            | Builtin::AtomicsStore
            | Builtin::AtomicsLoad
            | Builtin::AtomicsAnd
            | Builtin::AtomicsCompareExchange
    ) {
        return Some(match builtin {
            Builtin::AtomicsPause => "pause",
            Builtin::AtomicsAdd => "add",
            Builtin::AtomicsStore => "store",
            Builtin::AtomicsLoad => "load",
            Builtin::AtomicsAnd => "and",
            _ => "compareExchange",
        });
    }
    if let Some(v) = dataview::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = array::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = date::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = disposable::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = finalization_registry::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = function::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = regexp::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = object::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = number::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = error::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = bigint::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = symbol::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = intl::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = reflect::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = math::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = collections::short_name(builtin) {
        return Some(v);
    }
    json::short_name(builtin)
        .or_else(|| promise::short_name(builtin).or_else(|| string::short_name(builtin)))
}
