//! Wasm integer semantics written once over `WasmStage`: the traps of an integer operator and
//! the order in which it checks them. The interpreter runs them on machine words
//! (`ConcreteWasm`), the Wasm tier on Cranelift values (`Emitter`, `vm/compiled/wasm_stage.rs`),
//! so a native function traps exactly where the interpreter does. Everything a stage computes
//! without a trap is its primitive set: `operate` and `operate_unary`.

use crate::WasmTrap;
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};

/// The integer width an operation runs at; an i32 operator is its i64 namesake at `I32`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Width {
    I32,
    I64,
}

impl Width {
    pub(crate) const fn bits(self) -> u32 {
        match self {
            Self::I32 => i32::BITS,
            Self::I64 => i64::BITS,
        }
    }

    /// The most negative value of the width, sign-extended to 64 bits.
    pub(crate) const fn minimum(self) -> i64 {
        i64::MIN >> (i64::BITS - self.bits())
    }
}

/// The machine a Wasm integer operation runs on.
pub(crate) trait WasmStage {
    /// A `width` integer; a comparison's flag is an `I32` word holding 0 or 1.
    type Word: Copy;
    /// What a trap leaves with: the trap for the interpreter; nothing for a specializer, whose
    /// code branches to the trap's exit.
    type Exit;
    fn constant(&mut self, width: Width, value: i64) -> Self::Word;
    /// `operator` on `width` operands; a comparison yields a flag. A division's divisor is
    /// nonzero, and a signed division's quotient fits (`integer_binary` has trapped otherwise).
    fn operate(
        &mut self,
        operator: Binary,
        width: Width,
        left: Self::Word,
        right: Self::Word,
    ) -> Self::Word;
    fn operate_unary(&mut self, operator: Unary, width: Width, value: Self::Word) -> Self::Word;
    /// Continues where `flag` is 0; traps where it is 1.
    fn trap_when(&mut self, flag: Self::Word, trap: WasmTrap) -> Result<(), Self::Exit>;
}

/// Whether `operator` compares: its result is an `I32` flag at either width.
pub(crate) const fn is_comparison(operator: Binary) -> bool {
    matches!(
        operator,
        Binary::Equal
            | Binary::NotEqual
            | Binary::LessSigned
            | Binary::LessUnsigned
            | Binary::GreaterSigned
            | Binary::GreaterUnsigned
            | Binary::LessEqualSigned
            | Binary::LessEqualUnsigned
            | Binary::GreaterEqualSigned
            | Binary::GreaterEqualUnsigned
    )
}

/// The width of `operator`'s result at `width` operands.
pub(crate) const fn binary_result(operator: Binary, width: Width) -> Width {
    if is_comparison(operator) {
        Width::I32
    } else {
        width
    }
}

/// The width of `operator`'s result at a `width` operand: a test of zero is a flag.
pub(crate) const fn unary_result(operator: Unary, width: Width) -> Width {
    match operator {
        Unary::EqualZero => Width::I32,
        _ => width,
    }
}

const fn divides(operator: Binary) -> bool {
    matches!(
        operator,
        Binary::DivideSigned
            | Binary::DivideUnsigned
            | Binary::RemainderSigned
            | Binary::RemainderUnsigned
    )
}

/// `operator` on `left` and `right`: a division by zero traps, then a signed quotient that
/// overflows traps. The one place these rules are written.
#[inline(always)]
pub(crate) fn integer_binary<S: WasmStage>(
    stage: &mut S,
    operator: Binary,
    width: Width,
    left: S::Word,
    right: S::Word,
) -> Result<S::Word, S::Exit> {
    if divides(operator) {
        check_division(stage, operator, width, left, right)?;
    }
    Ok(stage.operate(operator, width, left, right))
}

#[inline(always)]
fn check_division<S: WasmStage>(
    stage: &mut S,
    operator: Binary,
    width: Width,
    left: S::Word,
    right: S::Word,
) -> Result<(), S::Exit> {
    let zero = stage.constant(width, 0);
    let by_zero = stage.operate(Binary::Equal, width, right, zero);
    stage.trap_when(by_zero, WasmTrap::IntegerDivideByZero)?;
    if operator != Binary::DivideSigned {
        return Ok(());
    }
    let minimum = stage.constant(width, width.minimum());
    let negative_one = stage.constant(width, -1);
    let is_minimum = stage.operate(Binary::Equal, width, left, minimum);
    let is_negative_one = stage.operate(Binary::Equal, width, right, negative_one);
    let overflow = stage.operate(Binary::And, Width::I32, is_minimum, is_negative_one);
    stage.trap_when(overflow, WasmTrap::IntegerOverflow)
}
