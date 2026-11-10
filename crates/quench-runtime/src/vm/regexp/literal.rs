//! A literal replacement's result, and its length without building it.
use super::*;

/// What a literal replacement builds (`regexp_replace_with_literal`): its string, nothing (its
/// result unread), or its length alone (`.length` of it).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplaceResult {
    Value,
    Unused,
    Length,
}

impl ReplaceResult {
    /// The value `regexp_replace_with_literal` gives for a replacement of `length`.
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

/// The length of the string a literal replacement of `ranges` in `input` by `template` would
/// make, while it is within the string limit: the build's own observable outcome.
pub(super) fn literal_result_length(input: &JsString, ranges: &[Range<usize>], template: &JsString) -> Option<usize> {
    ranges
        .iter()
        .try_fold(input.units().len(), |length, range| (length - range.len()).checked_add(template.units().len()))
        .filter(|length| *length <= super::wtf16::MAX_STRING_UNITS)
}

impl<H: Host> Vm<H> {
    /// `String.prototype.replace`'s result length for a literal `template`: the string
    /// `regexp_replace_with_literal` would build, never built (`ReplaceResult::Length`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn regexp_symbol_replace_length(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: &JsString,
        input_value: Value,
        template: &JsString,
        matcher: &Rc<quench_regexp::Regex>,
        flags: RegExpFlags,
    ) -> Result<Value, JsError> {
        let input = &input.unkept_flat();
        let subject = self.regexp_subject(input, matcher);
        self.regexp_replace_with_literal(p, receiver, input, input_value, &subject, template, matcher, flags, ReplaceResult::Length)
    }
}
