//! Regexp searches of flat ASCII subjects made in place, from their bytes.
use super::*;

/// The result of a replace of a flat ASCII subject by a flat ASCII template with no `$`, made
/// from both borrowed out of the heap, and whether it is too long to be a string.
pub(super) struct AsciiReplace {
    /// The result where it is used and something matched.
    pub(super) output: Option<JsString>,
    pub(super) too_long: bool,
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
        result_used: bool,
        ranges: &mut smallvec::SmallVec<[Range<usize>; 8]>,
    ) -> Option<Self> {
        use crate::vm::wtf16::Units::Ascii;
        let (Some(Ascii(bytes)), Some(Ascii(literal))) = (
            heap.string(this)?.existing_flat().map(|flat| flat.view()),
            heap.string(replacement)?.existing_flat().map(|flat| flat.view()),
        ) else {
            return None;
        };
        if flags.contains(RegExpFlags::STICKY) || literal.contains(&b'$') {
            return None;
        }
        ascii_match_ranges(matcher, bytes, flags.contains(RegExpFlags::GLOBAL), ranges)?;
        let length = ranges
            .iter()
            .try_fold(bytes.len(), |length, range| (length - range.len()).checked_add(literal.len()))
            .filter(|length| *length <= super::wtf16::MAX_STRING_UNITS);
        let output = length.filter(|_| result_used && !ranges.is_empty()).map(|length| ascii_replaced(bytes, literal, ranges, length));
        Some(Self { output, too_long: length.is_none() })
    }
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

/// `bytes` with each of `ranges` replaced by `literal`, written once into storage of its
/// total `length`.
pub(super) fn ascii_replaced(bytes: &[u8], literal: &[u8], ranges: &[Range<usize>], length: usize) -> JsString {
    JsString::from_ascii_filled(length, |output| {
        let mut at = 0;
        let mut copied = 0;
        for range in ranges {
            put(output, &mut at, &bytes[copied..range.start]);
            put(output, &mut at, literal);
            copied = range.end;
        }
        put(output, &mut at, &bytes[copied..]);
        debug_assert_eq!(at, output.len(), "the pieces fill the string's length");
    })
}

/// The longest piece `put` writes byte by byte: two machine words.
const INLINE_PIECE_BYTES: usize = 2 * size_of::<usize>();

/// `piece` written at `at` of `output`, which moves on: a short piece byte by byte, as a
/// call to copy a few bytes costs more than the bytes.
#[inline(always)]
fn put(output: &mut [std::mem::MaybeUninit<u8>], at: &mut usize, piece: &[u8]) {
    let target = &mut output[*at..*at + piece.len()];
    if piece.len() <= INLINE_PIECE_BYTES {
        for (unit, byte) in target.iter_mut().zip(piece) {
            unit.write(*byte);
        }
    } else {
        target.write_copy_of_slice(piece);
    }
    *at += piece.len();
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
