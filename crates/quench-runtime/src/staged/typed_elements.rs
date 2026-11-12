//! A typed array's element access as staged rows: the element at an in-bounds index of a view
//! proved once (`TypedView`, `typed_view`), read as a Number and written from one. The kind the
//! view's proof carries picks the element's width and conversion: a kind the proof compared with
//! the site's observed one (`ViewKind::Static`) is a constant, so an access keeps one arm; the
//! record's kind word (`ViewKind::Dynamic`) is tried against the kinds in turn. The conversions
//! are the language's: ToNumber of the loaded bits, ToInt32 modulo the width, the binary32
//! rounding and the clamped byte.

use super::{NumericStage, Stage};
use crate::heap::TypedArrayKind;

/// A typed array's elements as an access reads them, once proved: the first element's address
/// and the element count (below 2^31, so an int32 index below zero is no index), in one width,
/// and the kind.
#[derive(Clone, Copy)]
pub(crate) struct TypedView<W> {
    pub(crate) values: W,
    pub(crate) length: W,
    pub(crate) kind: ViewKind<W>,
}

/// The kind of a proved view: the kind the proof compared the record's with, or the record's
/// `TypedArrayKind` discriminant, which an access dispatches on.
#[derive(Clone, Copy)]
pub(crate) enum ViewKind<W> {
    Static(NativeKind),
    Dynamic(W),
}

/// A kind whose elements native code reads and writes in place, with what its element is.
#[derive(Clone, Copy)]
pub(crate) struct NativeKind {
    pub(crate) kind: TypedArrayKind,
    element: Element,
}

impl NativeKind {
    /// `kind` as a native kind; `None` for Float16 and the BigInt kinds, which stay on the general path.
    pub(crate) fn new(kind: TypedArrayKind) -> Option<Self> {
        ELEMENTS.into_iter().find(|native| native.kind == kind)
    }
}

/// The integer elements, by the load that makes their int32 word and the store of its low bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Integer { Unsigned8, Clamped8, Signed8, Unsigned16, Signed16, Signed32, Unsigned32 }

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

/// The int32 word of the `integer` element at `address`.
#[inline(always)]
fn load_integer<S: Stage>(stage: &mut S, integer: Integer, address: S::Word) -> S::Word {
    match integer {
        Integer::Unsigned8 | Integer::Clamped8 => stage.load_u8(address, 0),
        Integer::Signed8 => stage.load_i8(address, 0),
        Integer::Unsigned16 => stage.load_u16(address, 0),
        Integer::Signed16 => stage.load_i16(address, 0),
        Integer::Signed32 => stage.load_i32(address, 0),
        Integer::Unsigned32 => stage.load_u32(address, 0),
    }
}

/// The Number (an `f64` word) of the `element` at `address`.
#[inline(always)]
fn load_number<S: NumericStage>(stage: &mut S, element: Element, address: S::Word) -> S::Word {
    match element {
        Element::Float64 => stage.load_f64(address, 0),
        Element::Float32 => {
            let single = stage.load_f32(address, 0);
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
        Integer::Unsigned8 | Integer::Clamped8 | Integer::Signed8 => stage.store_u8(address, 0, word),
        Integer::Unsigned16 | Integer::Signed16 => stage.store_u16(address, 0, word),
        Integer::Signed32 | Integer::Unsigned32 => {
            let word = stage.uextend32(word);
            stage.store_u32(address, 0, word);
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
        Element::Float64 => stage.store_f64(address, 0, number),
        Element::Float32 => {
            let single = stage.f64_to_f32(number);
            stage.store_f32(address, 0, single);
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
        Some((next, rest)) => stage.choose_leaving(named, |stage| access(stage, first),
            |stage| by_word(stage, kind, (*next, rest), access, miss)),
    }
}

/// What `access` makes of the element `kind` names: the one arm of a kind the proof fixed, else
/// the arm of the kind word that matches.
fn by_kind<S: Stage>(stage: &mut S, kind: ViewKind<S::Word>,
    access: &dyn Fn(&mut S, NativeKind) -> Option<S::Word>, miss: S::Miss) -> Option<S::Word> {
    match kind {
        ViewKind::Static(native) => access(stage, native),
        ViewKind::Dynamic(word) => by_word(stage, word, (ELEMENTS[0], &ELEMENTS[1..]), access, miss),
    }
}

/// The Number at element `index` of `view`: past the elements or of a kind without a native
/// element leaves at `miss`.
#[inline(always)]
pub(crate) fn typed_read<S: NumericStage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let in_bounds = stage.ult(index, view.length);
    stage.guard(in_bounds, miss)?;
    let access = |stage: &mut S, native: NativeKind| {
        let address = element_address(stage, view.values, index, native.kind);
        Some(load_number(stage, native.element, address))
    };
    by_kind(stage, view.kind, &access, miss)
}

/// Stores the Number `number` at element `index` of `view`: past the elements, of a kind
/// without a native element, or a ToInt32 the stage cannot make leaves at `miss`.
#[inline(always)]
pub(crate) fn typed_write<S: NumericStage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    number: S::Word, miss: S::Miss) -> Option<()> {
    let in_bounds = stage.ult(index, view.length);
    stage.guard(in_bounds, miss)?;
    let access = |stage: &mut S, native: NativeKind| {
        let address = element_address(stage, view.values, index, native.kind);
        store_number(stage, native.element, address, number, miss).map(|()| number)
    };
    by_kind(stage, view.kind, &access, miss).map(|_| ())
}
