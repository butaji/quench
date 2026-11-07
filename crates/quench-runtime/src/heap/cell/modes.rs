//! The explicit state tags of heap cells: one value per fact that flags once spread.

/// The internal slot that makes an object exotic or branded. One per object, set once at
/// creation. `Arguments` and `ModuleNamespace` also set the header's `EXOTIC_ACCESS` bit,
/// which fast paths read instead of this (`Object::has_exotic_access`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ObjectBrand {
    #[default]
    Ordinary,
    Arguments,
    ModuleNamespace,
    /// [[IsRawJSON]].
    RawJson,
    /// [[ErrorData]], the unforgeable Error brand.
    Error,
}

impl ObjectBrand {
    pub(crate) fn from_u8(byte: u8) -> Option<Self> {
        [Self::Ordinary, Self::Arguments, Self::ModuleNamespace, Self::RawJson, Self::Error]
            .get(usize::from(byte))
            .copied()
    }
}

/// Monotonic facts about an object's properties beside its shape: each bit is set once and
/// never cleared, so `holds_ordinary_elements` tests them with one mask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ElementFacts(pub(crate) u8);

impl ElementFacts {
    /// An index-keyed property descriptor has been recorded.
    pub(crate) const INDEXED_DESCRIPTORS: u8 = 1 << 0;
    /// A `length` descriptor has been recorded, as a non-writable array length is.
    pub(crate) const LENGTH_DESCRIPTOR: u8 = 1 << 1;
    /// Any property descriptor beside the shape has been recorded (`Vm::insert_descriptor`).
    pub(crate) const SIDE_DESCRIPTORS: u8 = 1 << 2;
    /// The array has held sparse elements (`Heap::sparse_set`), so its length may exceed its
    /// dense elements'. An array without extras has its dense length.
    pub(crate) const SPARSE_ELEMENTS: u8 = 1 << 3;
    /// The facts that keep elements from being ordinary and dense.
    pub(crate) const NOT_ORDINARY: u8 = Self::INDEXED_DESCRIPTORS | Self::LENGTH_DESCRIPTOR | Self::SPARSE_ELEMENTS;
    /// Every defined bit.
    pub(crate) const ALL: u8 = Self::NOT_ORDINARY | Self::SIDE_DESCRIPTORS;

    #[inline(always)]
    pub(crate) fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }
}

/// A buffer view's length: fixed (elements of a typed array, bytes of a DataView), or
/// tracking its resizable buffer past the view's offset, which derives it from the buffer at
/// each access. One word: the tracking case is a value no fixed length reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewLength(usize);

impl ViewLength {
    pub(crate) const TRACKING: Self = Self(usize::MAX);

    pub(crate) fn fixed(length: usize) -> Self {
        debug_assert_ne!(length, usize::MAX);
        Self(length)
    }

    /// `length` fixed, or tracking when `tracking` (which ignores `length`).
    pub(crate) fn new(length: usize, tracking: bool) -> Self {
        if tracking { Self::TRACKING } else { Self::fixed(length) }
    }

    /// The fixed length, or `None` while it tracks the buffer.
    #[inline(always)]
    pub(crate) fn get(self) -> Option<usize> {
        (self != Self::TRACKING).then_some(self.0)
    }

    #[inline(always)]
    pub(crate) fn is_tracking(self) -> bool {
        self == Self::TRACKING
    }

    /// The view's length, in units of `width` bytes, over an attached buffer of `byte_length`
    /// bytes from `offset`; `None` when the view is out of the buffer's bounds. The one
    /// derivation of a view's current length.
    #[inline(always)]
    pub(crate) fn within(self, offset: usize, width: usize, byte_length: usize) -> Option<usize> {
        match self.get() {
            None => (offset <= byte_length).then(|| (byte_length - offset) / width),
            Some(length) => (offset.saturating_add(length.saturating_mul(width)) <= byte_length).then_some(length),
        }
    }
}

/// An iterator helper's run state (`Vm::iterator_helper_next`, `iterator_helper_return`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum HelperPhase {
    /// No `next` has run yet: `return` closes it without stepping the source.
    #[default]
    Fresh,
    /// A `next` or `return` is running: a reentrant call throws.
    Running,
    /// Started and not running.
    Idle,
}
