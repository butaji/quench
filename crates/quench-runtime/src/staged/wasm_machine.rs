//! The interpreter's stages over the state of an instance: the scalar rules of `ConcreteWasm`
//! plus a table's primitives (`ConcreteTable`, which the VM resolves indirect calls on) and
//! linear memory over borrowed bytes (`ConcreteBytes` reads, `ConcreteMemory` reads and
//! writes: the access and bound rules of `MemoryLoad::read` and `MemoryStore::write`). For the
//! tests that check the Wasm tier's instance primitives against the declared semantics,
//! `ConcreteInstance` adds the globals over borrowed words.

use super::ConcreteWasm;
use super::wasm::{WasmStage, Width};
use super::wasm_call::WasmTable;
use super::wasm_float::{Float, Truncation};
#[cfg(test)]
use super::wasm_instance::WasmInstance;
use super::wasm_memory::{Access, WasmMemory, WasmMemoryWrite};
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

/// A stage over an instance: its globals by environment slot and its linear memory.
#[cfg(test)]
pub(crate) struct ConcreteInstance<'a> {
    pub(crate) globals: &'a mut [u64],
    pub(crate) memory: &'a mut [u8],
}

/// A stage over a table of function references; `matches` says whether a reference's function
/// has the expected signature type.
pub(crate) struct ConcreteTable<'a> {
    pub(crate) elements: &'a [Value],
    pub(crate) matches: &'a mut dyn FnMut(Value) -> bool,
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
        impl WasmStage for $stage {
            type Word = u64;
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

#[cfg(test)]
scalar_stage!(ConcreteInstance<'_>);
scalar_stage!(ConcreteTable<'_>, ConcreteBytes<'_>, ConcreteMemory<'_>);

impl WasmTable for ConcreteTable<'_> {
    fn table_length(&mut self) -> u64 {
        self.elements.len() as u64
    }

    fn table_element(&mut self, index: u64) -> u64 {
        self.elements[index as usize].to_bits()
    }

    fn reference_is_null(&mut self, reference: u64) -> u64 {
        u64::from(Value::from_bits(reference).is_null())
    }

    /// The signature type is the closure's to know: it closes over the expected one.
    fn reference_matches(&mut self, reference: u64, _: u32) -> u64 {
        u64::from((self.matches)(Value::from_bits(reference)))
    }
}

#[cfg(test)]
impl WasmInstance for ConcreteInstance<'_> {
    fn global_load(&mut self, slot: u32) -> u64 {
        u64::from(self.globals[slot as usize] as u32)
    }

    fn global_store(&mut self, slot: u32, value: u64) {
        self.globals[slot as usize] = u64::from(value as u32);
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

/// The flag that `extent` bytes from `index` pass the end of `memory`: the shared range check
/// of memories, tables and bulk operations.
#[inline(always)]
fn range_past(memory: &[u8], index: u64, extent: u64) -> u64 {
    let inside = usize::try_from(extent)
        .is_ok_and(|extent| checked_range(index, extent, memory.len()).is_ok());
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
                range_past(self.memory, index, extent)
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

readable_memory!(ConcreteBytes<'_>, ConcreteMemory<'_>);
writable_memory!(ConcreteMemory<'_>);
#[cfg(test)]
readable_memory!(ConcreteInstance<'_>);
#[cfg(test)]
writable_memory!(ConcreteInstance<'_>);
