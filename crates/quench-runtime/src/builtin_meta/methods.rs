//! Method metadata for builtins.

use crate::ops::Builtin;

use super::{
    array, bigint, collections, date, function, intl, math, number, object, reflect, regexp,
    string, symbol,
};

pub const fn function_name(builtin: Builtin) -> Option<&'static str> {
    if let Some(v) = array::fn_name(builtin) {
        return Some(v);
    }
    if let Some(v) = date::fn_name(builtin) {
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
    string::fn_name(builtin)
}

pub const fn function_length(builtin: Builtin) -> Option<f64> {
    if let Some(v) = array::fn_len(builtin) {
        return Some(v);
    }
    if let Some(v) = date::fn_len(builtin) {
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
    string::fn_len(builtin)
}

pub const fn short_name(builtin: Builtin) -> Option<&'static str> {
    if let Some(v) = array::short_name(builtin) {
        return Some(v);
    }
    if let Some(v) = date::short_name(builtin) {
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
    string::short_name(builtin)
}
