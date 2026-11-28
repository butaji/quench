//! The element writes of an operation, per alias class, and the maximum of them a row records.

use super::*;

/// The most element classes a row names before its element writes are `ElementWrites::Any`: the classes a
/// composed signature keeps apart, the same bound as a closed union of receiver classes (`typed_ir::MAX_CLASSES`).
pub const MAX_WRITE_CLASSES: usize = 4;

/// The indices of one array class an element write touches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassWrite {
    pub class: ElementClass,
    pub indices: Indices,
}

impl ClassWrite {
    const NONE: Self = Self { class: ElementClass::Unnamed, indices: Indices::Whole };
}

/// Which array elements a row writes, by the class of the array (`ElementClass`): nothing (bottom), up to
/// `MAX_WRITE_CLASSES` classes each with its indices, or any element of any array (top). The classes are sorted and
/// distinct, and `Classes` holds at least one, so equal writes are equal values.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ElementWrites {
    #[default]
    Nothing,
    Classes { len: u8, writes: [ClassWrite; MAX_WRITE_CLASSES] },
    Any,
}

impl ElementWrites {
    /// The writes of `entries`: a class listed twice joins its indices, and more classes than `MAX_WRITE_CLASSES` is `Any`.
    pub fn of(entries: impl IntoIterator<Item = ClassWrite>) -> Self {
        let mut writes = [ClassWrite::NONE; MAX_WRITE_CLASSES];
        let mut len = 0;
        for entry in entries {
            match writes[..len].iter().position(|write| write.class == entry.class) {
                Some(at) => writes[at].indices = writes[at].indices.join(entry.indices),
                None if len == MAX_WRITE_CLASSES => return Self::Any,
                None => {
                    writes[len] = entry;
                    len += 1;
                }
            }
        }
        if len == 0 {
            return Self::Nothing;
        }
        writes[..len].sort_by_key(|write| write.class);
        Self::Classes { len: len as u8, writes }
    }

    /// The listed classes, none for `Nothing` or `Any`.
    pub fn entries(&self) -> &[ClassWrite] {
        match self {
            Self::Classes { len, writes } => &writes[..usize::from(*len)],
            Self::Nothing | Self::Any => &[],
        }
    }

    /// The writes either row makes.
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Any, _) | (_, Self::Any) => Self::Any,
            (Self::Nothing, rest) | (rest, Self::Nothing) => rest,
            (Self::Classes { .. }, Self::Classes { .. }) => Self::of(self.entries().iter().chain(other.entries()).copied()),
        }
    }

    /// The writes both rows allow: a class only one lists is not written, and a class both list keeps the indices both
    /// name (`Indices::meet`). A kind row's `Any` gives way to the facts' writes.
    pub fn meet(self, other: Self) -> Self {
        match (self, other) {
            (Self::Nothing, _) | (_, Self::Nothing) => Self::Nothing,
            (Self::Any, rest) | (rest, Self::Any) => rest,
            (Self::Classes { .. }, Self::Classes { .. }) => Self::of(self.entries().iter().filter_map(|write| {
                other.entries().iter().find(|kept| kept.class == write.class)
                    .map(|kept| ClassWrite { class: write.class, indices: write.indices.meet(kept.indices) })
            })),
        }
    }

    /// The writes with each class and index `lift` maps to the caller's: a callee's writes seen from a call site.
    pub fn map(self, lift: impl Fn(ClassWrite) -> ClassWrite) -> Self {
        match self {
            Self::Classes { .. } => Self::of(self.entries().iter().map(|&write| lift(write))),
            other => other,
        }
    }
}
