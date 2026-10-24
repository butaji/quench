use std::mem::MaybeUninit;
use std::borrow::Cow;
use std::cell::{OnceCell, UnsafeCell};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

thread_local! {
    /// Bytes of text made on demand (a rope's flat units, a wide string's host text) since
    /// the collector last took them. A string is charged its flat bytes when allocated; text
    /// made later is charged here, so collections follow the memory programs materialize.
    static MATERIALIZED_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn charge_materialized(bytes: usize) {
    MATERIALIZED_BYTES.with(|total| total.set(total.get().saturating_add(bytes)));
}

/// Text bytes made on demand and not yet charged, which the caller now charges.
#[inline(always)]
pub(crate) fn take_materialized_bytes() -> usize {
    MATERIALIZED_BYTES.with(|total| total.replace(0))
}

/// Heap-owned JavaScript string. Its code units are authoritative: one byte each when every
/// unit is ASCII, UTF-16 otherwise. `host` is the explicit lossy Rust-text view used at
/// host/API boundaries; an ASCII string is its own host text.
#[derive(Clone)]
pub(crate) struct JsString {
    repr: Repr,
}

/// Where a string's units live.
#[derive(Clone)]
enum Repr {
    /// Flat ASCII text (`FlatUnits::Short` or `FlatUnits::Ascii`, never `Wide`) held by the
    /// string value itself: short text inline, longer text in one shared allocation of its
    /// bytes. An ASCII string is its own host text and is never a rope, so it needs nothing
    /// a record holds.
    Flat(FlatUnits),
    /// A rope, or wide units, whose host text is made on demand.
    Shared(Rc<JsStringInner>),
}
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<JsString>() == size_of::<FlatUnits>());

/// Flat code units. A string whose units are all ASCII is stored `Short` when they fit;
/// longer ASCII text is `Ascii`, or an `AsciiSlice` of another string's `Ascii` storage.
#[derive(Clone)]
pub(crate) enum FlatUnits {
    /// ASCII text held in the string's own allocation: most property names and tokens fit,
    /// and need no second allocation.
    Short(ShortAscii),
    Ascii(Rc<str>),
    /// A run of longer ASCII storage, shared instead of copied, as V8's sliced strings are:
    /// taking the rest of a scanned text costs one small record, not a copy of the rest.
    AsciiSlice(Rc<AsciiSlice>),
    /// UTF-16 units, at least one of them non-ASCII: all-ASCII text is always stored narrow
    /// (`flat_units`, `flattened`), which the regexp subject relies on (`regexp_subject`).
    Wide(Rc<[u16]>),
    /// A run of longer `Wide` storage, shared instead of copied as `AsciiSlice` shares ASCII
    /// storage. Its units may all be ASCII (the tail of a text whose only wide units lie
    /// before it), so, like `Wide`, it is never taken to be ASCII.
    WideSlice(Rc<WideSlice>),
}

/// The units `range` of shared `Wide` storage; never a slice of a slice.
pub(crate) struct WideSlice {
    units: Rc<[u16]>,
    range: Range<usize>,
}

/// Units below which a run of wide storage is copied into its canonical form instead of
/// shared (`FlatUnits::run`): a short run costs less to copy than a slice record and the
/// storage it would keep alive, and copying narrows it when it is ASCII. V8's sliced
/// strings start at 13 characters; a slice record here is two words past its count header.
const WIDE_SLICE_MIN_UNITS: usize = 32;

/// The bytes `range` of shared ASCII storage; never shorter than `SHORT_ASCII_CAPACITY`
/// allows inline, and never a slice of a slice.
pub(crate) struct AsciiSlice {
    text: Rc<str>,
    range: Range<usize>,
}

/// ASCII text of at most `SHORT_ASCII_CAPACITY` bytes, stored inline.
#[derive(Clone, Copy)]
pub(crate) struct ShortAscii {
    length: u8,
    bytes: [u8; SHORT_ASCII_CAPACITY],
}

/// `FlatUnits` is three words: a shared variant's tag word, then its pointer and length.
/// `Short` fills them, less its own tag and `ShortAscii::length`.
pub(crate) const SHORT_ASCII_CAPACITY: usize = 3 * size_of::<usize>() - 2;
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<FlatUnits>() == 3 * size_of::<usize>());

impl ShortAscii {
    pub(crate) fn as_str(&self) -> &str {
        // SAFETY: only `FlatUnits::ascii` fills the bytes, from ASCII text.
        unsafe { std::str::from_utf8_unchecked(&self.bytes[..usize::from(self.length)]) }
    }
}

impl FlatUnits {
    /// ASCII `text` in its canonical storage.
    fn ascii(text: &str) -> Self {
        debug_assert!(text.is_ascii());
        Self::ascii_with(text.len(), |bytes| {
            bytes.write_copy_of_slice(text.as_bytes());
        })
    }

    /// The canonical storage of `length` ASCII bytes that `fill` writes in place: short text
    /// inline, longer text straight into its shared allocation, with no staging copy or
    /// zeroing. `fill` must write every byte of the slice it is given.
    fn ascii_with(length: usize, fill: impl FnOnce(&mut [MaybeUninit<u8>])) -> Self {
        if length <= SHORT_ASCII_CAPACITY {
            let mut bytes = [MaybeUninit::new(0); SHORT_ASCII_CAPACITY];
            fill(&mut bytes[..length]);
            // SAFETY: every byte started initialized, and `fill` writes only bytes.
            let bytes = bytes.map(|byte| unsafe { byte.assume_init() });
            debug_assert!(bytes.is_ascii());
            return Self::Short(ShortAscii {
                length: length as u8,
                bytes,
            });
        }
        let mut storage = Rc::<[u8]>::new_uninit_slice(length);
        fill(Rc::get_mut(&mut storage).expect("a new allocation is unshared"));
        // SAFETY: `fill` wrote every byte.
        let storage = unsafe { storage.assume_init() };
        debug_assert!(storage.is_ascii());
        // SAFETY: `str` and `[u8]` share a layout, and ASCII bytes are UTF-8.
        Self::Ascii(unsafe { Rc::from_raw(Rc::into_raw(storage) as *const str) })
    }

