//! Joining flat pieces into new flat storage.
use super::*;

impl JsString {
    /// Flat `left` then flat `right`, which is short: copied into storage of their exact
    /// length while `left` is short too, else into an append buffer later appends extend.
    pub(super) fn extended(left: &Self, right: &Self) -> Self {
        if left.len() <= APPEND_BUFFER_MIN_UNITS {
            return Self::joined(left, right);
        }
        match (left.units(), right.units()) {
            (Units::Ascii(head), Units::Ascii(tail)) => {
                let buffer = AppendBuffer::made_of(head.iter().copied(), tail.iter().copied());
                let length = stored_length(buffer.filled());
                Self::from_flat(FlatUnits::AsciiPrefix(buffer, length))
            }
            (head, tail) => {
                let buffer = AppendBuffer::made_of(head.iter(), tail.iter());
                let length = stored_length(buffer.filled());
                Self::from_flat(FlatUnits::WidePrefix(buffer, length))
            }
        }
    }

    /// Two flat pieces in one copy, straight into the joined storage when both are ASCII (a
    /// template literal's pieces, a key built from parts, a rope's last leaf and the text
    /// appended to it).
    pub(super) fn joined(left: &Self, right: &Self) -> Self {
        if let (Repr::Flat(FlatUnits::Short(head)), Repr::Flat(FlatUnits::Short(tail))) = (&left.repr, &right.repr)
            && let Some(joined) = head.joined(tail)
        {
            return Self { repr: Repr::Flat(FlatUnits::Short(joined)) };
        }
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
    }
}
