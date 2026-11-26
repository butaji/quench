//! A typed array's element access as staged rows: the element at an in-bounds index of a view
//! proved once (`TypedView`, `typed_view`), read as a Number and written from one. The kind the
//! view's proof carries picks the element's width and conversion: a kind the proof compared with
//! the site's observed one (`ViewKind::Static`) is a constant, so an access keeps one arm; the
//! record's kind word (`ViewKind::Dynamic`) is tried against the kinds in turn. The conversions
//! are the language's: ToNumber of the loaded bits, ToInt32 modulo the width, the binary32
//! rounding and the clamped byte.

use super::region::Region;
use super::{NumericStage, Stage};
use crate::heap::TypedArrayKind;
use crate::typed_ir::{IntRange, Type};
use crate::Value;

/// A typed array's elements as an access reads them, once proved: the first element's address
/// and the element count (below 2^31, so an int32 index below zero is no index), in one width,
/// and the kind.
#[derive(Clone, Copy)]
pub struct TypedView<W> {
    pub values: W,
    pub length: W,
    pub kind: ViewKind<W>,
    /// Whether the access's index is proved below `length` (a range proof, or its loop's entry
    /// comparing its bound with the length once: `lane_region::loop_versions`), so no test of it
    /// remains.
    pub within: bool,
}

/// The kind of a proved view: the kind the proof compared the record's with, or the record's
/// `TypedArrayKind` discriminant, which an access dispatches on.
#[derive(Clone, Copy)]
pub enum ViewKind<W> {
    Static(NativeKind),
    Dynamic(W),
}

/// A kind whose elements native code reads and writes in place, with what its element is.
#[derive(Clone, Copy)]
pub struct NativeKind {
    pub kind: TypedArrayKind,
    element: Element,
}

impl NativeKind {
    /// `kind` as a native kind; `None` for Float16 and the BigInt kinds, which stay on the general path.
    pub fn new(kind: TypedArrayKind) -> Option<Self> {
        ELEMENTS.into_iter().find(|native| native.kind == kind)
    }

    /// The type of an element read of this kind: an integer element's range (an int32 type for
    /// the kinds of at most 32 bits, a signed one's or an unsigned one's), a float element's Number.
    pub fn element_type(self) -> Type {
        match self.element {
            Element::Integer(integer) => Type::int(integer.range(self.kind.width())),
            Element::Float32 | Element::Float64 => Type::NUMBER,
        }
    }

    /// Whether every element of this kind is an int32 (`typed_read_integer` reads it as one): every integer kind but Uint32.
    #[inline] pub const fn is_int32(self) -> bool { matches!(self.element, Element::Integer(integer) if !matches!(integer, Integer::Unsigned32)) }
}

/// The integer elements, by the load that makes their int32 word and the store of its low bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Integer { Unsigned8, Clamped8, Signed8, Unsigned16, Signed16, Signed32, Unsigned32 }

impl Integer {
    fn signed(self) -> bool { matches!(self, Self::Signed8 | Self::Signed16 | Self::Signed32) }

    /// The range of an element of `width` bytes.
    fn range(self, width: usize) -> IntRange {
        let bits = width as u32 * u8::BITS;
        if self.signed() {
            let half = 1_i64 << (bits - 1);
            IntRange { min: -half, max: half - 1 }
        } else {
            IntRange { min: 0, max: (1_i64 << bits) - 1 }
        }
    }
}

/// What a kind's element is: an integer, or a binary32 or binary64 float.
#[derive(Clone, Copy)]
enum Element {
    Integer(Integer),
    Float32,
    Float64,
}

