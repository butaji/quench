//! Method metadata for builtins.

use crate::ops::Builtin;

use super::{array, bigint, date, function, intl, math, number, object, regexp, reflect, string, symbol};

pub const fn function_name(builtin: Builtin) -> Option<&'static str> {
    array::fn_name(builtin)
        .or_else(|| date::fn_name(builtin))
        .or_else(|| object::fn_name(builtin))
        .or_else(|| function::fn_name(builtin))
        .or_else(|| regexp::fn_name(builtin))
        .or_else(|| number::fn_name(builtin))
        .or_else(|| bigint::fn_name(builtin))
        .or_else(|| symbol::fn_name(builtin))
        .or_else(|| intl::fn_name(builtin))
        .or_else(|| reflect::fn_name(builtin))
        .or_else(|| math::fn_name(builtin))
        .or_else(|| string::fn_name(builtin))
}

pub const fn function_length(builtin: Builtin) -> Option<f64> {
    array::fn_len(builtin)
        .or_else(|| date::fn_len(builtin))
        .or_else(|| object::fn_len(builtin))
        .or_else(|| function::fn_len(builtin))
        .or_else(|| regexp::fn_len(builtin))
        .or_else(|| number::fn_len(builtin))
        .or_else(|| bigint::fn_len(builtin))
        .or_else(|| symbol::fn_len(builtin))
        .or_else(|| intl::fn_len(builtin))
        .or_else(|| reflect::fn_len(builtin))
        .or_else(|| math::fn_len(builtin))
        .or_else(|| string::fn_len(builtin))
}

pub const fn short_name(builtin: Builtin) -> Option<&'static str> {
    array::short_name(builtin)
        .or_else(|| date::short_name(builtin))
        .or_else(|| object::short_name(builtin))
        .or_else(|| function::short_name(builtin))
        .or_else(|| regexp::short_name(builtin))
        .or_else(|| number::short_name(builtin))
        .or_else(|| bigint::short_name(builtin))
        .or_else(|| symbol::short_name(builtin))
        .or_else(|| intl::short_name(builtin))
        .or_else(|| reflect::short_name(builtin))
        .or_else(|| math::short_name(builtin))
        .or_else(|| string::short_name(builtin))
}
