//! The fast arms of the key and iterator opcodes over a register frame: what a register's own
//! word proves, so a tier that holds it pays no call. Where the word does not prove it the
//! operation leaves at the miss edge, and the full operation (`Vm::value_step`,
//! `Vm::require_iterator_result`) decides: the interpreter runs the fast arm first on
//! `Concrete` (`dispatch_fast::lane_frame`), the b-jit runs it as a stencil row ahead of the
//! helper step that is its slow arm.

use super::{Frame, Stage, Step, value};
use crate::bytecode::Register;

/// Continue where register `register` holds a direct object or array record, which is an
/// object and so an iterator result `RequireIteratorResult` accepts; any other value leaves at
/// `miss`, where another object kind passes and a primitive throws.
#[inline(always)]
pub(crate) fn require_iterator_result<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, register: Register,
    miss: S::Miss) -> Step<S::Word, F::Control> {
    let word = frame.tagged(stage, register);
    let object = value::is_direct_object(stage, word);
    let array = value::is_direct_array(stage, word);
    let record = stage.any(object, array);
    match stage.guard(record, miss) {
        Some(()) => Step::Value(word),
        None => Step::Unresolved,
    }
}

/// `destination` defined with register `source`'s word where it is a direct string, which is
/// its own property key; any other value leaves at `miss`, where an integer, a symbol or an
/// object takes the full conversion.
#[inline(always)]
pub(crate) fn to_property_key<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    source: Register, miss: S::Miss) -> Step<S::Word, F::Control> {
    let word = frame.tagged(stage, source);
    let string = value::is_direct_string(stage, word);
    match stage.guard(string, miss) {
        Some(()) => Step::Value(frame.move_register(stage, destination, source)),
        None => Step::Unresolved,
    }
}
