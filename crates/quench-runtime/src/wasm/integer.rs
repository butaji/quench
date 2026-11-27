//! Integer rules have one declaration; width-specific selectors and execution
//! are macro-derived. Selector order is the residual ABI, not a Wasm opcode.

// The runtime's list macros come from the crate root (`lib.rs`); a stencil build mounts its own.
#[cfg(stencil_build)]
#[macro_use]
#[path = "../staged/wasm_lists.rs"]
mod wasm_lists;

use super::WasmTrap;
#[cfg(not(stencil_build))]
use super::WasmValue;

#[cfg(test)]
use super::numeric::NumericResult;
use super::numeric::selectors;
#[cfg(not(stencil_build))]
use crate::staged::ConcreteWasm;
#[cfg(not(stencil_build))]
use crate::staged::machine::MachineStage;
#[cfg(not(stencil_build))]
use crate::staged::wasm::{Width, binary_result, integer_binary, unary_result};

macro_rules! binary_family {
    ($(@ $decoded:ident)? $enum:ident, $signed:ty, $unsigned:ty, $variant:ident,
     ($signed_alias:ident, $unsigned_alias:ident), ($($argument:ident),+);
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        selectors!($(@ $decoded)? $enum; $($name, $wasm;)+);
        impl $enum {
            pub(crate) fn allowed_in_constant_expression(self) -> bool {
                matches!(self, Self::Add | Self::Subtract | Self::Multiply)
            }

            /// The operator written out on its own: the oracle the staged rules are tested
            /// against (`staged::wasm_tests`); execution goes through the staged rules (`evaluate`).
            #[cfg(test)]
            #[inline(always)] pub(crate) fn apply(self, $($argument: $signed),+) -> Result<WasmValue, WasmTrap> {
                self.result($($argument),+).map(|value| value.into_wasm(WasmValue::$variant))
            }

            #[cfg(test)]
            #[inline(always)]
            fn result(self, $($argument: $signed),+) -> Result<NumericResult<$signed>, WasmTrap> {
                type $signed_alias = $signed;
                type $unsigned_alias = $unsigned;
                match self { $(Self::$name => $body,)+ }
            }
        }
    };
}

/// Each operator is declared once, with its rule and, in brackets, the machine row it is
/// (`staged::machine::MachineOperator`), in `i64_binary_list!` (`staged/wasm_lists.rs`): the Cranelift
/// tier's operator rows are generated from these declarations (`I64BinaryOperator::machine`).
macro_rules! integer_binary_operators {
    ($aliases:tt, $arguments:tt; $($name:ident, $i32:ident, $i64:ident [$($machine:tt)+] => $body:expr;)+
     @i64 $($wide_name:ident, $wide_wasm:ident [$($wide_machine:tt)+] => $wide_body:expr;)+) => {
        #[cfg(not(stencil_build))]
        impl I64BinaryOperator {
            /// The machine row this operator is, from its declaration.
            pub const fn machine(self) -> crate::staged::machine::MachineOperator {
                #[allow(unused_imports)]
                use crate::staged::machine::{Binary::*, MachineOperator::*, Relation::*};
                match self { $(Self::$name => $($machine)+,)+ $(Self::$wide_name => $($wide_machine)+,)+ }
            }
        }
        binary_family!(I32BinaryOperator, i32, u32, I32, $aliases, $arguments; $($name, $i32 => $body;)+);
        binary_family!(@decoded I64BinaryOperator, i64, u64, I64, $aliases, $arguments; $($name, $i64 => $body;)+ $($wide_name, $wide_wasm => $wide_body;)+);
    };
}

macro_rules! unary_family {
    ($enum:ident, $signed:ty, $variant:ident, $signed_alias:ident, $argument:ident;
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        selectors!(@decoded $enum; $($name, $wasm;)+);
        impl $enum {
            #[cfg(test)]
            #[inline(always)] pub(crate) fn apply(self, $argument: $signed) -> WasmValue {
                type $signed_alias = $signed;
                let result: NumericResult<$signed> = match self { $(Self::$name => $body,)+ };
                result.into_wasm(WasmValue::$variant)
            }
        }
    };
}

macro_rules! integer_unary_operators {
    ($signed:ident, $argument:ident;
     $($name:ident, $i32:ident, $i64:ident => $body:expr;)+
     @i64 $($wide_name:ident, $wide_wasm:ident => $wide_body:expr;)+) => {
        selectors!(@decoded I32UnaryOperator; $($name, $i32;)+);
        unary_family!(I64UnaryOperator, i64, I64, $signed, $argument; $($name, $i64 => $body;)+ $($wide_name, $wide_wasm => $wide_body;)+);
    };
}

i64_binary_list!(integer_binary_operators);

i64_unary_list!(integer_unary_operators);

/// Wide arithmetic has its own multi-register lowering ahead of scalar operators.
#[cfg(not(stencil_build))]
pub(super) fn wide_integer(op: &wasmparser::Operator<'_>) -> bool {
    matches!(
        op,
        wasmparser::Operator::I64Add128
            | wasmparser::Operator::I64Sub128
            | wasmparser::Operator::I64MulWideS
            | wasmparser::Operator::I64MulWideU
    )
}

