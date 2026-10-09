use std::cell::OnceCell;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

/// Heap-owned JavaScript string. The UTF-16 units are authoritative; `host`
/// is only the explicit lossy Rust-text view used at host/API boundaries.
#[derive(Clone)]
pub(crate) struct JsString {
    inner: Rc<JsStringInner>,
}

enum JsStringData {
    Flat(Rc<[u16]>),
    Concat(Rc<JsStringInner>, Rc<JsStringInner>),
}

struct JsStringInner {
    data: JsStringData,
    length: usize,
    flat: OnceCell<Rc<[u16]>>,
    host: OnceCell<String>,
}

impl JsStringInner {
    fn flat(units: Rc<[u16]>, host: Option<String>) -> Self {
        let length = units.len();
        let flat = OnceCell::new();
        let _ = flat.set(units.clone());
        let host_cell = OnceCell::new();
        if let Some(host) = host {
            let _ = host_cell.set(host);
        }
        Self {
            data: JsStringData::Flat(units),
            length,
            flat,
            host: host_cell,
        }
    }

    fn concat(left: Rc<Self>, right: Rc<Self>) -> Self {
        let length = left.length.saturating_add(right.length);
        Self {
            data: JsStringData::Concat(left, right),
            length,
            flat: OnceCell::new(),
            host: OnceCell::new(),
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
    units: Rc<[u16]>,
    range: Range<usize>,
}

/// A transient concatenation plan, not a guest rope. Slices retain only the
/// canonical UTF-16 storage. Overflow stops storage growth, but callers still
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
        let units = string.shared_units();
        let length = units.len();
        self.append_units(units, 0..length);
    }

    pub(super) fn append_slice(&mut self, string: &JsString, range: Range<usize>) {
        self.append_units(string.shared_units(), range);
    }

    fn append_units(&mut self, units: Rc<[u16]>, range: Range<usize>) {
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
            if Rc::ptr_eq(&previous.units, &units) && previous.range.end == range.start {
                previous.range.end = range.end;
                return;
            }
        }
        if self.parts.try_reserve(1).is_err() {
            self.length = Err(StringBuildError::Allocation);
            self.parts.clear();
            return;
        }
        self.parts.push(StringPart {
            units,
            range,
        });
    }

    pub(super) fn finish(self) -> Result<JsString, StringBuildError> {
        let length = self.length?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(length)
            .map_err(|_| StringBuildError::Allocation)?;
        for part in self.parts {
            output.extend_from_slice(&part.units[part.range]);
        }
        Ok(JsString::from_units(&output))
    }
}

impl JsString {
    pub(crate) fn from_units(units: &[u16]) -> Self {
        let units = Rc::from(units);
        let host = String::from_utf16_lossy(&units);
        Self {
            inner: Rc::new(JsStringInner::flat(units, Some(host))),
        }
    }

    pub(crate) fn from_str(text: &str) -> Self {
        Self {
            inner: Rc::new(JsStringInner::flat(
                Rc::from(text.encode_utf16().collect::<Vec<_>>()),
                Some(text.to_owned()),
            )),
        }
    }

    pub(crate) fn units(&self) -> &[u16] {
        self.inner
            .flat
            .get_or_init(|| {
                let mut output = Vec::with_capacity(self.inner.length);
                let mut pending = vec![self.inner.clone()];
                while let Some(part) = pending.pop() {
                    match &part.data {
                        JsStringData::Flat(units) => output.extend_from_slice(units),
                        JsStringData::Concat(left, right) => {
                            pending.push(right.clone());
                            pending.push(left.clone());
                        }
                    }
                }
                Rc::from(output)
            })
            .as_ref()
    }

    fn shared_units(&self) -> Rc<[u16]> {
        self.inner
            .flat
            .get_or_init(|| {
                let mut output = Vec::with_capacity(self.inner.length);
                let mut pending = vec![self.inner.clone()];
                while let Some(part) = pending.pop() {
                    match &part.data {
                        JsStringData::Flat(units) => output.extend_from_slice(units),
                        JsStringData::Concat(left, right) => {
                            pending.push(right.clone());
                            pending.push(left.clone());
                        }
                    }
                }
                Rc::from(output)
            })
            .clone()
    }

    pub(crate) fn host_string(&self) -> &str {
        self.inner
            .host
            .get_or_init(|| String::from_utf16_lossy(self.units()))
    }

    #[cfg(feature = "profile-aggregate")]
    pub(crate) fn len(&self) -> usize {
        self.inner.length
    }

    pub(crate) fn has_lossless_host_string(&self) -> bool {
        self.host_string().encode_utf16().eq(self.units().iter().copied())
    }

    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(crate) fn capacity(&self) -> usize {
        self.units().len() * std::mem::size_of::<u16>() + self.host_string().capacity()
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

    pub(crate) fn find_units(&self, search: &[u16], start: usize) -> Option<usize> {
        if search.is_empty() {
            return Some(start.min(self.units().len()));
        }
        if search.len() > self.units().len() {
            return None;
        }
        (start..=self.units().len().saturating_sub(search.len()))
            .find(|index| self.units()[*index..*index + search.len()] == *search)
    }

    pub(crate) fn split_units(&self, separator: &[u16]) -> Vec<Self> {
        if separator.is_empty() {
            return self
                .units()
                .iter()
                .map(|unit| Self::from_units(std::slice::from_ref(unit)))
                .collect();
        }
        let mut parts = Vec::new();
        let mut cursor = 0;
        while let Some(offset) = self.find_units(separator, cursor) {
            parts.push(Self::from_units(&self.units()[cursor..offset]));
            cursor = offset + separator.len();
        }
        parts.push(Self::from_units(&self.units()[cursor..]));
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
        self.units().hash(state);
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
        let units = Rc::from(value.encode_utf16().collect::<Vec<_>>());
        Self {
            inner: Rc::new(JsStringInner::flat(units, Some(value))),
        }
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
        assert_eq!(value.find_units(&[b'|' as u16], 0), Some(1));
        let parts = value.split_units(&[b'|' as u16]);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].units(), &[0xD800]);
        assert_eq!(parts[1].units(), &[0xDC00]);
    }
}
