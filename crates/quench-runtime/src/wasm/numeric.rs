//! Shared typed numeric results and macro-derived selector views.

use super::WasmValue;

pub(super) enum NumericResult<T> {
    Value(T),
    Comparison(bool),
}

impl<T> NumericResult<T> {
    pub(super) fn into_wasm(self, integer: impl FnOnce(T) -> WasmValue) -> WasmValue {
        match self {
            Self::Value(value) => integer(value),
            Self::Comparison(value) => WasmValue::I32(i32::from(value)),
        }
    }
}

macro_rules! selectors {
    ($enum:ident; $($name:ident, $wasm:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub(crate) enum $enum { $($name,)+ }
        impl $enum {
            pub(super) const ALL: &'static [Self] = &[$(Self::$name,)+];
            pub(crate) fn from_tag(tag: u32) -> Option<Self> { Self::ALL.get(tag as usize).copied() }
            pub(crate) fn from_wasm(operator: &wasmparser::Operator<'_>) -> Option<Self> {
                match operator { $(wasmparser::Operator::$wasm => Some(Self::$name),)+ _ => None }
            }
        }
    };
}

pub(super) use selectors;

const IEEE_IMPLICIT_SIGNIFICAND_BITS: u32 = 1;

pub(super) const fn sign_mask(bit_width: u32) -> u64 {
    1 << (bit_width - 1)
}

pub(super) const fn canonical_nan_bits(precision: u32, infinity: u64) -> u64 {
    let fraction_width = precision - IEEE_IMPLICIT_SIGNIFICAND_BITS;
    let quiet_bit = 1 << (fraction_width - 1);
    infinity | quiet_bit
}
