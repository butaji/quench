//! The typed-array kinds as data: the enum in discriminant order, its count, and which kinds have
//! a native element. This file is std-free and names nothing else of the runtime, so the b-jit
//! stencils include it by path too (`quench-bjit/stencils/codes.rs` takes `TYPED_KINDS` and
//! `NON_NATIVE_KINDS` from it). `typed_array_kind.rs` re-exports the enum and adds the rest.

/// Declares the kinds in discriminant order, each with whether it has a native element. The enum,
/// its `ALL` and `COUNT`, and `has_native_element` all derive from this one list.
macro_rules! typed_array_kinds {
    (@count) => { 0 };
    (@count $head:ident $($tail:ident)*) => { 1 + typed_array_kinds!(@count $($tail)*) };
    ($($name:ident => $native:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum TypedArrayKind {
            $($name),+
        }

        impl TypedArrayKind {
            /// Every kind, in discriminant order.
            pub(crate) const ALL: [Self; typed_array_kinds!(@count $($name)+)] = [$(Self::$name),+];
            /// The number of kinds: one past the last discriminant.
            pub(crate) const COUNT: usize = typed_array_kinds!(@count $($name)+);

            /// Whether the kind's elements native code reads and writes in place: not the BigInt
            /// kinds, which are 64-bit words, nor Float16, which has no native conversion.
            pub(crate) const fn has_native_element(self) -> bool {
                match self {
                    $(Self::$name => $native),+
                }
            }
        }
    };
}

typed_array_kinds! {
    Uint8 => true,
    Uint8Clamped => true,
    Uint16 => true,
    Uint32 => true,
    Int8 => true,
    Int16 => true,
    Int32 => true,
    BigInt64 => false,
    BigUint64 => false,
    Float16 => false,
    Float32 => true,
    Float64 => true,
}

impl TypedArrayKind {
    /// The element width in bytes.
    #[inline(always)]
    pub(crate) const fn width(self) -> usize {
        match self {
            Self::Uint8 | Self::Uint8Clamped | Self::Int8 => size_of::<u8>(),
            Self::Uint16 | Self::Int16 | Self::Float16 => size_of::<u16>(),
            Self::Uint32 | Self::Int32 | Self::Float32 => size_of::<u32>(),
            Self::BigInt64 | Self::BigUint64 | Self::Float64 => size_of::<u64>(),
        }
    }
}
