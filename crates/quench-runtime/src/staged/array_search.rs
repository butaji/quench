//! Array indexOf/includes/lastIndexOf over dense elements: one search body, derived from
//! `Vm::search_dense` (`vm/array_view.rs`), which `Array.prototype.indexOf`, `includes` and
//! `lastIndexOf` all run.
//!
//! The forward scan of the element words is the generic `scan_dense`, instantiated on `Concrete` here and on
//! `Emit` by the u-jit's `element_search` (`vm/compiled/lane_region/builtin_calls.rs`), so both derive from one
//! loop. The reverse scan (`lastIndexOf`) and the NaN and content compares stay plain functions over the
//! decoded values: `Stage` has no descending loop, and those compares are not word compares.

use std::ops::Range;

use super::numeric::NumericStage;
use super::region::Region;
use super::Concrete;
use crate::Value;
use crate::heap::ArrayElements;

/// How a search compares an element with the searched value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Match {
    /// `indexOf` and `lastIndexOf`: IsStrictlyEqual.
    Strict,
    /// `includes`: SameValueZero, and a hole no prototype holds reads as `undefined`.
    SameValueZero,
}

/// The outcome of a search over a range.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Search {
    Found(usize),
    Missing,
    /// The view cannot decide from this index on (no view, or a hole a prototype may hold):
    /// the generic algorithm continues here.
    Resume(usize),
}

/// How the elements equal to a searched value are recognised, decided once per search. Every
/// kind but `Content` and `Nan` compares the raw words of the elements.
#[derive(Clone, Copy)]
enum Kind {
    /// Equal values carry exactly this word.
    One(u64),
    /// An integral Number: equal values carry its int32 word or its double word.
    Two([u64; 2]),
    /// Zero: equal values carry the int32 0 word or the double word of either zero.
    Three([u64; 3]),
    /// NaN under SameValueZero: any NaN double word.
    Nan,
    /// NaN under strict equality: no element is equal.
    Never,
    /// A string or BigInt: equal by content, compared per element.
    Content,
}

/// The searched value with the facts `search` needs, computed once per search.
#[derive(Clone, Copy)]
pub(crate) struct Needle {
    kind: Kind,
    /// The narrow word a narrow element equal to the value carries, or `None` when no narrow
    /// element can be equal (only a hole stops the walk then).
    narrow: Option<u32>,
}

impl Needle {
    /// The words an element equal to the needle carries, when it is compared word for word: the
    /// kinds that are not `Nan` or `Content`.
    fn words(&self) -> &[u64] {
        match &self.kind {
            Kind::One(word) => std::slice::from_ref(word),
            Kind::Two(words) => &words[..],
            Kind::Three(words) => &words[..],
            Kind::Nan | Kind::Never | Kind::Content => &[],
        }
    }

    /// Whether the elements equal to the needle are found word for word (`scan_dense`).
    fn is_word_kind(&self) -> bool {
        matches!(self.kind, Kind::One(_) | Kind::Two(_) | Kind::Three(_) | Kind::Never)
    }

    /// The needle for `search` under `mode`. `content` says whether `search` is a string or a
    /// BigInt, which equal elements are recognised by content (`Content`); a Number is never one.
    pub(crate) fn new(search: Value, mode: Match, content: bool) -> Self {
        let kind = match search.as_number() {
            Some(number) => number_kind(number, mode),
            None if content => Kind::Content,
            None => Kind::One(search.to_bits()),
        };
        Self { kind, narrow: narrow_key(search) }
    }
}

/// The bit pattern of negative zero, which is a double word.
const NEGATIVE_ZERO_BITS: u64 = 0x8000_0000_0000_0000;

/// The int32 a Number equals, when it is integral and within `i32`.
fn integer_of(number: f64) -> Option<i32> {
    let int = number as i32;
    (f64::from(int) == number).then_some(int)
}

/// The narrow word of the elements equal to `search`, when no content comparison is needed.
fn narrow_key(search: Value) -> Option<u32> {
    match search.as_number() {
        Some(number) => integer_of(number).and_then(Value::narrow_integer),
        None => search.to_narrow(),
    }
}

/// The kind of a Number needle, under `mode`.
fn number_kind(number: f64, mode: Match) -> Kind {
    if number.is_nan() {
        return if mode == Match::SameValueZero { Kind::Nan } else { Kind::Never };
    }
    if number == 0.0 {
        return Kind::Three([Value::integer(0).to_bits(), 0.0_f64.to_bits(), NEGATIVE_ZERO_BITS]);
    }
    let double = number.to_bits();
    match integer_of(number) {
        Some(int) => Kind::Two([Value::integer(int).to_bits(), double]),
        None => Kind::One(double),
    }
}

