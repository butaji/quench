//! Facts as typed proofs (S1.5): the words a numeric row takes as an int32 or a binary64 operand.
//!
//! A numeric row computes only on an `Int32` or a `Float64`, never on a bare word. Each has a
//! private field, so a word becomes one in only these ways:
//! - `proven`: the frame holds a fact that proves the representation (a type, a register it
//!   already guarded, a representation the specializer fixed). Constructing one is `unsafe`
//!   because no test runs: the caller cites the fact in a `// Fact:` comment.
//! - a row's own result (`pub(crate)` constructors), whose representation the row defines;
//! - a constant the row folds (`constant`).
//!
//! The census counts `proven(` outside `staged`, so the places that assert a fact without a
//! test only shrink.

use super::Stage;

/// The low 32 bits of an int32, proved; the stage's word otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Int32<W>(W);

/// The IEEE bits of a binary64, proved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Float64<W>(W);

impl<W: Copy> Int32<W> {
    /// The int32 `word` holds by the caller's fact, read with no test.
    ///
    /// # Safety
    /// The caller holds a fact (a type proof, a guard it already ran, a fixed representation)
    /// that `word` is an int32's low 32 bits in its stage. A wrong fact gives a wrong answer.
    #[inline(always)]
    pub unsafe fn proven(word: W) -> Self { Int32(word) }
    /// A word a staged row computed as an int32.
    #[inline(always)]
    pub(crate) fn of_row(word: W) -> Self { Int32(word) }
    /// The proved word.
    #[inline(always)]
    pub fn word(self) -> W { self.0 }
}

impl<W: Copy> Float64<W> {
    /// The binary64 `word` holds by the caller's fact, read with no test.
    ///
    /// # Safety
    /// As `Int32::proven`: the caller holds a fact that `word` is a binary64's IEEE bits.
    #[inline(always)]
    pub unsafe fn proven(word: W) -> Self { Float64(word) }
    /// A word a staged row computed as a binary64.
    #[inline(always)]
    pub(crate) fn of_row(word: W) -> Self { Float64(word) }
    /// The proved word.
    #[inline(always)]
    pub fn word(self) -> W { self.0 }
}

impl<W: Copy> Int32<W> {
    /// The int32 constant `value`, folded into `stage`.
    #[inline(always)]
    pub fn constant<S: super::NumericStage<Word = W>>(stage: &mut S, value: i32) -> Self { Int32(stage.i32_constant(value)) }
}

impl<W: Copy> Float64<W> {
    /// The binary64 constant `value`, folded into `stage`.
    #[inline(always)]
    pub fn constant<S: super::NumericStage<Word = W>>(stage: &mut S, value: f64) -> Self { Float64(stage.f64_constant(value)) }
}

/// The proved operand types a stage's numeric rows take.
pub type IntOf<S> = Int32<<S as Stage>::Word>;
/// The proved binary64 operand type a stage's numeric rows take.
pub type NumberOf<S> = Float64<<S as Stage>::Word>;

#[cfg(not(stencil_build))]
/// The single concrete guard: a tagged value's int32, tested once by its tag; `None` where it
/// holds none. Every machine-word frame (the interpreter's, the lane's) proves its operands here.
impl Int32<u64> {
    #[inline(always)]
    pub fn checked(value: crate::Value) -> Option<Self> { value.as_int().map(|int| Int32(u64::from(int as u32))) }
}

#[cfg(not(stencil_build))]
/// The single concrete guard for a Number: an int32 widened, a double as it is; `None` where the
/// value is no Number.
impl Float64<u64> {
    #[inline(always)]
    pub fn checked(value: crate::Value) -> Option<Self> { value.as_number().map(|number| Float64(number.to_bits())) }
}

impl<W: Copy> Int32<W> {
    /// ToInt32 of a proved binary64 by the stage's own conversion (`NumericStage::to_int32`).
    #[inline(always)]
    pub fn to_int32<S: super::NumericStage<Word = W>>(stage: &mut S, number: Float64<W>, miss: S::Miss) -> Option<Self> {
        stage.to_int32(number.0, miss).map(Int32)
    }
}
