//! The storage of a Wasm GC object's fields and of a Wasm table's references, each behind
//! one word so their cells stay compact.

use crate::staged::record_heads::WasmGcHead;
use crate::value::Value;
use crate::wasm::CanonicalSignature;
use crate::wasm::gc::{GcField, ObjectLayout};
use std::ops::Range;

/// A Wasm GC object's payload: its bytes in one allocation behind one word, laid out by the
/// object's type (`ObjectLayout`). A struct with a descriptor keeps it in its last word and marks
/// the slice. The accessors take the layout the object's declarations give; the collector follows
/// only the reference words (`for_each_reference`).
#[derive(Clone, Debug, Default)]
pub(crate) struct WasmGcFields(crate::heap::thin_slice::ThinBoxSlice<u8>);

/// The bytes of a word: an array's length and a descriptor take one.
const WORD: usize = 8;

/// The word at byte `at` of `bytes`, little-endian.
fn read_word(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + WORD].try_into().expect("a whole word"))
}

/// Writes `bits` as the word at byte `at` of `bytes`, little-endian.
fn write_word(bytes: &mut [u8], at: usize, bits: u64) {
    bytes[at..at + WORD].copy_from_slice(&bits.to_le_bytes());
}

impl WasmGcFields {
    /// A zeroed payload for an object of `layout`: `length` elements for an array, and for a struct
    /// its `descriptor` (`Some`) in the last word, marked. `None` where no room is left for it.
    pub(crate) fn new(layout: &ObjectLayout, length: usize, descriptor: Option<Value>) -> Option<Self> {
        let bytes = layout.bytes(length)?;
        let mut payload = crate::heap::thin_slice::ThinBoxSlice::<u8>::try_zeroed(bytes)?;
        if let ObjectLayout::Array { .. } = layout {
            write_word(&mut payload, 0, length as u64);
        }
        if let Some(value) = descriptor {
            write_word(&mut payload, bytes.checked_sub(WORD)?, value.to_bits());
            payload.mark();
        }
        Some(Self(payload))
    }

    /// The descriptor of a struct that has one: its last word.
    pub(crate) fn descriptor(&self) -> Option<Value> {
        self.0.marked().then(|| Value::from_bits(read_word(&self.0, self.0.len() - WORD)))
    }

    /// The number of fields of a struct, or the length of an array.
    pub(crate) fn count(&self, layout: &ObjectLayout) -> usize {
        match layout {
            ObjectLayout::Struct { offsets, .. } => offsets.len(),
            ObjectLayout::Array { .. } => read_word(&self.0, 0) as usize,
        }
    }

    /// The address of field `index` of a struct, or element `index` of an array; `None` past the
    /// end. Valid until the payload is dropped or replaced.
    pub(crate) fn slot(&mut self, layout: &ObjectLayout, index: usize) -> Option<*mut u8> {
        if index >= self.count(layout) {
            return None;
        }
        let offset = layout.offset(index)?;
        Some(self.0.as_mut_ptr().wrapping_add(offset))
    }

    /// Stores the bits `bits` (the stored form of the value, `GcField::raw_of`) at field or element
    /// `index` of `layout`, which the caller has checked against the layout.
    pub(crate) fn put(&mut self, layout: &ObjectLayout, index: usize, field: GcField, bits: u64) {
        let offset = layout.offset(index).expect("a field the layout holds");
        field.store(&mut self.0[offset..offset + field.size()], bits);
    }

    /// Stores `bits` in `extent` elements of the array `layout` from element `start`.
    pub(crate) fn fill(&mut self, layout: &ObjectLayout, field: GcField, start: usize, extent: usize, bits: u64) {
        for index in start..start + extent {
            self.put(layout, index, field, bits);
        }
    }

    /// Copies the bytes `input` of this payload to `output..`, which may overlap.
    pub(crate) fn copy_within(&mut self, input: Range<usize>, output: usize) {
        self.0.copy_within(input, output);
    }

    /// Copies the bytes `input` of `source` to `output` of this payload.
    pub(crate) fn copy_from(&mut self, output: Range<usize>, source: &Self, input: Range<usize>) {
        self.0[output].copy_from_slice(&source.0[input]);
    }

    /// Calls `visit` on each reference word the object holds: a struct's reference fields and its
    /// descriptor, or an array's elements when they are references. No other word is followed.
    pub(crate) fn for_each_reference(&self, layout: &ObjectLayout, mut visit: impl FnMut(Value)) {
        match layout {
            ObjectLayout::Struct { references, .. } => {
                for &at in references.iter() {
                    visit(Value::from_bits(read_word(&self.0, at)));
                }
            }
            ObjectLayout::Array { reference: true, .. } => {
                for index in 0..self.count(layout) {
                    let at = layout.offset(index).expect("an element of the array");
                    visit(Value::from_bits(read_word(&self.0, at)));
                }
            }
            ObjectLayout::Array { reference: false, .. } => {}
        }
    }

