//! Integer rules have one declaration; width-specific selectors and execution
//! are macro-derived. Selector order is the residual ABI, not a Wasm opcode.

use super::{WasmTrap, WasmValue};

use super::numeric::{NumericResult, selectors};

macro_rules! binary_family {
    ($enum:ident, $signed:ty, $unsigned:ty, $variant:ident,
     ($signed_alias:ident, $unsigned_alias:ident), ($($argument:ident),+);
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        selectors!($enum; $($name, $wasm;)+);
        impl $enum {
            pub(crate) fn allowed_in_constant_expression(self) -> bool {
                matches!(self, Self::Add | Self::Subtract | Self::Multiply)
            }

            pub(crate) fn apply(self, $($argument: $signed),+) -> Result<WasmValue, WasmTrap> {
                type $signed_alias = $signed;
                type $unsigned_alias = $unsigned;
                let result: Result<NumericResult<$signed>, WasmTrap> = match self { $(Self::$name => $body,)+ };
                result.map(|value| value.into_wasm(WasmValue::$variant))
            }
        }
    };
}

macro_rules! integer_binary_operators {
    ($aliases:tt, $arguments:tt; $($name:ident, $i32:ident, $i64:ident => $body:expr;)+
     @i64 $($wide_name:ident, $wide_wasm:ident => $wide_body:expr;)+) => {
        binary_family!(I32BinaryOperator, i32, u32, I32, $aliases, $arguments; $($name, $i32 => $body;)+);
        binary_family!(I64BinaryOperator, i64, u64, I64, $aliases, $arguments; $($name, $i64 => $body;)+ $($wide_name, $wide_wasm => $wide_body;)+);
    };
}

