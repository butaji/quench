#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Class, Op, WideInstruction};
use crate::wasm::CanonicalSignature;

/// The call step `op` is (`Decoder::WasmCall`), or none for an opcode that makes no call. A const
/// fn, so a free fn: an inherent impl of `Call` cannot live outside quench-staged.
#[inline(always)]
pub const fn call_of(op: Op) -> Option<Call> {
    match op.class() {
        Class::CallKnown => Some(Call::Known),
        Class::WasmIndirectTarget => Some(Call::IndirectTarget),
        Class::WasmRefFunc => Some(Call::RefFunc),
        Class::Call => Some(Call::Reference),
        _ => None,
    }
}

/// The function a call step names, read from its bytecode: runtime only.
pub trait CallWasmCallExt: Sized {
    /// The function a call names statically.
    fn callee(self, instruction: WideInstruction) -> Option<usize>;
}

impl CallWasmCallExt for Call {
    fn callee(self, instruction: WideInstruction) -> Option<usize> {
        (self == Self::Known).then(|| usize::from(instruction.known_function_index()))
    }
}

/// `indirect_target_word` over the canonical signature the program's type names.
#[inline(always)]
pub fn indirect_target<S: WasmTable>(
    stage: &mut S,
    index: S::Word,
    expected: CanonicalSignature,
) -> Result<(S::Word, S::Row), S::Exit> {
    debug_assert_ne!(expected, CanonicalSignature::NULL);
    let word = stage.constant(Width::I64, expected.word() as i64);
    let null = stage.constant(Width::I64, CanonicalSignature::NULL.word() as i64);
    indirect_target_word(stage, index, word, null)
}
