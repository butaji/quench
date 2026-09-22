use std::rc::Rc;

/// Heap-owned JavaScript string. The UTF-16 units are authoritative; `host`
/// is only the explicit lossy Rust-text view used at host/API boundaries.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct JsString {
    units: Rc<[u16]>,
    host: String,
}

impl JsString {
    pub(crate) fn from_units(units: &[u16]) -> Self {
        let units = Rc::from(units);
        let host = String::from_utf16_lossy(&units);
        Self { units, host }
    }

    pub(crate) fn from_str(text: &str) -> Self {
        Self {
            units: Rc::from(text.encode_utf16().collect::<Vec<_>>()),
            host: text.to_owned(),
        }
    }

    pub(crate) fn units(&self) -> &[u16] {
        &self.units
    }

    pub(crate) fn host_string(&self) -> &str {
        &self.host
    }

    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(crate) fn capacity(&self) -> usize {
        self.units.len() * std::mem::size_of::<u16>() + self.host.capacity()
    }

    pub(crate) fn push_str(&mut self, text: &str) {
        let mut units = self.units.to_vec();
        units.extend(text.encode_utf16());
        self.units = Rc::from(units);
        self.host.push_str(text);
    }

    pub(crate) fn push_js_string(&mut self, text: &Self) {
        let mut units = self.units.to_vec();
        units.extend(text.units.iter().copied());
        self.units = Rc::from(units);
        self.host.push_str(&text.host);
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
            host: value,
        }
    }
}

impl From<JsString> for String {
    fn from(value: JsString) -> Self {
        value.host
    }
}

impl FromIterator<char> for JsString {
    fn from_iter<T: IntoIterator<Item = char>>(iter: T) -> Self {
        Self::from(iter.into_iter().collect::<String>())
    }
}

impl std::fmt::Display for JsString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.host)
    }
}

#[cfg(test)]
mod tests {
    use super::JsString;

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
