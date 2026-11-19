//! The Wasm i32 conditional jumps and the fused add-and-jump, as staged rows (`Decoder::WasmJump`).
//!
//! A Wasm i32 is an untagged int32 in its slot, so every operand is read as an int32 word
//! (`NumericFrame::int_operand`), and the jump's edge is the frame's `branch`. The comparisons are
//! the i32 operators of `staged::wasm` (`I32BinaryOperator`), written once over `Stage` primitives:
//! an unsigned order is `ult`, and a signed order is the unsigned order of the words with their sign
//! bits flipped. A tier that proves no int32 operand leaves the row (`Step::Unresolved`) to the
//! general step, which runs the Wasm operator.

use super::{NumericFrame, NumericStage, Stage, Step};
use crate::bytecode::{Operand, Register};
#[cfg(not(stencil_build))]
use crate::bytecode::{Op, WideInstruction};
#[cfg(not(stencil_build))]
use crate::wasm::integer::I32BinaryOperator;

/// An i32 comparison: its relation, and for an order whether it compares signed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum WasmRelation {
    Equal,
    NotEqual,
    Less { signed: bool },
    Greater { signed: bool },
    LessEqual { signed: bool },
    GreaterEqual { signed: bool },
}

impl WasmRelation {
    /// The relation `operator` names, where it is a comparison.
    #[cfg(not(stencil_build))]
    fn of(operator: I32BinaryOperator) -> Option<Self> {
        Some(match operator {
            I32BinaryOperator::Equal => Self::Equal,
            I32BinaryOperator::NotEqual => Self::NotEqual,
            I32BinaryOperator::LessSigned => Self::Less { signed: true },
            I32BinaryOperator::LessUnsigned => Self::Less { signed: false },
            I32BinaryOperator::GreaterSigned => Self::Greater { signed: true },
            I32BinaryOperator::GreaterUnsigned => Self::Greater { signed: false },
            I32BinaryOperator::LessEqualSigned => Self::LessEqual { signed: true },
            I32BinaryOperator::LessEqualUnsigned => Self::LessEqual { signed: false },
            I32BinaryOperator::GreaterEqualSigned => Self::GreaterEqual { signed: true },
            I32BinaryOperator::GreaterEqualUnsigned => Self::GreaterEqual { signed: false },
            _ => return None,
        })
    }
}

/// The right operand of a comparison: a register, or the 16-bit constant an immediate form carries,
/// sign-extended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum WasmRight {
    Register(Register),
    Constant(i32),
}

/// A Wasm i32 jump: where its edge goes, and what it writes first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum WasmJump {
    /// Jumps where `left` relates to `right` (`WasmJumpI32*`, and their immediate forms).
    Compare { relation: WasmRelation, left: Register, right: WasmRight },
    /// Jumps where register `register` is nonzero, or where it is zero when not `nonzero`
    /// (`WasmJumpI32NonZero`, `WasmJumpI32Zero`).
    Test { register: Register, nonzero: bool },
    /// Defines `destination` with `source` plus `addend`, then jumps where the sum is nonzero
    /// (`WasmI32AddImmediateJumpNonZero`).
    AddNonZero { destination: Register, source: Register, addend: i32 },
}

impl WasmJump {
    /// The i32 jump `instruction` is, from its record's operands (`a`, `b`, `c`): a comparison's
    /// right operand is `b` (a register, or a signed constant for an immediate form), and an add's
    /// addend is `c`.
    #[cfg(not(stencil_build))]
    pub(crate) fn decode(instruction: WideInstruction) -> Option<Self> {
        let op = instruction.op();
        if let Some(operator) = I32BinaryOperator::from_jump_op(op) {
            return Some(Self::Compare {
                relation: WasmRelation::of(operator)?,
                left: instruction.a(),
                right: WasmRight::Register(instruction.b()),
            });
        }
        if let Some(operator) = I32BinaryOperator::from_immediate_jump_op(op) {
            return Some(Self::Compare {
                relation: WasmRelation::of(operator)?,
                left: instruction.a(),
                right: WasmRight::Constant(i32::from(instruction.b() as i16)),
            });
        }
        Some(match op {
            Op::WasmJumpI32Zero => Self::Test { register: instruction.a(), nonzero: false },
            Op::WasmJumpI32NonZero => Self::Test { register: instruction.a(), nonzero: true },
            Op::WasmI32AddImmediateJumpNonZero => Self::AddNonZero {
                destination: instruction.a(),
                source: instruction.b(),
                addend: i32::from(instruction.c() as i16),
            },
            _ => return None,
        })
    }
}

/// The unsigned order of `word` with its sign bit flipped: the signed order of the int32 words.
/// `word` is zero-extended, so the flip and the 32-bit mask are one add and one and.
#[inline(always)]
fn signed_key<S: Stage>(stage: &mut S, word: S::Word) -> S::Word {
    let flipped = stage.add_imm(word, 0x8000_0000u64);
    stage.and_imm(flipped, 0xffff_ffffu64)
}

