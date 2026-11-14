//! A typed array's Number element read and write at an index: the hot path of every index loop
//! over one. A site that proved the receiver's kind passes it (`Some`), and the kind is then a
//! constant of the instantiation: the width, the conversion and the stored bytes are decided
//! when the site's handler is built, not per access. The general entry passes `None`.
//!
//! A lane run proves a view's bytes once (`Heap::prove_typed_storage`), and an access through
//! the proof is a bounds test and one load or store (`proved_element`, `proved_store`); the
//! checked entries below serve every buffer a proof does not cover.

use super::typed_array_access::TypedArrayElement;
use super::*;
use crate::heap::{ProvedElements, TYPED_ARRAYS};

impl<H: Host> Vm<H> {
    /// `typed_array_get` of a non-BigInt typed array, which allocates nothing: the element,
    /// or `undefined` past the end or of a detached buffer. One read of the view's cell, and
    /// an integer element's value made as an integer, so an index loop over a `Uint8Array`
    /// (ws's frame masking) converts through no float. `None` for any other object.
    #[inline]
    pub(super) fn typed_array_number_element(&self, object: Value, index: usize) -> Option<Value> {
        self.number_element(object, index, None)
    }

    /// `typed_array_number_element` at a site of the kind `KIND` (`TYPED_ARRAYS` row): `None`
    /// for a receiver of another kind, which leaves the read to the general entry.
    #[inline(always)]
    pub(super) fn typed_element_at<const KIND: usize>(&self, object: Value, index: u32) -> Option<Value> {
        self.number_element(object, index as usize, Some(TYPED_ARRAYS[KIND].kind))
    }

    #[inline(always)]
    fn number_element(&self, object: Value, index: usize, site: Option<TypedArrayKind>) -> Option<Value> {
        let view = self.typed_array_view(object)?;
        if site.is_some_and(|kind| kind != view.kind) {
            return None;
        }
        let kind = site.unwrap_or(view.kind);
        if kind.is_bigint() {
            return None;
        }
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get(view.buffer) else {
            return Some(Value::UNDEFINED);
        };
        // An unlocked store reads in place; the atomically shared one holds its lock for the read.
        let locked;
        let bytes = match bytes.unlocked() {
            Some(bytes) => bytes,
            None => {
                locked = bytes.read();
                &locked
            }
        };
        let Some(start) = view.element(index, bytes.len()) else {
            return Some(Value::UNDEFINED);
        };
        // SAFETY: `element` found the whole element within the bytes.
        Some(unsafe { Self::element_value(kind, bytes[start..start + kind.width()].as_ptr()) })
    }

    /// The Number element of `kind` at `at`, an integer where it is one.
    ///
    /// # Safety
    /// `at` names `kind.width()` readable bytes.
    #[inline(always)]
    unsafe fn element_value(kind: TypedArrayKind, at: *const u8) -> Value {
        unsafe {
            match kind {
                TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => Value::integer(i32::from(at.read())),
                TypedArrayKind::Int8 => Value::integer(i32::from(at.cast::<i8>().read())),
                TypedArrayKind::Uint16 => Value::integer(i32::from(at.cast::<u16>().read_unaligned())),
                TypedArrayKind::Int16 => Value::integer(i32::from(at.cast::<i16>().read_unaligned())),
                TypedArrayKind::Int32 => Value::integer(at.cast::<i32>().read_unaligned()),
                TypedArrayKind::Uint32 => {
                    let element = at.cast::<u32>().read_unaligned();
                    i32::try_from(element).map_or_else(|_| Value::overflowed_integer(f64::from(element)), Value::integer)
                }
                TypedArrayKind::Float32 => Value::number(f64::from(at.cast::<f32>().read_unaligned())),
                TypedArrayKind::Float64 => Value::number(at.cast::<f64>().read_unaligned()),
                _ => {
                    let mut element = [0; size_of::<u64>()];
                    std::ptr::copy_nonoverlapping(at, element.as_mut_ptr(), kind.width());
                    Value::number(Self::typed_array_number_of(kind, &element))
                }
            }
        }
    }

