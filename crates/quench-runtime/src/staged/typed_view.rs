//! A typed array's view proved from its record, as loads over `NATIVE_TYPED_ARRAY_LAYOUT`: the
//! value is a legacy slot below the table's pages whose entry names a view record (the typed array family), the
//! record's kind is the one the site observed, its published address and count are current
//! (its stamp is the process's buffer-relocation count) and it is attached. The proof is the same
//! loads on every stage: the interpreter runs it on `Concrete` (`concrete_typed_element`), a
//! specializer emits it once where a loop enters. A proof that fails leaves at `miss`, where the
//! VM publishes the view again (`Vm::publish_typed_view`) or declines it.

use super::typed_elements::{element_number, field_bits, store_element_number, store_field_bits, typed_read, NativeKind, TypedView, ViewKind};
use super::region::Region;
use super::{value, Concrete, Stage};
use crate::heap::{TypedArrayKind, NATIVE_TYPED_ARRAY_LAYOUT as VIEW};

/// Where a proof reads the typed arrays' records and the buffer-relocation count, which a
/// stage holds as words: the directory of the pages of record addresses by legacy slot, its page
/// count, and the address of the count.
#[derive(Clone, Copy)]
pub struct TypedRecords<W> {
    pub directory: W,
    pub pages: W,
    pub relocations: W,
}

/// What an access asks of a view: the kind its site observed, `None` where any kind serves and
/// the access dispatches on the record's, and whether it stores.
#[derive(Clone, Copy)]
pub struct ViewAccess {
    pub kind: Option<NativeKind>,
    pub writable: bool,
}

impl ViewAccess {
    pub(crate) const READ: Self = Self { kind: None, writable: false };
}

/// The record of the typed array `base`: `None` (leaving at `miss`) for any other value.
#[inline(always)]
pub(crate) fn view_record<S: Stage>(stage: &mut S, records: TypedRecords<S::Word>, base: S::Word, miss: S::Miss) -> Option<S::Word> {
    let legacy = value::is_legacy_slot(stage, base);
    stage.guard(legacy, miss)?;
    let slot = stage.and_imm(base, u64::from(u32::MAX));
    let page = stage.shr_imm(slot, VIEW.page_shift);
    let paged = stage.ult(page, records.pages);
    stage.guard(paged, miss)?;
    let page_offset = stage.mul_imm(page, VIEW.entry_size as u64);
    let page_entry = stage.add(records.directory, page_offset);
    let entries = stage.load_u64(Region::HeapMeta.mutable(), page_entry, 0);
    let within = stage.and_imm(slot, VIEW.page_mask as u64);
    let offset = stage.mul_imm(within, VIEW.entry_size as u64);
    let entry = stage.add(entries, offset);
    let record = stage.load_u64(Region::HeapMeta.mutable(), entry, 0);
    let named = stage.ne_imm(record, 0u32);
    stage.guard(named, miss)?;
    Some(record)
}

/// The view of the typed array `base`, proved for `access`: `None` (leaving at `miss`) for any
/// other value, another kind than the one asked, a stale or detached record, or (to store) bytes
/// another buffer shares.
#[inline(always)]
pub fn prove_typed_view<S: Stage>(stage: &mut S, records: TypedRecords<S::Word>, base: S::Word,
    access: ViewAccess, miss: S::Miss) -> Option<TypedView<S::Word>> {
    let record = view_record(stage, records, base, miss)?;
    let kind_word = stage.load_u8(Region::Records.mutable(), record, VIEW.kind_offset as i32);
    let kind = match access.kind {
        Some(native) => {
            let observed = stage.eq_imm(kind_word, native.kind as u64);
            stage.guard(observed, miss)?;
            ViewKind::Static(native)
        }
        None => ViewKind::Dynamic(kind_word),
    };
    let stamp = stage.load_u64(Region::Records.mutable(), record, VIEW.stamp_offset as i32);
    let current = stage.load_u64(Region::HeapMeta.mutable(), records.relocations, 0);
    let moved = stage.sub(stamp, current);
    let fresh = stage.eq_imm(moved, 0u32);
    stage.guard(fresh, miss)?;
    let detached = stage.load_u8(Region::Records.mutable(), record, VIEW.detached_offset as i32);
    let attached = stage.eq_imm(detached, 0u32);
    stage.guard(attached, miss)?;
    if access.writable {
        let unshared = stage.load_u8(Region::Records.mutable(), record, VIEW.unshared_offset as i32);
        let alone = stage.eq_imm(unshared, 1u32);
        stage.guard(alone, miss)?;
    }
    let values = stage.load_u64(Region::Records.mutable(), record, VIEW.data_offset as i32);
    let length = stage.load_u32(Region::Records.mutable(), record, VIEW.length_offset as i32);
    Some(TypedView { values, length, kind, within: false })
}

