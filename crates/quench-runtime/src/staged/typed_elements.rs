//! A typed array's element access as staged rows: the element at an in-bounds index of a view
//! proved once (`TypedView`), read as a Number and written from one. The kind word the view's
//! proof carries picks the element's width and conversion; a stage that holds it as a constant
//! (a specializer proving the kind) keeps one arm, any other tries the kinds in turn. The
//! conversions are the language's: ToNumber of the loaded bits, ToInt32 modulo the width, the
//! binary32 rounding and the clamped byte.

use super::{NumericStage, Stage};
use crate::heap::TypedArrayKind;

/// A typed array's elements as an access reads them, once proved: the first element's address,
/// the element count (below 2^31, so an int32 index below zero is no index) and the
/// `TypedArrayKind` discriminant, in one width.
#[derive(Clone, Copy)]
pub(crate) struct TypedView<W> {
    pub(crate) values: W,
    pub(crate) length: W,
    pub(crate) kind: W,
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
const ELEMENTS: [(TypedArrayKind, Element); 9] = [
    (TypedArrayKind::Float64, Element::Float64),
    (TypedArrayKind::Int32, Element::Integer(Integer::Signed32)),
    (TypedArrayKind::Uint8, Element::Integer(Integer::Unsigned8)),
    (TypedArrayKind::Float32, Element::Float32),
    (TypedArrayKind::Uint32, Element::Integer(Integer::Unsigned32)),
    (TypedArrayKind::Int16, Element::Integer(Integer::Signed16)),
    (TypedArrayKind::Uint16, Element::Integer(Integer::Unsigned16)),
    (TypedArrayKind::Int8, Element::Integer(Integer::Signed8)),
    (TypedArrayKind::Uint8Clamped, Element::Integer(Integer::Clamped8)),
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

/// What `access` makes of the element the kind word `kind` names, one arm per kind of `ELEMENTS`
/// from `first`; a kind no arm names leaves at `miss`.
fn by_kind<S: Stage>(stage: &mut S, kind: S::Word, (first, rest): ((TypedArrayKind, Element), &[(TypedArrayKind, Element)]),
    access: &dyn Fn(&mut S, TypedArrayKind, Element) -> Option<S::Word>, miss: S::Miss) -> Option<S::Word> {
    let (discriminant, element) = first;
    let named = stage.eq_imm(kind, discriminant as u64);
    match rest.split_first() {
        None => {
            stage.guard(named, miss)?;
            access(stage, discriminant, element)
        }
        Some((next, rest)) => stage.choose_leaving(named, |stage| access(stage, discriminant, element),
            |stage| by_kind(stage, kind, (*next, rest), access, miss)),
    }
}

/// The Number at element `index` of `view`: past the elements or of a kind without a native
/// element leaves at `miss`.
#[inline(always)]
pub(crate) fn typed_read<S: NumericStage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let in_bounds = stage.ult(index, view.length);
    stage.guard(in_bounds, miss)?;
    let access = |stage: &mut S, kind, element| {
        let address = element_address(stage, view.values, index, kind);
        Some(load_number(stage, element, address))
    };
    by_kind(stage, view.kind, (ELEMENTS[0], &ELEMENTS[1..]), &access, miss)
}

/// Stores the Number `number` at element `index` of `view`: past the elements, of a kind
/// without a native element, or a ToInt32 the stage cannot make leaves at `miss`.
#[inline(always)]
pub(crate) fn typed_write<S: NumericStage>(stage: &mut S, view: TypedView<S::Word>, index: S::Word,
    number: S::Word, miss: S::Miss) -> Option<()> {
    let in_bounds = stage.ult(index, view.length);
    stage.guard(in_bounds, miss)?;
    let access = |stage: &mut S, kind, element| {
        let address = element_address(stage, view.values, index, kind);
        store_number(stage, element, address, number, miss).map(|()| number)
    };
    by_kind(stage, view.kind, (ELEMENTS[0], &ELEMENTS[1..]), &access, miss).map(|_| ())
}
