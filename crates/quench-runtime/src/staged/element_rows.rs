//! Element access as staged rows: `base[key]` (`GetIndex`) and `base[key] = value`
//! (`SetIndex`) over a dense array's backing. The array is proved once per access (`dense_view`:
//! a direct array record, its backing and, for a store, its sole ownership; a frame that kept
//! the proof answers it), the key's int32 is read through the frame, and the element is the
//! shared dense read or replacement (`elements`). A frame that kept the view of a typed array
//! (`ElementView::Typed`, proved once at its loop's entering edge) answers the same rows with the
//! typed element (`typed_elements`): a Number read, a Number store converted to the kind. What the dense arm cannot decide (a hole, an
//! index past the backing, a store that adds an element or needs the barrier's log, a key that
//! is no int32) is the frame's host path (`PropertyFrame::index_get_host`, `index_set_host`):
//! the VM's own element step, which the interpreter makes itself.
//!
//! The interpreter's reads and replacements of a record array are the same rows without a
//! frame (`elements::concrete_record_element`, `concrete_record_replace`).

use super::elements::{prove_view, read_element, replace_present, DenseView};
use super::property::Owner;
use super::typed_elements::{typed_read, typed_read_value, typed_write, TypedView};
use super::{Concrete, ElementFrame, NumericFrame, NumericStage, Operation, Stage, Step};
#[cfg(not(stencil_build))]
use crate::bytecode::{Op, WideInstruction};
use crate::bytecode::{Operand, Register};
use crate::typed_ir::Type;

/// An array's elements as an access reads them, once proved: a dense array's backing or a
/// typed array's elements, whose kind the proof carries.
#[derive(Clone, Copy)]
pub enum ElementView<W> {
    Dense(DenseView<W>),
    Typed(TypedView<W>),
}

#[cfg(not(stencil_build))]
impl<W, P> Operation<W, P> {
    #[cfg(not(stencil_build))]
    /// The element rows of `decode`: `GetIndex` of a register base and a register or immediate
    /// key (the tier defines the result register), and `SetIndex`; `None` where the instruction
    /// completes a frame or an operand names a local or a field.
    #[inline(always)]
    pub fn decode_element(instruction: WideInstruction) -> Option<Self> {
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

/// The dense view of `base` this access proves and keeps, from the array records.
#[inline(always)]
fn prove_and_keep<S: Stage, F: ElementFrame<S>>(stage: &mut S, frame: &mut F, base: S::Word, writable: bool,
    miss: S::Miss) -> Option<DenseView<S::Word>> {
    let records = frame.array_records(stage)?;
    let view = prove_view(stage, records, base, writable, miss)?;
    frame.keep_view(stage, base, writable, view);
    Some(view)
}

/// The dense view of `base` the frame kept of an earlier access, else the one this access proves
/// and keeps.
#[inline(always)]
fn dense_view<S: Stage, F: ElementFrame<S>>(stage: &mut S, frame: &mut F, base: S::Word, writable: bool,
    miss: S::Miss) -> Option<DenseView<S::Word>> {
    if let Some(ElementView::Dense(view)) = frame.kept_view(stage, base, writable) { return Some(view); }
    prove_and_keep(stage, frame, base, writable, miss)
}

/// `base[key]`: the present dense element, else the VM's read; a typed array's element where the
/// frame kept the view of one. The array and the key's int32 are proved first, and leave at the
/// frame's miss edge where they do not hold, so the host read only ever follows a hole or an
/// index past the backing.
#[inline(always)]
fn read<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, base: Register, key: Operand) -> Option<S::Word> {
    let miss = frame.miss();
    let object = frame.tagged(stage, base);
    let view = match frame.kept_view(stage, object, false) {
        Some(view) => view,
        None => match frame.proved_view(stage, base, false) {
            Some(view) => ElementView::Dense(view),
            // A frame that keeps no views reads the element of any backing in place.
            None if frame.reads_any_backing() => {
                let records = frame.array_records(stage)?;
                // merge: HEAD's let-else keeps a key that is no offset into the backing on the host read path
                // (theirs' `?` leaves the row instead); HEAD's proved-view probe arm is dropped for theirs' view order.
                let Some(index) = frame.index_offset(stage, key, miss) else {
                    let key = frame.key_word(stage, key)?;
                    return frame.index_get_host(stage, object, key, miss);
                };
                return frame.element_probed(stage, miss,
                    #[inline(always)] |stage, _, absent| super::elements::any_backing_element(stage, records, object, index, absent),
                    #[inline(always)] |stage, frame, miss| {
                        let key = frame.key_word(stage, key)?;
                        frame.index_get_host(stage, object, key, miss)
                    });
            }
            None => ElementView::Dense(prove_and_keep(stage, frame, object, false, miss)?),
        },
    };
    let index = frame.index_key(stage, key, miss)?;
    let index = stage.uextend32(index);
    match view {
        ElementView::Typed(view) if frame.typed_values() => typed_read_value(stage, view, index, miss),
        ElementView::Typed(view) => typed_read(stage, view, index, miss),
        ElementView::Dense(view) => frame.element_probed(stage, miss,
            #[inline(always)] |stage, _, absent| read_element(stage, view, index, absent),
            #[inline(always)] |stage, frame, miss| {
                let key = frame.key_word(stage, key)?;
                frame.index_get_host(stage, object, key, miss)
            }),
    }
}