/// The kinds whose elements native code reads and writes in place, in the order a kind word is
/// tried. Float16 and BigInt elements stay on the general path.
const ELEMENTS: [NativeKind; 9] = [
    NativeKind { kind: TypedArrayKind::Float64, element: Element::Float64 },
    NativeKind { kind: TypedArrayKind::Int32, element: Element::Integer(Integer::Signed32) },
    NativeKind { kind: TypedArrayKind::Uint8, element: Element::Integer(Integer::Unsigned8) },
    NativeKind { kind: TypedArrayKind::Float32, element: Element::Float32 },
    NativeKind { kind: TypedArrayKind::Uint32, element: Element::Integer(Integer::Unsigned32) },
    NativeKind { kind: TypedArrayKind::Int16, element: Element::Integer(Integer::Signed16) },
    NativeKind { kind: TypedArrayKind::Uint16, element: Element::Integer(Integer::Unsigned16) },
    NativeKind { kind: TypedArrayKind::Int8, element: Element::Integer(Integer::Signed8) },
    NativeKind { kind: TypedArrayKind::Uint8Clamped, element: Element::Integer(Integer::Clamped8) },
];

/// The position of `kind` among the native kinds: the index a typed element record keeps in its
/// immediate, which `native_kind_at` reads back (`None` for Float16 and the BigInt kinds).
pub fn native_kind_index(kind: TypedArrayKind) -> Option<u32> {
    ELEMENTS.iter().position(|native| native.kind == kind).map(|index| index as u32)
}

/// The native kind at `index` (`native_kind_index`).
#[inline(always)]
pub fn native_kind_at(index: u32) -> NativeKind { ELEMENTS[index as usize] }

/// The int32 word of the `integer` element at `address`.
#[inline(always)]
fn load_integer<S: Stage>(stage: &mut S, integer: Integer, address: S::Word) -> S::Word {
    match integer {
        Integer::Unsigned8 | Integer::Clamped8 => stage.load_u8(Region::Elements.mutable(), address, 0),
        Integer::Signed8 => stage.load_i8(Region::Elements.mutable(), address, 0),
        Integer::Unsigned16 => stage.load_u16(Region::Elements.mutable(), address, 0),
        Integer::Signed16 => stage.load_i16(Region::Elements.mutable(), address, 0),
        Integer::Signed32 => stage.load_i32(Region::Elements.mutable(), address, 0),
        Integer::Unsigned32 => stage.load_u32(Region::Elements.mutable(), address, 0),
    }
}

/// The Number (an `f64` word) of the `element` at `address`.
#[inline(always)]
fn load_number<S: NumericStage>(stage: &mut S, element: Element, address: S::Word) -> S::Word {
    match element {
        Element::Float64 => stage.load_f64(Region::Elements.mutable(), address, 0),
        Element::Float32 => {
            let single = stage.load_f32(Region::Elements.mutable(), address, 0);
            stage.f32_to_f64(single)
        }
        Element::Integer(integer) => {
            let word = load_integer(stage, integer, address);
            if integer == Integer::Unsigned32 { stage.u32_to_f64(word) } else { stage.i32_to_f64(word) }
        }
    }
}

/// Stores the low bytes of the int32 word `word` as the `integer` element at `address`.
#[inline(always)]
fn store_integer<S: Stage>(stage: &mut S, integer: Integer, address: S::Word, word: S::Word) {
    match integer {
        Integer::Unsigned8 | Integer::Clamped8 | Integer::Signed8 => stage.store_u8(Region::Elements.mutable(), address, 0, word),
        Integer::Unsigned16 | Integer::Signed16 => stage.store_u16(Region::Elements.mutable(), address, 0, word),
        Integer::Signed32 | Integer::Unsigned32 => {
            let word = stage.uextend32(word);
            stage.store_u32(Region::Elements.mutable(), address, 0, word);
        }
    }
}

/// Stores the Number `number` as the `element` at `address`: a float as it is (binary32
/// rounded), a clamped byte by its clamp, any other integer by ToInt32 modulo its width; a
/// ToInt32 the stage cannot make leaves at `miss`.
#[inline(always)]
fn store_number<S: NumericStage>(stage: &mut S, element: Element, address: S::Word, number: S::Word,
    miss: S::Miss) -> Option<()> {
    match element {
        Element::Float64 => stage.store_f64(Region::Elements.mutable(), address, 0, number),
        Element::Float32 => {
            let single = stage.f64_to_f32(number);
            stage.store_f32(Region::Elements.mutable(), address, 0, single);
        }
        Element::Integer(integer) => {
            let word = match integer {
                Integer::Clamped8 => stage.clamp_u8(number),
                _ => stage.to_int32(number, miss)?,
            };
            store_integer(stage, integer, address, word);
        }
    }
    Some(())
}

