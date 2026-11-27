//! The trap enum on its own, so the b-jit's stencils (`quench-bjit/stencils`) name the same
//! traps the staged rules raise.

/// A WebAssembly trap, distinct from a JavaScript throw or invalid residual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmTrap {
    Unreachable,
    CallStackExhausted,
    IntegerDivideByZero,
    IntegerOverflow,
    InvalidConversionToInteger,
    OutOfBoundsMemory,
    UnalignedAtomic,
    WaitOnUnsharedMemory,
    TooManyWaiters,
    OutOfBoundsTable,
    OutOfBoundsArray,
    UndefinedElement,
    UninitializedElement,
    NullReference,
    NullFunctionReference,
    NullI31Reference,
    NullStructReference,
    NullArrayReference,
    NullExceptionReference,
    NullDescriptorReference,
    ArrayTooLarge,
    CastFailure,
    DescriptorCastFailure,
    IndirectCallTypeMismatch,
}

/// The traps compiled code raises, in status order: a trap's status is its position plus one
/// (zero is a return), the same for the u-jit's code and the b-jit's stencils.
/// Every trap is listed: a compiled GC, table or exception rule raises its own trap like any other.
pub const COMPILED_TRAPS: [WasmTrap; 24] = [
    WasmTrap::Unreachable,
    WasmTrap::OutOfBoundsMemory,
    WasmTrap::IntegerDivideByZero,
    WasmTrap::IntegerOverflow,
    WasmTrap::CallStackExhausted,
    WasmTrap::InvalidConversionToInteger,
    WasmTrap::UndefinedElement,
    WasmTrap::UninitializedElement,
    WasmTrap::IndirectCallTypeMismatch,
    WasmTrap::UnalignedAtomic,
    WasmTrap::WaitOnUnsharedMemory,
    WasmTrap::TooManyWaiters,
    WasmTrap::OutOfBoundsTable,
    WasmTrap::OutOfBoundsArray,
    WasmTrap::NullReference,
    WasmTrap::NullFunctionReference,
    WasmTrap::NullI31Reference,
    WasmTrap::NullStructReference,
    WasmTrap::NullArrayReference,
    WasmTrap::NullExceptionReference,
    WasmTrap::NullDescriptorReference,
    WasmTrap::ArrayTooLarge,
    WasmTrap::CastFailure,
    WasmTrap::DescriptorCastFailure,
];

impl WasmTrap {
    /// The status compiled code leaves with on this trap; `None` for a trap compiled code never
    /// raises.
    pub const fn compiled_status(self) -> Option<u32> {
        let mut index = 0;
        while index < COMPILED_TRAPS.len() {
            if COMPILED_TRAPS[index] as u8 == self as u8 {
                return Some(index as u32 + 1);
            }
            index += 1;
        }
        None
    }
}

impl std::fmt::Display for WasmTrap {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str(match self {
            Self::UndefinedElement => "undefined element",
            Self::UninitializedElement => "uninitialized element",
            Self::NullReference => "null reference",
            Self::NullFunctionReference => "null function reference",
            Self::NullArrayReference => "null array reference",
            Self::NullExceptionReference => "null exception reference",
            Self::NullDescriptorReference => "null descriptor reference",
            Self::ArrayTooLarge => "array too large",
            Self::NullStructReference => "null structure reference",
            Self::NullI31Reference => "null i31 reference",
            Self::CastFailure => "cast failure",
            Self::DescriptorCastFailure => "descriptor cast failure",
            Self::IndirectCallTypeMismatch => "indirect call type mismatch",
            Self::OutOfBoundsArray => "out of bounds array access",
            Self::OutOfBoundsTable => "out of bounds table access",
            Self::WaitOnUnsharedMemory => "atomic wait on unshared memory",
            Self::TooManyWaiters => "too many atomic waiters",
            Self::UnalignedAtomic => "unaligned atomic",
            Self::OutOfBoundsMemory => "out of bounds memory access",
            Self::Unreachable => "unreachable",
            Self::CallStackExhausted => "call stack exhausted",
            Self::IntegerDivideByZero => "integer divide by zero",
            Self::IntegerOverflow => "integer overflow",
            Self::InvalidConversionToInteger => "invalid conversion to integer",
        })
    }
}
