use std::borrow::Cow;
use std::cell::{Cell, OnceCell};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

/// Heap-owned JavaScript string. Its code units are authoritative: one byte each when every
/// unit is ASCII, UTF-16 otherwise. `host` is the explicit lossy Rust-text view used at
/// host/API boundaries; an ASCII string is its own host text.
#[derive(Clone)]
pub(crate) struct JsString {
    inner: Rc<JsStringInner>,
}

/// Flat code units. A string whose units are all ASCII is always stored `Ascii`, so equal
/// strings share a representation.
#[derive(Clone)]
pub(crate) enum FlatUnits {
    Ascii(Rc<str>),
    Wide(Rc<[u16]>),
}

impl FlatUnits {
    fn view(&self) -> Units<'_> {
        match self {
            Self::Ascii(text) => Units::Ascii(text.as_bytes()),
            Self::Wide(units) => Units::Wide(units),
        }
    }

    fn same_storage(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Ascii(left), Self::Ascii(right)) => Rc::ptr_eq(left, right),
            (Self::Wide(left), Self::Wide(right)) => Rc::ptr_eq(left, right),
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
            return (start..=last).find(|index| haystack[*index..*index + needle.len()] == *needle);
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
        let text: String = units.iter().map(|unit| char::from(*unit as u8)).collect();
        FlatUnits::Ascii(Rc::from(text))
    } else {
        FlatUnits::Wide(Rc::from(units))
    }
}

/// A string is a rope until its units are first read; flattening then releases the halves, so
/// the text is held once.
struct JsStringInner {
    length: usize,
    /// Every unit is ASCII; known at construction, before a rope flattens.
    ascii: bool,
    flat: OnceCell<FlatUnits>,
    /// The concatenated halves, present exactly while `flat` is unset.
    rope: Cell<Option<(Rc<JsStringInner>, Rc<JsStringInner>)>>,
    /// The host text of a `Wide` string; an `Ascii` string's units are its host text.
    host: OnceCell<Rc<str>>,
}

impl JsStringInner {
    fn flat(units: FlatUnits) -> Self {
        let (length, ascii) = match &units {
            FlatUnits::Ascii(text) => (text.len(), true),
            FlatUnits::Wide(units) => (units.len(), false),
        };
        Self {
            length,
            ascii,
            flat: OnceCell::from(units),
            rope: Cell::new(None),
            host: OnceCell::new(),
        }
    }

    fn concat(left: Rc<Self>, right: Rc<Self>) -> Self {
        Self {
            length: left.length.saturating_add(right.length),
            ascii: left.ascii && right.ascii,
            flat: OnceCell::new(),
            rope: Cell::new(Some((left, right))),
            host: OnceCell::new(),
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
            let (left, right) = part.rope.take().expect("an unflattened string is a rope");
            pending.push(right.clone());
            pending.push(left.clone());
            part.rope.set(Some((left, right)));
        }
    }
}

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

struct StringPart {
    units: FlatUnits,
    range: Range<usize>,
}

/// A transient concatenation plan, not a guest rope. Slices retain only the
/// canonical storage. Overflow stops storage growth, but callers still
/// perform all replacement effects before finish reports the error.
pub(super) struct JsStringBuilder {
    parts: Vec<StringPart>,
    length: Result<usize, StringBuildError>,
}

