//! The emission vocabulary of the Wasm lowering: each residual instruction the
//! lowering writes is named here by its meaning. Every emit function returns an
//! `Emission` (opcode, operands and immediate), and the lowering writes it through
//! `Lowering::emit_emission`. The lowering names no opcode itself, and every function
//! returns exactly the operands the lowering wrote before, so the encoding is unchanged.
//!
//! `control` holds control flow, locals, calls, exceptions, arrays and register moves; `numeric`
//! holds the i32, i64 and scalar-conversion operators and the selector families; `data` holds
//! the reference, i31, global, table, memory, atomic and segment emits. The predicates over
//! lowered instructions and the operator-to-opcode tables stay here.

mod control;
mod data;
mod numeric;

pub(crate) use control::*;
pub(crate) use data::*;
pub(crate) use numeric::*;

use crate::bytecode::{Op, Register, WideInstruction};
use crate::wasm::integer::{I32BinaryOperator, I32UnaryOperator, I64BinaryOperator};

/// One instruction a lowering emits: its opcode, its three register operands and its immediate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Emission {
    pub(crate) op: Op,
    pub(crate) a: Register,
    pub(crate) b: Register,
    pub(crate) c: Register,
    pub(crate) imm: u32,
}

impl Emission {
    const fn new(op: Op, a: Register, b: Register, c: Register, imm: u32) -> Self {
        Self { op, a, b, c, imm }
    }
}

// A comparison and the conditional jump taken when it holds, with its negation.
// First-class i32 opcodes name their operator so dispatch decodes no selector.
macro_rules! i32_direct_operators {
    ($($name:ident => $register:ident, $immediate:ident;)+) => {
        impl I32BinaryOperator {
            #[cfg(not(stencil_build))]
            pub(crate) const fn register_op(self) -> Op {
                match self { $(Self::$name => Op::$register,)+ }
            }
            #[cfg(not(stencil_build))]
            pub(crate) const fn immediate_op(self) -> Op {
                match self { $(Self::$name => Op::$immediate,)+ }
            }
            /// The i64 operator of the same name: the i64 selectors extend the i32 ones.
            #[inline]
            pub(crate) const fn widen(self) -> I64BinaryOperator {
                match self { $(Self::$name => I64BinaryOperator::$name,)+ }
            }
            #[cfg(not(stencil_build))]
            pub const fn from_register_op(op: Op) -> Option<Self> {
                match op { $(Op::$register => Some(Self::$name),)+ _ => None }
            }
            #[cfg(not(stencil_build))]
            pub const fn from_immediate_op(op: Op) -> Option<Self> {
                match op { $(Op::$immediate => Some(Self::$name),)+ _ => None }
            }
        }
    };
}
i32_direct_operators! {
    Add => WasmI32Add, WasmI32AddImmediate;
    Subtract => WasmI32Subtract, WasmI32SubtractImmediate;
    Multiply => WasmI32Multiply, WasmI32MultiplyImmediate;
    DivideSigned => WasmI32DivideSigned, WasmI32DivideSignedImmediate;
    DivideUnsigned => WasmI32DivideUnsigned, WasmI32DivideUnsignedImmediate;
    RemainderSigned => WasmI32RemainderSigned, WasmI32RemainderSignedImmediate;
    RemainderUnsigned => WasmI32RemainderUnsigned, WasmI32RemainderUnsignedImmediate;
    And => WasmI32And, WasmI32AndImmediate;
    Or => WasmI32Or, WasmI32OrImmediate;
    Xor => WasmI32Xor, WasmI32XorImmediate;
    ShiftLeft => WasmI32ShiftLeft, WasmI32ShiftLeftImmediate;
    ShiftRightSigned => WasmI32ShiftRightSigned, WasmI32ShiftRightSignedImmediate;
    ShiftRightUnsigned => WasmI32ShiftRightUnsigned, WasmI32ShiftRightUnsignedImmediate;
    RotateLeft => WasmI32RotateLeft, WasmI32RotateLeftImmediate;
    RotateRight => WasmI32RotateRight, WasmI32RotateRightImmediate;
    Equal => WasmI32Equal, WasmI32EqualImmediate;
    NotEqual => WasmI32NotEqual, WasmI32NotEqualImmediate;
    LessSigned => WasmI32LessSigned, WasmI32LessSignedImmediate;
    LessUnsigned => WasmI32LessUnsigned, WasmI32LessUnsignedImmediate;
    GreaterSigned => WasmI32GreaterSigned, WasmI32GreaterSignedImmediate;
    GreaterUnsigned => WasmI32GreaterUnsigned, WasmI32GreaterUnsignedImmediate;
    LessEqualSigned => WasmI32LessEqualSigned, WasmI32LessEqualSignedImmediate;
    LessEqualUnsigned => WasmI32LessEqualUnsigned, WasmI32LessEqualUnsignedImmediate;
    GreaterEqualSigned => WasmI32GreaterEqualSigned, WasmI32GreaterEqualSignedImmediate;
    GreaterEqualUnsigned => WasmI32GreaterEqualUnsigned, WasmI32GreaterEqualUnsignedImmediate;
}

