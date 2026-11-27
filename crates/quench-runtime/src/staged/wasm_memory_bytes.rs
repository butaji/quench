//! The interpreter's access to linear memory over bytes: `MemoryLoad::read` and `MemoryStore::write`
//! check and move a scalar, lane or vector access through the staged rules (`Access`,
//! `wasm_memory_vector`), and `read_at`/`write_at` are their unchecked forms over raw bytes.

use super::wasm_machine::{ConcreteBytes, ConcreteMemory};
use super::wasm_memory::{Access, AccessWasmMemoryExt, WasmMemory, WasmMemoryWrite, access_of_load, access_of_store};
use super::wasm_memory_vector::{lane, load_lane, load_vector, store_vector};
use crate::wasm::memory::{MemoryLoad, MemoryLoadMemoryOpsExt, MemoryStore};
use crate::{WasmTrap, WasmType, WasmValue};

impl MemoryLoad {
    /// The value at `address` of `bytes`: out of bounds traps. A scalar load is the staged access
    /// (`Access::read`); a v128 load is `load_vector`; the other vector loads widen, splat or zero
    /// the staged lane (`load_lane`).
    #[inline(always)]
    pub(crate) fn read(self, bytes: &[u8], address: u64) -> Result<WasmValue, WasmTrap> {
        read_memory(bytes, self, address)
    }

    /// The value at `at`, unchecked.
    ///
    /// # Safety
    /// `width` bytes from `at` must be readable.
    #[inline(always)]
    pub(crate) unsafe fn read_at(self, at: *const u8) -> WasmValue {
        // SAFETY: the caller keeps `width` bytes readable.
        let memory = unsafe { std::slice::from_raw_parts(at, self.width()) };
        let mut stage = ConcreteBytes { memory };
        match access_of_load(self) {
            Some(access) => WasmValue::from_bits(access.ty, stage.memory_load(access, 0, 0)),
            None if matches!(self, Self::V128Load) => WasmValue::V128(stage.memory_load_vector(0, 0)),
            None => self.vector_of_word(stage.memory_load(lane(self.width()), 0, 0)),
        }
    }
}

/// The value at `address` of `bytes` as `operator` loads it: out of bounds traps. A scalar load is
/// the staged access (`Access::read`); a v128 load is `load_vector`; the other vector loads widen,
/// splat or zero the staged lane (`load_lane`). The one body of `MemoryLoad::read`, which the lane
/// handlers also call.
#[inline(always)]
pub fn read_memory(bytes: &[u8], operator: MemoryLoad, address: u64) -> Result<WasmValue, WasmTrap> {
    if matches!(operator, MemoryLoad::V128Load) {
        return load_vector(&mut ConcreteBytes { memory: bytes }, address, 0, false).map(WasmValue::V128);
    }
    let Some(access) = access_of_load(operator) else {
        let word = load_lane(&mut ConcreteBytes { memory: bytes }, operator.width(), address, 0, false)?;
        return Ok(operator.vector_of_word(word));
    };
    access.read(bytes, address)
}

/// Write `value` at `address` of `bytes` as `operator` stores it: out of bounds traps before any
/// byte changes. A scalar store is the staged access (`Access::write`); a v128 store is
/// `store_vector`. The one body of `MemoryStore::write`, which the lane handlers also call.
#[inline(always)]
pub fn write_memory(bytes: &mut [u8], operator: MemoryStore, address: u64, value: WasmValue) -> Result<(), WasmTrap> {
    if operator.value_type() == WasmType::V128 {
        let WasmValue::V128(word) = value else { unreachable!("decoded store type") };
        return store_vector(&mut ConcreteMemory { memory: bytes }, address, 0, word, false);
    }
    access_of_store(operator).write(bytes, address, value)
}

impl MemoryStore {
    /// Write `value` at `address` of `bytes`: out of bounds traps before any byte changes. A scalar
    /// store is the staged access (`Access::write`); a v128 store is `store_vector`.
    #[inline(always)]
    pub(crate) fn write(self, bytes: &mut [u8], address: u64, value: WasmValue) -> Result<(), WasmTrap> {
        write_memory(bytes, self, address, value)
    }

    /// Write `value` at `at`, unchecked.
    ///
    /// # Safety
    /// `width` bytes from `at` must be writable.
    #[inline(always)]
    pub(crate) unsafe fn write_at(self, at: *mut u8, value: WasmValue) {
        if self.value_type() == WasmType::V128 {
            let WasmValue::V128(word) = value else { unreachable!("decoded store type") };
            // SAFETY: the caller keeps `width` bytes writable.
            let memory = unsafe { std::slice::from_raw_parts_mut(at, self.width()) };
            return ConcreteMemory { memory }.memory_store_vector(0, 0, word);
        }
        let access = access_of_store(self);
        // SAFETY: the caller keeps `width` bytes writable.
        let memory = unsafe { std::slice::from_raw_parts_mut(at, access.bytes) };
        ConcreteMemory { memory }.memory_store(access, 0, 0, value.word());
    }
}