    /// The number of units, read from the representation without making a view.
    #[inline]
    fn len(&self) -> usize {
        match self {
            Self::Short(text) => usize::from(text.length),
            Self::Ascii(text) => text.len(),
            Self::AsciiSlice(slice) => slice.range.len(),
            Self::Wide(units) => units.len(),
            Self::WideSlice(slice) => slice.range.len(),
        }
    }

    /// The unit at `index`, read from the representation without making a view.
    #[inline(always)]
    pub(crate) fn unit(&self, index: usize) -> Option<u16> {
        match self {
            Self::Short(text) => text.as_str().as_bytes().get(index).map(|byte| u16::from(*byte)),
            Self::Ascii(text) => text.as_bytes().get(index).map(|byte| u16::from(*byte)),
            Self::AsciiSlice(slice) => (index < slice.range.len())
                .then(|| u16::from(slice.text.as_bytes()[slice.range.start + index])),
            Self::Wide(units) => units.get(index).copied(),
            Self::WideSlice(slice) => (index < slice.range.len())
                .then(|| slice.units[slice.range.start + index]),
        }
    }

    #[inline]
    pub(crate) fn view(&self) -> Units<'_> {
        match self {
            Self::Short(text) => Units::Ascii(text.as_str().as_bytes()),
            Self::Ascii(_) | Self::AsciiSlice(_) => {
                let (text, range) = self.shared_ascii().expect("shared ASCII storage");
                Units::Ascii(&text.as_bytes()[range])
            }
            Self::Wide(units) => Units::Wide(units),
            Self::WideSlice(slice) => Units::Wide(&slice.units[slice.range.clone()]),
        }
    }

    /// Whether the units are known to be ASCII; a wide slice may be, but is not known to be.
    pub(crate) fn is_ascii(&self) -> bool {
        !matches!(self, Self::Wide(_) | Self::WideSlice(_))
    }

    /// The shared wide storage these units view and the units they cover.
    fn shared_wide(&self) -> Option<(&Rc<[u16]>, Range<usize>)> {
        match self {
            Self::Wide(units) => Some((units, 0..units.len())),
            Self::WideSlice(slice) => Some((&slice.units, slice.range.clone())),
            Self::Short(_) | Self::Ascii(_) | Self::AsciiSlice(_) => None,
        }
    }

    /// The shared ASCII storage these units view and the bytes they cover.
    pub(crate) fn shared_ascii(&self) -> Option<(&Rc<str>, Range<usize>)> {
        match self {
            Self::Ascii(text) => Some((text, 0..text.len())),
            Self::AsciiSlice(slice) => Some((&slice.text, slice.range.clone())),
            Self::Short(_) | Self::Wide(_) | Self::WideSlice(_) => None,
        }
    }

    /// The units `range` of these, sharing ASCII storage when the run is too long to hold
    /// inline. `range` must lie within the units.
    fn run(&self, range: Range<usize>) -> Self {
        if range.len() > SHORT_ASCII_CAPACITY
            && let Some((text, own)) = self.shared_ascii()
        {
            if range.len() == own.len() {
                return self.clone();
            }
            return Self::AsciiSlice(Rc::new(AsciiSlice {
                text: Rc::clone(text),
                range: own.start + range.start..own.start + range.end,
            }));
        }
        if range.len() >= WIDE_SLICE_MIN_UNITS
            && let Some((units, own)) = self.shared_wide()
        {
            if range.len() == own.len() {
                return self.clone();
            }
            return Self::WideSlice(Rc::new(WideSlice {
                units: Rc::clone(units),
                range: own.start + range.start..own.start + range.end,
            }));
        }
        match self.view().slice(range) {
            // SAFETY: ASCII units are UTF-8.
            Units::Ascii(bytes) => Self::ascii(unsafe { std::str::from_utf8_unchecked(bytes) }),
            Units::Wide(units) => flat_units(units),
        }
    }

    fn same_storage(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Ascii(left), Self::Ascii(right)) => Rc::ptr_eq(left, right),
            (Self::AsciiSlice(left), Self::AsciiSlice(right)) => Rc::ptr_eq(left, right),
            (Self::Wide(left), Self::Wide(right)) => Rc::ptr_eq(left, right),
            (Self::WideSlice(left), Self::WideSlice(right)) => Rc::ptr_eq(left, right),
            _ => false,
        }
    }
}

