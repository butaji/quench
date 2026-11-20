//! What a replacement makes of its match ranges: its string, nothing, or its length alone.
use super::*;

/// What a replacement builds (`regexp_replace_with_literal`, `replace_ascii_input`): its string,
/// nothing (its result unread), or its length alone (`.length` of it).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplaceResult {
    Value,
    Unused,
    Length,
}

impl ReplaceResult {
    /// What a replacement whose result is `used` builds: its string, else nothing.
    pub(super) const fn of(used: bool) -> Self {
        if used { Self::Value } else { Self::Unused }
    }

    /// The result of a replacement that changes no unit of an `input` of `length` units: the
    /// input value itself, else its length or nothing.
    pub(super) fn unchanged(self, input: Value, length: usize) -> Value {
        match self {
            Self::Value => input,
            Self::Unused => Value::UNDEFINED,
            Self::Length => Value::length(length),
        }
    }

    /// The result of a replacement of `length` units that builds no string: its length or
    /// nothing. A built string is its own result, so `Value` never asks.
    pub(super) fn value(self, length: usize) -> Value {
        match self {
            Self::Value => unreachable!("a built replacement answers itself"),
            Self::Unused => Value::UNDEFINED,
            Self::Length => Value::length(length),
        }
    }
}

/// Whether `template` has a `$` substitution, which reads the match or its captures.
pub(super) fn has_substitution(template: &JsString) -> bool {
    template.units().iter().any(|unit| unit == u16::from(b'$'))
}

/// The length of the string a literal replacement of `ranges` in an input of `input` units by a
/// template of `template` units makes, while it is within the string limit: the build's only
/// observable outcome (`None` past it, where the build raises).
pub(super) fn result_length(input: usize, ranges: &[Range<usize>], template: usize) -> Option<usize> {
    ranges
        .iter()
        .try_fold(input, |length, range| (length - range.len()).checked_add(template))
        .filter(|length| *length <= super::wtf16::MAX_STRING_UNITS)
}
