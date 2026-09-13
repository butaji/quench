//! Native layer: unboxed scalars and a 16-byte `v128` slot.

// Integer kernels share one dispatch shape across widths. The operation
// catalogs stay local to each width, while this macro derives the enum and
// table-backed execution once so the two lanes cannot drift structurally.
macro_rules! define_integer_kernels {
    (
        binary $binary:ident,
        unary $unary:ident,
        type $ty:ty;
        binary_ops { $( $binary_name:ident => $binary_fn:ident ),+ $(,)? }
        unary_ops { $( $unary_name:ident => $unary_fn:ident ),+ $(,)? }
    ) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(u8)]
        pub enum $binary {
            $( $binary_name ),+
        }

        impl $binary {
            pub fn is_rel(self) -> bool {
                matches!(self, Self::Eq | Self::Ne | Self::LtS | Self::LtU
                    | Self::LeS | Self::LeU | Self::GtS | Self::GtU
                    | Self::GeS | Self::GeU)
            }

            pub fn apply(self, lhs: $ty, rhs: $ty) -> Result<$ty, crate::unwind::Trap> {
                const TABLE: &[fn($ty, $ty) -> Result<$ty, crate::unwind::Trap>] =
                    &[$($binary_fn),+];
                TABLE[self as usize](lhs, rhs)
            }
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(u8)]
        pub enum $unary {
            $( $unary_name ),+
        }

        impl $unary {
            pub fn apply(self, src: $ty) -> $ty {
                const TABLE: &[fn($ty) -> $ty] = &[$($unary_fn),+];
                TABLE[self as usize](src)
            }
        }
    };
}

macro_rules! define_native_accessors {
    ($( $name:ident => $variant:ident : $ty:ty ),+ $(,)?) => {
        $(
            pub fn $name(self) -> Option<$ty> {
                match self {
                    Self::$variant(value) => Some(value),
                    _ => None,
                }
            }
        )+
    };
}

mod conv;
mod float;
mod i32_ops;
mod i64_ops;
mod simd;
pub(crate) mod simd_extra;
pub(crate) mod simd_more;

pub use conv::{Bits, ConvOp};
pub use float::{BinF32, BinF64, UnF32, UnF64, CANON_F32, CANON_F64};
pub use i32_ops::{BinI32, UnI32};
pub use i64_ops::{BinI64, UnI64};
pub use simd::SimdOp;

/// Function/table/GC reference. `Func` names an instance id plus a local index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefVal {
    Null,
    Func {
        inst: u32,
        index: u32,
    },
    Extern(u32),
    Host(u32),
    Struct(u32),
    Array(u32),
    I31(u32),
    Exn(u32),
    /// externref wrapping a Native any (i31/struct/array).
    ExternBox(u32),
}

/// Native payload. Not a Dynamic heap object and not a 8-byte tagged word.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Native {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
    V128(u128),
    Ref(RefVal),
}

impl Native {
    define_native_accessors! {
        as_i32 => I32: i32,
        as_i64 => I64: i64,
        as_f32_bits => F32: u32,
        as_f64_bits => F64: u64,
        as_v128 => V128: u128,
        as_ref => Ref: RefVal,
    }

    pub fn zero_i32() -> Self {
        Self::I32(0)
    }
}

#[cfg(test)]
mod tests {
    use super::{Native, RefVal};

    #[test]
    fn v128_is_sixteen_bytes() {
        assert_eq!(std::mem::size_of::<u128>(), 16);
        let slot = Native::V128(0);
        assert!(matches!(slot, Native::V128(0)));
    }

    #[test]
    fn generated_accessors_preserve_native_tags() {
        assert_eq!(Native::I32(7).as_i32(), Some(7));
        assert_eq!(Native::I32(7).as_i64(), None);
        assert_eq!(Native::F64(11).as_f64_bits(), Some(11));
        assert_eq!(Native::Ref(RefVal::Null).as_ref(), Some(RefVal::Null));
    }
}