/// The two words an order compares: as they are, or with their sign bits flipped where `signed`.
#[inline(always)]
fn order_keys<S: Stage>(stage: &mut S, signed: bool, left: S::Word, right: S::Word) -> (S::Word, S::Word) {
    if signed {
        let left = signed_key(stage, left);
        let right = signed_key(stage, right);
        (left, right)
    } else {
        (left, right)
    }
}

/// Whether two zero-extended int32 words are equal.
#[inline(always)]
fn equal<S: Stage>(stage: &mut S, left: S::Word, right: S::Word) -> S::Cond {
    let below = stage.ult(left, right);
    let above = stage.ult(right, left);
    let apart = stage.any(below, above);
    stage.not(apart)
}

/// `left relation right` on zero-extended int32 words, as a condition.
#[inline(always)]
fn relate<S: Stage>(stage: &mut S, relation: WasmRelation, left: S::Word, right: S::Word) -> S::Cond {
    match relation {
        WasmRelation::Equal => equal(stage, left, right),
        WasmRelation::NotEqual => {
            let same = equal(stage, left, right);
            stage.not(same)
        }
        WasmRelation::Less { signed } => {
            let (left, right) = order_keys(stage, signed, left, right);
            stage.ult(left, right)
        }
        WasmRelation::Greater { signed } => {
            let (left, right) = order_keys(stage, signed, left, right);
            stage.ult(right, left)
        }
        WasmRelation::LessEqual { signed } => {
            let (left, right) = order_keys(stage, signed, left, right);
            let above = stage.ult(right, left);
            stage.not(above)
        }
        WasmRelation::GreaterEqual { signed } => {
            let (left, right) = order_keys(stage, signed, left, right);
            let below = stage.ult(left, right);
            stage.not(below)
        }
    }
}

/// `jump` on `frame`: its operands read as int32s, its result written where it defines one, and
/// its edge taken from the condition. An operand the frame cannot read as an int32 leaves the row.
#[inline(always)]
pub(crate) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, jump: WasmJump) -> Step<S::Word, F::Control> {
    match jump {
        WasmJump::Compare { relation, left, right } => {
            let miss = frame.miss();
            let Some(left) = frame.int_operand(stage, Operand::register(left), miss) else {
                return Step::Unresolved;
            };
            let right = match right {
                WasmRight::Register(register) => {
                    let Some(right) = frame.int_operand(stage, Operand::register(register), miss) else {
                        return Step::Unresolved;
                    };
                    right
                }
                WasmRight::Constant(constant) => stage.constant(u64::from(constant as u32)),
            };
            let taken = relate(stage, relation, left, right);
            Step::Control(frame.branch(stage, taken))
        }
        WasmJump::Test { register, nonzero } => {
            let miss = frame.miss();
            let Some(word) = frame.int_operand(stage, Operand::register(register), miss) else {
                return Step::Unresolved;
            };
            let zero = stage.eq_imm(word, 0u64);
            let taken = if nonzero { stage.not(zero) } else { zero };
            Step::Control(frame.branch(stage, taken))
        }
        WasmJump::AddNonZero { destination, source, addend } => {
            let miss = frame.miss();
            let Some(word) = frame.int_operand(stage, Operand::register(source), miss) else {
                return Step::Unresolved;
            };
            let addend = stage.constant(u64::from(addend as u32));
            let sum = stage.add(word, addend);
            let sum = stage.and_imm(sum, 0xffff_ffffu64);
            let value = frame.int_result(stage, sum);
            let fact = frame.int_fact();
            frame.write(stage, destination, value, fact);
            let zero = stage.eq_imm(sum, 0u64);
            let taken = stage.not(zero);
            Step::Control(frame.branch(stage, taken))
        }
    }
}

/// A Wasm branch table (`WasmBranchTable`): the i32 selector in `index` picks one of `entries`
/// jump records that follow the table record; the last entry is the default, which every selector
/// past it takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct BranchTable {
    pub(crate) index: Register,
    /// The entry count: the record's case count plus the default.
    pub(crate) entries: u32,
}

impl BranchTable {
    /// The table `instruction` is: its selector is `a`, and its immediate counts the cases before
    /// the default (`branch_table_entries`), so the entries are that count plus one.
    #[cfg(not(stencil_build))]
    pub(crate) fn decode(instruction: WideInstruction) -> Option<Self> {
        if instruction.op() != Op::WasmBranchTable {
            return None;
        }
        Some(Self { index: instruction.a(), entries: instruction.imm().checked_add(1)? })
    }
}

/// `table` on `frame`: its selector read as an int32, and the entry it picks (`Frame::branch_table`,
/// which clamps it to the default). A selector the frame cannot read as an int32 leaves the row.
#[inline(always)]
pub(crate) fn branch_table<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, table: BranchTable) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let Some(index) = frame.int_operand(stage, Operand::register(table.index), miss) else {
        return Step::Unresolved;
    };
    match frame.branch_table(stage, index, table.entries) {
        Some(edge) => Step::Control(edge),
        None => Step::Unresolved,
    }
}
