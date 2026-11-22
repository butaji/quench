//! The Wasm loads and stores of a function's memory as staged rows (`Decoder::WasmMemory`).
//! The direct ops address the function's memory the frame holds (`Frame::direct_memory`). The
//! generic ops (`WasmMemoryAddress`, `WasmMemoryLoad`, `WasmMemoryStore`) name their memory by
//! register, which the frame resolves (`Frame::wasm_memory`). The access is the memory rule of
//! `staged::wasm_memory` (`load`, `store`) over the bytes it addresses: the bounds check and the
//! bytes an access moves are the rule's. An access past the memory's end, a value the frame does
//! not hold in its word's representation, or a vector access leaves the row to its miss, which
//! the hand handler of the op (or the general step) runs and raises.

use super::register_frame::WasmRep;
use super::wasm_machine::{ConcreteBytes, ConcreteMemory};
use super::wasm_memory::{self, Access};
use super::{Concrete, DecodeContext, Fields, Frame, Operation, Step};
use crate::bytecode::{Class, Register};
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use crate::WasmType;

/// A Wasm memory access, with the operands its record names.
#[derive(Clone, Copy)]
pub(crate) enum WasmAccessOperation {
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
    /// The access `instruction` denotes. A direct load or store takes its address from `c` and its
    /// offset from its immediate. A generic load or store takes its value from `a`, its memory from
    /// `b` and its effective address from `c`, with the access its immediate tag names. An effective
    /// address takes its index from `b`, its memory from `c`, and its offset from its constant
    /// (read through `context`, or the lane's own constant).
    pub(crate) fn decode<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        let op = instruction.op();
        let wide = instruction.wide();
        if let Some(access) = MemoryLoad::from_direct_op(op).and_then(Access::of_load) {
            return Some(Self::Load { access, destination: wide.a(), address: wide.c(), offset: wide.imm() });
        }
        if let Some(store) = MemoryStore::from_direct_op(op) {
            return Some(Self::Store {
                access: Access::of_store(store),
                source: wide.a(),
                address: wide.c(),
                offset: wide.imm(),
            });
        }
        match op.class() {
            Class::WasmMemoryLoad => Some(Self::GenericLoad {
                operator: MemoryLoad::from_tag(wide.imm())?,
                destination: wide.a(),
                memory: wide.b(),
                address: wide.c(),
            }),
            Class::WasmMemoryStore => Some(Self::GenericStore {
                operator: MemoryStore::from_tag(wide.imm())?,
                source: wide.a(),
                memory: wide.b(),
                address: wide.c(),
            }),
            Class::WasmMemoryAddress => Some(Self::EffectiveAddress {
                destination: wide.a(),
                index: wide.b(),
                memory: wide.c(),
                offset: instruction.constant(context)?.value.bits64()?,
            }),
            _ => None,
        }
    }
}

impl<W, P> Operation<W, P> {
    /// The memory access `instruction` denotes (`Decoder::WasmMemory`).
    pub fn decode_wasm_access<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        WasmAccessOperation::decode(instruction, context).map(Self::WasmAccess)
    }
}

/// Run `operation` on `frame`: a load's value, or a store's value; `Unresolved` where the frame
/// leaves it, including an out-of-bounds access, which the general step traps.
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
                wasm_memory::load(&mut ConcreteBytes { memory: &*memory }, access, index, offset, false)
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
            let Some(tagged) = frame.wasm_read(stage, source, WasmRep::Tagged) else {
                return Step::Unresolved;
            };
            let Some(word) = frame.wasm_read(stage, source, rep) else { return Step::Unresolved };
            let stored = {
                let Some(memory) = frame.direct_memory() else { return Step::Unresolved };
                wasm_memory::store(&mut ConcreteMemory { memory }, access, index, offset, word, false)
            };
            match stored {
                Ok(()) => Step::Value(tagged),
                Err(_) => Step::Unresolved,
            }
        }
        WasmAccessOperation::GenericLoad { operator, destination, memory, address } => {
            let access = match Access::of_load(operator) {
                Some(access) => access,
                None => return Step::Unresolved,
            };
            let Some(rep) = representation(access.ty) else { return Step::Unresolved };
            let Some(index) = frame.wasm_read(stage, address, WasmRep::Bits64) else {
                return Step::Unresolved;
            };
            let loaded = {
                let Some((bytes, _)) = frame.wasm_memory(stage, memory) else { return Step::Unresolved };
                wasm_memory::load(&mut ConcreteBytes { memory: &*bytes }, access, index, 0, false)
            };
            let Ok(word) = loaded else { return Step::Unresolved };
            match frame.wasm_write(stage, destination, word, rep) {
                Some(tagged) => Step::Value(tagged),
                None => Step::Unresolved,
            }
        }
        WasmAccessOperation::GenericStore { operator, source, memory, address } => {
            let access = Access::of_store(operator);
            let Some(rep) = representation(access.ty) else { return Step::Unresolved };
            let Some(index) = frame.wasm_read(stage, address, WasmRep::Bits64) else {
                return Step::Unresolved;
            };
            let Some(tagged) = frame.wasm_read(stage, source, WasmRep::Tagged) else {
                return Step::Unresolved;
            };
            let Some(word) = frame.wasm_read(stage, source, rep) else { return Step::Unresolved };
            let stored = {
                let Some((bytes, _)) = frame.wasm_memory(stage, memory) else { return Step::Unresolved };
                wasm_memory::store(&mut ConcreteMemory { memory: bytes }, access, index, 0, word, false)
            };
            match stored {
                Ok(()) => Step::Value(tagged),
                Err(_) => Step::Unresolved,
            }
        }
        WasmAccessOperation::EffectiveAddress { destination, index, memory, offset } => {
            let Some((_, wide)) = frame.wasm_memory(stage, memory) else { return Step::Unresolved };
            let rep = if wide { WasmRep::Bits64 } else { WasmRep::Int32 };
            let Some(address) = frame.wasm_read(stage, index, rep) else { return Step::Unresolved };
            let Some(effective) = address.checked_add(offset) else { return Step::Unresolved };
            match frame.wasm_write(stage, destination, effective, WasmRep::Bits64) {
                Some(tagged) => Step::Value(tagged),
                None => Step::Unresolved,
            }
        }
    }
}