/// The interpreter's view of the typed array `base` among the record pages of `directory`: the proof
/// without a frame; `None` for any other value, a record the VM has not published (or has
/// withdrawn) or one whose published address is stale.
#[inline(always)]
pub(crate) fn concrete_typed_view(directory: (*const usize, usize), base: crate::Value) -> Option<TypedView<u64>> {
    concrete_typed_access(directory, base, ViewAccess::READ)
}

/// The interpreter's view of the typed array `base` proved for `access` (a kind the site observed,
/// and whether it stores), among the record pages of `directory`: the element row's proof without
/// a frame, a store's `unshared` test included.
#[inline(always)]
pub fn concrete_typed_access((directory, pages): (*const usize, usize), base: crate::Value, access: ViewAccess)
    -> Option<TypedView<u64>> {
    let relocations = crate::heap::buffer_relocations_address();
    let records = TypedRecords { directory: directory.addr() as u64, pages: pages as u64, relocations: relocations.addr() as u64 };
    prove_typed_view(&mut Concrete::new(std::ptr::null()), records, base.to_bits(), access, ())
}

/// The interpreter's Number (an `f64`'s bits) at element `index` of `view`: the element row
/// without a frame; `None` for a kind without a native element or an index past the elements.
#[inline(always)]
pub(crate) fn concrete_typed_read(view: TypedView<u64>, index: usize) -> Option<u64> {
    typed_read(&mut Concrete::new(std::ptr::null()), view, index as u64, ())
}

/// The Number of the DataView element `kind` at byte `index` of `bytes` (a view's bytes), in
/// `little_endian` order: the bounds check and the element's read (`element_number`). The fast
/// path and the native DataView get are this one body. `None` past the bytes.
#[inline(always)]
pub(crate) fn data_view_get(bytes: &[u8], index: usize, kind: TypedArrayKind, little_endian: bool) -> Option<f64> {
    let field = bytes.get(index..index.checked_add(kind.width())?)?;
    Some(element_number(kind, field, little_endian))
}

/// `data_view_get`'s set: the Number `number` written as the element `kind` at byte `index` of
/// `bytes` (`store_element_number`). `None` past the bytes, writing nothing.
#[inline(always)]
pub(crate) fn data_view_set(bytes: &mut [u8], index: usize, kind: TypedArrayKind, number: f64,
    little_endian: bool) -> Option<()> {
    let field = bytes.get_mut(index..index.checked_add(kind.width())?)?;
    store_element_number(kind, number, field, little_endian);
    Some(())
}

/// The unsigned `width`-byte field at byte `index` of `bytes`, in `little_endian` order: a DataView
/// BigInt element's bits (`field_bits`). `None` past the bytes.
#[inline(always)]
pub(crate) fn data_view_bits(bytes: &[u8], index: usize, width: usize, little_endian: bool) -> Option<u64> {
    let field = bytes.get(index..index.checked_add(width)?)?;
    Some(field_bits(field, little_endian))
}

/// `data_view_bits`' set: the low `width` bytes of `bits` at byte `index` of `bytes`
/// (`store_field_bits`). `None` past the bytes, writing nothing.
#[inline(always)]
pub(crate) fn data_view_store_bits(bytes: &mut [u8], index: usize, width: usize, bits: u64,
    little_endian: bool) -> Option<()> {
    let field = bytes.get_mut(index..index.checked_add(width)?)?;
    store_field_bits(field, bits, little_endian);
    Some(())
}