/// A borrowed run of a string's code units.
#[derive(Clone, Copy)]
pub(crate) enum Units<'a> {
    Ascii(&'a [u8]),
    Wide(&'a [u16]),
}

impl<'a> Units<'a> {
    pub(crate) fn len(self) -> usize {
        match self {
            Self::Ascii(bytes) => bytes.len(),
            Self::Wide(units) => units.len(),
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub(crate) fn get(self, index: usize) -> Option<u16> {
        match self {
            Self::Ascii(bytes) => bytes.get(index).map(|byte| u16::from(*byte)),
            Self::Wide(units) => units.get(index).copied(),
        }
    }

    /// The unit at `index`; panics past the end, as slice indexing does.
    pub(crate) fn at(self, index: usize) -> u16 {
        match self {
            Self::Ascii(bytes) => u16::from(bytes[index]),
            Self::Wide(units) => units[index],
        }
    }

    pub(crate) fn iter(self) -> UnitsIter<'a> {
        match self {
            Self::Ascii(bytes) => UnitsIter::Ascii(bytes.iter()),
            Self::Wide(units) => UnitsIter::Wide(units.iter()),
        }
    }

    /// The units in `range`; panics on an out-of-bounds range, as slice indexing does.
    pub(crate) fn slice(self, range: Range<usize>) -> Self {
        match self {
            Self::Ascii(bytes) => Self::Ascii(&bytes[range]),
            Self::Wide(units) => Self::Wide(&units[range]),
        }
    }

    pub(crate) fn get_range(self, range: Range<usize>) -> Option<Self> {
        match self {
            Self::Ascii(bytes) => bytes.get(range).map(Self::Ascii),
            Self::Wide(units) => units.get(range).map(Self::Wide),
        }
    }

    pub(crate) fn to_vec(self) -> Vec<u16> {
        self.iter().collect()
    }

    /// The units as UTF-16, borrowed when they already are.
    pub(crate) fn wide(self) -> Cow<'a, [u16]> {
        match self {
            Self::Ascii(bytes) => Cow::Owned(bytes.iter().map(|byte| u16::from(*byte)).collect()),
            Self::Wide(units) => Cow::Borrowed(units),
        }
    }

    pub(crate) fn extend_into(self, output: &mut Vec<u16>) {
        match self {
            Self::Ascii(bytes) => output.extend(bytes.iter().map(|byte| u16::from(*byte))),
            Self::Wide(units) => output.extend_from_slice(units),
        }
    }

    pub(crate) fn starts_with(self, prefix: Units<'_>) -> bool {
        prefix.len() <= self.len() && self.slice(0..prefix.len()) == prefix
    }

    /// The first index at or after `start` where `search` occurs.
    pub(crate) fn find(self, search: Units<'_>, start: usize) -> Option<usize> {
        if search.is_empty() {
            return Some(start.min(self.len()));
        }
        if search.len() > self.len() {
            return None;
        }
        let last = self.len() - search.len();
        if let (Self::Ascii(haystack), Units::Ascii(needle)) = (self, search) {
            if start > last {
                return None;
            }
            return memchr::memmem::find(&haystack[start..], needle).map(|offset| start + offset);
        }
        let first = search.at(0);
        (start..=last).find(|index| {
            self.at(*index) == first && self.slice(*index..*index + search.len()) == search
        })
    }
}

impl PartialEq for Units<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Ascii(left), Self::Ascii(right)) => left == right,
            (Self::Wide(left), Self::Wide(right)) => left == right,
            (left, right) => left.len() == right.len() && left.iter().eq(right.iter()),
        }
    }
}

impl Eq for Units<'_> {}

impl PartialEq<[u16]> for Units<'_> {
    fn eq(&self, other: &[u16]) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter().copied())
    }
}

impl<const N: usize> PartialEq<[u16; N]> for Units<'_> {
    fn eq(&self, other: &[u16; N]) -> bool {
        *self == other[..]
    }
}

impl PartialEq<&[u16]> for Units<'_> {
    fn eq(&self, other: &&[u16]) -> bool {
        *self == **other
    }
}

impl<const N: usize> PartialEq<&[u16; N]> for Units<'_> {
    fn eq(&self, other: &&[u16; N]) -> bool {
        *self == other[..]
    }
}

impl PartialEq<Vec<u16>> for Units<'_> {
    fn eq(&self, other: &Vec<u16>) -> bool {
        *self == other[..]
    }
}

impl Ord for Units<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (*self, *other) {
            (Self::Ascii(left), Self::Ascii(right)) => left.cmp(right),
            (Self::Wide(left), Self::Wide(right)) => left.cmp(right),
            (left, right) => left.iter().cmp(right.iter()),
        }
    }
}

impl PartialOrd for Units<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Debug for Units<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

#[derive(Clone)]
pub(crate) enum UnitsIter<'a> {
    Ascii(std::slice::Iter<'a, u8>),
    Wide(std::slice::Iter<'a, u16>),
}

impl Iterator for UnitsIter<'_> {
    type Item = u16;

    fn next(&mut self) -> Option<u16> {
        match self {
            Self::Ascii(bytes) => bytes.next().map(|byte| u16::from(*byte)),
            Self::Wide(units) => units.next().copied(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Ascii(bytes) => bytes.size_hint(),
            Self::Wide(units) => units.size_hint(),
        }
    }
}

impl DoubleEndedIterator for UnitsIter<'_> {
    fn next_back(&mut self) -> Option<u16> {
        match self {
            Self::Ascii(bytes) => bytes.next_back().map(|byte| u16::from(*byte)),
            Self::Wide(units) => units.next_back().copied(),
        }
    }
}

impl ExactSizeIterator for UnitsIter<'_> {}

fn is_ascii_unit(unit: u16) -> bool {
    unit < 0x80
}

/// `units` in their canonical storage.
fn flat_units(units: &[u16]) -> FlatUnits {
    if units.iter().all(|unit| is_ascii_unit(*unit)) {
        FlatUnits::ascii_with(units.len(), |bytes| {
            for (byte, unit) in bytes.iter_mut().zip(units) {
                byte.write(*unit as u8);
            }
        })
    } else {
        FlatUnits::Wide(Rc::from(units))
    }
}

/// A string is a rope until its units are first read; flattening then releases the halves, so
/// the text is held once.
struct JsStringInner {
    /// Within `MAX_STRING_UNITS`, so a `u32`; with `ascii` it fills one word.
    length: u32,
    /// Every unit is ASCII; known at construction, before a rope flattens.
    ascii: bool,
    flat: OnceCell<FlatUnits>,
    /// What the string holds beside its units, behind one word: see `Aside`. Mutated only
    /// through the methods below, which keep a `Host` in place once made.
    aside: UnsafeCell<Option<Rc<Aside>>>,
}

/// A string's rarely present parts, which never coexist: the concatenated halves, present
/// exactly while `flat` is unset, and a `Wide` string's host text, made after flattening (an
/// ASCII string's units are its host text). Sharing one word keeps a flat string's
/// allocation small.
enum Aside {
    Rope(Rc<JsStringInner>, Rc<JsStringInner>),
    Host(Rc<str>),
}

