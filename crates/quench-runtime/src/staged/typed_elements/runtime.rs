//! The runtime's own element conversions over bytes (byte order, Float16, the Concrete
//! instantiation of the rows above): not part of the stencil build, which has no Concrete stage.

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
        return f64::from_bits(load_number(&mut Concrete::pure(), element, field.as_ptr() as u64));
    }
    let word = native_word(field, little_endian);
    match kind {
        TypedArrayKind::Float16 => half_to_f64(u16::from_ne_bytes([word[0], word[1]])),
        _ => {
            let element = element_of(kind).expect("a BigInt element is no Number");
            f64::from_bits(load_number(&mut Concrete::pure(), element, word.as_ptr() as u64))
        }
    }
}

/// The int32 of the element of `kind` whose native bytes are `field`, for a kind whose every
/// element is an int32 (the integer kinds of at most 32 bits but Uint32); `None` otherwise. The
/// integer load (`load_integer`), with no Number made of it.
#[inline(always)]
pub fn element_integer(kind: TypedArrayKind, field: &[u8]) -> Option<i32> {
    match element_of(kind)? {
        Element::Integer(Integer::Unsigned32) | Element::Float32 | Element::Float64 => None,
        Element::Integer(integer) => {
            // Native bytes need no word: the load reads the field's `kind.width()` bytes in place.
            debug_assert_eq!(field.len(), kind.width());
            Some(load_integer(&mut Concrete::pure(), integer, field.as_ptr() as u64) as u32 as i32)
        }
    }
}

/// The encoding of `number` as an element of `kind` (ToInt32 modulo the width, the clamped byte,
/// the binary32 or binary16 rounding), in native order in the first `kind.width()` bytes: the
/// element's store (`store_number`) into a word. Panics for the BigInt kinds, which take BigInts.
#[inline(always)]
pub fn element_word(kind: TypedArrayKind, number: f64) -> [u8; 8] {
    let mut word = [0; 8];
    match kind {
        TypedArrayKind::Float16 => word[..2].copy_from_slice(&f64_to_half(number).to_ne_bytes()),
        _ => {
            let element = element_of(kind).expect("a BigInt element takes a BigInt");
            let stored = store_number(&mut Concrete::pure(), element, word.as_mut_ptr() as u64,
                number.to_bits(), ());
            debug_assert!(stored.is_some(), "a concrete store converts every Number");
        }
    }
    word
}

/// Writes `number` as an element of `kind` into `field` (`kind.width()` bytes) in `little_endian`
/// order: `element_word`, byte-swapped where the order differs from the host's.
#[inline(always)]
pub fn store_element_number(kind: TypedArrayKind, number: f64, field: &mut [u8], little_endian: bool) {
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
pub fn field_bits(field: &[u8], little_endian: bool) -> u64 {
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
pub fn store_field_bits(field: &mut [u8], bits: u64, little_endian: bool) {
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
pub fn element_at(index: f64, length: usize) -> Option<usize> {
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
pub fn find_number_element(elements: &[u8], kind: TypedArrayKind, from: usize, search: f64, nan_matches: bool)
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
