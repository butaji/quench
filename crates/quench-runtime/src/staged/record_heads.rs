// The heads of the native records that generated code reads by field offset. ONE source: the runtime
// embeds each head as the first field of its record (`head`, with `Deref`/`DerefMut` forwarding so
// existing field accesses keep compiling), and the stencil crate mounts this file (`stencils/layout.rs`)
// and takes the `offset_of!` values as the `NATIVE_*_LAYOUT` constants of `staged/layout_offsets.rs`.
// Only word-sized fields, `#[repr(C)]` (packed where the record is), so the offsets are the same in both
// builds. Nothing here names a runtime item: the file must compile in the stencil crate.
#![allow(dead_code)]

/// The words of an ordinary object's record (`heap::cell::Object`): its payload word and its layout id.
#[repr(C, packed(4))]
pub(crate) struct ObjectHead {
    /// One word: with `ObjectExtras`, `EXTRAS_TAG` and a pointer to them; without extras, the inline values as
    /// narrow values, or the storage position (`ValueVec`'s start), or an array's element backing.
    pub(crate) payload: u64,
    /// The object's layout (`heap::layouts`): the interned pair of its header words.
    pub(crate) layout: u32,
}

/// A dense element backing's header (`heap::array_elements::Header`), the words before its values.
#[repr(C)]
pub(crate) struct ArrayHeaderHead {
    /// Owners of this backing; zero where no owner counts it.
    pub(crate) owners: u32,
    /// The number of values the backing holds.
    pub(crate) len: u32,
    /// Room for values after the header, with the allocator's flags in the bits it sets.
    pub(crate) capacity: u32,
    /// How many values below `len` are not int32s, holes included (`OTHERS_UNKNOWN` when uncounted).
    pub(crate) others: u32,
}

/// What native code reads of a typed array's view record (`heap::typed_view::NativeView`): the first
/// element's address, the publication stamp, the element count, the kind, and the three flags.
#[repr(C)]
#[derive(Debug)]
pub(crate) struct NativeViewHead {
    /// The first element's address.
    pub(crate) data: usize,
    /// The buffer's relocation count when `data` and `length` were published.
    pub(crate) stamp: u64,
    /// The element count, below 2^31.
    pub(crate) length: u32,
    /// The `TypedArrayKind` discriminant, or the no-kind marker.
    pub(crate) kind: u8,
    /// Set while the view cannot be addressed in place.
    pub(crate) detached: u8,
    /// Set when the buffer's bytes belong to this view's buffer alone.
    pub(crate) unshared: u8,
    /// The table entry that holds this record's address, null while it has none.
    pub(crate) home: *mut usize,
}
