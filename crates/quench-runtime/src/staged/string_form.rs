// The flat string forms a string record is read in: the heads of the short and shared forms, and
// the offsets native code takes from them. `vm::wtf16::native_string` embeds the runtime's own
// `ShortAscii` and `Rc` in these heads and re-exports the constants; the stencils instantiate the
// heads with words of the same size and alignment. `base_rows` checks each against the runtime.
#![allow(dead_code)]

use core::mem::{offset_of, size_of};

/// A short record's inline ASCII text: its unit count, then its units (`#[repr(C)]`).
#[repr(C)]
pub struct ShortAsciiHead<Bytes> {
    pub length: u8,
    pub bytes: Bytes,
}

/// A short form: the tag byte, then its text (`FlatUnits::Short`).
#[repr(C)]
pub struct ShortFormHead<Text> {
    pub tag: u8,
    pub text: Text,
}

/// A shared form: the tag byte, then the handle to the shared allocation (`FlatUnits::Ascii` and
/// `FlatUnits::Wide`). The handle is an `Rc`: a pointer, then a length for `str` and `[u16]`.
#[repr(C)]
pub struct SharedFormHead<Storage> {
    pub tag: u8,
    pub storage: Storage,
}

/// The shared form with the stencil's words: the handle as a pointer and a length.
pub type SharedFormWords = SharedFormHead<[usize; 2]>;
/// The short form with the stencil's words: a byte-sized unit count and its units.
pub type ShortFormWords = ShortFormHead<ShortAsciiHead<u8>>;

/// Bytes a string record occupies: the largest form, a tag word, then a pointer and a length.
pub const RECORD_SIZE: usize = size_of::<SharedFormWords>();
/// Where a `Short` record keeps its unit count, and its units.
pub const SHORT_LENGTH_OFFSET: i32 = (offset_of!(ShortFormWords, text) + offset_of!(ShortAsciiHead<u8>, length)) as i32;
pub const SHORT_UNITS_OFFSET: u64 = (offset_of!(ShortFormWords, text) + offset_of!(ShortAsciiHead<u8>, bytes)) as u64;
/// Where an `Ascii` or `Wide` record keeps the address of its shared allocation and its unit
/// count (the pointer of a `Rc<str>` or `Rc<[u16]>`, then its length).
pub const SHARED_POINTER_OFFSET: i32 = offset_of!(SharedFormWords, storage) as i32;
pub const SHARED_LENGTH_OFFSET: i32 = SHARED_POINTER_OFFSET + size_of::<usize>() as i32;
/// How far into that allocation the units start: `Rc`'s allocation is `#[repr(C)] RcInner { strong,
/// weak, value }`, so units of an alignment no larger than a word start two words in. A test probes
/// the compiler's own `Rc` against it (`vm::wtf16::native_string`).
#[inline]
pub const fn shared_units_offset() -> u64 {
    2 * size_of::<usize>() as u64
}

/// A prefix form: the tag byte, the handle to the append buffer, then the prefix's unit count
/// (`FlatUnits::AsciiPrefix`).
#[repr(C)]
pub struct PrefixFormHead<Buffer> {
    pub tag: u8,
    pub buffer: Buffer,
    pub length: u32,
}

/// The prefix form with the stencil's words: the handle as a pointer.
pub type PrefixFormWords = PrefixFormHead<usize>;

/// Where an `AsciiPrefix` record keeps the address of its buffer's `Rc` allocation (where a shared
/// form keeps its storage's), and its unit count.
pub const PREFIX_BUFFER_OFFSET: i32 = offset_of!(PrefixFormWords, buffer) as i32;
pub const PREFIX_LENGTH_OFFSET: i32 = offset_of!(PrefixFormWords, length) as i32;
const _: () = assert!(PREFIX_BUFFER_OFFSET == SHARED_POINTER_OFFSET);
const _: () = assert!(size_of::<PrefixFormWords>() <= RECORD_SIZE);

/// Where an append buffer (`#[repr(C)] AppendBuffer`, the value of the prefix's `Rc`) keeps the
/// address of its first unit: its first word.
pub const APPEND_UNITS_OFFSET: i32 = 0;