    /// `typed_element_at` through a proof of the view's bytes (`ProvedElements`).
    #[inline(always)]
    pub(super) fn proved_element<const KIND: usize>(elements: ProvedElements, index: u32) -> Value {
        let kind = TYPED_ARRAYS[KIND].kind;
        if index as usize >= elements.bound {
            return Value::UNDEFINED;
        }
        // SAFETY: the proof bounds `elements.bound` elements of `kind` from `data`.
        unsafe { Self::element_value(kind, elements.data.as_ptr().add(index as usize * kind.width())) }
    }

    /// `typed_array_set` of a Number into a typed array of Number content: its ToNumber has no
    /// effect, so the write needs neither a root nor a conversion. False leaves the store to
    /// `typed_array_set`.
    #[inline]
    pub(super) fn typed_array_set_number(&mut self, object: Value, index: usize, value: Value) -> bool {
        self.set_number(object, index, value, None)
    }

    /// `typed_array_set_number` at a site of the kind `KIND` (`TYPED_ARRAYS` row): false for a
    /// receiver of another kind, which leaves the store to the general entry.
    #[inline(always)]
    pub(super) fn typed_element_set<const KIND: usize>(&mut self, object: Value, index: u32, value: Value) -> bool {
        self.set_number(object, index as usize, value, Some(TYPED_ARRAYS[KIND].kind))
    }

    #[inline(always)]
    fn set_number(&mut self, object: Value, index: usize, value: Value, site: Option<TypedArrayKind>) -> bool {
        let Some(number) = value.as_number() else {
            return false;
        };
        let Some(view) = self.typed_array_view(object) else {
            return false;
        };
        if site.is_some_and(|kind| kind != view.kind) || view.kind.is_bigint() {
            return false;
        }
        self.typed_array_write_view(view, index, &TypedArrayElement::Number(number));
        true
    }

    /// `typed_element_set` through a proof of the view's bytes: a store past them writes
    /// nothing. False for a proof of bytes the view does not own alone, which stores through
    /// the general entry, and for a value that is no Number.
    #[inline(always)]
    pub(super) fn proved_store<const KIND: usize>(elements: ProvedElements, index: u32, value: Value) -> bool {
        let kind = TYPED_ARRAYS[KIND].kind;
        if !elements.writable {
            return false;
        }
        if index as usize >= elements.bound {
            return value.as_number().is_some();
        }
        // SAFETY: as `proved_element`; a writable proof names bytes the view owns alone.
        unsafe { Self::write_element(kind, elements.data.as_ptr().add(index as usize * kind.width()), value) }
    }

    /// Writes the Number `value` as `kind`'s bytes at `at`: an integer's ToInt8 through
    /// ToUint32 is its low bytes, and Uint8Clamped clamps it. False for a value that is no
    /// Number, which writes nothing.
    ///
    /// # Safety
    /// `at` names `kind.width()` writable bytes.
    #[inline(always)]
    unsafe fn write_element(kind: TypedArrayKind, at: *mut u8, value: Value) -> bool {
        unsafe {
            match (kind, value.as_int()) {
                (TypedArrayKind::Uint8Clamped, Some(integer)) => at.write(integer.clamp(0, i32::from(u8::MAX)) as u8),
                (TypedArrayKind::Uint8 | TypedArrayKind::Int8, Some(integer)) => at.write(integer as u8),
                (TypedArrayKind::Uint16 | TypedArrayKind::Int16, Some(integer)) => at.cast::<u16>().write_unaligned(integer as u16),
                (TypedArrayKind::Uint32 | TypedArrayKind::Int32, Some(integer)) => at.cast::<u32>().write_unaligned(integer as u32),
                (kind, _) => {
                    let Some(number) = value.as_number() else { return false };
                    match kind {
                        TypedArrayKind::Float64 => at.cast::<f64>().write_unaligned(number),
                        TypedArrayKind::Float32 => at.cast::<f32>().write_unaligned(number as f32),
                        _ => {
                            let encoded = Self::typed_array_element_bytes(kind, &TypedArrayElement::Number(number));
                            std::ptr::copy_nonoverlapping(encoded.as_ptr(), at, kind.width());
                        }
                    }
                }
            }
        }
        true
    }
}
