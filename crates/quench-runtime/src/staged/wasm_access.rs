//! The Wasm loads and stores of a function's memory as staged rows (`Decoder::WasmMemory`).
//! The direct ops address the function's memory the frame holds (`Frame::direct_memory`). The
//! generic ops (`WasmMemoryAddress`, `WasmMemoryLoad`, `WasmMemoryStore`) name their memory by
//! register, which the frame resolves (`Frame::wasm_memory`). The access is the memory rule of
//! `staged::wasm_memory` (`load`, `store`) over the bytes it addresses: the bounds check and the
//! bytes an access moves are the rule's. An access past the memory's end, a value the frame does
//! not hold in its word's representation, or a vector access leaves the row to its miss, which
//! the hand handler of the op (or the general step) runs and raises. The decode of an access from
//! its record, and the row's step over the runtime's memory bytes, are in `wasm_access/glue.rs`.

use super::register_frame::WasmRep;
use super::wasm_memory::Access;
use crate::bytecode::Register;
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use crate::WasmType;

/// A Wasm memory access, with the operands its record names.
#[derive(Clone, Copy)]
pub enum WasmAccessOperation {
    /// `destination = load[address + offset]` of the function's memory (a direct op): the address
    /// is the 32-bit register `address` plus the record's constant `offset`.
    Load { access: Access, destination: Register, address: Register, offset: u32 },
    /// `store[address + offset] = source` of the function's memory (a direct op).
    Store { access: Access, source: Register, address: Register, offset: u32 },
    /// `destination = load[address]` of the memory in `memory`, the scalar access `operator` names
    /// (`WasmMemoryLoad`); `address` holds the effective address `EffectiveAddress` made.
    GenericLoad { operator: MemoryLoad, destination: Register, memory: Register, address: Register },
    /// `store[address] = source` of the memory in `memory`, the scalar access `operator` names
    /// (`WasmMemoryStore`).
    GenericStore { operator: MemoryStore, source: Register, memory: Register, address: Register },
    /// `destination = index + offset`, the effective address of a generic access (`WasmMemoryAddress`):
    /// the index in `index`, read as the memory's index type (named by the memory in `memory`).
    EffectiveAddress { destination: Register, index: Register, memory: Register, offset: u64 },
    /// `destination = memory.size` of the memory in `memory` (`WasmMemorySize`): its length in pages,
    /// as the memory's index type.
    Size { destination: Register, memory: Register },
    /// `memory.fill` of the memory in `memory` (`WasmMemoryFill`): `length` copies of the byte in
    /// `byte` from the index in `output`, both read as the memory's index type.
    Fill { memory: Register, byte: Register, length: Register, output: Register },
    /// `memory.copy` from the memory in `source` to the memory in `destination` (`WasmMemoryCopy`):
    /// `length` bytes from the index in `input` to the index in `output`. The row copies within one
    /// memory; a copy between two memories leaves to its miss.
    Copy { destination: Register, source: Register, length: Register, output: Register, input: Register },
}

/// The register representation a value of type `ty` takes: an i32 or f32 as its 32 bits, an i64 or
/// f64 as its 64 bits. `None` for a type a scalar access does not move.
pub const fn representation(ty: WasmType) -> Option<WasmRep> {
    match ty {
        WasmType::I32 | WasmType::F32 => Some(WasmRep::Int32),
        WasmType::I64 | WasmType::F64 => Some(WasmRep::Bits64),
        _ => None,
    }
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;