/// The address of element `index` of `values` for `kind`.
#[inline(always)]
fn element_address<S: Stage>(stage: &mut S, values: S::Word, index: S::Word, kind: TypedArrayKind) -> S::Word {
    let offset = stage.mul_imm(index, kind.width() as u64);
    stage.add(values, offset)
}

/// What `access` makes of the element the kind word `kind` names, one arm per kind of `kinds`
/// from the first; a kind no arm names leaves at `miss`.
fn by_word<S: Stage>(stage: &mut S, kind: S::Word, (first, rest): (NativeKind, &[NativeKind]),
    access: &dyn Fn(&mut S, NativeKind) -> Option<S::Word>, miss: S::Miss) -> Option<S::Word> {
    let named = stage.eq_imm(kind, first.kind as u64);
    match rest.split_first() {
        None => {
            stage.guard(named, miss)?;
            access(stage, first)
        }
        Some((next, rest)) => stage.choose_leaving(named, #[inline(always)] |stage| access(stage, first),
            #[inline(always)] |stage| by_word(stage, kind, (*next, rest), access, miss)),
    }
}

/// The Number at element `index` of `values` for `native`.
#[inline(always)]
fn read_element<S: NumericStage>(stage: &mut S, values: S::Word, index: S::Word, native: NativeKind) -> Option<S::Word> {
    let address = element_address(stage, values, index, native.kind);
    Some(load_number(stage, native.element, address))
}

/// The `Value` word of element `index` of `values` for `native`: an int32-ranged integer element
/// int-tagged from its load (no float round trip), any other element's Number boxed as
/// `Value::number` boxes it (`Stage::box_number`: 3.0 is the int32 3, NaN the canonical NaN).
#[inline(always)]
fn read_element_value<S: NumericStage>(stage: &mut S, values: S::Word, index: S::Word, native: NativeKind) -> Option<S::Word> {
    if let Element::Integer(integer) = native.element && native.is_int32() {
        let address = element_address(stage, values, index, native.kind);
        let word = load_integer(stage, integer, address);
        let payload = stage.uextend32(word);
        return Some(super::value::int_word(stage, payload));
    }
    let number = read_element(stage, values, index, native)?;
    Some(stage.box_number(super::Float64::of_row(number)))
}

/// Stores `number` at element `index` of `values` for `native`.
#[inline(always)]
fn write_element<S: NumericStage>(stage: &mut S, access: (S::Word, S::Word, S::Word), native: NativeKind,
    miss: S::Miss) -> Option<S::Word> {
    let (values, index, number) = access;
    let address = element_address(stage, values, index, native.kind);
    store_number(stage, native.element, address, number, miss).map(|()| number)
}

/// Continues where `index` is below the elements of `view`, which a proof of it (`TypedView::within`)
/// has made so before any run; else leaves at `miss`.
#[inline(always)]
fn bounded<S: Stage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word, miss: S::Miss) -> Option<()> {
    if view.within { return Some(()); }
    let in_bounds = stage.ult(index, view.length);
    stage.guard(in_bounds, miss)
}

/// The Number at element `index` of `view`: past the elements or of a kind without a native
/// element leaves at `miss`. A kind the proof fixed is one direct arm (no closure to outline).
#[inline(always)]
pub(crate) fn typed_read<S: NumericStage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    bounded(stage, view, index, miss)?;
    match view.kind {
        ViewKind::Static(native) => read_element(stage, view.values, index, native),
        ViewKind::Dynamic(word) => by_word(stage, word, (ELEMENTS[0], &ELEMENTS[1..]),
            &|stage, native| read_element(stage, view.values, index, native), miss),
    }
}