/// A length in units as stored; every string is within `MAX_STRING_UNITS`.
fn stored_length(length: usize) -> u32 {
    u32::try_from(length).expect("a string's length is within MAX_STRING_UNITS")
}

impl JsStringInner {
    fn flat(units: FlatUnits) -> Self {
        let (length, ascii) = (units.view().len(), units.is_ascii());
        Self {
            length: stored_length(length),
            ascii,
            flat: OnceCell::from(units),
            aside: UnsafeCell::new(None),
        }
    }

    fn concat(left: Rc<Self>, right: Rc<Self>) -> Self {
        Self {
            length: left.length.saturating_add(right.length),
            ascii: left.ascii && right.ascii,
            flat: OnceCell::new(),
            aside: UnsafeCell::new(Some(Rc::new(Aside::Rope(left, right)))),
        }
    }

    /// The halves of a string that is still a rope.
    fn halves(&self) -> Option<(Rc<Self>, Rc<Self>)> {
        // SAFETY: a shared read; the slot is written only while no reference into it is live
        // (`release_halves` and `host`'s first write below).
        match unsafe { (*self.aside.get()).as_deref() } {
            Some(Aside::Rope(left, right)) => Some((left.clone(), right.clone())),
            Some(Aside::Host(_)) | None => None,
        }
    }

    /// Drops the halves once the units are flat.
    fn release_halves(&self) {
        // SAFETY: while the string is a rope nothing borrows into the slot (`halves` clones),
        // and no host text exists before flattening.
        unsafe {
            let slot = self.aside.get();
            if matches!((*slot).as_deref(), Some(Aside::Rope(..))) {
                *slot = None;
            }
        }
    }

    /// A flat `Wide` string's host text, if `host` has made it.
    fn cached_host(&self) -> Option<&str> {
        // SAFETY: a shared read; a `Host` is never replaced once written (`host`).
        match unsafe { (*self.aside.get()).as_deref() } {
            Some(Aside::Host(host)) => Some(host),
            Some(Aside::Rope(..)) | None => None,
        }
    }

    /// A flat `Wide` string's host text, made by `make` on first use.
    fn host(&self, make: impl FnOnce() -> Rc<str>) -> &Rc<str> {
        // SAFETY: the slot is written only while empty, when no reference into it exists; a
        // `Host` is never replaced, so the returned borrow stays valid for `&self`.
        unsafe {
            let slot = self.aside.get();
            if (*slot).is_none() {
                let host = make();
                charge_materialized(host.len());
                *slot = Some(Rc::new(Aside::Host(host)));
            }
            match (*slot).as_deref() {
                Some(Aside::Host(host)) => host,
                Some(Aside::Rope(..)) | None => {
                    unreachable!("a flat string's aside is its host text")
                }
            }
        }
    }

    /// A flat `Wide` string's host text, if made.
    fn existing_host(&self) -> Option<&Rc<str>> {
        // SAFETY: as `host`: a made `Host` is never replaced.
        match unsafe { (*self.aside.get()).as_deref() } {
            Some(Aside::Host(host)) => Some(host),
            Some(Aside::Rope(..)) | None => None,
        }
    }

    /// Calls `visit` with each flat run of this string, in order.
    fn for_each_run(self: &Rc<Self>, mut visit: impl FnMut(Units<'_>)) {
        let mut pending = vec![self.clone()];
        while let Some(part) = pending.pop() {
            if let Some(units) = part.flat.get() {
                visit(units.view());
                continue;
            }
            let (left, right) = part.halves().expect("an unflattened string is a rope");
            pending.push(right);
            pending.push(left);
        }
    }
}

/// Concatenation copies a piece at most this long into its flat neighbor instead of making a
/// rope node: a node and the separate halves it keeps alive cost more than copying a short
/// piece, and a rope's leaves then hold at least this many units each.
const ROPE_LEAF_MAX_UNITS: usize = 512;
/// A flat left side at most this long takes a short right side by copy, so text built by
/// appends stays flat until it is long; past it, an append copies at most a leaf. Each such
/// append copies the whole prefix, so the bound is the most an append copies: from 8192,
/// pdfjs 12.67G -> 9.42G instructions with max RSS 84.8 -> 86.8 MB, and web-ssr's built
/// text 89.8 -> 100.9 MB (Bun without its JIT: 88.8 and 124.3 MB; 512 saved 2% more on
/// pdfjs for 3.5 MB more on web-ssr; JetStream, Linux x86_64, 2026-10-19).
const FLAT_PREFIX_MAX_UNITS: usize = 2048;

/// Flat UTF-16 strings retain the existing Node-compatible length policy.
/// V8's public String::kMaxLength on the supported 64-bit host is this limit;
/// Node exposes it as buffer.constants.MAX_STRING_LENGTH.
pub(crate) const MAX_STRING_UNITS: usize = 536_870_888;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StringBuildError {
    InvalidLength,
    Allocation,
}

fn checked_string_length(current: usize, additional: usize) -> Result<usize, StringBuildError> {
    current
        .checked_add(additional)
        .filter(|length| *length <= MAX_STRING_UNITS)
        .ok_or(StringBuildError::InvalidLength)
}

/// Parts a builder holds before it allocates: a replacement's prefix, substitution and
/// rest, or a short join, need no allocation beside the result.
const BUILDER_INLINE_PARTS: usize = 4;

struct StringPart {
    units: FlatUnits,
    range: Range<usize>,
}

/// A transient concatenation plan, not a guest rope. Slices retain only the
/// canonical storage. Overflow stops storage growth, but callers still
/// perform all replacement effects before finish reports the error.
pub(super) struct JsStringBuilder {
    parts: smallvec::SmallVec<[StringPart; BUILDER_INLINE_PARTS]>,
    length: Result<usize, StringBuildError>,
}

impl Default for JsStringBuilder {
    fn default() -> Self {
        Self {
            parts: smallvec::SmallVec::new(),
            length: Ok(0),
        }
    }
}

impl JsStringBuilder {
    pub(super) fn append(&mut self, string: &JsString) {
        let units = string.flat();
        let length = units.view().len();
        self.append_units(units.clone(), 0..length);
    }

