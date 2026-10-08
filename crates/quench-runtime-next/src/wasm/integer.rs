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
    ($aliases:tt, $arguments:tt; $($name:ident, $i32:ident, $i64:ident => $body:expr;)+) => {
        binary_family!(I32BinaryOperator, i32, u32, I32, $aliases, $arguments; $($name, $i32 => $body;)+);
        binary_family!(I64BinaryOperator, i64, u64, I64, $aliases, $arguments; $($name, $i64 => $body;)+);
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
