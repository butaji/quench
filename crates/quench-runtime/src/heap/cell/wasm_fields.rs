//! The storage of a Wasm GC object's fields and of a Wasm table's references, each behind
//! one word so their cells stay compact.

use crate::value::Value;
use crate::wasm::CanonicalSignature;
use std::ops::Range;

/// A Wasm GC object's field values in one allocation behind one word. A struct with a
/// descriptor keeps it after its fields and marks the slice; `Deref` reads the fields.
#[derive(Clone, Debug, Default)]
pub(crate) struct WasmGcFields(crate::heap::thin_slice::ThinBoxSlice<Value>);

impl WasmGcFields {
    /// `len` fields made by `field(index)`, then `descriptor`; `None` when one is missing
    /// or no room is left for them.
    pub(crate) fn try_new(len: usize, descriptor: Option<Value>, mut field: impl FnMut(usize) -> Option<Value>) -> Option<Self> {
        let total = len.checked_add(usize::from(descriptor.is_some()))?;
        let mut values = crate::heap::thin_slice::ThinBoxSlice::try_from_fn(total, |index| if index < len { field(index) } else { descriptor })?;
        if descriptor.is_some() {
            values.mark();
        }
        Some(Self(values))
    }

    pub(crate) fn descriptor(&self) -> Option<Value> {
        self.0.marked().then(|| self.0[self.0.len() - 1])
    }

    /// Every value the object holds, its descriptor included: the edges a trace follows.
    pub(crate) fn edges(&self) -> &[Value] {
        &self.0
    }

    pub(crate) fn allocated_bytes(&self) -> usize {
        self.0.allocated_bytes()
    }
}

impl From<Vec<Value>> for WasmGcFields {
    fn from(values: Vec<Value>) -> Self {
        Self(values.into())
    }
}

impl std::ops::Deref for WasmGcFields {
    type Target = [Value];
    #[inline(always)]
    fn deref(&self) -> &[Value] {
        let values = &*self.0;
        &values[..values.len() - usize::from(self.0.marked())]
    }
}

impl std::ops::DerefMut for WasmGcFields {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [Value] {
        let fields = self.len();
        &mut self.0[..fields]
    }
}

/// What a table element presents to an indirect call beside its reference, one row of the
/// table's mirror, which compiled code reads in place: the canonical signature of its function
/// (`CanonicalSignature::NULL` for the null reference, `FOREIGN` where it has none), the index
/// of the function in the code table of its instance (`NO_ENTRY` for a function the module does
/// not define) and the instance's environment (`Value` bits). A row is written with the element,
/// so each is read from the function once per write, not per call; none of them is a reference
/// the collector follows, the element being the one that keeps its function and instance alive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct TableTarget {
    pub(crate) signature: CanonicalSignature,
    pub(crate) entry: u32,
    pub(crate) instance: u64,
}

impl TableTarget {
    /// The entry of a row whose element is no function of a module.
    pub(crate) const NO_ENTRY: u32 = u32::MAX;
    /// The row of a function no module defines, which the VM checks against the expected type.
    pub(crate) const FOREIGN: Self = Self {
        signature: CanonicalSignature::FOREIGN,
        entry: Self::NO_ENTRY,
        instance: 0,
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
        assert_eq!(TableTarget::SIZE, 16);
        assert_eq!(
            [TableTarget::SIGNATURE, TableTarget::ENTRY, TableTarget::INSTANCE],
            [0, 4, 8]
        );
        assert_eq!([TableWords::BASE, TableWords::LENGTH], [0, 8]);
    }
}