impl Default for JsStringBuilder {
    fn default() -> Self {
        Self {
            parts: Vec::new(),
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
        let ascii = self
            .parts
            .iter()
            .all(|part| matches!(part.units, FlatUnits::Ascii(_)));
        if ascii {
            let mut output = String::new();
            output
                .try_reserve_exact(length)
                .map_err(|_| StringBuildError::Allocation)?;
            for part in &self.parts {
                if let FlatUnits::Ascii(text) = &part.units {
                    output.push_str(&text[part.range.clone()]);
                }
            }
            return Ok(JsString::from_flat(FlatUnits::Ascii(Rc::from(output))));
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(length)
            .map_err(|_| StringBuildError::Allocation)?;
        for part in &self.parts {
            part.units.view().slice(part.range.clone()).extend_into(&mut output);
        }
        Ok(JsString::from_units(&output))
    }
}

impl JsString {
    fn from_flat(units: FlatUnits) -> Self {
        Self {
            inner: Rc::new(JsStringInner::flat(units)),
        }
    }

    pub(crate) fn from_units(units: &[u16]) -> Self {
        Self::from_flat(flat_units(units))
    }

    /// The string of `units`, sharing their storage when it is already canonical.
    pub(crate) fn from_unit_run(units: Units<'_>) -> Self {
        match units {
            Units::Ascii(bytes) => Self::from_flat(FlatUnits::Ascii(Rc::from(
                std::str::from_utf8(bytes).expect("ASCII units are UTF-8"),
            ))),
            Units::Wide(units) => Self::from_units(units),
        }
    }

    pub(crate) fn from_str(text: &str) -> Self {
        if text.is_ascii() {
            return Self::from_flat(FlatUnits::Ascii(Rc::from(text)));
        }
        Self::from_flat(FlatUnits::Wide(Rc::from(
            text.encode_utf16().collect::<Vec<_>>(),
        )))
    }

    pub(crate) fn units(&self) -> Units<'_> {
        self.flat().view()
    }

    /// The flat units, flattening a rope on first use.
    pub(crate) fn flat(&self) -> &FlatUnits {
        self.inner.flat.get_or_init(|| {
            let units = if self.inner.ascii {
                let mut output = String::with_capacity(self.inner.length);
                self.inner.for_each_run(|run| {
                    if let Units::Ascii(bytes) = run {
                        output.push_str(std::str::from_utf8(bytes).expect("ASCII units are UTF-8"));
                    }
                });
                FlatUnits::Ascii(Rc::from(output))
            } else {
                let mut output = Vec::with_capacity(self.inner.length);
                self.inner.for_each_run(|run| run.extend_into(&mut output));
                FlatUnits::Wide(Rc::from(output))
            };
            drop(self.inner.rope.take());
            units
        })
    }

    pub(crate) fn host_string(&self) -> &str {
        self.host_text()
    }

    /// The host text `host_string` borrows, shared rather than copied.
    pub(crate) fn host_name(&self) -> Rc<str> {
        self.host_text().clone()
    }

    fn host_text(&self) -> &Rc<str> {
        match self.flat() {
            FlatUnits::Ascii(text) => text,
            FlatUnits::Wide(units) => self
                .inner
                .host
                .get_or_init(|| Rc::<str>::from(String::from_utf16_lossy(units))),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.inner.length
    }

    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(crate) fn capacity(&self) -> usize {
        match self.flat() {
            FlatUnits::Ascii(text) => text.len(),
            FlatUnits::Wide(units) => {
                std::mem::size_of_val(&**units) + self.inner.host.get().map_or(0, |host| host.len())
            }
        }
    }

    #[cfg(feature = "profile-memory")]
    pub(crate) fn memory_parts(&self) -> ((usize, usize), Option<(usize, usize)>) {
        const RC_HEADER_BYTES: usize = 2 * std::mem::size_of::<usize>();
        let unit_storage = match self.flat() {
            FlatUnits::Ascii(text) => (
                Rc::as_ptr(text) as *const u8 as usize,
                RC_HEADER_BYTES + text.len(),
            ),
            FlatUnits::Wide(units) => (
                Rc::as_ptr(units) as *const u16 as usize,
                RC_HEADER_BYTES + std::mem::size_of_val(&**units),
            ),
        };
        let host_storage = self.inner.host.get().map(|host| {
            (
                Rc::as_ptr(host) as *const u8 as usize,
                RC_HEADER_BYTES + host.len(),
            )
        });
        (unit_storage, host_storage)
    }

    pub(crate) fn push_js_string(&mut self, text: &Self) {
        if self.inner.length == 0 {
            *self = text.clone();
            return;
        }
        if text.inner.length == 0 {
            return;
        }
        self.inner = Rc::new(JsStringInner::concat(
            self.inner.clone(),
            text.inner.clone(),
        ));
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
                power = Self {
                    inner: Rc::new(JsStringInner::concat(
                        power.inner.clone(),
                        power.inner.clone(),
                    )),
                };
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
        self.units() == other.units()
    }
}

impl Eq for JsString {}

impl Hash for JsString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Canonical storage makes equal strings share a representation.
        match self.units() {
            Units::Ascii(bytes) => bytes.hash(state),
            Units::Wide(units) => units.hash(state),
        }
    }
}

impl std::fmt::Debug for JsString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("JsString").field(&self.host_string()).finish()
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
        assert_eq!(value.units(), &[0xD800, b'a' as u16, b'b' as u16, b'c' as u16]);
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
        assert_eq!(value.find_units(super::Units::Wide(&[b'|' as u16]), 0), Some(1));
        let parts = value.split_units(super::Units::Wide(&[b'|' as u16]));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].units(), &[0xD800]);
        assert_eq!(parts[1].units(), &[0xDC00]);
    }

    #[test]
    fn host_text_is_lazy_and_utf16_remains_authoritative() {
        let paired = JsString::from_units(&[0xD83E, 0xDD80]);
        let lone = JsString::from_units(&[0xD800]);

        assert!(paired.inner.host.get().is_none());
        assert!(lone.inner.host.get().is_none());
        assert!(paired.inner.host.get().is_none());
        assert!(lone.inner.host.get().is_none());
        assert_eq!(lone.host_string(), "�");
        assert_eq!(lone.units(), &[0xD800]);
        assert!(lone.inner.host.get().is_some());
    }
}
