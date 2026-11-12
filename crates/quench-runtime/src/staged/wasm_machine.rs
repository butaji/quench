//! The interpreter's stage over the state of an instance: the scalar rules of `ConcreteWasm`
//! plus the instance primitives (`WasmInstance`) over borrowed words and bytes.

use super::ConcreteWasm;
use super::wasm::{WasmStage, Width};
use super::wasm_float::{Float, Truncation};
use super::wasm_instance::WasmInstance;
use crate::WasmTrap;
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};

/// A stage over an instance: its globals by environment slot.
pub(crate) struct ConcreteInstance<'a> {
    pub(crate) globals: &'a mut [u64],
}

/// The scalar primitives are `ConcreteWasm`'s: one definition, delegated.
macro_rules! scalar_stage {
    ($($name:ident($($argument:ident: $ty:ty),*) -> $result:ty;)+) => {$(
        #[inline(always)]
        fn $name(&mut self, $($argument: $ty),*) -> $result {
            ConcreteWasm.$name($($argument),*)
        }
    )+};
}

impl WasmStage for ConcreteInstance<'_> {
    type Word = u64;
    type Exit = WasmTrap;

    scalar_stage! {
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

impl WasmInstance for ConcreteInstance<'_> {
    fn global_load(&mut self, slot: u32) -> u64 {
        u64::from(self.globals[slot as usize] as u32)
    }

    fn global_store(&mut self, slot: u32, value: u64) {
        self.globals[slot as usize] = u64::from(value as u32);
    }
}
