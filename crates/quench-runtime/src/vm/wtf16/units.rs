//! Borrowed runs of a string's code units (`Units`) and their iteration and comparison.

use super::*;

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

    /// These units' UTF-8 encoding, each lone surrogate encoded as the replacement character
    /// (`utf8_len` counts it): an ASCII run's bytes as they are stored.
    pub(crate) fn to_utf8(self) -> Vec<u8> {
        match self {
            Self::Ascii(bytes) => bytes.to_vec(),
            Self::Wide(units) => utf16_lossy(units).into_bytes(),
        }
    }

    /// The length of these units' UTF-8 encoding, each lone surrogate encoded as the
    /// replacement character, as V8's `String::Utf8Length` counts it: an ASCII run's length,
    /// or a wide run's code points summed without copying the units.
    pub(crate) fn utf8_len(self) -> usize {
        match self {
            Self::Ascii(bytes) => bytes.len(),
            Self::Wide(units) => char::decode_utf16(units.iter().copied())
                .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER).len_utf8())
                .sum(),
        }
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
