//! The legacy `RegExp` statics and the match they hold.
use super::*;

/// The legacy `RegExp` statics: the last match, which holds the subject it was found in, and
/// the `RegExp.input` a script set since, which reads in place of that subject.
#[derive(Clone)]
pub(super) struct LegacyStatics {
    pub(super) last: LegacyRegExpMatch,
    pub(super) input_override: Option<JsString>,
}

impl LegacyStatics {
    pub(super) const EMPTY: Self = Self { last: LegacyRegExpMatch::Empty, input_override: None };

    /// What `RegExp.input` reads, or `None` after a match of a subclass or other non-builtin
    /// that poisoned the statics and no script has set it since.
    pub(super) fn input(&self) -> Option<JsString> {
        self.input_override.clone().or_else(|| match &self.last {
            LegacyRegExpMatch::Empty => Some(JsString::from_units(&[])),
            LegacyRegExpMatch::Matched { input, .. } | LegacyRegExpMatch::Pending { input, .. } => Some(input.clone()),
            LegacyRegExpMatch::PendingValues { .. } => unreachable!("a read resolves a pending match first"),
            LegacyRegExpMatch::Invalid => None,
        })
    }
}

#[derive(Clone)]
pub(super) enum LegacyRegExpMatch {
    Empty,
    Matched {
        input: JsString,
        matched: quench_regexp::Match,
    },
    /// The match `matcher` finds in `input` from `start`, where a match is known to begin:
    /// found again only when a static is read (`resolve_regexp_legacy_match`), as most
    /// matches' statics are never read.
    Pending {
        input: JsString,
        matcher: Rc<quench_regexp::Regex>,
        start: usize,
    },
    /// `Pending` of a steady call, whose subject is a string value and whose regexp is its
    /// receiver: two values the collector keeps, where `Pending`'s owners cost a count each
    /// per call.
    PendingValues {
        subject: Value,
        regexp: Value,
        start: usize,
    },
    Invalid,
}

impl LegacyStatics {
    /// The values a `PendingValues` match holds, which the collector keeps.
    pub(super) fn pending_values(&self) -> impl Iterator<Item = Value> {
        match self.last {
            LegacyRegExpMatch::PendingValues { subject, regexp, .. } => Some([subject, regexp]),
            _ => None,
        }
        .into_iter()
        .flatten()
    }
}