#[cfg(not(stencil_build))]
impl super::Lowering<'_> {
    /// Wide results are two ordinary stack values, not a separate numeric payload.
    pub(super) fn wide_integer_operator(
        &mut self,
        op: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        use crate::staged::wasm_emit;
        use I64BinaryOperator as Binary;
        let (arithmetic, product_high) = match op {
            wasmparser::Operator::I64Add128 => (Binary::Add, None),
            wasmparser::Operator::I64Sub128 => (Binary::Subtract, None),
            wasmparser::Operator::I64MulWideS => {
                (Binary::Multiply, Some(Binary::MultiplyHighSigned))
            }
            wasmparser::Operator::I64MulWideU => {
                (Binary::Multiply, Some(Binary::MultiplyHighUnsigned))
            }
            _ => return Ok(false),
        };
        if self.path == super::Reachability::Dead {
            return Ok(true);
        }
        if let Some(product_high) = product_high {
            let right = self.pop()?;
            let left = self.pop()?;
            let depth = self.depth;
            self.depth = right + 1;
            let high = self.push()?;
            self.emit_emission(wasm_emit::i64_binary(product_high, high, left, right))?;
            self.emit_emission(wasm_emit::i64_binary(arithmetic, left, left, right))?;
            self.emit_emission(wasm_emit::move_register(right, high))?;
            self.depth = depth;
        } else {
            let right_high = self.pop()?;
            let right_low = self.pop()?;
            let left_high = self.pop()?;
            let left_low = self.pop()?;
            let depth = self.depth;
            self.depth = right_high + 1;
            let low = self.push()?;
            let carry = self.push()?;
            let high = self.push()?;
            self.emit_emission(wasm_emit::i64_binary(arithmetic, low, left_low, right_low))?;
            let (compare_left, compare_right) = if arithmetic == Binary::Add {
                (low, left_low)
            } else {
                (left_low, right_low)
            };
            self.emit_emission(wasm_emit::i64_binary(
                Binary::LessUnsigned,
                carry,
                compare_left,
                compare_right,
            ))?;
            self.emit_emission(wasm_emit::scalar_convert(
                carry,
                carry,
                super::conversion::ScalarConversionOperator::ExtendI32Unsigned,
            ))?;
            self.emit_emission(wasm_emit::i64_binary(arithmetic, high, left_high, right_high))?;
            self.emit_emission(wasm_emit::i64_binary(arithmetic, high, high, carry))?;
            self.emit_emission(wasm_emit::move_register(left_low, low))?;
            self.emit_emission(wasm_emit::move_register(left_high, high))?;
            self.depth = depth;
        }
        self.push()?;
        self.push()?;
        Ok(true)
    }
}

#[cfg(not(stencil_build))]
impl I32BinaryOperator {
    /// `apply` as a raw i32 payload, comparisons yielding 0 or 1: the interpreter's execution
    /// of the operator, through the staged rules (`staged::wasm`).
    #[inline(always)]
    pub fn evaluate(self, left: i32, right: i32) -> Result<i32, WasmTrap> {
        integer_binary(
            &mut ConcreteWasm,
            self.widen(),
            Width::I32,
            u64::from(left as u32),
            u64::from(right as u32),
        )
        .map(|word| word as i32)
    }
}

#[cfg(not(stencil_build))]
impl I64BinaryOperator {
    /// `apply` through the staged rules (`staged::wasm`).
    #[inline(always)]
    pub(crate) fn evaluate(self, left: i64, right: i64) -> Result<WasmValue, WasmTrap> {
        let word = integer_binary(
            &mut ConcreteWasm,
            self,
            Width::I64,
            left as u64,
            right as u64,
        )?;
        Ok(match binary_result(self, Width::I64) {
            Width::I32 => WasmValue::I32(word as i32),
            Width::I64 => WasmValue::I64(word as i64),
        })
    }
}

impl I32UnaryOperator {
    /// `apply` as a raw i32 payload, through the staged rules (`staged::wasm`).
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub(crate) fn evaluate(self, value: i32) -> i32 {
        ConcreteWasm.operate_unary(self.widen(), Width::I32, u64::from(value as u32)) as i32
    }

    /// The i64 operator of the same name: the i64 selectors extend the i32 ones.
    pub(crate) const fn widen(self) -> I64UnaryOperator {
        match self {
            Self::EqualZero => I64UnaryOperator::EqualZero,
            Self::LeadingZeros => I64UnaryOperator::LeadingZeros,
            Self::TrailingZeros => I64UnaryOperator::TrailingZeros,
            Self::PopulationCount => I64UnaryOperator::PopulationCount,
            Self::ExtendSigned8 => I64UnaryOperator::ExtendSigned8,
            Self::ExtendSigned16 => I64UnaryOperator::ExtendSigned16,
        }
    }
}

#[cfg(not(stencil_build))]
impl I64UnaryOperator {
    /// `apply` through the staged rules (`staged::wasm`).
    #[inline(always)]
    pub(crate) fn evaluate(self, value: i64) -> WasmValue {
        let word = ConcreteWasm.operate_unary(self, Width::I64, value as u64);
        match unary_result(self, Width::I64) {
            Width::I32 => WasmValue::I32(word as i32),
            Width::I64 => WasmValue::I64(word as i64),
        }
    }
}

/// A bit-field extraction: `(value >>> shift) & mask`, the shared semantics of
/// the fused `WasmI32ShiftRightUnsignedAndImmediate`.
#[cfg(not(stencil_build))]
#[inline(always)]
pub(crate) fn shift_right_unsigned_and(value: i32, shift: i32, mask: i32) -> i32 {
    let shifted = I32BinaryOperator::ShiftRightUnsigned
        .evaluate(value, shift)
        .expect("shifts do not trap");
    I32BinaryOperator::And
        .evaluate(shifted, mask)
        .expect("and does not trap")
}
