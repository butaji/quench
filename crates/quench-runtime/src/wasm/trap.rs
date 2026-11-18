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
