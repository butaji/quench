//! The interpreter's stages over the state of an instance: the scalar rules of `ConcreteWasm`
//! plus a table's primitives (`ConcreteTable`, which the VM resolves indirect calls on) and
//! linear memory over borrowed bytes (`ConcreteBytes` reads, `ConcreteMemory` reads and
//! writes: the access and bound rules of `MemoryLoad::read` and `MemoryStore::write`).
//! `ConcreteInstance` adds the globals over borrowed cells, which the interpreter's global steps
//! and the tests of the tier's instance primitives run.

use super::ConcreteWasm;
use super::machine::{Machine, MachineStage};
use super::wasm::Width;
use super::wasm_call::WasmTable;
use super::wasm_float::{Float, Truncation};
use super::wasm_instance::{Storage, WasmInstance};
use super::wasm_memory::{Access, WasmMemory, WasmMemoryWrite};
use crate::heap::TableTarget;
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use crate::wasm::memory::checked_range;
use crate::{Value, WasmTrap};

/// A stage over a linear memory's bytes, which it reads.
pub(crate) struct ConcreteBytes<'a> {
    pub(crate) memory: &'a [u8],
}

/// A stage over a linear memory's bytes, which it reads and writes.
pub(crate) struct ConcreteMemory<'a> {
    pub(crate) memory: &'a mut [u8],
}

/// A stage over an instance: the native slots of its globals by environment slot and its
/// linear memory.
pub(crate) struct ConcreteInstance<'a> {
    pub(crate) globals: &'a mut [u64],
    pub(crate) memory: &'a mut [u8],
}

/// A stage over a table of function references and the row of each (`TableTarget`); `related` says
/// whether a reference whose id is not the expected one satisfies the expected type anyway.
pub(crate) struct ConcreteTable<'a> {
    pub(crate) elements: &'a [Value],
    pub(crate) targets: &'a [TableTarget],
    pub(crate) related: &'a mut dyn FnMut(Value) -> bool,
}

/// Methods that are `ConcreteWasm`'s: one definition, delegated.
macro_rules! delegated {
    ($($name:ident($($argument:ident: $ty:ty),*) -> $result:ty;)+) => {$(
        #[inline(always)]
        fn $name(&mut self, $($argument: $ty),*) -> $result {
            ConcreteWasm.$name($($argument),*)
        }
    )+};
}

/// The scalar primitives of a stage over an instance.
macro_rules! scalar_stage {
    ($($stage:ty),+) => {$(
        impl Machine for $stage {
            type Word = u64;
        }

        impl MachineStage for $stage {
            type Exit = WasmTrap;

            delegated! {
                constant(width: Width, value: i64) -> u64;
                operate(operator: Binary, width: Width, left: u64, right: u64) -> u64;
                operate_unary(operator: Unary, width: Width, value: u64) -> u64;
                operate_float(operator: FloatBinary, float: Float, left: u64, right: u64) -> u64;
                operate_float_unary(operator: FloatUnary, float: Float, value: u64) -> u64;
                float_unordered(float: Float, left: u64, right: u64) -> u64;
                float_constant(float: Float, bits: u64) -> u64;
                select(flag: u64, taken: u64, other: u64) -> u64;
                truncation_overflows(truncation: Truncation, value: u64) -> u64;
                convert(operator: ScalarConversionOperator, value: u64) -> u64;
                trap_when(flag: u64, trap: WasmTrap) -> Result<(), WasmTrap>;
            }
        }
    )+};
}

scalar_stage!(
    ConcreteInstance<'_>,
    ConcreteTable<'_>,
    ConcreteBytes<'_>,
    ConcreteMemory<'_>
);

impl WasmTable for ConcreteTable<'_> {
    fn table_range_exceeds(&mut self, index: u64, extent: u64) -> u64 {
        range_past(self.elements.len(), index, extent)
    }

    fn element_signature(&mut self, index: u64) -> u64 {
        self.targets[index as usize].signature.word()
    }

    fn table_element(&mut self, index: u64) -> u64 {
        self.elements[index as usize].to_bits()
    }

    /// Equal ids are equivalent types; the closure knows the expected type, which it closes over.
    fn signature_accepts(&mut self, reference: u64, signature: u64, expected: u64) -> u64 {
        u64::from(signature == expected || (self.related)(Value::from_bits(reference)))
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
    let raw = u64::from_le_bytes(word);
    let unused = u64::BITS - access.bytes as u32 * u8::BITS;
    let value = if access.signed {
        ((raw << unused) as i64) >> unused
    } else {
        raw as i64
    };
    match Width::of(access.ty) {
        Some(width) => ConcreteWasm.constant(width, value),
        None => raw,
    }
}

/// Writes the low bytes of `value` as the checked `access` at `index + offset`.
#[inline(always)]
fn store_word(memory: &mut [u8], access: Access, index: u64, offset: u32, value: u64) {
    let bytes = &value.to_le_bytes()[..access.bytes];
    memory[access_bytes(access, index, offset)].copy_from_slice(bytes);
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
