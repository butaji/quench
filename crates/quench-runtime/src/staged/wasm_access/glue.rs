//! Runtime glue of `wasm_access`: the decode of an access from its record, the decode's
//! `Operation` row, and the row's step over the runtime's memory bytes (`ConcreteBytes`,
//! `ConcreteMemory`, which name the runtime's table and memory targets).
#![allow(unused_imports)]
use super::*;
use crate::bytecode::Class;
use crate::staged::register_frame::WasmRep;
use crate::staged::wasm_machine::{ConcreteBytes, ConcreteMemory};
use crate::staged::wasm_memory;
use crate::staged::{Concrete, DecodeContext, Fields, Frame, Operation, Step};

/// The decode of a `WasmAccessOperation` from a record.
pub trait WasmAccessOperationWasmAccessExt: Sized {
    /// The access `instruction` denotes. A size takes its result register from `a` and its memory
    /// from `b`; a fill its memory from `a`, its byte from `b`, its length from `c` and its
    /// destination index from the register pair; a copy its memories from `a` and `b`, its length
    /// from `c` and its two indexes from the pair. A direct load or store takes its address from `c` and its
    /// offset from its immediate. A generic load or store takes its value from `a`, its memory from
    /// `b` and its effective address from `c`, with the access its immediate tag names. An effective
    /// address takes its index from `b`, its memory from `c`, and its offset from its constant
    /// (read through `context`, or the lane's own constant).
    fn decode<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self>;
}

impl WasmAccessOperationWasmAccessExt for WasmAccessOperation {
    fn decode<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
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
            Class::WasmMemorySize => Some(Self::Size { destination: wide.a(), memory: wide.b() }),
            Class::WasmMemoryFill => {
                let (output, _) = wide.register_pair();
                Some(Self::Fill { memory: wide.a(), byte: wide.b(), length: wide.c(), output })
            }
            Class::WasmMemoryCopy => {
                let (output, input) = wide.register_pair();
                Some(Self::Copy { destination: wide.a(), source: wide.b(), length: wide.c(), output, input })
            }
            _ => None,
        }
    }
}

/// The decode of an `Operation`'s memory-access row from a record.
pub trait OperationWasmAccessExt<W, P>: Sized {
    /// The memory access `instruction` denotes (`Decoder::WasmMemory`).
    fn decode_wasm_access<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self>;
}

impl<W, P> OperationWasmAccessExt<W, P> for Operation<W, P> {
    fn decode_wasm_access<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        WasmAccessOperation::decode(instruction, context).map(Self::WasmAccess)
    }
}

/// Run `operation` on `frame`: a load's value, or a store's value; `Unresolved` where the frame
/// leaves it, including an out-of-bounds access, which the general step traps.
#[inline(always)]
pub fn step<F: Frame<Concrete>>(
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
        WasmAccessOperation::Size { destination, memory } => {
            let Some(page_bits) = frame.wasm_memory_page_bits(stage, memory) else { return Step::Unresolved };
            let (pages, wide) = {
                let Some((bytes, wide)) = frame.wasm_memory(stage, memory) else { return Step::Unresolved };
                (wasm_memory::pages(&mut ConcreteBytes { memory: &*bytes }, page_bits), wide)
            };
            let rep = if wide { WasmRep::Bits64 } else { WasmRep::Int32 };
            match frame.wasm_write(stage, destination, pages, rep) {
                Some(tagged) => Step::Value(tagged),
                None => Step::Unresolved,
            }
        }
        WasmAccessOperation::Fill { memory, byte, length, output } => {
            // The index type is the memory's: resolved once for the operands, then again for the
            // bytes, which the operand reads must not hold borrowed.
            let Some((_, wide)) = frame.wasm_memory(stage, memory) else { return Step::Unresolved };
            let rep = if wide { WasmRep::Bits64 } else { WasmRep::Int32 };
            let Some(index) = frame.wasm_read(stage, output, rep) else { return Step::Unresolved };
            let Some(count) = frame.wasm_read(stage, length, rep) else { return Step::Unresolved };
            let Some(value) = frame.wasm_read(stage, byte, WasmRep::Int32) else { return Step::Unresolved };
            let Some(tagged) = frame.wasm_read(stage, byte, WasmRep::Tagged) else { return Step::Unresolved };
            let filled = {
                let Some((bytes, _)) = frame.wasm_memory(stage, memory) else { return Step::Unresolved };
                wasm_memory::fill(&mut ConcreteMemory { memory: bytes }, index, value, count)
            };
            match filled {
                Ok(()) => Step::Value(tagged),
                Err(_) => Step::Unresolved,
            }
        }
        WasmAccessOperation::Copy { destination, source, length, output, input } => {
            // One memory: both registers hold the same memory value, so one index type reads all
            // three operands (`copy_length_type` of two equal types is that type).
            let Some(memory) = frame.wasm_read(stage, destination, WasmRep::Tagged) else { return Step::Unresolved };
            if frame.wasm_read(stage, source, WasmRep::Tagged) != Some(memory) {
                return Step::Unresolved;
            }
            let Some((_, wide)) = frame.wasm_memory(stage, destination) else { return Step::Unresolved };
            let rep = if wide { WasmRep::Bits64 } else { WasmRep::Int32 };
            let Some(to) = frame.wasm_read(stage, output, rep) else { return Step::Unresolved };
            let Some(from) = frame.wasm_read(stage, input, rep) else { return Step::Unresolved };
            let Some(count) = frame.wasm_read(stage, length, rep) else { return Step::Unresolved };
            let copied = {
                let Some((bytes, _)) = frame.wasm_memory(stage, destination) else { return Step::Unresolved };
                wasm_memory::copy(&mut ConcreteMemory { memory: bytes }, to, from, count)
            };
            match copied {
                Ok(()) => Step::Value(memory),
                Err(_) => Step::Unresolved,
            }
        }
    }
}
