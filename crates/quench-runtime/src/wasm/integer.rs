//! Integer rules have one declaration; width-specific selectors and execution
//! are macro-derived. Selector order is the residual ABI, not a Wasm opcode.

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
    ($enum:ident, $signed:ty, $unsigned:ty, $variant:ident,
     ($signed_alias:ident, $unsigned_alias:ident), ($($argument:ident),+);
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        selectors!($enum; $($name, $wasm;)+);
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
/// (`staged::machine::MachineOperator`): the Cranelift tier's operator rows are generated from
/// these declarations (`I64BinaryOperator::machine`), not listed again beside the backend.
macro_rules! integer_binary_operators {
    ($aliases:tt, $arguments:tt; $($name:ident, $i32:ident, $i64:ident [$($machine:tt)+] => $body:expr;)+
     @i64 $($wide_name:ident, $wide_wasm:ident [$($wide_machine:tt)+] => $wide_body:expr;)+) => {
        binary_family!(I32BinaryOperator, i32, u32, I32, $aliases, $arguments; $($name, $i32 => $body;)+);
        binary_family!(I64BinaryOperator, i64, u64, I64, $aliases, $arguments; $($name, $i64 => $body;)+ $($wide_name, $wide_wasm => $wide_body;)+);
        #[cfg(not(stencil_build))]
        impl I64BinaryOperator {
            /// The machine row this operator is, from its declaration.
            pub(crate) const fn machine(self) -> crate::staged::machine::MachineOperator {
                #[allow(unused_imports)]
                use crate::staged::machine::{Binary::*, MachineOperator::*, Relation::*};
                match self { $(Self::$name => $($machine)+,)+ $(Self::$wide_name => $($wide_machine)+,)+ }
            }
        }
    };
}

macro_rules! unary_family {
    ($enum:ident, $signed:ty, $variant:ident, $signed_alias:ident, $argument:ident;
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        selectors!($enum; $($name, $wasm;)+);
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
        selectors!(I32UnaryOperator; $($name, $i32;)+);
        unary_family!(I64UnaryOperator, i64, I64, $signed, $argument; $($name, $i64 => $body;)+ $($wide_name, $wide_wasm => $wide_body;)+);
    };
}

integer_binary_operators! { (Signed, Unsigned), (left, right);
    Add, I32Add, I64Add [Binary(Add)] => Ok(NumericResult::Value(left.wrapping_add(right)));
    Subtract, I32Sub, I64Sub [Binary(Sub)] => Ok(NumericResult::Value(left.wrapping_sub(right)));
    Multiply, I32Mul, I64Mul [Binary(Mul)] => Ok(NumericResult::Value(left.wrapping_mul(right)));
    DivideSigned, I32DivS, I64DivS [Binary(DivideSigned)] => {
        if right == 0 { Err(WasmTrap::IntegerDivideByZero) }
        else { left.checked_div(right).map(NumericResult::Value).ok_or(WasmTrap::IntegerOverflow) }
    };
    DivideUnsigned, I32DivU, I64DivU [Binary(DivideUnsigned)] => {
        (left as Unsigned).checked_div(right as Unsigned).map(|v| NumericResult::Value(v as Signed))
            .ok_or(WasmTrap::IntegerDivideByZero)
    };
    RemainderSigned, I32RemS, I64RemS [SignedRemainder] => {
        if right == 0 { Err(WasmTrap::IntegerDivideByZero) }
        else { Ok(NumericResult::Value(left.wrapping_rem(right))) }
    };
    RemainderUnsigned, I32RemU, I64RemU [Binary(RemainderUnsigned)] => {
        (left as Unsigned).checked_rem(right as Unsigned).map(|v| NumericResult::Value(v as Signed))
            .ok_or(WasmTrap::IntegerDivideByZero)
    };
    And, I32And, I64And [Binary(And)] => Ok(NumericResult::Value(left & right));
    Or, I32Or, I64Or [Binary(Or)] => Ok(NumericResult::Value(left | right));
    Xor, I32Xor, I64Xor [Binary(Xor)] => Ok(NumericResult::Value(left ^ right));
    ShiftLeft, I32Shl, I64Shl [Binary(ShiftLeft)] => Ok(NumericResult::Value(left.wrapping_shl(right as u32)));
    ShiftRightSigned, I32ShrS, I64ShrS [Binary(ShiftRightSigned)] => Ok(NumericResult::Value(left.wrapping_shr(right as u32)));
    ShiftRightUnsigned, I32ShrU, I64ShrU [Binary(ShiftRightUnsigned)] => Ok(NumericResult::Value((left as Unsigned).wrapping_shr(right as u32) as Signed));
    RotateLeft, I32Rotl, I64Rotl [Binary(RotateLeft)] => Ok(NumericResult::Value(left.rotate_left(right as u32)));
    RotateRight, I32Rotr, I64Rotr [Binary(RotateRight)] => Ok(NumericResult::Value(left.rotate_right(right as u32)));
    Equal, I32Eq, I64Eq [Relation(Equal)] => Ok(NumericResult::Comparison(left == right));
    NotEqual, I32Ne, I64Ne [Relation(NotEqual)] => Ok(NumericResult::Comparison(left != right));
    LessSigned, I32LtS, I64LtS [Relation(SignedLess)] => Ok(NumericResult::Comparison(left < right));
    LessUnsigned, I32LtU, I64LtU [Relation(UnsignedLess)] => Ok(NumericResult::Comparison((left as Unsigned) < (right as Unsigned)));
    GreaterSigned, I32GtS, I64GtS [Relation(SignedGreater)] => Ok(NumericResult::Comparison(left > right));
    GreaterUnsigned, I32GtU, I64GtU [Relation(UnsignedGreater)] => Ok(NumericResult::Comparison((left as Unsigned) > (right as Unsigned)));
    LessEqualSigned, I32LeS, I64LeS [Relation(SignedLessEqual)] => Ok(NumericResult::Comparison(left <= right));
    LessEqualUnsigned, I32LeU, I64LeU [Relation(UnsignedLessEqual)] => Ok(NumericResult::Comparison((left as Unsigned) <= (right as Unsigned)));
    GreaterEqualSigned, I32GeS, I64GeS [Relation(SignedGreaterEqual)] => Ok(NumericResult::Comparison(left >= right));
    GreaterEqualUnsigned, I32GeU, I64GeU [Relation(UnsignedGreaterEqual)] => Ok(NumericResult::Comparison((left as Unsigned) >= (right as Unsigned)));
    @i64
    MultiplyHighSigned, I64MulWideS [Binary(MulHighSigned)] => Ok(NumericResult::Value(((i128::from(left) * i128::from(right)) >> i64::BITS) as i64));
    MultiplyHighUnsigned, I64MulWideU [Binary(MulHighUnsigned)] => Ok(NumericResult::Value(((u128::from(left as u64) * u128::from(right as u64)) >> u64::BITS) as i64));
}

integer_unary_operators! { Signed, value;
    EqualZero, I32Eqz, I64Eqz => NumericResult::Comparison(value == 0);
    LeadingZeros, I32Clz, I64Clz => NumericResult::Value(value.leading_zeros() as Signed);
    TrailingZeros, I32Ctz, I64Ctz => NumericResult::Value(value.trailing_zeros() as Signed);
    PopulationCount, I32Popcnt, I64Popcnt => NumericResult::Value(value.count_ones() as Signed);
    ExtendSigned8, I32Extend8S, I64Extend8S => NumericResult::Value(value as i8 as Signed);
    ExtendSigned16, I32Extend16S, I64Extend16S => NumericResult::Value(value as i16 as Signed);
    @i64 ExtendSigned32, I64Extend32S => NumericResult::Value(value as i32 as Signed);
}

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
    pub(crate) fn evaluate(self, left: i32, right: i32) -> Result<i32, WasmTrap> {
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
