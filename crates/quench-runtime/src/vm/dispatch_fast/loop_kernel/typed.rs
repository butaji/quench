//! A kernel's loads and stores of typed array elements: each kind's element as an f64, and
//! an f64 stored as the kind's element, as the typed array element get and set do.
use super::*;
use crate::heap::TypedArrayKind;
use crate::staged::typed_elements::{
    element_number, store_double_element, store_number_element, NATIVE_LITTLE_ENDIAN,
};
use crate::Value;

/// Decodes `out.len()` elements of `kind` from element `first`, stepping by `step`.
///
/// # Safety
/// Every element read lies inside the typed array whose first element is at `elements`.
#[inline(always)]
pub(super) unsafe fn load(kind: TypedArrayKind, elements: *const u8, first: i64, step: i64, out: &mut [f64]) {
    // Each arm loads with its kind as a constant, so the staged element load (`element_number`)
    // folds to that kind's one conversion, as the typed array element get makes it.
    macro_rules! load {
        ($kind:expr) => {{
            let width = $kind.width();
            for (k, o) in out.iter_mut().enumerate() {
                // SAFETY: the caller's contract, for the element at `first + step * k`.
                let field = unsafe {
                    std::slice::from_raw_parts(elements.offset(((first + step * k as i64) as isize) * width as isize), width)
                };
                *o = element_number($kind, field, NATIVE_LITTLE_ENDIAN);
            }
        }};
    }
    match kind {
        TypedArrayKind::Uint8 => load!(TypedArrayKind::Uint8),
        TypedArrayKind::Uint8Clamped => load!(TypedArrayKind::Uint8Clamped),
        TypedArrayKind::Int8 => load!(TypedArrayKind::Int8),
        TypedArrayKind::Uint16 => load!(TypedArrayKind::Uint16),
        TypedArrayKind::Int16 => load!(TypedArrayKind::Int16),
        TypedArrayKind::Uint32 => load!(TypedArrayKind::Uint32),
        TypedArrayKind::Int32 => load!(TypedArrayKind::Int32),
        TypedArrayKind::Float32 => load!(TypedArrayKind::Float32),
        TypedArrayKind::Float64 => load!(TypedArrayKind::Float64),
        TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 | TypedArrayKind::Float16 => {
            unreachable!("no kernel spans a BigInt or Float16 array (`Vm::typed_array_span`)")
        }
    }
}

/// Stores `numbers` as elements of `kind` from element `first`, stepping by `step`.
///
/// # Safety
/// Every element written lies inside the typed array whose first element is at `elements`,
/// whose buffer is the view's alone.
#[inline(always)]
pub(super) unsafe fn store(
    kind: TypedArrayKind,
    elements: *mut u8,
    first: isize,
    step: isize,
    numbers: impl Iterator<Item = f64>,
) {
    // Each arm stores with its kind as a constant, so the staged element store folds to that kind's
    // one conversion: `store_number_element` for an integer or float Number, then
    // `store_double_element` for a non-integer Number into an integer kind (ToUint8 modulo the
    // width, ToUint8Clamp, ToUint16, ToUint32). The integer kinds of one width store alike.
    macro_rules! store {
        ($kind:expr) => {{
            let width = $kind.width() as isize;
            for (k, number) in numbers.enumerate() {
                let value = Value::number(number);
                // SAFETY: the caller's contract, for the element at `first + step * k`.
                unsafe {
                    let target = elements.offset((first + step * k as isize) * width);
                    if !store_number_element($kind, target, value) {
                        store_double_element($kind, target, value);
                    }
                }
            }
        }};
    }
    match kind {
        TypedArrayKind::Uint8 | TypedArrayKind::Int8 => store!(TypedArrayKind::Uint8),
        TypedArrayKind::Uint8Clamped => store!(TypedArrayKind::Uint8Clamped),
        TypedArrayKind::Uint16 | TypedArrayKind::Int16 => store!(TypedArrayKind::Uint16),
        TypedArrayKind::Uint32 | TypedArrayKind::Int32 => store!(TypedArrayKind::Uint32),
        TypedArrayKind::Float32 => store!(TypedArrayKind::Float32),
        TypedArrayKind::Float64 => store!(TypedArrayKind::Float64),
        TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 | TypedArrayKind::Float16 => {
            unreachable!("no kernel spans a BigInt or Float16 array (`Vm::typed_array_span`)")
        }
    }
}
