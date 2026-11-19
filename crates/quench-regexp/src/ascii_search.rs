//! Searches of an ASCII subject's bytes, for a caller that holds them and needs no `Subject`.

use super::{Match, Regex};
use std::ops::Range;

impl Regex {
    /// `backtracking_find` of an ASCII subject's bytes, for a caller that holds them and needs
    /// no `Subject`: `None` when the pattern did not compile to the backtracking matcher.
    pub fn find_ascii(&self, bytes: &[u8], start: usize, sticky: bool, captures: bool) -> Option<Option<Match>> {
        let begin = match self.ascii_begin(bytes, start, sticky)? {
            Some(begin) => begin,
            None => return Some(None),
        };
        let program = &self.complete().backtracking;
        if sticky && program.refuses_start(bytes, start) {
            return Some(None);
        }
        Some(program.find(bytes, begin, sticky, captures).map(|found| self.offset_match(found, captures)))
    }

    /// Whether a non-sticky search from any position in any subject matches at that position:
    /// some path through the pattern matches empty unconditionally, so a caller that reads
    /// only where a match starts needs no search.
    #[inline]
    pub fn matches_at_every_start(&self) -> bool {
        self.complete().empty_everywhere
    }

    /// The range `find_ascii` finds, for a caller that observes no captures.
    pub fn find_range_ascii(&self, bytes: &[u8], start: usize, sticky: bool) -> Option<Option<Range<usize>>> {
        // A pattern that is one literal run is one search for it (`LiteralRun`).
        let complete = self.complete();
        if let Some(finder) = complete.literal_run.finder(&complete.required_literal).filter(|_| !sticky) {
            let rest = bytes.get(start..)?;
            return Some(finder.find(rest).map(|at| start + at..start + at + finder.needle().len()));
        }
        let begin = match self.ascii_begin(bytes, start, sticky)? {
            Some(begin) => begin,
            None => return Some(None),
        };
        let program = complete.bounds.as_ref().unwrap_or(&complete.backtracking);
        Some(program.find_bounds(bytes, begin, sticky))
    }

    /// The ranges of every match of a scan over an ASCII subject's `bytes`, passed to `found`
    /// in order (each search starts where the one before ended, one unit on from an empty
    /// match), in one run of the matcher's workspace; `false` where the pattern did not
    /// compile to the backtracking matcher.
    pub fn find_all_ranges_ascii(&self, bytes: &[u8], found: impl FnMut(Range<usize>)) -> bool {
        match self.ascii_begin(bytes, 0, false) {
            None => false,
            Some(None) => true,
            Some(Some(begin)) => {
                self.complete().backtracking.find_all_bounds(bytes, begin, found);
                true
            }
        }
    }

    /// Where the backtracking program begins a search of an ASCII subject's `bytes` from
    /// `start`: `Some(None)` where the search finds nothing (the rest is shorter than any
    /// match, or lacks the literal every match consumes; a sticky search tries the one start
    /// instead, and a program that begins with the literal scans for it itself,
    /// `Program::searches_prefix`), else `Some(Some(begin))`. The first hit of that literal
    /// is found once, and no match starts before the hit less the literal's reach
    /// (`CompleteMatcher::literal_reach`), so a search from `begin` finds the leftmost match a
    /// search from `start` does. `None` where the pattern did not compile to the backtracking
    /// matcher.
    fn ascii_begin(&self, bytes: &[u8], start: usize, sticky: bool) -> Option<Option<usize>> {
        let complete = self.complete();
        if start > bytes.len() || bytes.len() - start < complete.min_length {
            return Some(None);
        }
        // `required_bytes` holds no literal the program's own prefix search finds
        // (`CompleteMatcher::lower`), so a held one is the only scan of the subject for it.
        let Some(literal) = complete.required_bytes.as_ref().filter(|_| !sticky) else {
            return Some(Some(start));
        };
        let Some(hit) = literal.find(&bytes[start..]) else {
            return Some(None);
        };
        Some(Some(start + hit.saturating_sub(complete.literal_reach.unwrap_or(hit))))
    }
}
