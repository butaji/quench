//! Integer rules have one declaration; width-specific selectors and execution
//! are macro-derived. Selector order is the residual ABI, not a Wasm opcode.

use super::{WasmTrap, WasmValue};
use wasmparser::Operator;

enum IntegerResult<T> {
    Value(T),
    Comparison(bool),
}

impl<T> IntegerResult<T> {
    fn into_wasm(self, integer: impl FnOnce(T) -> WasmValue) -> WasmValue {
        match self {
            Self::Value(value) => integer(value),
            Self::Comparison(value) => WasmValue::I32(i32::from(value)),
        }
    }
}

macro_rules! selectors {
    ($enum:ident; $($name:ident, $wasm:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub(crate) enum $enum { $($name,)+ }
        impl $enum {
            const ALL: &'static [Self] = &[$(Self::$name,)+];
            pub(crate) fn from_tag(tag: u32) -> Option<Self> { Self::ALL.get(tag as usize).copied() }
            pub(crate) fn from_wasm(operator: &Operator<'_>) -> Option<Self> {
                match operator { $(Operator::$wasm => Some(Self::$name),)+ _ => None }
            }
        }
    };
}

macro_rules! binary_family {
    ($enum:ident, $signed:ty, $unsigned:ty, $variant:ident,
     ($signed_alias:ident, $unsigned_alias:ident), ($($argument:ident),+);
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        selectors!($enum; $($name, $wasm;)+);
        impl $enum {
            pub(crate) fn apply(self, $($argument: $signed),+) -> Result<WasmValue, WasmTrap> {
                type $signed_alias = $signed;
                type $unsigned_alias = $unsigned;
                let result: Result<IntegerResult<$signed>, WasmTrap> = match self { $(Self::$name => $body,)+ };
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
                let result: IntegerResult<$signed> = match self { $(Self::$name => $body,)+ };
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
    Add, I32Add, I64Add => Ok(IntegerResult::Value(left.wrapping_add(right)));
    Subtract, I32Sub, I64Sub => Ok(IntegerResult::Value(left.wrapping_sub(right)));
    Multiply, I32Mul, I64Mul => Ok(IntegerResult::Value(left.wrapping_mul(right)));
    DivideSigned, I32DivS, I64DivS => {
        if right == 0 { Err(WasmTrap::IntegerDivideByZero) }
        else { left.checked_div(right).map(IntegerResult::Value).ok_or(WasmTrap::IntegerOverflow) }
    };
    DivideUnsigned, I32DivU, I64DivU => {
        (left as Unsigned).checked_div(right as Unsigned).map(|v| IntegerResult::Value(v as Signed))
            .ok_or(WasmTrap::IntegerDivideByZero)
    };
    RemainderSigned, I32RemS, I64RemS => {
        if right == 0 { Err(WasmTrap::IntegerDivideByZero) }
        else { Ok(IntegerResult::Value(left.wrapping_rem(right))) }
    };
    RemainderUnsigned, I32RemU, I64RemU => {
        (left as Unsigned).checked_rem(right as Unsigned).map(|v| IntegerResult::Value(v as Signed))
            .ok_or(WasmTrap::IntegerDivideByZero)
    };
    And, I32And, I64And => Ok(IntegerResult::Value(left & right));
    Or, I32Or, I64Or => Ok(IntegerResult::Value(left | right));
    Xor, I32Xor, I64Xor => Ok(IntegerResult::Value(left ^ right));
    ShiftLeft, I32Shl, I64Shl => Ok(IntegerResult::Value(left.wrapping_shl(right as u32)));
    ShiftRightSigned, I32ShrS, I64ShrS => Ok(IntegerResult::Value(left.wrapping_shr(right as u32)));
    ShiftRightUnsigned, I32ShrU, I64ShrU => Ok(IntegerResult::Value((left as Unsigned).wrapping_shr(right as u32) as Signed));
    RotateLeft, I32Rotl, I64Rotl => Ok(IntegerResult::Value(left.rotate_left(right as u32)));
    RotateRight, I32Rotr, I64Rotr => Ok(IntegerResult::Value(left.rotate_right(right as u32)));
    Equal, I32Eq, I64Eq => Ok(IntegerResult::Comparison(left == right));
    NotEqual, I32Ne, I64Ne => Ok(IntegerResult::Comparison(left != right));
    LessSigned, I32LtS, I64LtS => Ok(IntegerResult::Comparison(left < right));
    LessUnsigned, I32LtU, I64LtU => Ok(IntegerResult::Comparison((left as Unsigned) < (right as Unsigned)));
    GreaterSigned, I32GtS, I64GtS => Ok(IntegerResult::Comparison(left > right));
    GreaterUnsigned, I32GtU, I64GtU => Ok(IntegerResult::Comparison((left as Unsigned) > (right as Unsigned)));
    LessEqualSigned, I32LeS, I64LeS => Ok(IntegerResult::Comparison(left <= right));
    LessEqualUnsigned, I32LeU, I64LeU => Ok(IntegerResult::Comparison((left as Unsigned) <= (right as Unsigned)));
    GreaterEqualSigned, I32GeS, I64GeS => Ok(IntegerResult::Comparison(left >= right));
    GreaterEqualUnsigned, I32GeU, I64GeU => Ok(IntegerResult::Comparison((left as Unsigned) >= (right as Unsigned)));
}

integer_unary_operators! { Signed, value;
    EqualZero, I32Eqz, I64Eqz => IntegerResult::Comparison(value == 0);
    LeadingZeros, I32Clz, I64Clz => IntegerResult::Value(value.leading_zeros() as Signed);
    TrailingZeros, I32Ctz, I64Ctz => IntegerResult::Value(value.trailing_zeros() as Signed);
    PopulationCount, I32Popcnt, I64Popcnt => IntegerResult::Value(value.count_ones() as Signed);
    ExtendSigned8, I32Extend8S, I64Extend8S => IntegerResult::Value(value as i8 as Signed);
    ExtendSigned16, I32Extend16S, I64Extend16S => IntegerResult::Value(value as i16 as Signed);
    @i64 ExtendSigned32, I64Extend32S => IntegerResult::Value(value as i32 as Signed);
}

macro_rules! integer_conversions {
    ($argument:ident; $($name:ident, $wasm:ident, $source:ident => $body:expr;)+) => {
        selectors!(IntegerConversionOperator; $($name, $wasm;)+);
        impl IntegerConversionOperator {
            pub(crate) fn source_type(self) -> super::WasmType {
                match self { $(Self::$name => super::WasmType::$source,)+ }
            }
            pub(crate) fn apply(self, value: WasmValue) -> Option<WasmValue> {
                match self {
                    $(Self::$name => match value {
                        WasmValue::$source($argument) => Some($body),
                        _ => None,
                    },)+
                }
            }
        }
    };
}

integer_conversions! { value;
    WrapI64, I32WrapI64, I64 => WasmValue::I32(value as i32);
    ExtendI32Signed, I64ExtendI32S, I32 => WasmValue::I64(i64::from(value));
    ExtendI32Unsigned, I64ExtendI32U, I32 => WasmValue::I64(i64::from(value as u32));
}