    /// The address of field `index` of a struct, or element `index` of an array, for a read from a
    /// shared borrow: no write barrier is passed, so the address is read from, never written to.
    /// `None` past the end.
    pub(crate) fn address(&self, layout: &ObjectLayout, index: usize) -> Option<*const u8> {
        if index >= self.count(layout) {
            return None;
        }
        let offset = layout.offset(index)?;
        Some(self.0.as_ptr().wrapping_add(offset))
    }

    pub(crate) fn allocated_bytes(&self) -> usize {
        self.0.allocated_bytes()
    }
}

/// A Wasm GC object's record (`Heap::alloc_wasm_gc`): its head first, so compiled code reads the
/// payload's address, length and type index at fixed offsets (`NATIVE_WASM_GC_LAYOUT`), then the
/// declarations its layout comes from and its payload. The head names the payload's bytes: it is
/// written whenever the payload is installed (`new`, `set_fields`), and the bytes change only in place.
#[repr(C)]
pub(crate) struct WasmGcRecord {
    head: WasmGcHead,
    declarations: crate::WasmTypes,
    fields: WasmGcFields,
}

impl WasmGcRecord {
    /// The record of an object of type `ty` of `declarations`, with payload `fields`.
    pub(crate) fn new(declarations: crate::WasmTypes, ty: u32, fields: WasmGcFields) -> Self {
        let mut record = Self {
            head: WasmGcHead { payload: 0, length: 0, ty },
            declarations,
            fields,
        };
        record.sync_head();
        record
    }

    /// Points the head at the payload's current bytes.
    fn sync_head(&mut self) {
        self.head.payload = self.fields.0.as_ptr() as u64;
        self.head.length = self.fields.0.len() as u64;
    }

    pub(crate) fn declarations(&self) -> &crate::WasmTypes {
        &self.declarations
    }

    /// The index of the object's type in its declarations.
    pub(crate) fn ty(&self) -> u32 {
        self.head.ty
    }

    /// The layout of the object's payload (`declarations`' layout of its type).
    pub(crate) fn layout(&self) -> Option<&ObjectLayout> {
        self.declarations.layout(self.head.ty)
    }

    pub(crate) fn fields(&self) -> &WasmGcFields {
        &self.fields
    }

    /// The payload, mutably, for writes its bytes take in place (the head stays valid).
    pub(crate) fn fields_mut(&mut self) -> &mut WasmGcFields {
        &mut self.fields
    }

    /// The layout and the payload, mutably: a write into the payload needs the layout it follows.
    pub(crate) fn split_mut(&mut self) -> Option<(&ObjectLayout, &mut WasmGcFields)> {
        let layout = self.declarations.layout(self.head.ty)?;
        Some((layout, &mut self.fields))
    }

    /// Replaces the payload, and points the head at the new one.
    pub(crate) fn set_fields(&mut self, fields: WasmGcFields) {
        self.fields = fields;
        self.sync_head();
    }

    pub(crate) fn allocated_bytes(&self) -> usize {
        self.fields.allocated_bytes()
    }

    /// Pushes each reference word the object holds, which the collector follows.
    pub(crate) fn trace_children(&self, work: &mut Vec<Value>) {
        if let Some(layout) = self.layout() {
            self.fields.for_each_reference(layout, |value| work.push(value));
        }
    }
}

/// What a table element presents to an indirect call beside its reference, one row of the
/// table's mirror, which compiled code reads in place: the canonical signature of its function
/// (`CanonicalSignature::NULL` for the null reference, `FOREIGN` where it has none), the index
/// of the function in the code table of its instance (`NO_ENTRY` for a function the module does
/// not define) and the address of the instance's record (`InstanceContext`), which lives as long as
/// the function does. A row is written with the element,
/// so each is read from the function once per write, not per call; none of them is a reference
/// the collector follows, the element being the one that keeps its function and instance alive.
///
/// `body` is the address of the function's compiled body in its instance's published code table,
/// or 0 (`NO_ENTRY` rows, rows of an instance with no published body for the function): a call
/// that finds 0 takes the VM's slow path. It is a copy, so it is kept coherent by its writers:
/// a row is written with the body the instance publishes for the function, and an instance's
/// publication rewrites the rows that name it (`WasmTableElements::refresh_bodies`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct TableTarget {
    pub(crate) signature: CanonicalSignature,
    pub(crate) entry: u32,
    pub(crate) instance: u64,
    pub(crate) body: usize,
}

