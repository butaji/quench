//! Shared typed numeric results and macro-derived selector views.

#[cfg(not(stencil_build))]
use super::{ScalarBits, WasmValue};

#[cfg(test)]
pub(super) enum NumericResult<T> {
    Value(T),
    Comparison(bool),
}

#[cfg(test)]
impl<T> NumericResult<T> {
    pub(super) fn into_wasm(self, integer: impl FnOnce(T) -> WasmValue) -> WasmValue {
        match self {
            Self::Value(value) => integer(value),
            Self::Comparison(value) => WasmValue::I32(i32::from(value)),
        }
    }
}

#[cfg(not(stencil_build))]
impl WasmValue {
    /// The word `from_bits` reads back: a 32-bit scalar's payload, zero-extended.
    #[inline]
    pub(crate) fn word(self) -> u64 {
        match self.bits() {
            Some(ScalarBits::Bits32(bits)) => u64::from(bits),
            Some(ScalarBits::Bits64(bits)) => bits,
            _ => unreachable!("numeric scalar"),
        }
    }
}

macro_rules! selectors {
    ($enum:ident; $($name:ident, $wasm:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub(crate) enum $enum { $($name,)+ }
        impl $enum {
            // Families with first-class opcodes, such as i32 binary operators,
            // are never decoded from a selector.
            #[allow(dead_code)]
            pub(super) const ALL: &'static [Self] = &[$(Self::$name,)+];
            #[allow(dead_code)] #[inline(always)]
            pub(crate) const fn from_tag(tag: u32) -> Option<Self> { if (tag as usize) < Self::ALL.len() { Some(Self::ALL[tag as usize]) } else { None } }
            /// How many selectors the family has.
            #[allow(dead_code)]
            pub(crate) const COUNT: usize = Self::ALL.len();
            #[cfg(not(stencil_build))]
            pub(crate) fn from_wasm(operator: &wasmparser::Operator<'_>) -> Option<Self> {
                match operator { $(wasmparser::Operator::$wasm => Some(Self::$name),)+ _ => None }
            }
        }
    };
}

pub(super) use selectors;

const IEEE_IMPLICIT_SIGNIFICAND_BITS: u32 = 1;

pub(crate) const fn sign_mask(bit_width: u32) -> u64 {
    1 << (bit_width - 1)
}

pub(crate) const fn canonical_nan_bits(precision: u32, infinity: u64) -> u64 {
    let fraction_width = precision - IEEE_IMPLICIT_SIGNIFICAND_BITS;
    let quiet_bit = 1 << (fraction_width - 1);
    infinity | quiet_bit
}
