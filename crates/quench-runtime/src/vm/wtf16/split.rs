//! `String.prototype.split`'s gathered parts. The cuts themselves are `staged::string_split::runs`.

/// The parts a split makes that are held without an allocation while they are gathered:
/// most splits (a path, a key, a short record) make no more.
const SPLIT_INLINE_PARTS: usize = 8;

/// A split's parts in order, gathered before their array is made.
pub(crate) type SplitParts<T> = smallvec::SmallVec<[T; SPLIT_INLINE_PARTS]>;