// A comparison and the conditional jump taken when it holds, with its negation.
macro_rules! i32_comparison_jumps {
    ($($name:ident => $jump:ident, $immediate:ident, $negation:ident;)+) => {
        impl I32BinaryOperator {
            #[cfg(not(stencil_build))]
            pub(crate) const fn jump_op(self) -> Option<Op> {
                match self { $(Self::$name => Some(Op::$jump),)+ _ => None }
            }
            #[cfg(not(stencil_build))]
            pub(crate) const fn immediate_jump_op(self) -> Option<Op> {
                match self { $(Self::$name => Some(Op::$immediate),)+ _ => None }
            }
            pub(crate) const fn negated_comparison(self) -> Option<Self> {
                match self { $(Self::$name => Some(Self::$negation),)+ _ => None }
            }
            #[cfg(not(stencil_build))]
            pub const fn from_jump_op(op: Op) -> Option<Self> {
                match op { $(Op::$jump => Some(Self::$name),)+ _ => None }
            }
            #[cfg(not(stencil_build))]
            pub const fn from_immediate_jump_op(op: Op) -> Option<Self> {
                match op { $(Op::$immediate => Some(Self::$name),)+ _ => None }
            }
        }
    };
}
i32_comparison_jumps! {
    Equal => WasmJumpI32Equal, WasmJumpI32EqualImmediate, NotEqual;
    NotEqual => WasmJumpI32NotEqual, WasmJumpI32NotEqualImmediate, Equal;
    LessSigned => WasmJumpI32LessSigned, WasmJumpI32LessSignedImmediate, GreaterEqualSigned;
    LessUnsigned => WasmJumpI32LessUnsigned, WasmJumpI32LessUnsignedImmediate, GreaterEqualUnsigned;
    GreaterSigned => WasmJumpI32GreaterSigned, WasmJumpI32GreaterSignedImmediate, LessEqualSigned;
    GreaterUnsigned => WasmJumpI32GreaterUnsigned, WasmJumpI32GreaterUnsignedImmediate, LessEqualUnsigned;
    LessEqualSigned => WasmJumpI32LessEqualSigned, WasmJumpI32LessEqualSignedImmediate, GreaterSigned;
    LessEqualUnsigned => WasmJumpI32LessEqualUnsigned, WasmJumpI32LessEqualUnsignedImmediate, GreaterUnsigned;
    GreaterEqualSigned => WasmJumpI32GreaterEqualSigned, WasmJumpI32GreaterEqualSignedImmediate, LessSigned;
    GreaterEqualUnsigned => WasmJumpI32GreaterEqualUnsigned, WasmJumpI32GreaterEqualUnsignedImmediate, LessUnsigned;
}

// Predicates over lowered instructions, by meaning.

/// The i32 operator a register-form instruction computes, if it is one.
#[cfg(not(stencil_build))]
pub(crate) fn register_operator(instruction: WideInstruction) -> Option<I32BinaryOperator> {
    I32BinaryOperator::from_register_op(instruction.op())
}

/// The i32 operator an immediate-form instruction computes, if it is one.
#[cfg(not(stencil_build))]
pub(crate) fn immediate_operator(instruction: WideInstruction) -> Option<I32BinaryOperator> {
    I32BinaryOperator::from_immediate_op(instruction.op())
}

/// Whether `instruction` is the immediate form of `operator`.
#[cfg(not(stencil_build))]
pub(crate) fn is_immediate_of(instruction: WideInstruction, operator: I32BinaryOperator) -> bool {
    instruction.op() == operator.immediate_op()
}

/// Whether a comparison has a register-operand conditional jump.
#[cfg(not(stencil_build))]
pub(crate) fn has_jump(comparison: I32BinaryOperator) -> bool {
    comparison.jump_op().is_some()
}

/// Whether a comparison has a constant-operand conditional jump.
#[cfg(not(stencil_build))]
pub(crate) fn has_immediate_jump(comparison: I32BinaryOperator) -> bool {
    comparison.immediate_jump_op().is_some()
}

/// Whether `instruction` is the i32 `eqz` unary operator.
pub(crate) fn is_i32_equal_zero(instruction: WideInstruction) -> bool {
    instruction.op() == Op::WasmI32Unary
        && I32UnaryOperator::from_tag(instruction.imm()) == Some(I32UnaryOperator::EqualZero)
}
