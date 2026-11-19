//! The runtime's `crate::wasm` as the staged Wasm rules name it: the operator selectors, the
//! traps and the numeric constants, compiled from the runtime's own files, so a Wasm stencil runs
//! the operators the interpreter decodes. Their interpreter-only parts (decoding from
//! `wasmparser`, bytecode op tables, `WasmValue` execution) are `cfg(not(stencil_build))`.

#[path = "../../quench-runtime/src/wasm/trap.rs"]
mod trap;
#[path = "../../quench-runtime/src/wasm/numeric.rs"]
pub(crate) mod numeric;
#[path = "../../quench-runtime/src/wasm/integer.rs"]
pub(crate) mod integer;
#[path = "../../quench-runtime/src/wasm/float.rs"]
pub(crate) mod float;
#[path = "../../quench-runtime/src/wasm/conversion.rs"]
pub(crate) mod conversion;

pub(crate) use trap::WasmTrap;

/// The bytes of a v128: the staged definition both builds share.
pub(crate) use crate::staged::wasm::V128_BYTES;

/// The value types a staged rule reads a width from; references never reach a stencil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WasmType {
    I32,
    I64,
    F32,
    F64,
    V128,
}
