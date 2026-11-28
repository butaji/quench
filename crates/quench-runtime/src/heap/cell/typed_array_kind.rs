//! The typed-array kinds and what each one is.

use super::Native;

pub use crate::staged::typed_array_catalog::TypedArrayKind;

/// The typed-array kinds, one row each: `(Kind, NativeVariant, "Name", width)`, in
/// [`TypedArrayKind`] order. `$m` is a callback called as `$m! { rows }`; the snapshot kind
/// encoding is generated from this list.
macro_rules! for_each_typed_array {
    ($m:ident) => {
        $m! {
            (Uint8, Uint8Array, "Uint8Array", 1),
            (Uint8Clamped, Uint8ClampedArray, "Uint8ClampedArray", 1),
            (Uint16, Uint16Array, "Uint16Array", 2),
            (Uint32, Uint32Array, "Uint32Array", 4),
            (Int8, Int8Array, "Int8Array", 1),
            (Int16, Int16Array, "Int16Array", 2),
            (Int32, Int32Array, "Int32Array", 4),
            (BigInt64, BigInt64Array, "BigInt64Array", 8),
            (BigUint64, BigUint64Array, "BigUint64Array", 8),
            (Float16, Float16Array, "Float16Array", 2),
            (Float32, Float32Array, "Float32Array", 4),
            (Float64, Float64Array, "Float64Array", 8),
        }
    };
}
pub(crate) use for_each_typed_array;

#[quench_macros::test_visible]
/// What one typed-array kind is: its constructor intrinsic, that constructor's name and the
/// element width in bytes.
pub struct TypedArrayRow {
    pub(crate) kind: TypedArrayKind,
    pub(crate) native: Native,
    pub(crate) name: &'static str,
    pub(crate) width: usize,
}

/// The typed-array kinds, one row each, in [`TypedArrayKind`] order.
pub const TYPED_ARRAYS: [TypedArrayRow; TypedArrayKind::COUNT] = [
    TypedArrayRow { kind: TypedArrayKind::Uint8, native: Native::Uint8Array, name: "Uint8Array", width: TypedArrayKind::Uint8.width() },
    TypedArrayRow { kind: TypedArrayKind::Uint8Clamped, native: Native::Uint8ClampedArray, name: "Uint8ClampedArray", width: TypedArrayKind::Uint8Clamped.width() },
    TypedArrayRow { kind: TypedArrayKind::Uint16, native: Native::Uint16Array, name: "Uint16Array", width: TypedArrayKind::Uint16.width() },
    TypedArrayRow { kind: TypedArrayKind::Uint32, native: Native::Uint32Array, name: "Uint32Array", width: TypedArrayKind::Uint32.width() },
    TypedArrayRow { kind: TypedArrayKind::Int8, native: Native::Int8Array, name: "Int8Array", width: TypedArrayKind::Int8.width() },
    TypedArrayRow { kind: TypedArrayKind::Int16, native: Native::Int16Array, name: "Int16Array", width: TypedArrayKind::Int16.width() },
    TypedArrayRow { kind: TypedArrayKind::Int32, native: Native::Int32Array, name: "Int32Array", width: TypedArrayKind::Int32.width() },
    TypedArrayRow { kind: TypedArrayKind::BigInt64, native: Native::BigInt64Array, name: "BigInt64Array", width: TypedArrayKind::BigInt64.width() },
    TypedArrayRow { kind: TypedArrayKind::BigUint64, native: Native::BigUint64Array, name: "BigUint64Array", width: TypedArrayKind::BigUint64.width() },
    TypedArrayRow { kind: TypedArrayKind::Float16, native: Native::Float16Array, name: "Float16Array", width: TypedArrayKind::Float16.width() },
    TypedArrayRow { kind: TypedArrayKind::Float32, native: Native::Float32Array, name: "Float32Array", width: TypedArrayKind::Float32.width() },
    TypedArrayRow { kind: TypedArrayKind::Float64, native: Native::Float64Array, name: "Float64Array", width: TypedArrayKind::Float64.width() },
];

const _: () = {
    let mut index = 0;
    while index < TypedArrayKind::COUNT {
        assert!(TYPED_ARRAYS[index].kind as usize == index, "typed-array rows follow kind order");
        index += 1;
    }
};

// `TypedArrayKind` is defined in the shared catalog (`typed_array_catalog.rs`), so its methods
// cannot stay an inherent impl here (spec 4.5). A `const fn` cannot be a trait method, so the
// const methods are free `const fn`s; the one non-const method is an extension trait.

#[inline(always)]
pub const fn typed_array_kind_is_bigint(kind: TypedArrayKind) -> bool {
    matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64)
}

/// Whether the elements are binary floats: their Numbers are stored as raw double words
/// (`Value::from_f64_raw`, decision 4.1), integral values included.
#[inline(always)]
pub const fn typed_array_kind_is_float(kind: TypedArrayKind) -> bool {
    matches!(kind, TypedArrayKind::Float16 | TypedArrayKind::Float32 | TypedArrayKind::Float64)
}

/// For a kind of 8-, 16- or 32-bit integers, whether they are signed: a Number such a
/// kind stores is reduced modulo its width (except Uint8Clamped's, which clamps), so an
/// integer of another such kind converts to its low bytes. `None` for the others.
pub const fn typed_array_kind_integer_signed(kind: TypedArrayKind) -> Option<bool> {
    match kind {
        TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped | TypedArrayKind::Uint16 | TypedArrayKind::Uint32 => Some(false),
        TypedArrayKind::Int8 | TypedArrayKind::Int16 | TypedArrayKind::Int32 => Some(true),
        TypedArrayKind::BigInt64
        | TypedArrayKind::BigUint64
        | TypedArrayKind::Float16
        | TypedArrayKind::Float32
        | TypedArrayKind::Float64 => None,
    }
}

/// Whether `Atomics.wait` and `Atomics.notify` accept the kind.
pub const fn typed_array_kind_is_waitable(kind: TypedArrayKind) -> bool {
    matches!(kind, TypedArrayKind::Int32 | TypedArrayKind::BigInt64)
}

/// The constructor's name, which is also the kind's class name.
pub const fn typed_array_kind_name(kind: TypedArrayKind) -> &'static str {
    TYPED_ARRAYS[kind as usize].name
}

/// The constructor intrinsic.
pub const fn typed_array_kind_native(kind: TypedArrayKind) -> Native {
    TYPED_ARRAYS[kind as usize].native
}

/// The non-const methods of `TypedArrayKind`.
pub trait TypedArrayKindTypedArrayKindExt: Sized {
    /// The kind whose constructor intrinsic is `native`.
    fn of_constructor(native: Native) -> Option<Self>;
}

impl TypedArrayKindTypedArrayKindExt for TypedArrayKind {
    fn of_constructor(native: Native) -> Option<Self> {
        TYPED_ARRAYS.iter().find(|row| row.native == native).map(|row| row.kind)
    }
}
