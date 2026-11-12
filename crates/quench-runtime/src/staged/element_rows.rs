//! Element access as staged rows: `base[key]` (`GetIndex`) and `base[key] = value`
//! (`SetIndex`) over a dense array's backing. The array is proved once per access (`proved_view`:
//! a direct array record, its backing and, for a store, its sole ownership; a frame that kept
//! the proof answers it), the key's int32 is read through the frame, and the element is the
//! shared dense read or replacement (`elements`). What the dense arm cannot decide (a hole, an
//! index past the backing, a store that adds an element or needs the barrier's log, a key that
//! is no int32) is the frame's host path (`PropertyFrame::index_get_host`, `index_set_host`):
//! the VM's own element step, which the interpreter makes itself.
//!
//! The interpreter's reads and replacements of a record array are the same rows without a
//! frame (`concrete_record_element`, `concrete_record_replace`).

use super::elements::{prove_view, read_element, replace_present, ElementView};
use super::property::Owner;
use super::{Concrete, ElementFrame, Frame, NumericFrame, NumericStage, Operation, PropertyFrame, Stage, Step};
use crate::bytecode::{Op, Operand, Register, WideInstruction};
use crate::typed_ir::Type;

impl<W, P> Operation<W, P> {
    /// The element rows of `decode`: `GetIndex` of a register base and a register or immediate
    /// key (the tier defines the result register), and `SetIndex`; `None` where the instruction
    /// completes a frame or an operand names a local or a field.
    pub(super) fn decode_element(instruction: WideInstruction) -> Option<Self> {
        if instruction.result_flags() != 0 { return None; }
        Some(match instruction.op() {
            Op::GetIndex => Self::ElementRead { base: instruction.operand_b().register_index()?, key: instruction.operand_c() },
            Op::SetIndex => Self::ElementWrite { value: instruction.register_a(), base: instruction.register_b(),
                key: instruction.register_c() },
            _ => return None,
        })
    }
}

/// The view of `base` the frame kept of an earlier access, else the one this access proves and
/// keeps.
#[inline(always)]
fn proved_view<S: Stage, F: ElementFrame<S>>(stage: &mut S, frame: &mut F, base: S::Word, writable: bool,
    miss: S::Miss) -> Option<ElementView<S::Word>> {
    if let Some(view) = frame.kept_view(stage, base, writable) { return Some(view); }
    let records = frame.array_records(stage)?;
    let view = prove_view(stage, records, base, writable, miss)?;
    frame.keep_view(stage, base, writable, view);
    Some(view)
}

/// `base[key]`: the present dense element, else the VM's read. The array and the key's int32
/// are proved first, and leave at the frame's miss edge where they do not hold, so the host
/// read only ever follows a hole or an index past the backing.
#[inline(always)]
fn read<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, base: Register, key: Operand) -> Option<S::Word> {
    let miss = frame.miss();
    let object = frame.tagged(stage, base);
    let view = proved_view(stage, frame, object, false, miss)?;
    let index = frame.index_key(stage, key, miss)?;
    let index = stage.uextend32(index);
    frame.element_probed(stage, miss,
        |stage, _, absent| read_element(stage, view, index, absent),
        |stage, frame, miss| {
            let key = frame.key_word(stage, key)?;
            frame.index_get_host(stage, object, key, miss)
        })
}

/// `base[key] = value`: the present dense element replaced in place where the array owns its
/// backing alone and the value needs no barrier log (an immediate type, or the frame's gate),
/// else the VM's store.
#[inline(always)]
fn write<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, (value, base, key): (Register, Register, Register))
    -> Option<S::Word> {
    let miss = frame.miss();
    let fact = frame.value_fact(value);
    let (word, object) = (frame.tagged(stage, value), frame.tagged(stage, base));
    let key = Operand::register(key);
    // Every operand's tagged word is made before the paths split, so the join sees only words
    // both paths dominate.
    let tagged_key = frame.key_word(stage, key)?;
    frame.stored(stage, Owner::Register(base), miss,
        |stage, frame, general| {
            if !fact.is_subtype(Type::IMMEDIATE) { frame.array_store_gate(stage, object, word, general)?; }
            let view = proved_view(stage, frame, object, true, general)?;
            let index = frame.index_key(stage, key, general)?;
            let index = stage.uextend32(index);
            replace_present(stage, view, index, word, general)
        },
        |stage, frame, miss| frame.index_set_host(stage, object, tagged_key, word, miss))?;
    Some(word)
}

/// The element rows of `step`: the word each produces, or `Unresolved` where the frame holds
/// no array records or no host path for what the dense arm leaves.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let word = match operation {
        Operation::ElementRead { base, key } => read(stage, frame, base, key),
        Operation::ElementWrite { value, base, key } => write(stage, frame, (value, base, key)),
        _ => None,
    };
    word.map_or(Step::Unresolved, Step::Value)
}

/// The interpreter's `base[index]` of a direct array among the array records at `records`: the
/// row's dense arm without a frame; `None` for any other base, a hole or an index past the elements.
#[inline(always)]
pub(crate) fn concrete_record_element(records: *const crate::heap::ArrayObject, base: crate::Value, index: usize)
    -> Option<crate::Value> {
    let mut stage = Concrete::new(std::ptr::null());
    let view = prove_view(&mut stage, records as u64, base.to_bits(), false, ())?;
    read_element(&mut stage, view, index as u64, ()).map(crate::Value::from_bits)
}

/// The interpreter's replacement of the present element `index` of a direct array among the array
/// records at `records` with `value`, which the caller's gate proved needs no barrier log: the
/// row's store arm without a frame; `false` for any other base, a backing or array that is not
/// the array's own to write in place, a hole or an index past the elements.
#[inline(always)]
pub(crate) fn concrete_record_replace(records: *const crate::heap::ArrayObject, base: crate::Value, index: usize,
    value: crate::Value) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    prove_view(&mut stage, records as u64, base.to_bits(), true, ())
        .and_then(|view| replace_present(&mut stage, view, index as u64, value.to_bits(), ()))
        .is_some()
}
