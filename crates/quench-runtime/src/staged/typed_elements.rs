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

    /// Whether this kind stores an int32 by its low bytes (ToInt32 modulo its width, which is the int32's
    /// own bits): every integer kind but the clamped byte (`typed_write_int`).
    #[inline] pub const fn wraps_int32(self) -> bool { matches!(self.element, Element::Integer(integer) if !matches!(integer, Integer::Clamped8)) }

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
/// How many kinds are native (`ELEMENTS`).
pub const NATIVE_KINDS: usize = 9;

const ELEMENTS: [NativeKind; NATIVE_KINDS] = [
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
pub const fn native_kind_at(index: u32) -> NativeKind { ELEMENTS[index as usize] }

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

/// The Number (an `f64` word) of the `element` at `address`. A float element's bytes may hold any
/// bits, so each float load is a float bit entry (`Stage::canonical_float`, decision 4.1).
#[inline(always)]
fn load_number<S: NumericStage>(stage: &mut S, element: Element, address: S::Word) -> S::Word {
    match element {
        Element::Float64 => {
            let number = stage.load_f64(Region::Elements.mutable(), address, 0);
            stage.canonical_float(number)
        }
        Element::Float32 => {
            let single = stage.load_f32(Region::Elements.mutable(), address, 0);
            let wide = stage.f32_to_f64(single);
            stage.canonical_float(wide)
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

/// What `access` makes of the element the kind word `kind` names, one arm per kind of `ELEMENTS`
/// in order; a kind no arm names leaves at `miss`. Unrolled: a stencil holds no kind table, only its holes.
#[inline(always)]
fn by_word<S: Stage>(stage: &mut S, kind: S::Word, access: &impl Fn(&mut S, NativeKind) -> Option<S::Word>,
    miss: S::Miss) -> Option<S::Word> {
    macro_rules! arms {
        ($stage:ident; $last:literal) => {{ let named = $stage.eq_imm(kind, ELEMENTS[$last].kind as u64); $stage.guard(named, miss)?; access($stage, ELEMENTS[$last]) }};
        ($stage:ident; $first:literal $(, $rest:literal)+) => {{ let named = $stage.eq_imm(kind, ELEMENTS[$first].kind as u64);
            $stage.choose_leaving(named, #[inline(always)] |stage| access(stage, ELEMENTS[$first]), #[inline(always)] |stage| arms!(stage; $($rest),+)) }};
    }
    const { assert!(ELEMENTS.len() == 9, "one arm per native kind") };
    arms!(stage; 0, 1, 2, 3, 4, 5, 6, 7, 8)
}

/// The Number at element `index` of `values` for `native`.
#[inline(always)]
fn read_element<S: NumericStage>(stage: &mut S, values: S::Word, index: S::Word, native: NativeKind) -> Option<S::Word> {
    let address = element_address(stage, values, index, native.kind);
    Some(load_number(stage, native.element, address))
}

/// The `Value` word of element `index` of `values` for `native`: an int32-ranged integer element
/// int-tagged from its load (no float round trip), an integer element's other Number boxed as
/// `Value::number` boxes it (`Stage::box_number`), and a binary float element's Number stored as its
/// raw double word (`Stage::box_raw_number`, decision 4.1).
#[inline(always)]
fn read_element_value<S: NumericStage>(stage: &mut S, values: S::Word, index: S::Word, native: NativeKind) -> Option<S::Word> {
    if let Element::Integer(integer) = native.element && native.is_int32() {
        let address = element_address(stage, values, index, native.kind);
        let word = load_integer(stage, integer, address);
        let payload = stage.uextend32(word);
        return Some(super::value::int_word(stage, payload));
    }
    let number = super::Float64::of_row(read_element(stage, values, index, native)?);
    Some(match native.element {
        Element::Integer(_) => stage.box_number(number),
        Element::Float32 | Element::Float64 => stage.box_raw_number(number),
    })
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
pub fn typed_read<S: NumericStage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    bounded(stage, view, index, miss)?;
    match view.kind {
        ViewKind::Static(native) => read_element(stage, view.values, index, native),
        ViewKind::Dynamic(word) => by_word(stage, word,
            &#[inline(always)] |stage, native| read_element(stage, view.values, index, native), miss),
    }
}

/// `typed_read` as a `Value` word (`read_element_value`), for a frame that writes the read into a
/// value register (`ElementFrame::typed_values`): every register holds `Value::number`'s encoding,
/// so an Int32Array element reads as the int32 an array's element would.
#[inline(always)]
pub fn typed_read_value<S: NumericStage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    bounded(stage, view, index, miss)?;
    match view.kind {
        ViewKind::Static(native) => read_element_value(stage, view.values, index, native),
        ViewKind::Dynamic(word) => by_word(stage, word,
            &#[inline(always)] |stage, native| read_element_value(stage, view.values, index, native), miss),
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
pub fn typed_write<S: NumericStage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    number: S::Word, miss: S::Miss) -> Option<()> {
    bounded(stage, view, index, miss)?;
    let access = (view.values, index, number);
    match view.kind {
        ViewKind::Static(native) => write_element(stage, access, native, miss),
        ViewKind::Dynamic(word) => by_word(stage, word,
            &#[inline(always)] |stage, native| write_element(stage, access, native, miss), miss),
    }
    .map(|_| ())
}

/// Stores the int32 `int` at element `index` of `view`, whose kind the proof fixed to one that
/// wraps an int32 (`NativeKind::wraps_int32`): its low bytes are the element, with no Number made
/// of it. Past the elements leaves at `miss`; another view is `typed_write`'s (nothing emitted).
#[inline(always)]
pub fn typed_write_int<S: Stage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    int: S::Word, miss: S::Miss) -> Option<()> {
    let ViewKind::Static(NativeKind { kind, element: Element::Integer(integer) }) = view.kind else { return None };
    bounded(stage, view, index, miss)?;
    let address = element_address(stage, view.values, index, kind);
    store_integer(stage, integer, address, int);
    Some(())
}

#[cfg(not(stencil_build))]
#[path = "typed_elements/runtime.rs"]
mod runtime;
#[cfg(not(stencil_build))]
pub use runtime::*;
