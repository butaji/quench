#![allow(unused_imports)]
use super::*;

/// The value at `slot` of the live object `object`, whose exact shape (no dictionary) holds
/// it, through the arena `stage` is over: the interpreter's reading of `own_layout` and `read_slot`.
///
/// # Safety
/// `object` is a live record of the heap `stage` is over, whose exact shape covers `slot`.
#[inline(always)]
pub unsafe fn concrete_own_read(object: &crate::heap::Object, mut stage: Concrete, slot: usize) -> crate::Value {
    let record = object as *const crate::heap::Object as u64;
    let word = header_u32(&mut stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    let layout = formed_own_layout(&mut stage, record, word, Form::Either, slot + 1, PackedAccess::Reload, ())
        .expect("the interpreter knows every storage form");
    crate::value_vec::decode_staged_word(read_slot(&mut stage, layout, slot as u16, Representation::Tagged))
}
