//! A DataView's fixed-width element access as staged rows: the bytes at an in-bounds offset of a
//! view proved once (`DataViewWords`, `prove_data_view`), read as a Number and written from one.
//! A view's record is published with the kind `NO_KIND` (a view that is no typed array), its data
//! the first byte of the view and its length the view's byte length, so the proof is the typed
//! array's record loads with the kind the DataView publishes. The endianness is a condition the
//! caller computes: a big-endian integer is its bytes swapped, a big-endian float leaves at the
//! miss (no bit cast is on the stage), and an offset the view cannot hold leaves too, so the VM
//! raises what the call must. The conversions are the language's: ToNumber of the loaded bits,
//! ToInt32 modulo the width for a store, the binary32 rounding of a float store.

use super::region::Region;
use super::typed_view::{kind_view, TypedRecords};
use super::{NumericStage, Stage};
use crate::heap::NO_KIND;

/// The fixed-width element kinds a DataView reads and writes in place (Float16 and BigInt stay on the general path).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataViewElement {
    Int8,
    Uint8,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Float64,
}

impl DataViewElement {
    /// The element's width in bytes.
    pub(crate) const fn width(self) -> usize {
        match self {
            Self::Int8 | Self::Uint8 => 1,
            Self::Int16 | Self::Uint16 => 2,
            Self::Int32 | Self::Uint32 | Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }

    /// Whether the element is a binary32 or binary64 float.
    pub(crate) const fn is_float(self) -> bool {
        matches!(self, Self::Float32 | Self::Float64)
    }
}

/// A DataView's bytes as an access reads them, once proved: the address of its first byte and its byte length (below 2^31).
#[derive(Clone, Copy)]
pub struct DataViewWords<W> {
    pub values: W,
    pub length: W,
}

/// The view of the DataView `base`, proved for an access (`writable` to store): `None` (leaving at `miss`) for any other
/// value, a record that is no DataView (its kind is not `NO_KIND`), a stale or detached record, or (to store) bytes another
/// buffer shares.
#[inline(always)]
pub fn prove_data_view<S: Stage>(stage: &mut S, records: TypedRecords<S::Word>, base: S::Word, writable: bool,
    miss: S::Miss) -> Option<DataViewWords<S::Word>> {
    let (values, length) = kind_view(stage, records, base, NO_KIND, writable, miss)?;
    Some(DataViewWords { values, length })
}

/// The address of the element at `index` (an int32 word) of `view`, leaving at `miss` where the index is negative or the
/// element of `width` bytes does not lie inside the view.
#[inline(always)]
fn element_address<S: Stage>(stage: &mut S, view: DataViewWords<S::Word>, index: S::Word, width: usize, miss: S::Miss) -> Option<S::Word> {
    // A negative int32 is a huge unsigned word, so one test keeps the index non-negative and below 2^31.
    let negative = stage.ugt_imm(index, 0x7fff_ffffu64);
    let in_range = stage.not(negative);
    stage.guard(in_range, miss)?;
    // `index + width <= length`, as `index + width < length + 1` with no wrap.
    let end = stage.add_imm(index, width as u64);
    let room = stage.add_imm(view.length, 1u64);
    let fits = stage.ult(end, room);
    stage.guard(fits, miss)?;
    Some(stage.add(view.values, index))
}

/// The unsigned word of the `width`-byte integer at `address`, in its stored byte order.
#[inline(always)]
fn load_unsigned<S: Stage>(stage: &mut S, address: S::Word, width: usize) -> S::Word {
    match width {
        1 => stage.load_u8(Region::Elements.mutable(), address, 0),
        2 => stage.load_u16(Region::Elements.mutable(), address, 0),
        _ => stage.load_u32(Region::Elements.mutable(), address, 0),
    }
}

/// `word`'s low `width` bytes reversed (a 2- or 4-byte word; a byte is its own order), by shifts and masks.
#[inline(always)]
fn swap_bytes<S: Stage>(stage: &mut S, word: S::Word, width: usize) -> S::Word {
    match width {
        2 => {
            let low = stage.and_imm(word, 0xffu64);
            let up = stage.mul_imm(low, 1u64 << 8);
            let high = stage.shr_imm(word, 8u32);
            stage.or(up, high)
        }
        4 => {
            let byte0 = stage.and_imm(word, 0xffu64);
            let byte1 = stage.and_imm(word, 0xff00u64);
            let byte2 = stage.and_imm(word, 0xff_0000u64);
            let byte3 = stage.shr_imm(word, 24u32);
            let a = stage.mul_imm(byte0, 1u64 << 24);
            let b = stage.mul_imm(byte1, 1u64 << 8);
            let c = stage.shr_imm(byte2, 8u32);
            let ab = stage.or(a, b);
            let cd = stage.or(c, byte3);
            stage.or(ab, cd)
        }
        _ => word,
    }
}

/// The int32 word of `unsigned`, a `width`-byte value zero-extended, sign-extended from its top bit where the element is signed.
#[inline(always)]
fn sign_extend<S: Stage>(stage: &mut S, unsigned: S::Word, width: usize) -> S::Word {
    let shift = 32 - 8 * width as u32;
    let up = stage.mul_imm(unsigned, 1u64 << shift);
    stage.sar32_imm(up, shift)
}

/// The Number at byte offset `index` of `view` for `element`, the bytes read in the order `little` names. A big-endian
/// float leaves at `miss`; an offset the element does not fit inside leaves too.
#[inline(always)]
pub fn data_view_read<S: NumericStage>(stage: &mut S, view: DataViewWords<S::Word>, index: S::Word,
    element: DataViewElement, little: S::Cond, miss: S::Miss) -> Option<S::Word> {
    if element.is_float() {
        stage.guard(little, miss)?;
    }
    let address = element_address(stage, view, index, element.width(), miss)?;
    match element {
        // Float bit entries: the bytes may hold any bits (decision 4.1).
        DataViewElement::Float64 => {
            let number = stage.load_f64(Region::Elements.mutable(), address, 0);
            Some(stage.canonical_float(number))
        }
        DataViewElement::Float32 => {
            let single = stage.load_f32(Region::Elements.mutable(), address, 0);
            let wide = stage.f32_to_f64(single);
            Some(stage.canonical_float(wide))
        }
        _ => {
            let width = element.width();
            let raw = load_unsigned(stage, address, width);
            let word = if width == 1 {
                raw
            } else {
                let swapped = swap_bytes(stage, raw, width);
                stage.select(little, raw, swapped)
            };
            let int = match element {
                DataViewElement::Int8 | DataViewElement::Int16 => sign_extend(stage, word, width),
                _ => word,
            };
            Some(match element {
                DataViewElement::Uint32 => stage.u32_to_f64(int),
                _ => stage.i32_to_f64(int),
            })
        }
    }
}

/// Stores the Number `number` at byte offset `index` of `view` as `element`, the bytes written in the order `little` names.
/// A big-endian float, an offset the element does not fit inside, or a ToInt32 the stage cannot make leaves at `miss`, before
/// any byte is written.
#[inline(always)]
pub fn data_view_write<S: NumericStage>(stage: &mut S, view: DataViewWords<S::Word>, index: S::Word,
    element: DataViewElement, little: S::Cond, number: S::Word, miss: S::Miss) -> Option<()> {
    if element.is_float() {
        stage.guard(little, miss)?;
    }
    let width = element.width();
    let address = element_address(stage, view, index, width, miss)?;
    match element {
        DataViewElement::Float64 => stage.store_f64(Region::Elements.mutable(), address, 0, number),
        DataViewElement::Float32 => {
            let single = stage.f64_to_f32(number);
            stage.store_f32(Region::Elements.mutable(), address, 0, single);
        }
        _ => {
            let word = stage.to_int32(number, miss)?;
            let low = match width {
                1 => stage.and_imm(word, 0xffu64),
                2 => stage.and_imm(word, 0xffffu64),
                _ => stage.and_imm(word, 0xffff_ffffu64),
            };
            let stored = if width == 1 {
                low
            } else {
                let swapped = swap_bytes(stage, low, width);
                stage.select(little, low, swapped)
            };
            match width {
                1 => stage.store_u8(Region::Elements.mutable(), address, 0, stored),
                2 => stage.store_u16(Region::Elements.mutable(), address, 0, stored),
                _ => {
                    let stored = stage.uextend32(stored);
                    stage.store_u32(Region::Elements.mutable(), address, 0, stored);
                }
            }
        }
    }
    Some(())
}