    pub(super) fn append_slice(&mut self, string: &JsString, range: Range<usize>) {
        self.append_units(string.flat().clone(), range);
    }

    fn append_units(&mut self, units: FlatUnits, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let Ok(current) = self.length else {
            return;
        };
        self.length = checked_string_length(current, range.len());
        if self.length.is_err() {
            self.parts.clear();
            return;
        }
        if let Some(previous) = self.parts.last_mut() {
            if previous.units.same_storage(&units) && previous.range.end == range.start {
                previous.range.end = range.end;
                return;
            }
        }
        if self.parts.try_reserve(1).is_err() {
            self.length = Err(StringBuildError::Allocation);
            self.parts.clear();
            return;
        }
        self.parts.push(StringPart { units, range });
    }

    pub(super) fn finish(self) -> Result<JsString, StringBuildError> {
        let length = self.length?;
        if self.parts.iter().all(|part| part.units.is_ascii()) {
            return Ok(JsString::from_flat(FlatUnits::ascii_with(length, |output| {
                let mut written = 0;
                for part in &self.parts {
                    if let Units::Ascii(bytes) = part.units.view().slice(part.range.clone()) {
                        output[written..written + bytes.len()].write_copy_of_slice(bytes);
                        written += bytes.len();
                    }
                }
                assert_eq!(written, output.len(), "the parts fill the string's length");
            })));
        }
        let mut storage = Rc::<[u16]>::new_uninit_slice(length);
        let output = Rc::get_mut(&mut storage).expect("a new allocation is unshared");
        let mut written = 0;
        for part in &self.parts {
            let units = part.units.view().slice(part.range.clone());
            let target = &mut output[written..written + units.len()];
            match units {
                Units::Ascii(bytes) => {
                    for (unit, byte) in target.iter_mut().zip(bytes) {
                        unit.write(u16::from(*byte));
                    }
                }
                Units::Wide(units) => {
                    target.write_copy_of_slice(units);
                }
            }
            written += units.len();
        }
        assert_eq!(written, length, "the parts fill the string's length");
        // SAFETY: the parts wrote every unit.
        let storage = unsafe { storage.assume_init() };
        // Wide parts can contribute only ASCII units; such text is stored narrow.
        if storage.iter().all(|unit| is_ascii_unit(*unit)) {
            return Ok(JsString::from_units(&storage));
        }
        Ok(JsString::from_flat(FlatUnits::Wide(storage)))
    }
}

impl JsString {
    /// Non-materializing shape for aggregate diagnostics: inline flat, shared flat, or rope.
    #[cfg(feature = "profile-aggregate")]
    pub(crate) fn profile_shape(&self) -> usize {
        match &self.repr {
            Repr::Flat(_) => 0,
            Repr::Shared(inner) if inner.flat.get().is_some() => 1,
            Repr::Shared(_) => 2,
        }
    }

    fn from_flat(units: FlatUnits) -> Self {
        let repr = match units {
            FlatUnits::Short(_) | FlatUnits::Ascii(_) | FlatUnits::AsciiSlice(_) => {
                Repr::Flat(units)
            }
            FlatUnits::Wide(_) | FlatUnits::WideSlice(_) => {
                Repr::Shared(Rc::new(JsStringInner::flat(units)))
            }
        };
        Self { repr }
    }

    fn shared(inner: JsStringInner) -> Self {
        Self { repr: Repr::Shared(Rc::new(inner)) }
    }

    /// The string a rope node holds.
    fn from_node(node: Rc<JsStringInner>) -> Self {
        match node.flat.get() {
            Some(units @ (FlatUnits::Short(_) | FlatUnits::Ascii(_) | FlatUnits::AsciiSlice(_))) => Self {
                repr: Repr::Flat(units.clone()),
            },
            Some(FlatUnits::Wide(_) | FlatUnits::WideSlice(_)) | None => Self { repr: Repr::Shared(node) },
        }
    }

    /// The halves of a string that is still a rope.
    fn rope_halves(&self) -> Option<(Self, Self)> {
        match &self.repr {
            Repr::Flat(_) => None,
            Repr::Shared(inner) => inner
                .halves()
                .map(|(left, right)| (Self::from_node(left), Self::from_node(right))),
        }
    }

    /// Whether the units are flat: not a rope, or a rope already flattened.
    fn is_flat(&self) -> bool {
        match &self.repr {
            Repr::Flat(_) => true,
            Repr::Shared(inner) => inner.flat.get().is_some(),
        }
    }

    /// The string as a node a rope can hold.
    fn node(&self) -> Rc<JsStringInner> {
        match &self.repr {
            Repr::Flat(units) => Rc::new(JsStringInner::flat(units.clone())),
            Repr::Shared(inner) => inner.clone(),
        }
    }

    pub(crate) fn from_units(units: &[u16]) -> Self {
        Self::from_flat(flat_units(units))
    }

    /// The string of `units`, sharing their storage when it is already canonical.
    pub(crate) fn from_unit_run(units: Units<'_>) -> Self {
        match units {
            // SAFETY: `Units::Ascii` only views an ASCII `FlatUnits`, and every run of ASCII
            // bytes is UTF-8.
            Units::Ascii(bytes) => {
                Self::from_flat(FlatUnits::ascii(unsafe { std::str::from_utf8_unchecked(bytes) }))
            }
            Units::Wide(units) => Self::from_units(units),
        }
    }

    /// The units `range` of this string (`String.prototype.slice` and its kin). A long ASCII
    /// result shares this string's storage; any other result is copied.
    pub(crate) fn substring(&self, range: Range<usize>) -> Self {
        Self::from_flat(self.flat().run(range))
    }

