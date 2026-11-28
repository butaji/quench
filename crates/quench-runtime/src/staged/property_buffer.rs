// The head of a property buffer: the words native code reads first, the base pointer and the
// initialized length. `value_vec::buffer::PropertyBuffer` embeds it as its first field, so the
// offsets native code uses are `offset_of!` of this struct. Generic over the base type so the
// runtime keeps `NonNull<Value>` and the stencil can instantiate a same-size word.
#![allow(dead_code)]

/// The base and the initialized length, in declaration order (`#[repr(C)]`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct PropertyBufferHead<Base> {
    /// The first slot of the buffer's allocation.
    pub(crate) base: Base,
    /// The number of initialized slots from `base`.
    pub(crate) length: usize,
}
