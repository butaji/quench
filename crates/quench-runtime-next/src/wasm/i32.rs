//! Typed i32 operators. Each row owns its decoder selector and semantics;
//! lowering, residual validation and execution derive their views from it.

use super::WasmTrap;
use wasmparser::Operator;

// Declaration order is the residual selector ABI. FORMAT_VERSION changes
// whenever this table changes; selectors are never Wasm binary opcode tags.
macro_rules! operators {
    ($enum:ident, ($($argument:ident),+), $result:ty;
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub(crate) enum $enum { $($name,)+ }

        impl $enum {
            const ALL: &'static [Self] = &[$(Self::$name,)+];

            pub(crate) fn from_tag(tag: u32) -> Option<Self> {
                Self::ALL.get(tag as usize).copied()
            }

            pub(crate) fn from_wasm(operator: &Operator<'_>) -> Option<Self> {
                match operator {
                    $(Operator::$wasm => Some(Self::$name),)+
                    _ => None,
                }
            }

            pub(crate) fn apply(self, $($argument: i32),+) -> $result {
                match self { $(Self::$name => $body,)+ }
            }
        }
    };
}

operators! { I32BinaryOperator, (left, right), Result<i32, WasmTrap>;
    Add, I32Add => Ok(left.wrapping_add(right));
    Subtract, I32Sub => Ok(left.wrapping_sub(right));
    Multiply, I32Mul => Ok(left.wrapping_mul(right));
    DivideSigned, I32DivS => {
        if right == 0 { Err(WasmTrap::IntegerDivideByZero) }
        else { left.checked_div(right).ok_or(WasmTrap::IntegerOverflow) }
    };
    DivideUnsigned, I32DivU => {
        (left as u32).checked_div(right as u32).map(|v| v as i32)
            .ok_or(WasmTrap::IntegerDivideByZero)
    };
    RemainderSigned, I32RemS => {
        if right == 0 { Err(WasmTrap::IntegerDivideByZero) }
        else { Ok(left.wrapping_rem(right)) }
    };
    RemainderUnsigned, I32RemU => {
        (left as u32).checked_rem(right as u32).map(|v| v as i32)
            .ok_or(WasmTrap::IntegerDivideByZero)
    };
    And, I32And => Ok(left & right);
    Or, I32Or => Ok(left | right);
    Xor, I32Xor => Ok(left ^ right);
    ShiftLeft, I32Shl => Ok(left.wrapping_shl(right as u32));
    ShiftRightSigned, I32ShrS => Ok(left.wrapping_shr(right as u32));
    ShiftRightUnsigned, I32ShrU => Ok((left as u32).wrapping_shr(right as u32) as i32);
    RotateLeft, I32Rotl => Ok(left.rotate_left(right as u32));
    RotateRight, I32Rotr => Ok(left.rotate_right(right as u32));
    Equal, I32Eq => Ok(i32::from(left == right));
    NotEqual, I32Ne => Ok(i32::from(left != right));
    LessSigned, I32LtS => Ok(i32::from(left < right));
    LessUnsigned, I32LtU => Ok(i32::from((left as u32) < (right as u32)));
    GreaterSigned, I32GtS => Ok(i32::from(left > right));
    GreaterUnsigned, I32GtU => Ok(i32::from((left as u32) > (right as u32)));
    LessEqualSigned, I32LeS => Ok(i32::from(left <= right));
    LessEqualUnsigned, I32LeU => Ok(i32::from((left as u32) <= (right as u32)));
    GreaterEqualSigned, I32GeS => Ok(i32::from(left >= right));
    GreaterEqualUnsigned, I32GeU => Ok(i32::from((left as u32) >= (right as u32)));
}

operators! { I32UnaryOperator, (value), i32;
    EqualZero, I32Eqz => i32::from(value == 0);
    LeadingZeros, I32Clz => value.leading_zeros() as i32;
    TrailingZeros, I32Ctz => value.trailing_zeros() as i32;
    PopulationCount, I32Popcnt => value.count_ones() as i32;
    ExtendSigned8, I32Extend8S => value as i8 as i32;
    ExtendSigned16, I32Extend16S => value as i16 as i32;
}