    pub(crate) fn from_str(text: &str) -> Self {
        if text.is_ascii() {
            return Self::from_flat(FlatUnits::ascii(text));
        }
        Self::from_flat(FlatUnits::Wide(Rc::from(
            text.encode_utf16().collect::<Vec<_>>(),
        )))
    }

    /// `text` as a string, sharing its allocation when it is stored that way: ASCII text too
    /// long to sit inline (`FlatUnits::Ascii`).
    pub(crate) fn from_shared_text(text: &Rc<str>) -> Self {
        if text.is_ascii() && text.len() > SHORT_ASCII_CAPACITY {
            return Self::from_flat(FlatUnits::Ascii(Rc::clone(text)));
        }
        Self::from_str(text)
    }

    #[inline]
    pub(crate) fn units(&self) -> Units<'_> {
        self.flat().view()
    }

    /// The flat units when the string already has them: never flattens a rope, so a
    /// reader needs no call on its way.
    #[inline]
    pub(crate) fn existing_flat(&self) -> Option<&FlatUnits> {
        match &self.repr {
            Repr::Flat(units) => Some(units),
            Repr::Shared(inner) => inner.flat.get(),
        }
    }

    /// The flat units, flattening a rope on first use.
    #[inline]
    pub(crate) fn flat(&self) -> &FlatUnits {
        let inner = match &self.repr {
            Repr::Flat(units) => return units,
            Repr::Shared(inner) => inner,
        };
        match inner.flat.get() {
            Some(units) => units,
            None => Self::flatten_shared(inner),
        }
    }

    /// `flat` of a rope not yet flattened, out of line so readers of flat strings stay small.
    #[cold]
    #[inline(never)]
    fn flatten_shared(inner: &Rc<JsStringInner>) -> &FlatUnits {
        inner.flat.get_or_init(|| {
            let units = Self::flattened(inner);
            inner.release_halves();
            charge_materialized(match units.view() {
                Units::Ascii(bytes) => bytes.len(),
                Units::Wide(units) => std::mem::size_of_val(units),
            });
            units
        })
    }

