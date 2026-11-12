//! The interpreter's stage over the state of an instance: the scalar rules of `ConcreteWasm`
//! plus the instance primitives (`WasmInstance`, `WasmMemory`) over borrowed words and bytes.

use super::ConcreteWasm;
use super::wasm::{WasmStage, Width};
use super::wasm_call::WasmTable;
use super::wasm_float::{Float, Truncation};
use super::wasm_instance::WasmInstance;
use super::wasm_memory::{Access, WasmMemory};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use crate::{Value, WasmTrap};

/// A stage over an instance: its globals by environment slot and its linear memory.
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

scalar_stage!(ConcreteInstance<'_>, ConcreteTable<'_>);

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

impl WasmInstance for ConcreteInstance<'_> {
    fn global_load(&mut self, slot: u32) -> u64 {
        u64::from(self.globals[slot as usize] as u32)
    }

    fn global_store(&mut self, slot: u32, value: u64) {
        self.globals[slot as usize] = u64::from(value as u32);
    }
}

impl ConcreteInstance<'_> {
    /// The bytes of the checked access at `index + offset`.
    fn bytes(&self, access: Access, index: u64, offset: u32) -> std::ops::Range<usize> {
        let start = (index + u64::from(offset)) as usize;
        start..start + access.bytes
    }
}

impl WasmMemory for ConcreteInstance<'_> {
    fn guarded(&self) -> bool {
        false
    }

    fn memory_length(&mut self) -> u64 {
        self.memory.len() as u64
    }

    /// A concrete 32-bit word is already its zero-extended index.
    fn memory_index(&mut self, address: u64) -> u64 {
        address
    }

    fn memory_load(&mut self, access: Access, index: u64, offset: u32) -> u64 {
        let mut word = [0u8; size_of::<u64>()];
        word[..access.bytes].copy_from_slice(&self.memory[self.bytes(access, index, offset)]);
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

    fn memory_store(&mut self, access: Access, index: u64, offset: u32, value: u64) {
        let bytes = self.bytes(access, index, offset);
        self.memory[bytes].copy_from_slice(&value.to_le_bytes()[..access.bytes]);
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
