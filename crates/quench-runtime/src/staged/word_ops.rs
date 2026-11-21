//! The pure word and bool operations the interpreter's stage (`Concrete`, `staged/concrete.rs`) and
//! the stencil's stage (`StencilStage`, `quench-bjit/stencils/stage.rs`) both define. Each stage
//! expands `word_stage_ops!()` inside its own `impl Stage` block, so every body is compiled into
//! that stage's rows and stays `#[inline(always)]`.

/// The `Stage` methods whose bodies are identical in every stage whose `Word` is `u64` and whose
/// `Cond` is `bool`. Expanding them in both impls keeps one definition of each rule. It is sound
/// because each body reads only its arguments, and `number_truthy` reads only the stateless unit
/// `NumberF64Stage`, so the same tokens mean the same operation in each stage. Stages whose bodies
/// differ (the `*_imm` operations, which bind `imm` before use in one stage and not the other, and
/// `uextend32`) stay in their own impls.
macro_rules! word_stage_ops {
    () => {
    #[inline(always)]
    fn add(&mut self, a: u64, b: u64) -> u64 { a.wrapping_add(b) }
    #[inline(always)]
    fn or(&mut self, a: u64, b: u64) -> u64 { a | b }
    #[inline(always)]
    fn and(&mut self, a: u64, b: u64) -> u64 { a & b }
    #[inline(always)]
    fn shr_imm(&mut self, a: u64, shift: impl Into<u64>) -> u64 { a >> shift.into() }
    #[inline(always)]
    fn sar32_imm(&mut self, a: u64, shift: u32) -> u64 { u64::from(((a as u32 as i32) >> shift) as u32) }
    #[inline(always)]
    fn ult(&mut self, a: u64, b: u64) -> bool { a < b }
    #[inline(always)]
    fn sub(&mut self, a: u64, b: u64) -> u64 { a.wrapping_sub(b) }
    #[inline(always)]
    fn select(&mut self, cond: bool, then: u64, otherwise: u64) -> u64 { if cond { then } else { otherwise } }
    #[inline(always)]
    fn not(&mut self, cond: bool) -> bool { !cond }
    #[inline(always)]
    fn any(&mut self, a: bool, b: bool) -> bool { a | b }
    #[inline(always)]
    fn pick(&mut self, cond: bool, then: bool, otherwise: bool) -> bool { if cond { then } else { otherwise } }
    #[inline(always)]
    fn number_truthy(&mut self, bits: u64) -> bool {
        crate::vm::operations::number_truthiness(&mut crate::vm::operations::NumberF64Stage, f64::from_bits(bits))
    }
    };
}

// Re-exported so each stage names the macro by path (`word_ops::word_stage_ops!()`), which needs no
// textual order between the mounts.
pub(crate) use word_stage_ops;
