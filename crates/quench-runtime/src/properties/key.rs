//! Canonical property-key representation used by ordinary object fast paths.
//!
//! JavaScript symbols and exotic keys remain on the generic path; this type is
//! deliberately limited to string keys so it cannot accidentally bypass those
//! semantics.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PropertyKey(String);

impl PropertyKey {
    #[inline]
    pub(crate) fn new(key: &str) -> Self {
        Self(key.to_owned())
    }

    #[inline]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns a canonical array index, or `None` for ordinary names.
    #[inline]
    pub(crate) fn array_index(&self) -> Option<u32> {
        crate::strings::canonical_array_index(&self.0)
    }
}

impl From<&str> for PropertyKey {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::PropertyKey;

    #[test]
    fn preserves_identity_and_classifies_indices() {
        let key = PropertyKey::new("answer");
        assert_eq!(key.as_str(), "answer");
        assert_eq!(key.array_index(), None);
        assert_eq!(PropertyKey::new("12").array_index(), Some(12));
        assert_eq!(PropertyKey::new("01").array_index(), None);
    }
}