    /// The flat units without making a rope flat: a reader that needs the text only while it
    /// runs leaves the rope's halves, which share their storage with the strings they were
    /// cut from, in place of a copy kept for the string's life. Regular-expression matching
    /// flattens V8-v7 RegExp's input variants this way (306 ropes, 995K units).
    pub(crate) fn flat_unkept(&self) -> std::borrow::Cow<'_, FlatUnits> {
        match &self.repr {
            Repr::Flat(units) => std::borrow::Cow::Borrowed(units),
            Repr::Shared(inner) => match inner.flat.get() {
                Some(units) => std::borrow::Cow::Borrowed(units),
                None => std::borrow::Cow::Owned(Self::flattened(inner)),
            },
        }
    }

    /// A flat string with this string's units that leaves a rope unflattened (`flat_unkept`):
    /// the copy lives only as long as the returned string. An operation that reads a rope
    /// many times through helpers that take a string uses it in place of the rope, so the
    /// rope does not keep a flat copy of its text for its own life.
    pub(crate) fn unkept_flat(&self) -> JsString {
        match &self.repr {
            Repr::Flat(_) => self.clone(),
            Repr::Shared(_) => Self::from_flat(self.flat_unkept().into_owned()),
        }
    }

    /// The units of a rope's runs, in one canonical allocation.
    fn flattened(inner: &Rc<JsStringInner>) -> FlatUnits {
        let length = inner.length as usize;
        if inner.ascii {
            return FlatUnits::ascii_with(length, |output| {
                let mut written = 0;
                inner.for_each_run(|run| {
                    if let Units::Ascii(bytes) = run {
                        output[written..written + bytes.len()].write_copy_of_slice(bytes);
                        written += bytes.len();
                    }
                });
            });
        }
        let mut output = Vec::with_capacity(length);
        inner.for_each_run(|run| run.extend_into(&mut output));
        FlatUnits::Wide(Rc::from(output))
    }

    /// The host text a `Wide` string made, if any.
    fn existing_host(&self) -> Option<&Rc<str>> {
        match &self.repr {
            Repr::Flat(_) => None,
            Repr::Shared(inner) => inner.existing_host(),
        }
    }

    /// `host_string` as an owned copy. A `Wide` string with no cached host text is
    /// transcoded straight into the copy, so a one-off conversion (a script source handed
    /// to the compiler) leaves no second copy on the string.
    pub(crate) fn to_host_owned(&self) -> String {
        match (self.flat(), &self.repr) {
            (wide @ (FlatUnits::Wide(_) | FlatUnits::WideSlice(_)), Repr::Shared(inner)) => {
                inner.cached_host().map_or_else(
                    || {
                        let Units::Wide(units) = wide.view() else { unreachable!("wide units") };
                        String::from_utf16_lossy(units)
                    },
                    str::to_owned,
                )
            }
            _ => self.host_string().to_owned(),
        }
    }

    /// The string as host text: its own units when ASCII, lossy UTF-8 made once otherwise.
    pub(crate) fn host_string(&self) -> &str {
        match self.flat() {
            FlatUnits::Short(text) => text.as_str(),
            FlatUnits::Ascii(text) => text,
            FlatUnits::AsciiSlice(slice) => &slice.text[slice.range.clone()],
            wide @ (FlatUnits::Wide(_) | FlatUnits::WideSlice(_)) => match &self.repr {
                Repr::Shared(inner) => inner.host(|| {
                    let Units::Wide(units) = wide.view() else { unreachable!("wide units") };
                    Rc::<str>::from(String::from_utf16_lossy(units))
                }),
                Repr::Flat(_) => unreachable!("a flat string held by value is ASCII"),
            },
        }
    }

    /// Bytes of flat units this string holds; a rope owns only its node until it flattens.
    pub(crate) fn flat_unit_bytes(&self) -> usize {
        let flat = match &self.repr {
            Repr::Flat(units) => Some(units),
            Repr::Shared(inner) => inner.flat.get(),
        };
        match flat {
            // A wide slice holds its record; the units belong to the storage it shares, which
            // its root was charged for.
            Some(FlatUnits::WideSlice(_)) => std::mem::size_of::<WideSlice>(),
            Some(units) => match units.view() {
                Units::Ascii(bytes) => bytes.len(),
                Units::Wide(units) => std::mem::size_of_val(units),
            },
            None => 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        match &self.repr {
            Repr::Flat(units) => units.len(),
            Repr::Shared(inner) => inner.length as usize,
        }
    }

    pub(crate) fn capacity(&self) -> usize {
        match self.flat() {
            FlatUnits::Short(_) => 0,
            FlatUnits::Ascii(text) => text.len(),
            // The storage belongs to the string sliced; the slice owns its record.
            FlatUnits::AsciiSlice(_) => std::mem::size_of::<AsciiSlice>(),
            FlatUnits::Wide(units) => {
                std::mem::size_of_val(&**units)
                    + self.existing_host().map_or(0, |host| host.len())
            }
            // As an ASCII slice: the storage belongs to the string sliced.
            FlatUnits::WideSlice(_) => {
                std::mem::size_of::<WideSlice>() + self.existing_host().map_or(0, |host| host.len())
            }
        }
    }

    /// The string's unit storage and host text, each as (address, bytes), for the memory
    /// report. It reads the string as it is: a rope not yet flattened reports its own node,
    /// whose halves are strings of their own, and is not flattened to be measured (that
    /// kept a flat copy for the string's life, so a report inflated the memory it reported).
    pub(crate) fn memory_parts(&self) -> ((usize, usize), Option<(usize, usize)>) {
        const RC_HEADER_BYTES: usize = 2 * std::mem::size_of::<usize>();
        let Some(flat) = self.existing_flat() else {
            let Repr::Shared(inner) = &self.repr else {
                unreachable!("a flat representation always has flat units");
            };
            let node = (
                Rc::as_ptr(inner) as usize,
                RC_HEADER_BYTES + std::mem::size_of::<JsStringInner>(),
            );
            return (node, None);
        };
        let unit_storage = match flat {
            // Inline units live in the string's own allocation.
            FlatUnits::Short(text) => (text.bytes.as_ptr() as usize, 0),
            FlatUnits::Ascii(text) => (
                Rc::as_ptr(text) as *const u8 as usize,
                RC_HEADER_BYTES + text.len(),
            ),
            // Shared storage is reported once by its address, whichever string holds it.
            FlatUnits::AsciiSlice(slice) => (
                Rc::as_ptr(&slice.text) as *const u8 as usize,
                RC_HEADER_BYTES + slice.text.len(),
            ),
            FlatUnits::Wide(units) => (
                Rc::as_ptr(units) as *const u16 as usize,
                RC_HEADER_BYTES + std::mem::size_of_val(&**units),
            ),
            FlatUnits::WideSlice(slice) => (
                Rc::as_ptr(&slice.units) as *const u16 as usize,
                RC_HEADER_BYTES + std::mem::size_of_val(&*slice.units),
            ),
        };
        let host_storage = self.existing_host().map(|host| {
            (
                Rc::as_ptr(host) as *const u8 as usize,
                RC_HEADER_BYTES + host.len(),
            )
        });
        (unit_storage, host_storage)
    }

    pub(crate) fn push_js_string(&mut self, text: &Self) {
        if self.len() == 0 {
            *self = text.clone();
            return;
        }
        if text.len() == 0 {
            return;
        }
        let short = |piece: &Self| piece.len() <= ROPE_LEAF_MAX_UNITS;
        // Two flat pieces the policy below joins: one copy of each, straight into the joined
        // storage when both are ASCII (a template literal's pieces, a key built from parts,
        // a rope's last leaf and the text appended to it).
        let joined = |left: &Self, right: &Self| {
            if let (Some(Units::Ascii(head)), Some(Units::Ascii(tail))) =
                (left.existing_flat().map(FlatUnits::view), right.existing_flat().map(FlatUnits::view))
            {
                let units = FlatUnits::ascii_with(head.len() + tail.len(), |bytes| {
                    let (start, end) = bytes.split_at_mut(head.len());
                    start.write_copy_of_slice(head);
                    end.write_copy_of_slice(tail);
                });
                return Self { repr: Repr::Flat(units) };
            }
            let mut flat = JsStringBuilder::default();
            flat.append(left);
            flat.append(right);
            flat.finish()
                .expect("a copied concatenation is within the string length limit")
        };
        let rope = |left: &Self, right: &Self| {
            Self::shared(JsStringInner::concat(left.node(), right.node()))
        };
        // A flattened rope joins as the flat text it holds.
        if self.existing_flat().is_some()
            && text.existing_flat().is_some()
            && short(text)
            && self.len() <= FLAT_PREFIX_MAX_UNITS
        {
            *self = joined(self, text);
            return;
        }
        // The QuickJS policy: a short flat piece joins the flat side it meets, whether that
        // is the other operand or the facing half of a rope operand.
        *self = match (self.rope_halves(), text.rope_halves()) {
            (None, None) if short(text) && self.len() <= FLAT_PREFIX_MAX_UNITS => {
                joined(self, text)
            }
            (Some((left, right)), None) if short(text) && right.is_flat() && short(&right) => {
                rope(&left, &joined(&right, text))
            }
            (None, Some((left, right)))
                if self.len() <= FLAT_PREFIX_MAX_UNITS && left.is_flat() && short(&left) =>
            {
                rope(&joined(self, &left), &right)
            }
            _ => rope(self, text),
        };
    }

    pub(crate) fn repeat(&self, count: usize) -> Self {
        let mut count = count;
        let mut result = Self::from_str("");
        let mut power = self.clone();
        while count > 0 {
            if count & 1 != 0 {
                result.push_js_string(&power);
            }
            count >>= 1;
            if count > 0 {
                let node = power.node();
                power = Self::shared(JsStringInner::concat(node.clone(), node));
            }
        }
        result
    }

    pub(crate) fn find_units(&self, search: Units<'_>, start: usize) -> Option<usize> {
        self.units().find(search, start)
    }

    pub(crate) fn split_units(&self, separator: Units<'_>) -> Vec<Self> {
        let units = self.units();
        if separator.is_empty() {
            return (0..units.len())
                .map(|index| Self::from_unit_run(units.slice(index..index + 1)))
                .collect();
        }
        let mut parts = Vec::new();
        let mut cursor = 0;
        while let Some(offset) = units.find(separator, cursor) {
            parts.push(Self::from_unit_run(units.slice(cursor..offset)));
            cursor = offset + separator.len();
        }
        parts.push(Self::from_unit_run(units.slice(cursor..units.len())));
        parts
    }
}

