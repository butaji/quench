use std::cell::OnceCell;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

/// Heap-owned JavaScript string. The UTF-16 units are authoritative; `host`
/// memoizes the lossy Rust-text view only after a host/API boundary requests it.
#[derive(Clone, Debug)]
pub(crate) struct JsString {
    units: Rc<[u16]>,
    host: OnceCell<Rc<str>>,
}

impl PartialEq for JsString {
    fn eq(&self, other: &Self) -> bool {
        self.units == other.units
    }
}

impl Eq for JsString {}

impl Hash for JsString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.units.hash(state);
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
        self.append_slice(string, 0..string.units.len());
    }

    pub(super) fn append_slice(&mut self, string: &JsString, range: Range<usize>) {
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
            if Rc::ptr_eq(&previous.units, &string.units) && previous.range.end == range.start {
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
            units: Rc::clone(&string.units),
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
        Self {
            units: Rc::from(units),
            host: OnceCell::new(),
        }
    }

    pub(crate) fn from_str(text: &str) -> Self {
        Self {
            units: Rc::from(text.encode_utf16().collect::<Vec<_>>()),
            host: OnceCell::new(),
        }
    }

    pub(crate) fn shared_units(&self) -> &Rc<[u16]> {
        &self.units
    }

    pub(crate) fn units(&self) -> &[u16] {
        &self.units
    }

    pub(crate) fn host_string(&self) -> &str {
        self.host
            .get_or_init(|| Rc::<str>::from(String::from_utf16_lossy(&self.units)))
            .as_ref()
    }

    pub(crate) fn has_lossless_host_string(&self) -> bool {
        char::decode_utf16(self.units.iter().copied()).all(|decoded| decoded.is_ok())
    }

    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(crate) fn capacity(&self) -> usize {
        self.units.len() * std::mem::size_of::<u16>() + self.host.get().map_or(0, |host| host.len())
    }

    #[cfg(feature = "profile-memory")]
    pub(crate) fn memory_parts(&self) -> ((usize, usize), Option<(usize, usize)>) {
        const RC_HEADER_WORDS: usize = 2;
        let rc_header_bytes = RC_HEADER_WORDS * std::mem::size_of::<usize>();
        let units = (
            Rc::as_ptr(&self.units) as *const u16 as usize,
            rc_header_bytes + self.units.len() * std::mem::size_of::<u16>(),
        );
        let host = self.host.get().map(|host| {
            (
                Rc::as_ptr(host) as *const u8 as usize,
                rc_header_bytes + host.len(),
            )
        });
        (units, host)
    }

    pub(crate) fn push_js_string(&mut self, text: &Self) {
        // A chain of slice iterators has an exact length, so this allocates the result once.
        self.units = self
            .units
            .iter()
            .chain(text.units.iter())
            .copied()
            .collect();
        self.host = OnceCell::new();
    }

    pub(crate) fn repeat(&self, count: usize) -> Self {
        let mut units = Vec::with_capacity(self.units.len().saturating_mul(count));
        for _ in 0..count {
            units.extend(self.units.iter().copied());
        }
        Self::from_units(&units)
    }

    pub(crate) fn find_units(&self, search: &[u16], start: usize) -> Option<usize> {
        if search.is_empty() {
            return Some(start.min(self.units.len()));
        }
        if search.len() > self.units.len() {
            return None;
        }
        (start..=self.units.len().saturating_sub(search.len()))
            .find(|index| self.units[*index..*index + search.len()] == *search)
    }

    pub(crate) fn split_units(&self, separator: &[u16]) -> Vec<Self> {
        if separator.is_empty() {
            return self
                .units
                .iter()
                .map(|unit| Self::from_units(std::slice::from_ref(unit)))
                .collect();
        }
        let mut parts = Vec::new();
        let mut cursor = 0;
        while let Some(offset) = self.find_units(separator, cursor) {
            parts.push(Self::from_units(&self.units[cursor..offset]));
            cursor = offset + separator.len();
        }
        parts.push(Self::from_units(&self.units[cursor..]));
        parts
    }
}

impl From<&str> for JsString {
    fn from(value: &str) -> Self {
        Self::from_str(value)
    }
}

impl From<String> for JsString {
    fn from(value: String) -> Self {
        Self {
            units: Rc::from(value.encode_utf16().collect::<Vec<_>>()),
            host: OnceCell::new(),
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
    use std::hash::{DefaultHasher, Hash, Hasher};

    fn hash(string: &JsString) -> u64 {
        let mut hasher = DefaultHasher::new();
        string.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn host_text_is_lazy_and_does_not_affect_string_identity() {
        let cold = JsString::from_str("ascii");
        let warm = JsString::from_units(cold.units());

        assert!(cold.host.get().is_none());
        assert!(warm.host.get().is_none());
        assert_eq!(cold, warm);
        assert_eq!(hash(&cold), hash(&warm));

        assert_eq!(warm.host_string(), "ascii");
        assert!(warm.host.get().is_some());
        assert_eq!(cold, warm);
        assert_eq!(hash(&cold), hash(&warm));
    }

    #[test]
    fn lossless_host_view_check_does_not_materialize_the_view() {
        let paired = JsString::from_units(&[0xD83E, 0xDD80]);
        let lone = JsString::from_units(&[0xD800]);

        assert!(paired.has_lossless_host_string());
        assert!(!lone.has_lossless_host_string());
        assert!(paired.host.get().is_none());
        assert!(lone.host.get().is_none());
        assert_eq!(lone.host_string(), "�");
        assert_eq!(lone.units(), &[0xD800]);
    }

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