/// What a dense element must equal for `scan_dense` to stop on it.
#[derive(Clone, Copy)]
pub struct DenseNeedle<'a, W> {
    /// Elements whose word is one of these.
    pub words: &'a [W],
    /// Elements that are doubles float-equal to this binary64 word (`NumericStage::float_eq`).
    pub number: Option<W>,
    /// The hole word: a hole stops the scan too, so its caller settles it.
    pub hole: Option<W>,
}

/// The index in `from..to` of the first element of the dense backing at `base` (element 0's address) that
/// `needle` matches or that is a hole, or `to` when none is. Elements are `step` bytes (4 for a narrow word,
/// 8 for a wide or double one), a 4-byte element loaded as its zero-extended word. The loop walks addresses:
/// its state is the address after the element it examined, so the examined element is the state less a
/// step, whether the loop stopped on a match or on the end.
///
/// This is `Vm::search_dense`'s word walk and the u-jit's `element_search` scans, one body for both.
#[inline(always)]
pub fn scan_dense<S: NumericStage>(stage: &mut S, base: S::Word, from: S::Word, to: S::Word,
    needle: DenseNeedle<'_, S::Word>, step: u32) -> S::Word {
    let bytes = u64::from(step);
    let start = {
        let offset = stage.mul_imm(from, bytes);
        stage.add(base, offset)
    };
    let end = {
        let offset = stage.mul_imm(to, bytes);
        stage.add(base, offset)
    };
    let [past] = stage.loop_words([start], |stage, [cursor]| {
        let word = if step == 4 {
            stage.load_u32(Region::Elements.mutable(), cursor, 0)
        } else {
            stage.load_u64(Region::Elements.mutable(), cursor, 0)
        };
        let mut stop = stage.eq(cursor, end);
        for &candidate in needle.words {
            let same = stage.eq(word, candidate);
            stop = stage.any(stop, same);
        }
        if let Some(number) = needle.number {
            let same = stage.float_eq(word, number);
            stop = stage.any(stop, same);
        }
        if let Some(hole) = needle.hole {
            let same = stage.eq(word, hole);
            stop = stage.any(stop, same);
        }
        let go_on = stage.not(stop);
        let next = stage.add_imm(cursor, bytes);
        (go_on, [next])
    });
    // The state is one step past the element the loop stopped at: `(past - base) / step - 1`.
    let offset = stage.sub(past, base);
    let index = stage.shr_imm(offset, u64::from(step.trailing_zeros()));
    stage.add_imm(index, u64::MAX)
}

/// The offset in `range` of the first element that a word-kind `needle` matches or that is a hole, found by
/// `scan_dense` on `Concrete` over the backing's own words. A narrow backing compares its narrow key; a wide or
/// double backing compares its words, which hold each element's exact bits: a double element equal to a
/// Number's value has the bits of that Number's word (the needle's words hold them, `-0` and `+0` both).
fn scan_forward(elements: &ArrayElements, range: Range<usize>, needle: &Needle) -> Option<usize> {
    let mut stage = Concrete::pure();
    let key = needle.narrow.map(|key| [u64::from(key)]);
    // A backing with no hole (`holds_no_hole`) has no hole word for the walk to stop on.
    let holed = !elements.holds_no_hole();
    let (base, step, words, hole) = match elements.narrow_words() {
        Some(narrow) => {
            let words: &[u64] = match &key {
                Some(key) => &key[..],
                None => &[],
            };
            (narrow.as_ptr() as u64, 4, words, Value::DELETED.to_narrow().filter(|_| holed).map(u64::from))
        }
        None => {
            let base = match elements.wide_values() {
                Some(values) => values.as_ptr() as u64,
                None => elements.double_values().map_or(0, |doubles| doubles.as_ptr() as u64),
            };
            (base, 8, needle.words(), holed.then(|| Value::DELETED.to_bits()))
        }
    };
    // The needle's shape is fixed for the walk: each shape (its word count, a hole word or none, the width)
    // runs its own instance, so the walk tests only the words it compares, as the u-jit's emitted scan does.
    let (from, to) = (range.start as u64, range.end as u64);
    macro_rules! walk {
        ($words:expr) => {
            match (hole, step) {
                (Some(hole), 4) => scan_dense(&mut stage, base, from, to, DenseNeedle { words: $words, number: None, hole: Some(hole) }, 4),
                (Some(hole), _) => scan_dense(&mut stage, base, from, to, DenseNeedle { words: $words, number: None, hole: Some(hole) }, 8),
                (None, 4) => scan_dense(&mut stage, base, from, to, DenseNeedle { words: $words, number: None, hole: None }, 4),
                (None, _) => scan_dense(&mut stage, base, from, to, DenseNeedle { words: $words, number: None, hole: None }, 8),
            }
        };
    }
    let found = match *words {
        [] => walk!(&[]),
        [a] => walk!(&[a]),
        [a, b] => walk!(&[a, b]),
        [a, b, c] => walk!(&[a, b, c]),
        _ => walk!(words),
    };
    (found != range.end as u64).then(|| found as usize - range.start)
}