impl PartialEq for JsString {
    fn eq(&self, other: &Self) -> bool {
        if let (Repr::Flat(FlatUnits::Short(left)), Repr::Flat(FlatUnits::Short(right))) =
            (&self.repr, &other.repr)
        {
            // Inline text pads its unused bytes with zero, so whole records compare.
            return left.length == right.length && left.bytes == right.bytes;
        }
        // Strings of different lengths differ without flattening a rope or reading units.
        self.len() == other.len() && self.units() == other.units()
    }
}

impl Eq for JsString {}

impl Hash for JsString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hashed by units, so a slice and a copy of the same text agree.
        match self.units() {
            Units::Ascii(bytes) => bytes.hash(state),
            Units::Wide(units) => units.hash(state),
        }
    }
}

impl std::fmt::Debug for JsString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("JsString")
            .field(&self.host_string())
            .finish()
    }
}

impl From<&str> for JsString {
    fn from(value: &str) -> Self {
        Self::from_str(value)
    }
}

impl From<String> for JsString {
    fn from(value: String) -> Self {
        Self::from_str(&value)
    }
}

impl From<JsString> for String {
    fn from(value: JsString) -> Self {
        value.host_string().to_owned()
    }
}

impl FromIterator<char> for JsString {
    fn from_iter<T: IntoIterator<Item = char>>(iter: T) -> Self {
        Self::from(iter.into_iter().collect::<String>())
    }
}

impl std::fmt::Display for JsString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.host_string())
    }
}

#[cfg(test)]
mod tests {
    use super::JsString;

    #[test]
    fn concatenation_length_checks_the_limit_and_arithmetic_overflow() {
        use super::{MAX_STRING_UNITS, StringBuildError, checked_string_length};
        assert_eq!(
            checked_string_length(MAX_STRING_UNITS - 1, 1),
            Ok(MAX_STRING_UNITS)
        );
        assert_eq!(
            checked_string_length(MAX_STRING_UNITS, 1),
            Err(StringBuildError::InvalidLength)
        );
        assert_eq!(
            checked_string_length(usize::MAX, 1),
            Err(StringBuildError::InvalidLength)
        );
    }

    #[test]
    fn concatenation_plan_preserves_units_and_merges_adjacent_slices() {
        let source = JsString::from_units(&[0xD800, b'a' as u16, 0xDC00]);
        let mut plan = super::JsStringBuilder::default();
        plan.append_slice(&source, 0..1);
        plan.append_slice(&source, 1..3);
        assert_eq!(plan.parts.len(), 1);
        plan.append(&source);
        drop(source);
        assert_eq!(
            plan.finish().unwrap().units(),
            &[0xD800, b'a' as u16, 0xDC00, 0xD800, b'a' as u16, 0xDC00]
        );
    }

    #[test]
    fn preserves_lone_surrogates_until_host_conversion() {
        let value = JsString::from_units(&[0xD800, b'a' as u16, 0xDC00]);
        assert_eq!(value.units(), &[0xD800, b'a' as u16, 0xDC00]);
        assert_eq!(value.to_string(), "�a�");
    }

    #[test]
    fn encodes_unicode_scalars_as_utf16_units() {
        let value = JsString::from_str("A🦀");
        assert_eq!(value.units(), &[u16::from(b'A'), 0xD83E, 0xDD80]);
    }

    #[test]
    fn concatenation_keeps_utf16_units_and_flattens_on_demand() {
        let mut value = JsString::from_units(&[0xD800, b'a' as u16]);
        value.push_js_string(&JsString::from_str("bc"));
        assert_eq!(value.units().len(), 4);
        assert_eq!(
            value.units(),
            &[0xD800, b'a' as u16, b'b' as u16, b'c' as u16]
        );
        assert_eq!(value.host_string(), "�abc");
    }

    #[test]
    fn repeats_units_without_reencoding_surrogates() {
        let value = JsString::from_units(&[0xD800, b'a' as u16]);
        let repeated = value.repeat(2);
        assert_eq!(
            repeated.units(),
            &[0xD800, b'a' as u16, 0xD800, b'a' as u16]
        );
    }

    #[test]
    fn searches_and_splits_by_units() {
        let value = JsString::from_units(&[0xD800, b'|' as u16, 0xDC00]);
        assert_eq!(
            value.find_units(super::Units::Wide(&[b'|' as u16]), 0),
            Some(1)
        );
        let parts = value.split_units(super::Units::Wide(&[b'|' as u16]));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].units(), &[0xD800]);
        assert_eq!(parts[1].units(), &[0xDC00]);
    }

    #[test]
    fn host_text_is_lazy_and_utf16_remains_authoritative() {
        let paired = JsString::from_units(&[0xD83E, 0xDD80]);
        let lone = JsString::from_units(&[0xD800]);

        assert!(paired.existing_host().is_none());
        assert!(lone.existing_host().is_none());
        assert!(paired.existing_host().is_none());
        assert!(lone.existing_host().is_none());
        assert_eq!(lone.host_string(), "�");
        assert_eq!(lone.units(), &[0xD800]);
        assert!(lone.existing_host().is_some());
    }
}
