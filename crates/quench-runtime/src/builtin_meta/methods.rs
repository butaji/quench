//! Method metadata for builtins.

use crate::ops::Builtin;

use super::{
    array, atomics, bigint, collections, dataview, date, disposable, error, finalization_registry,
    function, intl, json, math, number, object, promise, reflect, regexp, string, symbol, temporal,
};

pub fn function_name(builtin: Builtin) -> Option<&'static str> {
    lookup(builtin, &NAME_PROVIDERS[..14])
        .or_else(|| shadow_realm_name(builtin))
        .or_else(|| lookup(builtin, &NAME_PROVIDERS[14..]))
}

pub fn function_length(builtin: Builtin) -> Option<f64> {
    lookup(builtin, &LENGTH_PROVIDERS[..14])
        .or_else(|| shadow_realm_length(builtin))
        .or_else(|| lookup(builtin, &LENGTH_PROVIDERS[14..]))
}

pub fn short_name(builtin: Builtin) -> Option<&'static str> {
    lookup(builtin, &SHORT_NAME_PROVIDERS[..14])
        .or_else(|| shadow_realm_short_name(builtin))
        .or_else(|| lookup(builtin, &SHORT_NAME_PROVIDERS[14..]))
}

const NAME_PROVIDERS: &[fn(Builtin) -> Option<&'static str>] = &[
    dataview::fn_name,
    array::fn_name,
    date::fn_name,
    disposable::fn_name,
    finalization_registry::fn_name,
    function::fn_name,
    regexp::fn_name,
    object::fn_name,
    number::fn_name,
    error::fn_name,
    bigint::fn_name,
    symbol::fn_name,
    intl::fn_name,
    temporal::fn_name,
    reflect::fn_name,
    math::fn_name,
    collections::fn_name,
    json::fn_name,
    promise::fn_name,
    string::fn_name,
    atomics::fn_name,
];

const LENGTH_PROVIDERS: &[fn(Builtin) -> Option<f64>] = &[
    dataview::fn_len,
    array::fn_len,
    date::fn_len,
    disposable::fn_len,
    finalization_registry::fn_len,
    function::fn_len,
    regexp::fn_len,
    object::fn_len,
    number::fn_len,
    error::fn_len,
    bigint::fn_len,
    symbol::fn_len,
    intl::fn_len,
    temporal::fn_len,
    reflect::fn_len,
    math::fn_len,
    collections::fn_len,
    json::fn_len,
    promise::fn_len,
    string::fn_len,
    atomics::fn_len,
];

const SHORT_NAME_PROVIDERS: &[fn(Builtin) -> Option<&'static str>] = &[
    dataview::short_name,
    array::short_name,
    date::short_name,
    disposable::short_name,
    finalization_registry::short_name,
    function::short_name,
    regexp::short_name,
    object::short_name,
    number::short_name,
    error::short_name,
    bigint::short_name,
    symbol::short_name,
    intl::short_name,
    temporal::short_name,
    reflect::short_name,
    math::short_name,
    collections::short_name,
    json::short_name,
    promise::short_name,
    string::short_name,
    atomics::short_name,
];

fn lookup<T: Copy>(builtin: Builtin, providers: &[fn(Builtin) -> Option<T>]) -> Option<T> {
    providers.iter().find_map(|provider| provider(builtin))
}

fn shadow_realm_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::ShadowRealmEvaluate => Some("ShadowRealm.prototype.evaluate"),
        Builtin::ShadowRealmImportValue => Some("ShadowRealm.prototype.importValue"),
        _ => None,
    }
}

fn shadow_realm_length(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::ShadowRealmEvaluate => Some(1.0),
        Builtin::ShadowRealmImportValue => Some(2.0),
        _ => None,
    }
}

fn shadow_realm_short_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::ShadowRealmEvaluate => Some("evaluate"),
        Builtin::ShadowRealmImportValue => Some("importValue"),
        _ => None,
    }
}
