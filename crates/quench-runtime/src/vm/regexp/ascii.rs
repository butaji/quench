//! Regexp searches of flat ASCII subjects made in place, from their bytes.
use super::*;

/// The ranges of the matches of `bytes` a replace reads: the first, or each of a global
/// search (an empty match advances it by one unit, a whole code point in ASCII). `None`
/// where the pattern has no compiled matcher over bytes.
pub(super) fn ascii_match_ranges(matcher: &quench_regexp::Regex, bytes: &[u8], global: bool) -> Option<smallvec::SmallVec<[Range<usize>; 8]>> {
    let mut ranges = smallvec::SmallVec::new();
    let mut next = matcher.find_range_ascii(bytes, 0, false)?;
    while let Some(range) = next {
        let resume = if range.is_empty() { range.end + 1 } else { range.end };
        ranges.push(range);
        if !global {
            break;
        }
        next = matcher.find_range_ascii(bytes, resume, false).flatten();
    }
    Some(ranges)
}

/// `bytes` with each of `ranges` replaced by `literal`, written once into storage of its
/// total `length`.
pub(super) fn ascii_replaced(bytes: &[u8], literal: &[u8], ranges: &[Range<usize>], length: usize) -> JsString {
    let mut parts: smallvec::SmallVec<[&[u8]; 17]> = smallvec::SmallVec::new();
    let mut copied = 0;
    for range in ranges {
        parts.push(&bytes[copied..range.start]);
        parts.push(literal);
        copied = range.end;
    }
    parts.push(&bytes[copied..]);
    JsString::from_ascii_parts(length, parts.into_iter())
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
