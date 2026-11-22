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
    pub(crate) const fn compiled_status(self) -> Option<u32> {
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
