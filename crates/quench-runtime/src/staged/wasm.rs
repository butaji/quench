//! Wasm integer semantics written once over `WasmStage`: the traps of an integer operator and
//! the order in which it checks them. The interpreter runs them on machine words
//! (`ConcreteWasm`), the Wasm tier on Cranelift values (`Emitter`, `vm/compiled/wasm_stage.rs`),
//! so a native function traps exactly where the interpreter does. Everything a stage computes
//! without a trap is its primitive set: `operate`, `operate_unary` and their float and
//! conversion counterparts (`staged::wasm_float` holds the float rules).

use super::machine::MachineStage;
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use crate::{WasmTrap, WasmType};

/// The bytes of a v128: the one definition, which the runtime's and the stencil's `crate::wasm`
/// re-export.
pub const V128_BYTES: usize = core::mem::size_of::<u128>();

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

    #[inline]
    pub(crate) const fn of(ty: WasmType) -> Option<Self> {
        match ty {
            WasmType::I32 => Some(Self::I32),
            WasmType::I64 => Some(Self::I64),
            _ => None,
        }
    }

    pub(crate) const fn ty(self) -> WasmType {
        match self {
            Self::I32 => WasmType::I32,
            Self::I64 => WasmType::I64,
        }
    }

    /// The most negative value of the width, sign-extended to 64 bits.
    #[inline]
    pub(crate) const fn minimum(self) -> i64 {
        i64::MIN >> (i64::BITS - self.bits())
    }
}

/// The machine a Wasm integer operation runs on: its primitives (`MachineStage`) and nothing
/// more, so every machine of the primitives is a Wasm stage.
pub(crate) trait WasmStage: MachineStage {}

impl<S: MachineStage> WasmStage for S {}

/// Whether `operator` compares: its result is an `I32` flag at either width.
#[inline(always)]
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
#[inline(always)]
pub(crate) const fn binary_result(operator: Binary, width: Width) -> Width {
    if is_comparison(operator) {
        Width::I32
    } else {
        width
    }
}

/// The width of `operator`'s result at a `width` operand: a test of zero is a flag.
#[inline(always)]
pub(crate) const fn unary_result(operator: Unary, width: Width) -> Width {
    match operator {
        Unary::EqualZero => Width::I32,
        _ => width,
    }
}

#[inline]
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

/// The integer operator a tag names. A tag outside the family names no row, so the first operator
/// stands in (the dispatch never reaches it).
#[cfg(stencil_build)]
pub(crate) const fn binary_operator(tag: u32) -> Binary {
    match Binary::from_tag(tag) {
        Some(operator) => operator,
        None => Binary::Add,
    }
}

/// The integer unary operator a tag names, as `binary_operator`.
#[cfg(stencil_build)]
pub(crate) const fn unary_operator(tag: u32) -> Unary {
    match Unary::from_tag(tag) {
        Some(operator) => operator,
        None => Unary::EqualZero,
    }
}
