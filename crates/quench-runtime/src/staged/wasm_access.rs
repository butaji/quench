//! The Wasm direct loads and stores of a function's memory as staged rows (`Decoder::WasmMemory`).
//! The access is the memory rule of `staged::wasm_memory` (`load`, `store`) run over the bytes the
//! frame addresses directly (`Frame::direct_memory`): the bounds check and the bytes an access moves
//! are the rule's. An access past the memory's end, or an operand the frame does not hold in its
//! word's representation, leaves the row to its miss, which the general step runs and raises.

use super::register_frame::WasmRep;
use super::wasm_machine::{ConcreteBytes, ConcreteMemory};
use super::wasm_memory::{Access, load, store};
use super::{Concrete, Frame, Operation, Step};
use crate::bytecode::{Register, WideInstruction};
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use crate::WasmType;

/// A direct access of the function's memory, with the operands its record names: the address is
/// the 32-bit register `address` plus the record's constant `offset`.
#[derive(Clone, Copy)]
pub(crate) enum WasmAccessOperation {
    /// `destination = load[address + offset]`.
    Load { access: Access, destination: Register, address: Register, offset: u32 },
    /// `store[address + offset] = source`.
    Store { access: Access, source: Register, address: Register, offset: u32 },
}

/// The register representation a value of type `ty` takes: an i32 or f32 as its 32 bits, an i64 or
/// f64 as its 64 bits. `None` for a type a scalar access does not move.
const fn representation(ty: WasmType) -> Option<WasmRep> {
    match ty {
        WasmType::I32 | WasmType::F32 => Some(WasmRep::Int32),
        WasmType::I64 | WasmType::F64 => Some(WasmRep::Bits64),
        _ => None,
    }
}

impl WasmAccessOperation {
    /// The access `instruction` denotes: a direct load or store, whose address is its `c` register and
    /// whose offset is its immediate.
    pub(crate) fn decode(instruction: WideInstruction) -> Option<Self> {
        let op = instruction.op();
        let (register, address, offset) = (instruction.a(), instruction.c(), instruction.imm());
        if let Some(access) = MemoryLoad::from_direct_op(op).and_then(Access::of_load) {
            return Some(Self::Load { access, destination: register, address, offset });
        }
        let store = MemoryStore::from_direct_op(op)?;
        Some(Self::Store { access: Access::of_store(store), source: register, address, offset })
    }
}

impl<W, P> Operation<W, P> {
    /// The direct memory access `instruction` denotes (`Decoder::WasmMemory`).
    pub(crate) fn decode_wasm_access(instruction: WideInstruction) -> Option<Self> {
        WasmAccessOperation::decode(instruction).map(Self::WasmAccess)
    }
}

/// Run `operation` on `frame`: a load's value, or a store's word; `Unresolved` where the frame leaves
/// it, including an out-of-bounds access, which the general step traps.
#[inline(always)]
pub(crate) fn step<F: Frame<Concrete>>(
    stage: &mut Concrete,
    frame: &mut F,
    operation: WasmAccessOperation,
) -> Step<u64, F::Control> {
    match operation {
        WasmAccessOperation::Load { access, destination, address, offset } => {
            let Some(rep) = representation(access.ty) else { return Step::Unresolved };
            let Some(index) = frame.wasm_read(stage, address, WasmRep::Int32) else {
                return Step::Unresolved;
            };
            let loaded = {
                let Some(memory) = frame.direct_memory() else { return Step::Unresolved };
                load(&mut ConcreteBytes { memory: &*memory }, access, index, offset, false)
            };
            let Ok(word) = loaded else { return Step::Unresolved };
            match frame.wasm_write(stage, destination, word, rep) {
                Some(tagged) => Step::Value(tagged),
                None => Step::Unresolved,
            }
        }
        WasmAccessOperation::Store { access, source, address, offset } => {
            let Some(rep) = representation(access.ty) else { return Step::Unresolved };
            let Some(index) = frame.wasm_read(stage, address, WasmRep::Int32) else {
                return Step::Unresolved;
            };
            let Some(word) = frame.wasm_read(stage, source, rep) else { return Step::Unresolved };
            let stored = {
                let Some(memory) = frame.direct_memory() else { return Step::Unresolved };
                store(&mut ConcreteMemory { memory }, access, index, offset, word, false)
            };
            match stored {
                Ok(()) => Step::Value(word),
                Err(_) => Step::Unresolved,
            }
        }
    }
}
