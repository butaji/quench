//! The Cranelift call signatures of compiled code: each callout and entry is built here from
//! its parameter and return types under one calling convention.

#[allow(unused_imports)]
use crate::ext_traits::*;
use cranelift_codegen::ir::{AbiParam, Signature, Type};
use cranelift_codegen::isa::CallConv;

/// A signature under `call_conv` whose parameters and returns have the given types, in order.
pub(crate) fn typed_signature(call_conv: CallConv, params: &[Type], returns: &[Type]) -> Signature {
    let mut signature = Signature::new(call_conv);
    signature.params.extend(params.iter().map(|&ty| AbiParam::new(ty)));
    signature.returns.extend(returns.iter().map(|&ty| AbiParam::new(ty)));
    signature
}

/// A signature under `call_conv` taking `params` and returning `returns` words of type `word`.
pub(crate) fn word_signature(call_conv: CallConv, params: usize, returns: usize, word: Type) -> Signature {
    typed_signature(call_conv, &vec![word; params], &vec![word; returns])
}