/// `typed_read` as a `Value` word (`read_element_value`), for a frame that writes the read into a
/// value register (`ElementFrame::typed_values`): every register holds `Value::number`'s encoding,
/// so an Int32Array element reads as the int32 an array's element would.
#[inline(always)]
pub(crate) fn typed_read_value<S: NumericStage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    bounded(stage, view, index, miss)?;
    match view.kind {
        ViewKind::Static(native) => read_element_value(stage, view.values, index, native),
        ViewKind::Dynamic(word) => by_word(stage, word, (ELEMENTS[0], &ELEMENTS[1..]),
            &|stage, native| read_element_value(stage, view.values, index, native), miss),
    }
}

/// The int32 word at element `index` of `view`, where the view's kind is fixed and its elements
/// are int32s (`NativeKind::is_int32`): the load itself, with no Number made of it. Past the
/// elements leaves at `miss`; any other view reads no such word (`None`, nothing emitted).
#[inline(always)]
pub fn typed_read_integer<S: Stage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let ViewKind::Static(native @ NativeKind { kind, element: Element::Integer(integer) }) = view.kind else { return None };
    if !native.is_int32() { return None; }
    bounded(stage, view, index, miss)?;
    let address = element_address(stage, view.values, index, kind);
    Some(load_integer(stage, integer, address))
}

/// Stores the Number `number` at element `index` of `view`: past the elements, of a kind
/// without a native element, or a ToInt32 the stage cannot make leaves at `miss`.
#[inline(always)]
pub(crate) fn typed_write<S: NumericStage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    number: S::Word, miss: S::Miss) -> Option<()> {
    bounded(stage, view, index, miss)?;
    let access = (view.values, index, number);
    match view.kind {
        ViewKind::Static(native) => write_element(stage, access, native, miss),
        ViewKind::Dynamic(word) => by_word(stage, word, (ELEMENTS[0], &ELEMENTS[1..]),
            &|stage, native| write_element(stage, access, native, miss), miss),
    }
    .map(|_| ())
}