impl TableTarget {
    /// The entry of a row whose element is no function of a module.
    pub(crate) const NO_ENTRY: u32 = u32::MAX;
    /// The row of a function no module defines, which the VM checks against the expected type.
    pub(crate) const FOREIGN: Self = Self {
        signature: CanonicalSignature::FOREIGN,
        entry: Self::NO_ENTRY,
        instance: 0,
        body: 0,
    };
    /// The row of the null reference.
    pub(crate) const NULL: Self = Self {
        signature: CanonicalSignature::NULL,
        ..Self::FOREIGN
    };
    pub(crate) const SIZE: i64 = size_of::<Self>() as i64;
    pub(crate) const SIGNATURE: i32 = std::mem::offset_of!(Self, signature) as i32;
    pub(crate) const ENTRY: i32 = std::mem::offset_of!(Self, entry) as i32;
    pub(crate) const INSTANCE: i32 = std::mem::offset_of!(Self, instance) as i32;
    pub(crate) const BODY: i32 = std::mem::offset_of!(Self, body) as i32;
}

/// The words compiled code reads to reach a table's mirror: its first row and its length. They
/// live behind a box of their own, so their address stays while the cell holds them, and are
/// written by the events that move the rows (`WasmTableElements::resize`).
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub(crate) struct TableWords {
    base: usize,
    length: u64,
}

impl TableWords {
    pub(crate) const BASE: i32 = std::mem::offset_of!(Self, base) as i32;
    pub(crate) const LENGTH: i32 = std::mem::offset_of!(Self, length) as i32;
}

/// A Wasm table's references and optional maximum, boxed together so the cell keeps four
/// words; element access reads the references through `Deref`. Beside each reference sits its
/// row of the mirror (`TableTarget`), which an indirect call's check compares and calls through,
/// and the words that publish the mirror (`TableWords`).
#[derive(Debug, Default)]
pub(crate) struct WasmTableElements {
    pub(crate) values: Vec<Value>,
    pub(crate) maximum: Option<u64>,
    targets: Vec<TableTarget>,
    words: Box<TableWords>,
}

impl Clone for WasmTableElements {
    fn clone(&self) -> Self {
        let mut elements = Self {
            values: self.values.clone(),
            maximum: self.maximum,
            targets: self.targets.clone(),
            words: Box::default(),
        };
        elements.publish();
        elements
    }
}

impl WasmTableElements {
    /// `length` elements of `value`, whose function's row is `target`; none where the
    /// allocation fails.
    pub(crate) fn filled(
        length: usize,
        maximum: Option<u64>,
        value: Value,
        target: TableTarget,
    ) -> Option<Self> {
        let mut elements = Self {
            maximum,
            ..Self::default()
        };
        elements.try_grow(length, value, target).then_some(elements)
    }

    /// The row of each element, in element order.
    #[inline]
    pub(crate) fn targets(&self) -> &[TableTarget] {
        &self.targets
    }

    /// The words that publish the mirror to compiled code.
    pub(crate) fn words(&self) -> &TableWords {
        &self.words
    }

    /// The bytes of heap the references and their rows hold.
    pub(crate) fn external_bytes(&self) -> usize {
        self.values.capacity() * size_of::<Value>()
            + self.targets.capacity() * size_of::<TableTarget>()
    }

    /// Grows to `length` elements of `value`; false, with nothing changed, where memory is
    /// exhausted.
    pub(crate) fn try_grow(&mut self, length: usize, value: Value, target: TableTarget) -> bool {
        let extra = length.saturating_sub(self.values.len());
        if self.values.try_reserve_exact(extra).is_err()
            || self.targets.try_reserve_exact(extra).is_err()
        {
            return false;
        }
        self.resize(length, value, target);
        true
    }

    /// Resizes to `length` elements, new ones `value`: the one event that moves the rows, so the
    /// published words are written here.
    pub(crate) fn resize(&mut self, length: usize, value: Value, target: TableTarget) {
        self.values.resize(length, value);
        self.targets.resize(length, target);
        self.publish();
    }

    fn publish(&mut self) {
        *self.words = TableWords {
            base: self.targets.as_ptr() as usize,
            length: self.targets.len() as u64,
        };
    }

    pub(crate) fn fill(&mut self, range: Range<usize>, value: Value, target: TableTarget) {
        self.values[range.clone()].fill(value);
        self.targets[range].fill(target);
    }

    /// Copies the elements `input` to `output..`, which may overlap.
    pub(crate) fn copy_within(&mut self, input: Range<usize>, output: usize) {
        self.values.copy_within(input.clone(), output);
        self.targets.copy_within(input, output);
    }

    /// Copies the elements `input` of `source` to `output`.
    pub(crate) fn copy_from(&mut self, output: Range<usize>, source: &Self, input: Range<usize>) {
        self.values[output.clone()].copy_from_slice(&source.values[input.clone()]);
        self.targets[output].copy_from_slice(&source.targets[input]);
    }

