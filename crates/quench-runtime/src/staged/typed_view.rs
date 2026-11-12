//! A typed array's view proved from its record, as loads over `NATIVE_TYPED_ARRAY_LAYOUT`: the
//! value is a legacy slot whose table entry names a view record (the typed array family), the
//! record's kind is the one the site observed, its published address and count are current
//! (its stamp is the process's buffer-relocation count) and it is attached. The proof is the same
//! loads on every stage: the interpreter runs it on `Concrete` (`concrete_typed_element`), a
//! specializer emits it once where a loop enters. A proof that fails leaves at `miss`, where the
//! VM publishes the view again (`Vm::publish_typed_view`) or declines it.

use super::typed_elements::{typed_read, NativeKind, TypedView, ViewKind};
use super::{value, Concrete, Stage};
use crate::heap::NATIVE_TYPED_ARRAY_LAYOUT as VIEW;

/// Where a proof reads the typed arrays' records and the buffer-relocation count, which a
/// stage holds as words: the table of record addresses by legacy slot (zero before the first
/// typed array), and the address of the count.
#[derive(Clone, Copy)]
pub(crate) struct TypedRecords<W> {
    pub(crate) entries: W,
    pub(crate) relocations: W,
}

/// What an access asks of a view: the kind its site observed, `None` where any kind serves and
/// the access dispatches on the record's, and whether it stores.
#[derive(Clone, Copy)]
pub(crate) struct ViewAccess {
    pub(crate) kind: Option<NativeKind>,
    pub(crate) writable: bool,
}

impl ViewAccess {
    pub(crate) const READ: Self = Self { kind: None, writable: false };
}

/// The record of the typed array `base`: `None` (leaving at `miss`) for any other value.
#[inline(always)]
fn view_record<S: Stage>(stage: &mut S, entries: S::Word, base: S::Word, miss: S::Miss) -> Option<S::Word> {
    let legacy = value::is_legacy_slot(stage, base);
    stage.guard(legacy, miss)?;
    let present = stage.ne_imm(entries, 0u32);
    stage.guard(present, miss)?;
    let slot = stage.and_imm(base, u64::from(u32::MAX));
    let offset = stage.mul_imm(slot, VIEW.entry_size as u64);
    let entry = stage.add(entries, offset);
    let record = stage.load_u64(entry, 0);
    let named = stage.ne_imm(record, 0u32);
    stage.guard(named, miss)?;
    Some(record)
}

/// The view of the typed array `base`, proved for `access`: `None` (leaving at `miss`) for any
/// other value, another kind than the one asked, a stale or detached record, or (to store) bytes
/// another buffer shares.
#[inline(always)]
pub(crate) fn prove_typed_view<S: Stage>(stage: &mut S, records: TypedRecords<S::Word>, base: S::Word,
    access: ViewAccess, miss: S::Miss) -> Option<TypedView<S::Word>> {
    let record = view_record(stage, records.entries, base, miss)?;
    let kind_word = stage.load_u8(record, VIEW.kind_offset as i32);
    let kind = match access.kind {
        Some(native) => {
            let observed = stage.eq_imm(kind_word, native.kind as u64);
            stage.guard(observed, miss)?;
            ViewKind::Static(native)
        }
        None => ViewKind::Dynamic(kind_word),
    };
    let stamp = stage.load_u64(record, VIEW.stamp_offset as i32);
    let current = stage.load_u64(records.relocations, 0);
    let moved = stage.sub(stamp, current);
    let fresh = stage.eq_imm(moved, 0u32);
    stage.guard(fresh, miss)?;
    let detached = stage.load_u8(record, VIEW.detached_offset as i32);
    let attached = stage.eq_imm(detached, 0u32);
    stage.guard(attached, miss)?;
    if access.writable {
        let unshared = stage.load_u8(record, VIEW.unshared_offset as i32);
        let alone = stage.eq_imm(unshared, 1u32);
        stage.guard(alone, miss)?;
    }
    let values = stage.load_u64(record, VIEW.data_offset as i32);
    let length = stage.load_u32(record, VIEW.length_offset as i32);
    Some(TypedView { values, length, kind })
}

/// The interpreter's view of the typed array `base` among the records at `entries`: the proof
/// without a frame; `None` for any other value, a record the VM has not published (or has
/// withdrawn) or one whose published address is stale.
#[inline(always)]
pub(crate) fn concrete_typed_view(entries: *const usize, base: crate::Value) -> Option<TypedView<u64>> {
    let relocations = crate::heap::buffer_relocations_address();
    let records = TypedRecords { entries: entries.addr() as u64, relocations: relocations.addr() as u64 };
    prove_typed_view(&mut Concrete::new(std::ptr::null()), records, base.to_bits(), ViewAccess::READ, ())
}

/// The interpreter's Number (an `f64`'s bits) at element `index` of `view`: the element row
/// without a frame; `None` for a kind without a native element or an index past the elements.
#[inline(always)]
pub(crate) fn concrete_typed_read(view: TypedView<u64>, index: usize) -> Option<u64> {
    typed_read(&mut Concrete::new(std::ptr::null()), view, index as u64, ())
}
