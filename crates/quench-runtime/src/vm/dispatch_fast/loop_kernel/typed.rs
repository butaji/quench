//! A kernel's loads and stores of typed array elements: each kind's element as an f64, and
//! an f64 stored as the kind's element, as the typed array element get and set do.
use super::*;
use crate::heap::TypedArrayKind;

/// Decodes `out.len()` elements of `kind` from element `first`, stepping by `step`.
///
/// # Safety
/// Every element read lies inside the typed array whose first element is at `elements`.
#[inline(always)]
pub(super) unsafe fn load(kind: TypedArrayKind, elements: *const u8, first: i64, step: i64, out: &mut [f64]) {
    macro_rules! load {
        ($type:ty) => {{
            let base = elements.cast::<$type>();
            for (k, o) in out.iter_mut().enumerate() {
                // SAFETY: the caller's contract.
                *o = f64::from(unsafe { base.offset((first + step * k as i64) as isize).read_unaligned() });
            }
        }};
    }
    match kind {
        TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => load!(u8),
        TypedArrayKind::Int8 => load!(i8),
        TypedArrayKind::Uint16 => load!(u16),
        TypedArrayKind::Int16 => load!(i16),
        TypedArrayKind::Uint32 => load!(u32),
        TypedArrayKind::Int32 => load!(i32),
        TypedArrayKind::Float32 => load!(f32),
        TypedArrayKind::Float64 => load!(f64),
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
pub(super) unsafe fn store<H: Host>(
    kind: TypedArrayKind,
    elements: *mut u8,
    first: isize,
    step: isize,
    numbers: impl Iterator<Item = f64>,
) {
    macro_rules! store {
        ($type:ty, $convert:expr) => {{
            let base = elements.cast::<$type>();
            for (k, number) in numbers.enumerate() {
                // SAFETY: the caller's contract.
                unsafe { base.offset(first + step * k as isize).write_unaligned($convert(number)) };
            }
        }};
    }
    let bits = crate::value::number_to_u32;
    match kind {
        TypedArrayKind::Uint8 | TypedArrayKind::Int8 => store!(u8, |n| bits(n) as u8),
        TypedArrayKind::Uint8Clamped => store!(u8, Vm::<H>::uint8_clamped_from_value),
        TypedArrayKind::Uint16 | TypedArrayKind::Int16 => store!(u16, |n| bits(n) as u16),
        TypedArrayKind::Uint32 | TypedArrayKind::Int32 => store!(u32, bits),
        TypedArrayKind::Float32 => store!(f32, |n| n as f32),
        TypedArrayKind::Float64 => store!(f64, |n| n),
        TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 | TypedArrayKind::Float16 => {
            unreachable!("no kernel spans a BigInt or Float16 array (`Vm::typed_array_span`)")
        }
    }
}
