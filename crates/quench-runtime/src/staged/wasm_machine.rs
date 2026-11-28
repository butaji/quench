//! The interpreter's stages over the state of an instance: the scalar rules of `ConcreteWasm`
//! plus a table's primitives (`ConcreteTable`, which the VM resolves indirect calls on) and
//! linear memory over borrowed bytes (`ConcreteBytes` reads, `ConcreteMemory` reads and
//! writes: the access and bound rules of `MemoryLoad::read` and `MemoryStore::write`).
//! `ConcreteInstance` adds the globals over borrowed cells, which the interpreter's global steps
//! and the tests of the tier's instance primitives run.

use super::wasm_concrete::scalar_stage;
use super::wasm_call::WasmTable;
use super::wasm_instance::{Storage, WasmInstance};
#[cfg(not(stencil_build))]
use super::wasm_memory::WasmMemoryPair;
use super::wasm_memory::{Access, WasmMemory, WasmMemoryWrite};
use crate::heap::TableTarget;
#[cfg(not(stencil_build))]
use crate::wasm::memory::MemoryStorage;
use crate::wasm::memory::checked_range;
use crate::wasm::V128_BYTES;
use crate::Value;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// A stage over a linear memory's bytes, which it reads.
pub(crate) struct ConcreteBytes<'a> {
    pub(crate) memory: &'a [u8],
}

/// A stage over a linear memory's bytes, which it reads and writes.
#[quench_macros::test_visible]
pub(crate) struct ConcreteMemory<'a> {
    pub(crate) memory: &'a mut [u8],
}

/// A stage over two linear memories, the source's storage it reads the length of and the
/// destination's it writes. The transfer runs through `MemoryStorage::copy_from`, which takes
/// the locks of shared memory, so no lock is held across the two memories.
#[cfg(not(stencil_build))]
pub(crate) struct ConcretePair<'a> {
    pub(crate) destination: &'a MemoryStorage,
    pub(crate) source: &'a MemoryStorage,
}

/// A stage over an instance: the native slots of its globals by environment slot and its
/// linear memory.
pub struct ConcreteInstance<'a> {
    pub globals: &'a mut [u64],
    pub memory: &'a mut [u8],
}

/// A stage over a table of function references and the row of each (`TableTarget`); `related` says
/// whether a reference whose id is not the expected one satisfies the expected type anyway.
pub(crate) struct ConcreteTable<'a> {
    pub(crate) elements: &'a [Value],
    pub(crate) targets: &'a [TableTarget],
    pub(crate) related: &'a mut dyn FnMut(Value) -> bool,
}

scalar_stage!(Concrete =>
    ConcreteInstance<'_>,
    ConcreteTable<'_>,
    ConcreteBytes<'_>,
    ConcreteMemory<'_>
);

impl WasmTable for ConcreteTable<'_> {
    #[inline(always)]
    fn table_range_exceeds(&mut self, index: u64, extent: u64) -> u64 {
        range_past(self.elements.len(), index, extent)
    }

    #[inline(always)]
    fn element_signature(&mut self, index: u64) -> u64 {
        self.targets[index as usize].signature.word()
    }

    #[inline(always)]
    fn table_element(&mut self, index: u64) -> u64 {
        self.elements[index as usize].to_bits()
    }

    type Row = TableTarget;

    #[inline(always)]
    fn element_row(&mut self, index: u64) -> TableTarget {
        self.targets[index as usize]
    }

    /// Equal ids are equivalent types; the closure knows the expected type, which it closes over.
    #[inline(always)]
    fn signature_accepts(&mut self, reference: u64, signature: u64, expected: u64) -> u64 {
        u64::from(signature == expected || (self.related)(Value::from_bits(reference)))
    }

    #[inline(always)]
    fn unless_equal(
        &mut self,
        signature: u64,
        expected: u64,
        otherwise: impl FnOnce(&mut Self) -> Result<u64, Self::Exit>,
    ) -> Result<u64, Self::Exit> {
        if signature == expected { Ok(1) } else { otherwise(self) }
    }
}

impl WasmInstance for ConcreteInstance<'_> {
    #[inline(always)]
    fn global_load(&mut self, slot: u32, storage: Storage) -> u64 {
        storage.narrow(self.globals[slot as usize])
    }

    #[inline(always)]
    fn global_store(&mut self, slot: u32, storage: Storage, value: u64) {
        self.globals[slot as usize] = storage.narrow(value);
    }
}

/// The bytes of the checked `access` at `index + offset`.
#[inline(always)]
fn access_bytes(access: Access, index: u64, offset: u32) -> std::ops::Range<usize> {
    let start = (index + u64::from(offset)) as usize;
    start..start + access.bytes
}

