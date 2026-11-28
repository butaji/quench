//! Decoding a narrow value, and the concrete own-property read.

use super::*;

/// The value a narrow word (`Value::to_narrow`), zero-extended, encodes (`Value::from_narrow`):
/// an integer's code shifted down under the int tag, or a reference's upper word by its code
/// beside its index.
#[inline(always)]
pub fn decode_narrow<S: Stage>(stage: &mut S, narrow: S::Word) -> S::Word {
    let upper_words = stage.constant(NarrowLayout::upper_words() as u64);
    decode_narrow_through(stage, narrow, upper_words)
}

/// `decode_narrow` with the reference codes' upper words at `upper_words`, a copy of
/// `NarrowLayout::upper_words` (an ahead-of-time image links its own).
#[inline(always)]
pub fn decode_narrow_through<S: Stage>(stage: &mut S, narrow: S::Word, upper_words: S::Word) -> S::Word {
    let flag = stage.and_imm(narrow, u64::from(NarrowLayout::INTEGER_FLAG));
    let integer = stage.ne_imm(flag, 0u32);
    stage.choose(integer, #[inline(always)] |stage| {
        let shifted = stage.sar32_imm(narrow, NarrowLayout::CODE_SHIFT);
        stage.or_imm(shifted, ValueLayout::INT_TAG)
    }, #[inline(always)] |stage| {
        let code = stage.shr_imm(narrow, NarrowLayout::CODE_SHIFT);
        let code = stage.and_imm(code, u64::from(NarrowLayout::CODE_MASK));
        let offset = stage.mul_imm(code, std::mem::size_of::<u64>() as u64);
        let entry = stage.add(upper_words, offset);
        let upper = stage.load_table(Region::HeapMeta.readonly(), entry);
        let lower = stage.shr_imm(narrow, NarrowLayout::INDEX_SHIFT);
        stage.or(upper, lower)
    })
}

#[cfg(not(stencil_build))]
#[path = "narrow/glue.rs"]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