/// The runtime's own element conversions over bytes (byte order, Float16, the Concrete
/// instantiation of the rows above): not part of the stencil build, which has no Concrete stage.
#[cfg(not(stencil_build))]
mod runtime {
use super::*;
use super::super::math::{f64_to_half, half_to_f64};
use super::super::Concrete;

/// The byte order of the host, which a typed array's elements are read and written in.
pub const NATIVE_LITTLE_ENDIAN: bool = cfg!(target_endian = "little");

/// The Stage row of the element of `kind`; `None` for Float16 and the BigInt kinds, which have
/// none (Float16 converts through `staged::math`'s binary16, the BigInt kinds take BigInts).
#[inline(always)]
fn element_of(kind: TypedArrayKind) -> Option<Element> {
    Some(match kind {
        TypedArrayKind::Int8 => Element::Integer(Integer::Signed8),
        TypedArrayKind::Uint8 => Element::Integer(Integer::Unsigned8),
        TypedArrayKind::Uint8Clamped => Element::Integer(Integer::Clamped8),
        TypedArrayKind::Int16 => Element::Integer(Integer::Signed16),
        TypedArrayKind::Uint16 => Element::Integer(Integer::Unsigned16),
        TypedArrayKind::Int32 => Element::Integer(Integer::Signed32),
        TypedArrayKind::Uint32 => Element::Integer(Integer::Unsigned32),
        TypedArrayKind::Float32 => Element::Float32,
        TypedArrayKind::Float64 => Element::Float64,
        TypedArrayKind::Float16 | TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => return None,
    })
}

/// The bytes of `field` in native order, in an eight-byte word whose first `field.len()` bytes
/// they fill: `field` reversed where `little_endian` is not the host's order (the byte swap a
/// DataView field needs; the Stage rows have no swap).
#[inline(always)]
pub fn native_word(field: &[u8], little_endian: bool) -> [u8; 8] {
    let mut word = [0; 8];
    let width = field.len();
    if little_endian == NATIVE_LITTLE_ENDIAN {
        word[..width].copy_from_slice(field);
    } else {
        for (target, source) in word[..width].iter_mut().zip(field.iter().rev()) {
            *target = *source;
        }
    }
    word
}

/// The Number of the element of `kind` whose bytes are `field` (`kind.width()` of them, in
/// `little_endian` order): the element's load (`load_number`) over the field in native order, or
/// binary16 for Float16. Both the typed array reads and the DataView gets (`staged::typed_view`)
/// are this body. Panics for the BigInt kinds, which are no Number.
#[inline(always)]
pub fn element_number(kind: TypedArrayKind, field: &[u8], little_endian: bool) -> f64 {
    if little_endian == NATIVE_LITTLE_ENDIAN
        && let Some(element) = element_of(kind)
    {
        // Native bytes need no word: the load reads the field's `kind.width()` bytes in place.
        debug_assert_eq!(field.len(), kind.width());
        return f64::from_bits(load_number(&mut Concrete::new(std::ptr::null()), element, field.as_ptr() as u64));
    }
    let word = native_word(field, little_endian);
    match kind {
        TypedArrayKind::Float16 => half_to_f64(u16::from_ne_bytes([word[0], word[1]])),
        _ => {
            let element = element_of(kind).expect("a BigInt element is no Number");
            f64::from_bits(load_number(&mut Concrete::new(std::ptr::null()), element, word.as_ptr() as u64))
        }
    }
}

/// The int32 of the element of `kind` whose native bytes are `field`, for a kind whose every
/// element is an int32 (the integer kinds of at most 32 bits but Uint32); `None` otherwise. The
/// integer load (`load_integer`), with no Number made of it.
#[inline(always)]
pub(crate) fn element_integer(kind: TypedArrayKind, field: &[u8]) -> Option<i32> {
    match element_of(kind)? {
        Element::Integer(Integer::Unsigned32) | Element::Float32 | Element::Float64 => None,
        Element::Integer(integer) => {
            // Native bytes need no word: the load reads the field's `kind.width()` bytes in place.
            debug_assert_eq!(field.len(), kind.width());
            Some(load_integer(&mut Concrete::new(std::ptr::null()), integer, field.as_ptr() as u64) as u32 as i32)
        }
    }
}

/// The encoding of `number` as an element of `kind` (ToInt32 modulo the width, the clamped byte,
/// the binary32 or binary16 rounding), in native order in the first `kind.width()` bytes: the
/// element's store (`store_number`) into a word. Panics for the BigInt kinds, which take BigInts.
#[inline(always)]
pub(crate) fn element_word(kind: TypedArrayKind, number: f64) -> [u8; 8] {
    let mut word = [0; 8];
    match kind {
        TypedArrayKind::Float16 => word[..2].copy_from_slice(&f64_to_half(number).to_ne_bytes()),
        _ => {
            let element = element_of(kind).expect("a BigInt element takes a BigInt");
            let stored = store_number(&mut Concrete::new(std::ptr::null()), element, word.as_mut_ptr() as u64,
                number.to_bits(), ());
            debug_assert!(stored.is_some(), "a concrete store converts every Number");
        }
    }
    word
}

/// Writes `number` as an element of `kind` into `field` (`kind.width()` bytes) in `little_endian`
/// order: `element_word`, byte-swapped where the order differs from the host's.
#[inline(always)]
pub(crate) fn store_element_number(kind: TypedArrayKind, number: f64, field: &mut [u8], little_endian: bool) {
    let word = element_word(kind, number);
    let width = field.len();
    if little_endian == NATIVE_LITTLE_ENDIAN {
        field.copy_from_slice(&word[..width]);
    } else {
        for (target, source) in field.iter_mut().zip(word[..width].iter().rev()) {
            *target = *source;
        }
    }
}

/// The unsigned integer `field` holds in `little_endian` order (at most eight bytes): a DataView
/// element's bits, the byte-swapped load the Stage rows do not have.
#[inline(always)]
pub(crate) fn field_bits(field: &[u8], little_endian: bool) -> u64 {
    let width = field.len();
    let mut word = [0; 8];
    if little_endian {
        word[..width].copy_from_slice(field);
        u64::from_le_bytes(word)
    } else {
        word[8 - width..].copy_from_slice(field);
        u64::from_be_bytes(word)
    }
}

/// Stores the low `field.len()` bytes of `bits` into `field` in `little_endian` order: the
/// inverse of `field_bits`.
#[inline(always)]
pub(crate) fn store_field_bits(field: &mut [u8], bits: u64, little_endian: bool) {
    let width = field.len();
    if little_endian {
        field.copy_from_slice(&bits.to_le_bytes()[..width]);
    } else {
        field.copy_from_slice(&bits.to_be_bytes()[8 - width..]);
    }
}

/// The relative position of `index` among `length` elements, shared by `Array.prototype.at` and
/// `%TypedArray%.prototype.at`: a negative index counts from the end, and the position lies in
/// `[0, length)` or the answer is `None` (`undefined`). `index` is already a ToIntegerOrInfinity
/// result (an integer, or an infinity that lies outside any length).
#[inline(always)]
pub(crate) fn element_at(index: f64, length: usize) -> Option<usize> {
    let position = if index < 0.0 { length as f64 + index } else { index };
    (position >= 0.0 && position < length as f64).then(|| position as usize)
}

/// The index of the first element of `elements` (the bytes of a run of `kind` elements, in native
/// order) at or after `from` whose Number matches `search`: strict equality, or under
/// `nan_matches` (SameValueZero) a NaN matching a NaN; zeros of either sign are equal.
///
/// The search value is converted to the kind once, outside the scan, and a value the kind cannot
/// hold exactly matches nothing. The scan is one loop per kind over fixed-width fields read with
/// `from_ne_bytes`, with no per-element conversion and no copy of the bytes.
#[inline(always)]
pub(crate) fn find_number_element(elements: &[u8], kind: TypedArrayKind, from: usize, search: f64, nan_matches: bool)
    -> Option<usize> {
    let rest = elements.get(from.checked_mul(kind.width())?..)?;
    let found = match kind {
        TypedArrayKind::Float64 => find_float::<8>(rest, search, nan_matches, f64::from_ne_bytes),
        TypedArrayKind::Float32 => {
            // A Number binary32 cannot hold exactly matches no element.
            if !search.is_nan() && f64::from(search as f32) != search {
                return None;
            }
            find_float::<4>(rest, search, nan_matches, |field| f64::from(f32::from_ne_bytes(field)))
        }
        TypedArrayKind::Float16 => find_float::<2>(rest, search, nan_matches, |field| half_to_f64(u16::from_ne_bytes(field))),
        TypedArrayKind::Int8 => find_integer(rest, search, i8::from_ne_bytes),
        TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => find_integer(rest, search, u8::from_ne_bytes),
        TypedArrayKind::Int16 => find_integer(rest, search, i16::from_ne_bytes),
        TypedArrayKind::Uint16 => find_integer(rest, search, u16::from_ne_bytes),
        TypedArrayKind::Int32 => find_integer(rest, search, i32::from_ne_bytes),
        TypedArrayKind::Uint32 => find_integer(rest, search, u32::from_ne_bytes),
        TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => return None,
    };
    found.map(|index| index + from)
}

/// The index, within `rest` (whole fields of `W` bytes), of the first field whose `matches` holds.
#[inline(always)]
fn scan_fields<const W: usize>(rest: &[u8], matches: impl Fn([u8; W]) -> bool) -> Option<usize> {
    // `chunks_exact(W)` yields slices of exactly `W` bytes, so the conversion cannot fail.
    rest.chunks_exact(W).position(|field| matches(field.try_into().unwrap()))
}

/// `find_number_element` for a float kind of `W`-byte fields, decoded to a Number by `decode`. A NaN
/// search matches only a NaN element, and only under SameValueZero; zeros of either sign compare equal.
#[inline(always)]
fn find_float<const W: usize>(rest: &[u8], search: f64, nan_matches: bool, decode: fn([u8; W]) -> f64)
    -> Option<usize> {
    if search.is_nan() {
        return if nan_matches { scan_fields::<W>(rest, |field| decode(field).is_nan()) } else { None };
    }
    scan_fields::<W>(rest, |field| decode(field) == search)
}

/// `find_number_element` for an integer kind of `W`-byte fields, read by `decode`. The search is
/// converted once: a NaN, an infinity, a fraction or a value outside the kind matches nothing.
#[inline(always)]
fn find_integer<T: TryFrom<i64> + PartialEq, const W: usize>(rest: &[u8], search: f64, decode: fn([u8; W]) -> T)
    -> Option<usize> {
    if search.fract() != 0.0 {
        return None;
    }
    let target = T::try_from(search as i64).ok()?;
    scan_fields::<W>(rest, |field| decode(field) == target)
}

/// Writes `element` at `target`, which need not be aligned.
///
/// # Safety
/// `target` is valid for a write of one `T`.
#[inline(always)]
unsafe fn write_raw<T>(target: *mut u8, element: T) {
    // SAFETY: the caller's contract.
    unsafe { target.cast::<T>().write_unaligned(element) }
}

/// Stores an integer `value` as an element of `kind` at `target` (its low bits, clamped for
/// Uint8Clamped), or any Number into a float kind: the hot element store of the cached and
/// quick typed stores. False, storing nothing, for a double into an integer kind
/// (`store_double_element` takes it, out of the hot handler) or any other value.
///
/// Integers and float kinds only: a double arm here, even out of line, kept the matched kind a
/// runtime value and pushed a frame in the hot handler, +2 to +9 Ir per Uint8 store (k16_sieve).
///
/// # Safety
/// `target` is valid for a write of one element of `kind`.
#[inline(always)]
pub unsafe fn store_number_element(kind: TypedArrayKind, target: *mut u8, value: Value) -> bool {
    macro_rules! store {
        ($element:expr) => {
            // SAFETY: the caller's contract.
            unsafe { write_raw(target, $element) }
        };
    }
    match (kind, value.as_int()) {
        (TypedArrayKind::Uint8 | TypedArrayKind::Int8, Some(integer)) => store!(integer as u8),
        (TypedArrayKind::Uint8Clamped, Some(integer)) => store!(integer.clamp(0, i32::from(u8::MAX)) as u8),
        (TypedArrayKind::Uint16 | TypedArrayKind::Int16, Some(integer)) => store!(integer as u16),
        (TypedArrayKind::Uint32 | TypedArrayKind::Int32, Some(integer)) => store!(integer as u32),
        (TypedArrayKind::Float32, _) => match value.as_number() {
            Some(number) => store!(number as f32),
            None => return false,
        },
        (TypedArrayKind::Float64, _) => match value.as_number() {
            Some(number) => store!(number),
            None => return false,
        },
        _ => return false,
    }
    true
}

/// `store_number_element`'s other half: a non-integer Number into an integer kind, ToUint32's low
/// bits or ToUint8Clamp of the double; false for a value that is no Number or a kind with no
/// conversion here. The hot store tries `store_number_element` first, then this.
///
/// # Safety
/// As `store_number_element`.
#[inline(always)]
pub unsafe fn store_double_element(kind: TypedArrayKind, target: *mut u8, value: Value) -> bool {
    use crate::value::{number_to_u32, number_to_uint8_clamp};
    let Some(number) = value.as_number() else { return false };
    // SAFETY: the caller's contract, for each arm.
    unsafe {
        match kind {
            TypedArrayKind::Uint8 | TypedArrayKind::Int8 => write_raw(target, number_to_u32(number) as u8),
            TypedArrayKind::Uint8Clamped => write_raw(target, number_to_uint8_clamp(number)),
            TypedArrayKind::Uint16 | TypedArrayKind::Int16 => write_raw(target, number_to_u32(number) as u16),
            TypedArrayKind::Uint32 | TypedArrayKind::Int32 => write_raw(target, number_to_u32(number)),
            _ => return false,
        }
    }
    true
}

}
#[cfg(not(stencil_build))]
pub use runtime::*;
