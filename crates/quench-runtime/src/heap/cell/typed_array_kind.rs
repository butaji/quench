//! The typed-array kinds and what each one is.

use super::Native;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypedArrayKind {
    Uint8,
    Uint8Clamped,
    Uint16,
    Uint32,
    Int8,
    Int16,
    Int32,
    BigInt64,
    BigUint64,
    Float16,
    Float32,
    Float64,
}


/// What one typed-array kind is: its constructor intrinsic, that constructor's name and the
/// element width in bytes.
pub(crate) struct TypedArrayRow {
    pub(crate) kind: TypedArrayKind,
    pub(crate) native: Native,
    pub(crate) name: &'static str,
    pub(crate) width: usize,
}

/// The typed-array kinds, one row each, in [`TypedArrayKind`] order.
pub(crate) const TYPED_ARRAYS: [TypedArrayRow; TypedArrayKind::COUNT] = [
    TypedArrayRow { kind: TypedArrayKind::Uint8, native: Native::Uint8Array, name: "Uint8Array", width: 1 },
    TypedArrayRow { kind: TypedArrayKind::Uint8Clamped, native: Native::Uint8ClampedArray, name: "Uint8ClampedArray", width: 1 },
    TypedArrayRow { kind: TypedArrayKind::Uint16, native: Native::Uint16Array, name: "Uint16Array", width: 2 },
    TypedArrayRow { kind: TypedArrayKind::Uint32, native: Native::Uint32Array, name: "Uint32Array", width: 4 },
    TypedArrayRow { kind: TypedArrayKind::Int8, native: Native::Int8Array, name: "Int8Array", width: 1 },
    TypedArrayRow { kind: TypedArrayKind::Int16, native: Native::Int16Array, name: "Int16Array", width: 2 },
    TypedArrayRow { kind: TypedArrayKind::Int32, native: Native::Int32Array, name: "Int32Array", width: 4 },
    TypedArrayRow { kind: TypedArrayKind::BigInt64, native: Native::BigInt64Array, name: "BigInt64Array", width: 8 },
    TypedArrayRow { kind: TypedArrayKind::BigUint64, native: Native::BigUint64Array, name: "BigUint64Array", width: 8 },
    TypedArrayRow { kind: TypedArrayKind::Float16, native: Native::Float16Array, name: "Float16Array", width: 2 },
    TypedArrayRow { kind: TypedArrayKind::Float32, native: Native::Float32Array, name: "Float32Array", width: 4 },
    TypedArrayRow { kind: TypedArrayKind::Float64, native: Native::Float64Array, name: "Float64Array", width: 8 },
];

const _: () = {
    let mut index = 0;
    while index < TypedArrayKind::COUNT {
        assert!(TYPED_ARRAYS[index].kind as usize == index, "typed-array rows follow kind order");
        index += 1;
    }
};

impl TypedArrayKind {
    pub(crate) const COUNT: usize = Self::Float64 as usize + 1;

    pub(crate) const fn is_bigint(self) -> bool {
        matches!(self, Self::BigInt64 | Self::BigUint64)
    }

    pub(crate) const fn width(self) -> usize {
        TYPED_ARRAYS[self as usize].width
    }

    /// The constructor's name, which is also the kind's class name.
    pub(crate) const fn name(self) -> &'static str {
        TYPED_ARRAYS[self as usize].name
    }

    /// The constructor intrinsic.
    pub(crate) const fn native(self) -> Native {
        TYPED_ARRAYS[self as usize].native
    }

    /// The kind whose constructor intrinsic is `native`.
    pub(crate) fn of_constructor(native: Native) -> Option<Self> {
        TYPED_ARRAYS.iter().find(|row| row.native == native).map(|row| row.kind)
    }
}