/// The offset of the first (last when `reverse`) value that is a hole or satisfies `matches`.
fn first_stop<I>(mut values: I, reverse: bool, matches: impl Fn(Value) -> bool) -> Option<usize>
where
    I: DoubleEndedIterator<Item = Value> + ExactSizeIterator,
{
    let stop = |value: Value| value.is_deleted() || matches(value);
    if reverse { values.rposition(stop) } else { values.position(stop) }
}

/// The offset of the first (last when `reverse`) narrow word that is a hole or `key`.
fn find_narrow(words: &[u32], reverse: bool, key: Option<u32>) -> Option<usize> {
    let hole = Value::DELETED.to_narrow();
    let stop = |word: &u32| Some(*word) == hole || key == Some(*word);
    if reverse { words.iter().rposition(stop) } else { words.iter().position(stop) }
}

/// Where the generic algorithm resumes a search of `range` the view cannot decide: its last
/// index when `reverse`, else its first.
pub(crate) fn resume_at(range: &Range<usize>, reverse: bool) -> Search {
    Search::Resume(if reverse { range.end - 1 } else { range.start })
}

/// The search of `range` of `elements` for `needle`, from its end when `reverse`: the first (last)
/// element that matches or is a hole is a stop. `settle` decides a stop at `index` (`hole` says
/// whether it is a hole): `Some` ends the search with that outcome, `None` continues past it.
/// `content` compares elements by content for a `Content` needle. Nothing here runs guest code.
pub(crate) fn search(elements: &ArrayElements, mut range: Range<usize>, reverse: bool, needle: &Needle,
    content: impl Fn(Value) -> bool, mut settle: impl FnMut(usize, bool) -> Option<Search>) -> Search {
    // Indices past the vector are the generic algorithm's.
    if range.end > elements.len() && !range.is_empty() {
        return resume_at(&range, reverse);
    }
    while !range.is_empty() {
        let Some(offset) = position_in(elements, range.clone(), reverse, needle, &content) else {
            break;
        };
        let index = range.start + offset;
        let hole = elements.get(index).is_some_and(Value::is_deleted);
        if let Some(settled) = settle(index, hole) {
            return settled;
        }
        range = if reverse { range.start..index } else { index + 1..range.end };
    }
    Search::Missing
}

/// The offset in `range` of the first (last when `reverse`) element that `needle` matches or that
/// is a hole, read from the backing's own form.
fn position_in(elements: &ArrayElements, range: Range<usize>, reverse: bool, needle: &Needle,
    content: &impl Fn(Value) -> bool) -> Option<usize> {
    if !reverse && needle.is_word_kind() {
        return scan_forward(elements, range, needle);
    }
    if let Some(words) = elements.narrow_words().filter(|_| !matches!(needle.kind, Kind::Content)) {
        return find_narrow(&words[range], reverse, needle.narrow);
    }
    match elements.wide_values() {
        Some(values) => position_of(values[range].iter().copied(), reverse, needle, content),
        None => position_of(elements.range(range), reverse, needle, content),
    }
}

/// `position_in` over values, comparing by the needle's kind.
fn position_of<I>(values: I, reverse: bool, needle: &Needle, content: &impl Fn(Value) -> bool) -> Option<usize>
where
    I: DoubleEndedIterator<Item = Value> + ExactSizeIterator,
{
    match needle.kind {
        Kind::One(word) => first_stop(values, reverse, |value| value.to_bits() == word),
        Kind::Two(words) => first_stop(values, reverse, |value| words.contains(&value.to_bits())),
        Kind::Three(words) => first_stop(values, reverse, |value| words.contains(&value.to_bits())),
        Kind::Never => first_stop(values, reverse, |_| false),
        Kind::Nan => first_stop(values, reverse, |value| {
            value.is_double() && f64::from_bits(value.to_bits()).is_nan()
        }),
        Kind::Content => first_stop(values, reverse, content),
    }
}

/// The index a `lastIndexOf` search starts from in an array of `length`, as an integer word: the
/// last index when no `fromIndex` is `present`, else the integer word `from` clamped as the spec
/// clamps it (a negative one counts from the end). Below zero when nothing is searched. The one
/// body of the start for every tier.
#[inline(always)]
pub fn last_index_start_row<S: super::Stage>(stage: &mut S, present: bool, from: S::Word, length: S::Word) -> S::Word {
    let last = stage.add_imm(length, u64::MAX);
    if !present {
        return last;
    }
    let sign = stage.shr_imm(from, 63u64);
    let negative = stage.ne_imm(sign, 0u64);
    let counted = stage.add(length, from);
    let inside = stage.ult(from, length);
    let clamped = stage.select(inside, from, last);
    stage.select(negative, counted, clamped)
}