    /// Writes `values`, whose functions have rows `targets`, from element `output`.
    pub(crate) fn copy_in(&mut self, output: usize, values: &[Value], targets: &[TableTarget]) {
        self.values[output..output + values.len()].copy_from_slice(values);
        self.targets[output..output + targets.len()].copy_from_slice(targets);
    }

    /// Rewrites the `body` of each row `refresh` names with `Some`: the rows of an instance whose
    /// publication changed, which the publisher calls for every table that holds one of its
    /// functions. `None` leaves the row alone. Rows whose entry is `NO_ENTRY` keep 0 whatever
    /// `refresh` says, so a body is never attached to a function no module defines.
    pub(crate) fn refresh_bodies(&mut self, mut refresh: impl FnMut(&TableTarget) -> Option<usize>) {
        for row in &mut self.targets {
            if row.entry == TableTarget::NO_ENTRY {
                row.body = 0;
            } else if let Some(body) = refresh(&*row) {
                row.body = body;
            }
        }
    }
}

macro_rules! deref_to_values {
    ($($owner:ty),*) => {$(
        impl std::ops::Deref for $owner {
            type Target = Vec<Value>;
            fn deref(&self) -> &Vec<Value> {
                &self.values
            }
        }

        impl std::ops::DerefMut for $owner {
            fn deref_mut(&mut self) -> &mut Vec<Value> {
                &mut self.values
            }
        }
    )*};
}
deref_to_values!(WasmTableElements);

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: TableTarget = TableTarget::NULL;
    const RIGHT: TableTarget = TableTarget::FOREIGN;
    const LINKED: TableTarget = TableTarget {
        signature: CanonicalSignature::NULL,
        entry: 7,
        instance: 9,
        body: 11,
    };

    fn elements(left: usize) -> WasmTableElements {
        let mut elements =
            WasmTableElements::filled(left, None, Value::from_bits(1), LEFT).unwrap();
        elements.resize(left + 2, Value::from_bits(2), RIGHT);
        elements
    }

    fn pairs(elements: &WasmTableElements) -> Vec<(u64, TableTarget)> {
        elements
            .values
            .iter()
            .map(|value| value.to_bits())
            .zip(elements.targets().iter().copied())
            .collect()
    }

    #[test]
    fn a_row_stays_with_its_reference_through_every_write() {
        let mut table = elements(2);
        assert_eq!(table.targets().len(), table.values.len());
        table.fill(1..3, Value::from_bits(3), RIGHT);
        assert_eq!(
            pairs(&table),
            [(1, LEFT), (3, RIGHT), (3, RIGHT), (2, RIGHT)]
        );
        table.copy_within(0..2, 2);
        assert_eq!(
            pairs(&table),
            [(1, LEFT), (3, RIGHT), (1, LEFT), (3, RIGHT)]
        );
        let source = elements(1);
        table.copy_from(0..2, &source, 1..3);
        assert_eq!(
            pairs(&table),
            [(2, RIGHT), (2, RIGHT), (1, LEFT), (3, RIGHT)]
        );
        table.copy_in(2, &[Value::from_bits(4)], &[LINKED]);
        assert_eq!(table.values[2].to_bits(), 4);
        assert_eq!(table.targets()[2], LINKED);
    }

    #[test]
    fn growth_extends_both_arrays_and_counts_both() {
        let mut table = elements(1);
        let before = table.external_bytes();
        assert!(table.try_grow(64, Value::from_bits(5), LEFT));
        assert_eq!(table.targets().len(), 64);
        assert_eq!(table.values.len(), 64);
        assert_eq!(table.targets()[63], LEFT);
        assert!(table.external_bytes() > before);
        assert!(!table.try_grow(usize::MAX, Value::from_bits(5), LEFT));
        assert_eq!(table.targets().len(), table.values.len());
    }

    #[test]
    fn the_published_words_follow_the_rows_through_growth_and_a_clone() {
        let mut table = elements(1);
        let read = |table: &WasmTableElements| {
            let words = table.words();
            (words.base, words.length)
        };
        assert_eq!(read(&table), (table.targets().as_ptr() as usize, 3));
        assert!(table.try_grow(4096, Value::from_bits(5), LEFT));
        assert_eq!(read(&table), (table.targets().as_ptr() as usize, 4096));
        let copy = table.clone();
        assert_eq!(read(&copy), (copy.targets().as_ptr() as usize, 4096));
        assert_ne!(read(&copy).0, read(&table).0);
    }

    #[test]
    fn a_row_is_laid_out_as_compiled_code_reads_it() {
        assert_eq!(TableTarget::SIZE, 24);
        assert_eq!(
            [TableTarget::SIGNATURE, TableTarget::ENTRY, TableTarget::INSTANCE, TableTarget::BODY],
            [0, 4, 8, 16]
        );
        assert_eq!([TableWords::BASE, TableWords::LENGTH], [0, 8]);
    }
}
