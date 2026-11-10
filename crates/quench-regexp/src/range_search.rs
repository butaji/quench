//! Range searches of a subject: a loop over a subject's matches that reads no captures.
use super::{ASCII_MAX, Expr, LiteralFinder, Regex, Subject, backtrack, literal_finder};
use std::{cell::OnceCell, ops::Range};

/// A pattern that is its `required_literal` alone, in ASCII: literals in sequence, which
/// match where that byte run occurs and nowhere else, so one search of an ASCII subject's
/// bytes finds a match's range. The finder is built by the first range search.
pub(crate) struct LiteralRun {
    whole: bool,
    finder: OnceCell<LiteralFinder>,
}

impl LiteralRun {
    /// The fact for `program`, whose required literal is `required` (empty for a
    /// case-insensitive pattern, whose literals match more than their own value).
    pub(crate) fn of(program: &Expr, required: &[u32]) -> Self {
        fn literals(expr: &Expr) -> bool {
            match expr {
                Expr::Literal(_) => true,
                Expr::Sequence(parts) => parts.iter().all(literals),
                _ => false,
            }
        }
        let whole = !required.is_empty() && required.iter().all(|unit| *unit <= u32::from(ASCII_MAX)) && literals(program);
        Self { whole, finder: OnceCell::new() }
    }

    /// The finder of the run `required`, for a pattern that is that run.
    pub(crate) fn finder(&self, required: &[u32]) -> Option<&LiteralFinder> {
        self.whole.then(|| self.finder.get_or_init(|| literal_finder(&required.iter().map(|unit| *unit as u8).collect::<Vec<_>>())))
    }
}

impl Regex {
    /// A searcher of `subject`'s match ranges, as `find_range_in_subject` finds them, with
    /// the backend chosen once for every search of a loop over the subject's matches.
    pub fn range_searcher<'a>(&'a self, subject: &'a Subject) -> RangeSearcher<'a> {
        let complete = self.complete();
        let backend = match (subject.ascii(), complete.literal_run.finder(&complete.required_literal), complete.backtracking.as_ref()) {
            (Some(bytes), Some(finder), _) => RangeBackend::Literal { finder, bytes },
            (Some(bytes), None, Some(program)) => RangeBackend::Backtracking { program, bytes },
            _ => RangeBackend::General,
        };
        RangeSearcher { regex: self, subject, backend }
    }
}

/// `Regex::range_searcher`'s searcher.
pub struct RangeSearcher<'a> {
    regex: &'a Regex,
    subject: &'a Subject,
    backend: RangeBackend<'a>,
}

/// The backend that answers a range search of an ASCII subject: the steps
/// `find_range_in_subject` takes for every search, decided once.
enum RangeBackend<'a> {
    /// One search for the literal a pattern is (`LiteralRun`).
    Literal { finder: &'a LiteralFinder, bytes: &'a [u8] },
    /// The backtracking program over the subject's bytes.
    Backtracking { program: &'a backtrack::Program, bytes: &'a [u8] },
    General,
}

impl RangeSearcher<'_> {
    /// Where the matches of a search from the subject's start begin, each as long as the
    /// first value, when the pattern is a literal run of that length (`LiteralRun`): its
    /// occurrences in order, none overlapping the one before.
    pub fn literal_starts(&self) -> Option<(usize, impl Iterator<Item = usize> + '_)> {
        match self.backend {
            RangeBackend::Literal { finder, bytes } => Some((finder.needle().len(), finder.find_iter(bytes))),
            _ => None,
        }
    }

    /// `Regex::find_range_in_subject(subject, start)`.
    pub fn find(&self, start: usize) -> Option<Range<usize>> {
        let regex = self.regex;
        if start > self.subject.len() {
            return None;
        }
        match self.backend {
            RangeBackend::Literal { finder, bytes } => {
                finder.find(&bytes[start..]).map(|at| start + at..start + at + finder.needle().len())
            }
            RangeBackend::Backtracking { program, bytes } => {
                let complete = regex.complete();
                if bytes.len().saturating_sub(start) < complete.min_length {
                    return None;
                }
                if let Some(literal) = complete.required_bytes.as_ref() {
                    literal.find(&bytes[start..])?;
                }
                program.find_bounds(bytes, start, false)
            }
            RangeBackend::General => regex.find_range_in_subject(self.subject, start, false),
        }
    }
}
