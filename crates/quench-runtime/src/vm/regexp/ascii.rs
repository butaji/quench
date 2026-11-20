//! Regexp searches of flat ASCII subjects made in place, from their bytes.
use super::*;
use super::replacement::{ReplaceResult, result_length};
use crate::staged::string_replace;

/// The result of a replace of a flat ASCII subject by a flat ASCII template with no `$`, made
/// from both borrowed out of the heap.
pub(super) struct AsciiReplace {
    /// The result where it is built as a string and something matched.
    pub(super) output: Option<JsString>,
    /// The result's length, `None` where it is too long to be a string.
    pub(super) length: Option<usize>,
}

impl AsciiReplace {
    /// The result of `this.replace(regexp, replacement)`, with the match ranges it read left in
    /// `ranges`; `None` where the subject or template is not flat ASCII, the template reads
    /// `$`, the regexp is sticky, or the pattern has no compiled matcher over bytes.
    pub(super) fn plan(
        heap: &crate::heap::Heap,
        (this, replacement): (Value, Value),
        matcher: &quench_regexp::Regex,
        flags: RegExpFlags,
        build: ReplaceResult,
        ranges: &mut smallvec::SmallVec<[Range<usize>; 8]>,
    ) -> Option<Self> {
        use crate::vm::wtf16::Units::Ascii;
        let Some(Ascii(literal)) = heap.string(replacement)?.existing_flat().map(|flat| flat.view()) else {
            return None;
        };
        if flags.contains(RegExpFlags::STICKY) || literal.contains(&b'$') {
            return None;
        }
        let input = heap.string(this)?;
        let mut plan = |bytes: &[u8]| Self::plan_bytes(bytes, literal, matcher, flags, build, ranges);
        match input.existing_flat().map(|flat| flat.view()) {
            Some(Ascii(bytes)) => plan(bytes),
            Some(_) => None,
            // An ASCII rope is searched in a reused buffer: the ranges index its units, and the
            // output copies them out.
            None => input.with_ascii_rope_bytes(plan)?,
        }
    }

    fn plan_bytes(
        bytes: &[u8],
        literal: &[u8],
        matcher: &quench_regexp::Regex,
        flags: RegExpFlags,
        build: ReplaceResult,
        ranges: &mut smallvec::SmallVec<[Range<usize>; 8]>,
    ) -> Option<Self> {
        ascii_match_ranges(matcher, bytes, flags.contains(RegExpFlags::GLOBAL), ranges)?;
        let length = result_length(bytes.len(), ranges.as_slice(), literal.len());
        let output = length
            .filter(|_| build == ReplaceResult::Value && !ranges.is_empty())
            .map(|length| string_replace::replaced_ascii(bytes, literal, ranges.iter().cloned(), length));
        Some(Self { output, length })
    }
}

/// The search of flat ASCII `bytes` from `start`, the one in-place body of every steady and
/// rope search (`Regex::find_ascii`). Where nothing reads the captures, the bounds alone are
/// searched (`Regex::find_range_ascii`), which holds the same match. `None` where the pattern
/// has no matcher over bytes.
#[inline(always)]
pub(super) fn ascii_search(
    regex: &quench_regexp::Regex,
    bytes: &[u8],
    start: usize,
    sticky: bool,
    captures: bool,
) -> Option<Option<quench_regexp::Match>> {
    if captures {
        return regex.find_ascii(bytes, start, sticky, true);
    }
    regex.find_range_ascii(bytes, start, sticky).map(|found| found.map(quench_regexp::Match::native))
}

/// The ranges of the matches of `bytes` a replace reads: the first, or each of a global
/// search (an empty match advances it by one unit, a whole code point in ASCII). `None`
/// where the pattern has no compiled matcher over bytes.
fn ascii_match_ranges(
    matcher: &quench_regexp::Regex,
    bytes: &[u8],
    global: bool,
    ranges: &mut smallvec::SmallVec<[Range<usize>; 8]>,
) -> Option<()> {
    if global {
        return matcher.find_all_ranges_ascii(bytes, |range| ranges.push(range)).then_some(());
    }
    ranges.extend(matcher.find_range_ascii(bytes, 0, false)?);
    Some(())
}

impl<H: Host> Vm<H> {
    /// The new RegExp object of regular expression literal site `site_index` of `program`
    /// when the site's record is cached for this realm: made from it alone, with no pattern
    /// or flags constant read again. `None` leaves the site to `regexp_literal_of`.
    pub(super) fn regexp_from_cached_site(&mut self, program: ProgramId, site_index: usize) -> Option<Value> {
        let meta = self.programs.regexp_literal_meta(program, site_index)?;
        let intrinsics = self.realm.intrinsics.regexp_intrinsics.get(&self.realm.globals)?;
        if meta.legacy_constructor.constructor() != intrinsics.constructor {
            return None;
        }
        let prototype = intrinsics.prototype;
        self.regexp_from_meta(prototype, meta).ok()
    }
}

impl<H: Host> Vm<H> {
    /// The match of `matcher` over `input` from `q` for the split loop (`regexp_split_builtin`):
    /// a flat ASCII input is searched in place, any other makes its prepared subject once,
    /// kept in `subject`.
    pub(super) fn regexp_split_find(
        &mut self,
        matcher: &quench_regexp::Regex,
        input: &JsString,
        subject: &mut Option<PreparedSubject>,
        q: usize,
    ) -> Option<quench_regexp::Match> {
        if let Some(found) = Self::regexp_search_ascii(matcher, input, q, false, None) {
            return found;
        }
        let subject = subject.get_or_insert_with(|| self.regexp_subject(input, matcher));
        matcher.find_in_subject(subject, q).next()
    }
}
