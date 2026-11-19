//! The emission vocabulary of the Wasm lowering: each residual instruction the
//! lowering writes is named here by its meaning. The lowering calls these
//! functions and never names an opcode itself. Encodings are unchanged: every
//! function writes exactly the operands the lowering wrote before.

use crate::Diagnostic;
use crate::bytecode::{ImmediateLayout, Op, Register, WideInstruction};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::integer::{I32BinaryOperator, I32UnaryOperator, I64BinaryOperator};

/// Where the lowering appends one residual instruction. The lowering implements
/// it by its own emit, so the operands and encoding stay those it always wrote.
pub(crate) trait Sink {
    fn write(
        &mut self,
        op: Op,
        a: Register,
        b: Register,
        c: Register,
        imm: u32,
    ) -> Result<(), Diagnostic>;
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
            pub(crate) const fn from_register_op(op: Op) -> Option<Self> {
                match op { $(Op::$register => Some(Self::$name),)+ _ => None }
            }
            #[cfg(not(stencil_build))]
            pub(crate) const fn from_immediate_op(op: Op) -> Option<Self> {
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
            pub(crate) const fn from_jump_op(op: Op) -> Option<Self> {
                match op { $(Op::$jump => Some(Self::$name),)+ _ => None }
            }
            #[cfg(not(stencil_build))]
            pub(crate) const fn from_immediate_jump_op(op: Op) -> Option<Self> {
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

// Emission. Each function appends one instruction to `sink`.

/// An unconditional jump whose target is patched when its label is bound.
pub(crate) fn jump(sink: &mut impl Sink) -> Result<(), Diagnostic> {
    sink.write(Op::Jump, 0, 0, 0, 0)
}

/// A jump taken when the i32 register `value` is zero.
pub(crate) fn jump_if_zero_i32(sink: &mut impl Sink, value: Register) -> Result<(), Diagnostic> {
    sink.write(Op::WasmJumpI32Zero, value, 0, 0, 0)
}

/// A jump taken when the i32 register `value` is nonzero.
pub(crate) fn jump_if_nonzero_i32(sink: &mut impl Sink, value: Register) -> Result<(), Diagnostic> {
    sink.write(Op::WasmJumpI32NonZero, value, 0, 0, 0)
}

/// A fused comparison of two registers that jumps when it holds.
#[cfg(not(stencil_build))]
pub(crate) fn compare_and_jump(
    sink: &mut impl Sink,
    comparison: I32BinaryOperator,
    left: Register,
    right: Register,
) -> Result<(), Diagnostic> {
    let op = comparison.jump_op().expect("fusable comparison has a jump");
    sink.write(op, left, right, 0, 0)
}

/// A fused comparison of a register with a constant that jumps when it holds.
#[cfg(not(stencil_build))]
pub(crate) fn compare_immediate_and_jump(
    sink: &mut impl Sink,
    comparison: I32BinaryOperator,
    left: Register,
    constant: Register,
) -> Result<(), Diagnostic> {
    let op = comparison
        .immediate_jump_op()
        .expect("fusable comparison has a constant jump");
    sink.write(op, left, constant, 0, 0)
}

/// A counted loop's step `value = source + addend`, absorbed into its back edge.
pub(crate) fn add_immediate_jump_if_nonzero(
    sink: &mut impl Sink,
    value: Register,
    source: Register,
    addend: Register,
) -> Result<(), Diagnostic> {
    sink.write(Op::WasmI32AddImmediateJumpNonZero, value, source, addend, 0)
}

/// A jump table over `cases` entries, followed by one jump per entry and the default.
pub(crate) fn branch_table(
    sink: &mut impl Sink,
    index: Register,
    cases: u32,
) -> Result<(), Diagnostic> {
    sink.write(Op::WasmBranchTable, index, 0, 0, cases)
}

/// Throws the exception object held in `reference`.
pub(crate) fn throw_ref(sink: &mut impl Sink, reference: Register) -> Result<(), Diagnostic> {
    sink.write(Op::WasmThrowRef, reference, 0, 0, 0)
}

/// `destination = (reference is null)`.
pub(crate) fn ref_is_null(
    sink: &mut impl Sink,
    destination: Register,
    reference: Register,
) -> Result<(), Diagnostic> {
    sink.write(Op::WasmRefIsNull, destination, reference, 0, 0)
}

/// `destination = operator(source)` for an i32 unary operator.
pub(crate) fn i32_unary(
    sink: &mut impl Sink,
    destination: Register,
    source: Register,
    operator: I32UnaryOperator,
) -> Result<(), Diagnostic> {
    sink.write(Op::WasmI32Unary, destination, source, 0, operator as u32)
}

/// Traps: the lowered code is unreachable.
pub(crate) fn unreachable(sink: &mut impl Sink) -> Result<(), Diagnostic> {
    sink.write(Op::WasmUnreachable, 0, 0, 0, 0)
}

/// `result = condition ? left : right`.
pub(crate) fn select(
    sink: &mut impl Sink,
    result: Register,
    left: Register,
    right: Register,
    condition: Register,
) -> Result<(), Diagnostic> {
    sink.write(
        Op::WasmSelect,
        result,
        left,
        right,
        ImmediateLayout::register_pair_immediate(condition, condition),
    )
}

/// `destination = local[slot]`.
pub(crate) fn load_local(
    sink: &mut impl Sink,
    destination: Register,
    slot: u16,
) -> Result<(), Diagnostic> {
    sink.write(Op::LoadLocal, destination, 0, 0, u32::from(slot))
}

/// `local[slot] = value`.
pub(crate) fn store_local(sink: &mut impl Sink, value: Register, slot: u16) -> Result<(), Diagnostic> {
    sink.write(Op::StoreLocal, value, 0, 0, u32::from(slot))
}

/// `destination = source`.
pub(crate) fn move_register(
    sink: &mut impl Sink,
    destination: Register,
    source: Register,
) -> Result<(), Diagnostic> {
    sink.write(Op::Move, destination, source, 0, 0)
}

/// `destination = constants[constant]`.
pub(crate) fn load_constant(
    sink: &mut impl Sink,
    destination: Register,
    constant: u32,
) -> Result<(), Diagnostic> {
    sink.write(Op::LoadConst, destination, 0, 0, constant)
}

/// `destination = array` of `count` elements, filled by `define_array_element`.
pub(crate) fn make_array(sink: &mut impl Sink, destination: Register, count: u16) -> Result<(), Diagnostic> {
    sink.write(Op::MakeArray, destination, 0, 0, u32::from(count))
}

/// `array[index] = element`.
pub(crate) fn define_array_element(
    sink: &mut impl Sink,
    element: Register,
    array: Register,
    index: u16,
) -> Result<(), Diagnostic> {
    sink.write(Op::DefineArrayElement, element, array, 0, u32::from(index))
}

/// Returns `value` from the function.
pub(crate) fn return_value(sink: &mut impl Sink, value: Register) -> Result<(), Diagnostic> {
    sink.write(Op::Return, value, 0, 0, 0)
}

/// Builds an exception object at `base` from the `count` values after the tag.
pub(crate) fn exception_new(sink: &mut impl Sink, base: Register, count: u16) -> Result<(), Diagnostic> {
    sink.write(Op::WasmExceptionNew, base, base, count, 0)
}

/// `condition = exception matches tag`.
pub(crate) fn exception_match(
    sink: &mut impl Sink,
    condition: Register,
    exception: Register,
    tag: Register,
) -> Result<(), Diagnostic> {
    sink.write(Op::WasmExceptionMatch, condition, exception, tag, 0)
}

/// `destination = payload[offset]` of `exception`.
pub(crate) fn exception_payload(
    sink: &mut impl Sink,
    destination: Register,
    exception: Register,
    offset: u16,
) -> Result<(), Diagnostic> {
    sink.write(Op::WasmExceptionPayload, destination, exception, 0, u32::from(offset))
}

/// Drops the instance binding of element or data segment `slot`.
pub(crate) fn segment_drop(sink: &mut impl Sink, slot: u16) -> Result<(), Diagnostic> {
    sink.write(Op::WasmSegmentDrop, 0, 0, 0, u32::from(slot))
}

/// Copies `length` items of element segment `elements` into `table` at `output`, from `input`.
pub(crate) fn table_init(
    sink: &mut impl Sink,
    table: Register,
    elements: Register,
    length: Register,
    output: Register,
    input: Register,
) -> Result<(), Diagnostic> {
    sink.write(
        Op::WasmTableInit,
        table,
        elements,
        length,
        ImmediateLayout::register_pair_immediate(output, input),
    )
}

/// `destination = left operator right` for an i64 operator.
pub(crate) fn i64_binary(
    sink: &mut impl Sink,
    operator: I64BinaryOperator,
    destination: Register,
    left: Register,
    right: Register,
) -> Result<(), Diagnostic> {
    sink.write(Op::WasmI64Binary, destination, left, right, operator as u32)
}

/// `destination = convert(source)` for a scalar conversion.
pub(crate) fn scalar_convert(
    sink: &mut impl Sink,
    destination: Register,
    source: Register,
    operator: ScalarConversionOperator,
) -> Result<(), Diagnostic> {
    sink.write(Op::WasmScalarConvert, destination, source, 0, operator as u32)
}
