//! Runtime glue of `element_rows`: the decode of the element rows (`GetIndex`, `SetIndex`) and the
//! interpreter's record replacement (`concrete_record_replace`, which names the runtime's array records).
#![allow(unused_imports)]
use super::*;

/// The element rows of `decode`: `GetIndex` and `SetIndex`.
pub trait OperationElementRowsExt<W, P>: Sized {
    fn decode_element(instruction: WideInstruction) -> Option<Self>;
}

impl<W, P> OperationElementRowsExt<W, P> for Operation<W, P> {
    #[cfg(not(stencil_build))]
    /// The element rows of `decode`: `GetIndex` of a register base and a register or immediate
    /// key (the tier defines the result register), and `SetIndex`; `None` where the instruction
    /// completes a frame or an operand names a local or a field.
    #[inline(always)]
    fn decode_element(instruction: WideInstruction) -> Option<Self> {
        if instruction.result_flags() != 0 { return None; }
        Some(match instruction.op() {
            Op::GetIndex => match instruction.operand_b().register_index() {
                Some(base) => Self::ElementRead { base, key: instruction.operand_c(), destination: instruction.result_register() },
                None => Self::ElementReadOperand { destination: instruction.result_register(),
                    base: instruction.operand_b(), key: instruction.operand_c() },
            },
            Op::SetIndex => Self::ElementWrite { value: instruction.register_a(), base: instruction.register_b(),
                key: instruction.register_c(), strict: instruction.boolean_flag().unwrap_or(false) },
            _ => return None,
        })
    }
}

/// The interpreter's replacement of the present element `index` of a direct array among the array
/// records at `records` with `value`, which the caller's gate proved needs no barrier log: the
/// row's store arm without a frame; `false` for any other base, a backing or array that is not
/// the array's own to write in place, a hole or an index past the elements.
#[inline(always)]
pub fn concrete_record_replace(records: *const crate::heap::ArrayObject, base: crate::Value, index: usize,
    value: crate::Value) -> bool {
    let mut stage = Concrete::new(core::ptr::null());
    prove_view(&mut stage, records as u64, base.to_bits(), true, ())
        .and_then(|view| replace_present(&mut stage, view, index as u64, value.to_bits(), ()))
        .is_some()
}