macro_rules! unary_family {
    ($enum:ident, $signed:ty, $variant:ident, $signed_alias:ident, $argument:ident;
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        selectors!($enum; $($name, $wasm;)+);
        impl $enum {
            pub(crate) fn apply(self, $argument: $signed) -> WasmValue {
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
        unary_family!(I32UnaryOperator, i32, I32, $signed, $argument; $($name, $i32 => $body;)+);
        unary_family!(I64UnaryOperator, i64, I64, $signed, $argument; $($name, $i64 => $body;)+ $($wide_name, $wide_wasm => $wide_body;)+);
    };
}

integer_binary_operators! { (Signed, Unsigned), (left, right);
    Add, I32Add, I64Add => Ok(NumericResult::Value(left.wrapping_add(right)));
    Subtract, I32Sub, I64Sub => Ok(NumericResult::Value(left.wrapping_sub(right)));
    Multiply, I32Mul, I64Mul => Ok(NumericResult::Value(left.wrapping_mul(right)));
    DivideSigned, I32DivS, I64DivS => {
        if right == 0 { Err(WasmTrap::IntegerDivideByZero) }
        else { left.checked_div(right).map(NumericResult::Value).ok_or(WasmTrap::IntegerOverflow) }
    };
    DivideUnsigned, I32DivU, I64DivU => {
        (left as Unsigned).checked_div(right as Unsigned).map(|v| NumericResult::Value(v as Signed))
            .ok_or(WasmTrap::IntegerDivideByZero)
    };
    RemainderSigned, I32RemS, I64RemS => {
        if right == 0 { Err(WasmTrap::IntegerDivideByZero) }
        else { Ok(NumericResult::Value(left.wrapping_rem(right))) }
    };
    RemainderUnsigned, I32RemU, I64RemU => {
        (left as Unsigned).checked_rem(right as Unsigned).map(|v| NumericResult::Value(v as Signed))
            .ok_or(WasmTrap::IntegerDivideByZero)
    };
    And, I32And, I64And => Ok(NumericResult::Value(left & right));
    Or, I32Or, I64Or => Ok(NumericResult::Value(left | right));
    Xor, I32Xor, I64Xor => Ok(NumericResult::Value(left ^ right));
    ShiftLeft, I32Shl, I64Shl => Ok(NumericResult::Value(left.wrapping_shl(right as u32)));
    ShiftRightSigned, I32ShrS, I64ShrS => Ok(NumericResult::Value(left.wrapping_shr(right as u32)));
    ShiftRightUnsigned, I32ShrU, I64ShrU => Ok(NumericResult::Value((left as Unsigned).wrapping_shr(right as u32) as Signed));
    RotateLeft, I32Rotl, I64Rotl => Ok(NumericResult::Value(left.rotate_left(right as u32)));
    RotateRight, I32Rotr, I64Rotr => Ok(NumericResult::Value(left.rotate_right(right as u32)));
    Equal, I32Eq, I64Eq => Ok(NumericResult::Comparison(left == right));
    NotEqual, I32Ne, I64Ne => Ok(NumericResult::Comparison(left != right));
    LessSigned, I32LtS, I64LtS => Ok(NumericResult::Comparison(left < right));
    LessUnsigned, I32LtU, I64LtU => Ok(NumericResult::Comparison((left as Unsigned) < (right as Unsigned)));
    GreaterSigned, I32GtS, I64GtS => Ok(NumericResult::Comparison(left > right));
    GreaterUnsigned, I32GtU, I64GtU => Ok(NumericResult::Comparison((left as Unsigned) > (right as Unsigned)));
    LessEqualSigned, I32LeS, I64LeS => Ok(NumericResult::Comparison(left <= right));
    LessEqualUnsigned, I32LeU, I64LeU => Ok(NumericResult::Comparison((left as Unsigned) <= (right as Unsigned)));
    GreaterEqualSigned, I32GeS, I64GeS => Ok(NumericResult::Comparison(left >= right));
    GreaterEqualUnsigned, I32GeU, I64GeU => Ok(NumericResult::Comparison((left as Unsigned) >= (right as Unsigned)));
    @i64
    MultiplyHighSigned, I64MulWideS => Ok(NumericResult::Value(((i128::from(left) * i128::from(right)) >> i64::BITS) as i64));
    MultiplyHighUnsigned, I64MulWideU => Ok(NumericResult::Value(((u128::from(left as u64) * u128::from(right as u64)) >> u64::BITS) as i64));
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
pub(super) fn wide_integer(op: &wasmparser::Operator<'_>) -> bool {
    matches!(
        op,
        wasmparser::Operator::I64Add128
            | wasmparser::Operator::I64Sub128
            | wasmparser::Operator::I64MulWideS
            | wasmparser::Operator::I64MulWideU
    )
}

impl super::Lowering<'_> {
    /// Wide results are two ordinary stack values, not a separate numeric payload.
    pub(super) fn wide_integer_operator(
        &mut self,
        op: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        use crate::bytecode::Op;
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
            self.emit(Op::WasmI64Binary, high, left, right, product_high as u32)?;
            self.emit(Op::WasmI64Binary, left, left, right, arithmetic as u32)?;
            self.emit(Op::Move, right, high, 0, 0)?;
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
            self.emit(
                Op::WasmI64Binary,
                low,
                left_low,
                right_low,
                arithmetic as u32,
            )?;
            let (compare_left, compare_right) = if arithmetic == Binary::Add {
                (low, left_low)
            } else {
                (left_low, right_low)
            };
            self.emit(
                Op::WasmI64Binary,
                carry,
                compare_left,
                compare_right,
                Binary::LessUnsigned as u32,
            )?;
            self.emit(
                Op::WasmScalarConvert,
                carry,
                carry,
                0,
                super::conversion::ScalarConversionOperator::ExtendI32Unsigned as u32,
            )?;
            self.emit(
                Op::WasmI64Binary,
                high,
                left_high,
                right_high,
                arithmetic as u32,
            )?;
            self.emit(Op::WasmI64Binary, high, high, carry, arithmetic as u32)?;
            self.emit(Op::Move, left_low, low, 0, 0)?;
            self.emit(Op::Move, left_high, high, 0, 0)?;
            self.depth = depth;
        }
        self.push()?;
        self.push()?;
        Ok(true)
    }
}