/// `base[key] = value` for a typed array whose view the frame kept: the Number `value` converted
/// to the element's kind. Any other value, key or index leaves at the frame's miss edge.
#[inline(always)]
fn write_typed<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, view: TypedView<S::Word>,
    (value, key): (Register, Register)) -> Option<S::Word> {
    let miss = frame.miss();
    let number = frame.number_operand(stage, Operand::register(value), miss)?;
    let index = frame.index_key(stage, Operand::register(key), miss)?;
    let index = stage.uextend32(index);
    typed_write(stage, view, index, number, miss)?;
    Some(number)
}

/// `base[key] = value`: the present dense element replaced in place where the array owns its
/// backing alone and the value needs no barrier log (an immediate type, or the frame's gate),
/// else the VM's store; a typed array's element where the frame kept the view of one.
#[inline(always)]
fn write<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, (value, base, key): (Register, Register, Register))
    -> Option<S::Word> {
    let miss = frame.miss();
    let object = frame.tagged(stage, base);
    if let Some(ElementView::Typed(view)) = frame.kept_view(stage, object, true) {
        return write_typed(stage, frame, view, (value, key));
    }
    let fact = frame.value_fact(value);
    let word = frame.tagged(stage, value);
    let key = Operand::register(key);
    // Every operand's tagged word is made before the paths split, so the join sees only words
    // both paths dominate.
    let tagged_key = frame.key_word(stage, key)?;
    frame.stored(stage, Owner::Register(base), miss,
        #[inline(always)] |stage, frame, general| {
            if fact.is_subtype(Type::IMMEDIATE) {
                // An immediate needs no log unless old records keep counts (`Stage::immediate_store_open`).
                let open = stage.immediate_store_open();
                stage.guard(open, general)?;
            } else {
                frame.array_store_gate(stage, object, word, general)?;
            }
            let view = match frame.proved_view(stage, base, true) {
                Some(view) => view,
                None => dense_view(stage, frame, object, true, general)?,
            };
            let index = frame.index_key(stage, key, general)?;
            let index = stage.uextend32(index);
            replace_present(stage, view, index, word, general)
        },
        #[inline(always)] |stage, frame, miss| frame.index_set_host(stage, object, tagged_key, word, miss))?;
    Some(word)
}

/// The element rows of `step`: the word each produces, or `Unresolved` where the frame holds
/// no array records or no host path for what the dense arm leaves.
#[inline(always)]
pub(super) fn step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::ElementRead { base, key, destination } => if const { super::op_kind::admits(KIND, &[super::op_kind::ELEMENT_READ]) } { match read(stage, frame, base, key) {
            Some(word) => Step::Value(word),
            // A read the dense arm leaves is the VM's indexed read, where the frame takes it (`CallOut::LoadElement`).
            None => super::callout::loading(stage, frame, super::CallOut::LoadElement { base: Operand::register(base), key }, destination),
        } } else { unreachable!() },
        Operation::ElementReadOperand { destination, base, key } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::ELEMENT_READ_OPERAND]) } { super::callout::loading(stage, frame, super::CallOut::LoadElement { base, key }, destination) } else { unreachable!() },
        // A store the dense arm leaves is the VM's keyed store, where the frame takes it (`CallOut::StoreElement`).
        Operation::ElementWrite { value, base, key, strict } => if const { super::op_kind::admits(KIND, &[super::op_kind::ELEMENT_WRITE]) } { match write(stage, frame, (value, base, key)) {
            Some(word) => Step::Value(word),
            None => super::callout::storing(stage, frame, super::CallOut::StoreElement { object: base, index: key, value, strict })
                .map_or(Step::Unresolved, Step::Value),
        } } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::ELEMENT_READ, super::op_kind::ELEMENT_READ_OPERAND, super::op_kind::ELEMENT_WRITE]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_of` for any operation.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// The interpreter's replacement of the present element `index` of a direct array among the array
/// records at `records` with `value`, which the caller's gate proved needs no barrier log: the
/// row's store arm without a frame; `false` for any other base, a backing or array that is not
/// the array's own to write in place, a hole or an index past the elements.
#[inline(always)]
pub(crate) fn concrete_record_replace(records: *const crate::heap::ArrayObject, base: crate::Value, index: usize,
    value: crate::Value) -> bool {
    let mut stage = Concrete::new(core::ptr::null());
    prove_view(&mut stage, records as u64, base.to_bits(), true, ())
        .and_then(|view| replace_present(&mut stage, view, index as u64, value.to_bits(), ()))
        .is_some()
}
