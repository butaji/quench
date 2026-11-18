//! Searches of an ASCII subject's bytes, for a caller that holds them and needs no `Subject`.

use super::{backtrack, Match, Regex};
use std::ops::Range;

impl Regex {
    /// `backtracking_find` of an ASCII subject's bytes, for a caller that holds them and needs
    /// no `Subject`: `None` when the pattern did not compile to the backtracking matcher.
    pub fn find_ascii(&self, bytes: &[u8], start: usize, sticky: bool, captures: bool) -> Option<Option<Match>> {
        let program = match self.ascii_program(bytes, start, sticky)? {
            Some(program) => program,
            None => return Some(None),
        };
        if sticky && program.refuses_start(bytes, start) {
            return Some(None);
        }
        Some(program.find(bytes, start, sticky, captures).map(|found| self.offset_match(found, captures)))
    }

    /// Whether a non-sticky search from any position in any subject matches at that position:
    /// some path through the pattern matches empty unconditionally, so a caller that reads
    /// only where a match starts needs no search.
    pub fn matches_at_every_start(&self) -> bool {
        self.complete().empty_everywhere
    }

    /// The range `find_ascii` finds, for a caller that observes no captures.
    pub fn find_range_ascii(&self, bytes: &[u8], start: usize, sticky: bool) -> Option<Option<Range<usize>>> {
        let program = match self.ascii_program(bytes, start, sticky)? {
            Some(program) => program,
            None => return Some(None),
        };
        Some(program.find_bounds(bytes, start, sticky))
    }

    /// The ranges of every match of a scan over an ASCII subject's `bytes`, passed to `found`
    /// in order (each search starts where the one before ended, one unit on from an empty
    /// match), in one run of the matcher's workspace; `false` where the pattern did not
    /// compile to the backtracking matcher.
    pub fn find_all_ranges_ascii(&self, bytes: &[u8], found: impl FnMut(Range<usize>)) -> bool {
        match self.ascii_program(bytes, 0, false) {
            None => false,
            Some(None) => true,
            Some(Some(program)) => {
                program.find_all_bounds(bytes, 0, found);
                true
            }
        }
    }

    /// The backtracking program to run over an ASCII subject's `bytes` from `start`, or
    /// `Some(None)` where the search finds nothing before a run: the rest is shorter than
    /// any match, or lacks the literal every match consumes (a sticky search tries the one
    /// start, whose start-set check costs less than scanning the remainder for it, and a
    /// program that begins with the literal scans for it itself, `Program::searches_prefix`).
    /// `None` where the pattern did not compile to the backtracking matcher.
    fn ascii_program(&self, bytes: &[u8], start: usize, sticky: bool) -> Option<Option<&backtrack::Program>> {
        let complete = self.complete();
        let program = &complete.backtracking;
        if start > bytes.len() || bytes.len() - start < complete.min_length {
            return Some(None);
        }
        if let Some(literal) = complete.required_bytes.as_ref().filter(|_| !sticky) {
            if !program.searches_prefix(literal.needle()) && literal.find(&bytes[start..]).is_none() {
                return Some(None);
            }
        }
        Some(Some(program))
    }
}
