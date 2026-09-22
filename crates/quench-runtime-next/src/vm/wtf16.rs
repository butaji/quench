use std::rc::Rc;

/// JavaScript strings are sequences of UTF-16 code units. Keep that sequence
/// intact while crossing string algorithms; conversion to Rust text is an
/// explicit host boundary and may use replacement characters for lone
/// surrogates.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Wtf16(Rc<[u16]>);

impl Wtf16 {
    pub(super) fn from_units(units: &[u16]) -> Self {
        Self(Rc::from(units))
    }

    pub(super) fn from_str(text: &str) -> Self {
        Self(Rc::from(text.encode_utf16().collect::<Vec<_>>()))
    }

    pub(super) fn units(&self) -> &[u16] {
        &self.0
    }

    /// Convert at the Rust/host boundary. Internal algorithms should use
    /// `units` so lone surrogates remain distinguishable until this point.
    pub(super) fn to_host_string(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Wtf16;

    #[test]
    fn preserves_lone_surrogates_until_host_conversion() {
        let value = Wtf16::from_units(&[0xD800, b'a' as u16, 0xDC00]);
        assert_eq!(value.units(), &[0xD800, b'a' as u16, 0xDC00]);
        assert_eq!(value.to_host_string(), "�a�");
    }

    #[test]
    fn encodes_unicode_scalars_as_utf16_units() {
        let value = Wtf16::from_str("A🦀");
        assert_eq!(value.units(), &[u16::from(b'A'), 0xD83E, 0xDD80]);
    }
}