/// The word of the checked `access` at `index + offset`: the bytes little-endian, sign-extended
/// where a narrow load asks, in the width of the access's type.
#[inline(always)]
fn load_word(memory: &[u8], access: Access, index: u64, offset: u32) -> u64 {
    let mut word = [0u8; size_of::<u64>()];
    word[..access.bytes].copy_from_slice(&memory[access_bytes(access, index, offset)]);
    access.loaded(u64::from_le_bytes(word))
}

/// Writes the low bytes of `value` as the checked `access` at `index + offset`.
#[inline(always)]
fn store_word(memory: &mut [u8], access: Access, index: u64, offset: u32, value: u64) {
    let bytes = &value.to_le_bytes()[..access.bytes];
    memory[access_bytes(access, index, offset)].copy_from_slice(bytes);
}

/// The v128 at the checked `index + offset`, little-endian.
#[inline(always)]
fn load_vector_word(memory: &[u8], index: u64, offset: u32) -> u128 {
    let mut word = [0u8; V128_BYTES];
    word.copy_from_slice(&memory[access_bytes(Access::VECTOR, index, offset)]);
    u128::from_le_bytes(word)
}

/// Writes the v128 `value` at the checked `index + offset`, little-endian.
#[inline(always)]
fn store_vector_word(memory: &mut [u8], index: u64, offset: u32, value: u128) {
    memory[access_bytes(Access::VECTOR, index, offset)].copy_from_slice(&value.to_le_bytes());
}

/// The flag that `extent` units from `index` pass the end of `length` units: the shared range
/// check of memories, tables and bulk operations.
#[inline(always)]
fn range_past(length: usize, index: u64, extent: u64) -> u64 {
    let inside =
        usize::try_from(extent).is_ok_and(|extent| checked_range(index, extent, length).is_ok());
    u64::from(!inside)
}

/// The reading half of a stage over bytes held in its `memory` field.
macro_rules! readable_memory {
    ($($stage:ty),+) => {$(
        impl WasmMemory for $stage {
            type Vector = u128;

            fn guarded(&self) -> bool {
                false
            }

            #[inline(always)]
            fn memory_length(&mut self) -> u64 {
                self.memory.len() as u64
            }

            /// A concrete 32-bit word is already its zero-extended index.
            #[inline(always)]
            fn memory_index(&mut self, address: u64) -> u64 {
                address
            }

            #[inline(always)]
            fn memory_load(&mut self, access: Access, index: u64, offset: u32) -> u64 {
                load_word(self.memory, access, index, offset)
            }

            #[inline(always)]
            fn memory_load_vector(&mut self, index: u64, offset: u32) -> u128 {
                load_vector_word(self.memory, index, offset)
            }

            #[inline(always)]
            fn range_exceeds(&mut self, index: u64, extent: u64) -> u64 {
                range_past(self.memory.len(), index, extent)
            }
        }
    )+};
}

/// The writing half, for the stages whose `memory` is mutable.
macro_rules! writable_memory {
    ($($stage:ty),+) => {$(
        impl WasmMemoryWrite for $stage {
            #[inline(always)]
            fn memory_store(&mut self, access: Access, index: u64, offset: u32, value: u64) {
                store_word(self.memory, access, index, offset, value);
            }

            #[inline(always)]
            fn memory_store_vector(&mut self, index: u64, offset: u32, value: u128) {
                store_vector_word(self.memory, index, offset, value);
            }

            fn memory_fill(&mut self, index: u64, byte: u64, length: u64) {
                self.memory[index as usize..(index + length) as usize].fill(byte as u8);
            }

            fn memory_copy(&mut self, destination: u64, source: u64, length: u64) {
                self.memory.copy_within(
                    source as usize..(source + length) as usize,
                    destination as usize,
                );
            }
        }
    )+};
}

readable_memory!(ConcreteBytes<'_>, ConcreteMemory<'_>, ConcreteInstance<'_>);
writable_memory!(ConcreteMemory<'_>, ConcreteInstance<'_>);

#[cfg(not(stencil_build))]
scalar_stage!(Concrete => ConcretePair<'_>);

#[cfg(not(stencil_build))]
impl WasmMemoryPair for ConcretePair<'_> {
    #[inline(always)]
    fn source_range_exceeds(&mut self, index: u64, extent: u64) -> u64 {
        range_past(self.source.len(), index, extent)
    }

    #[inline(always)]
    fn destination_range_exceeds(&mut self, index: u64, extent: u64) -> u64 {
        range_past(self.destination.len(), index, extent)
    }

    #[inline(always)]
    fn memory_transfer(&mut self, destination: u64, source: u64, length: u64) {
        if length == 0 {
            return;
        }
        let input = source as usize..(source + length) as usize;
        let output = destination as usize..(destination + length) as usize;
        self.destination.copy_from(self.source, input, output);
    }
}
