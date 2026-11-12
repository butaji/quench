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

/// A Wasm table's references and optional maximum, boxed together so the cell keeps four
/// words; element access reads the references through `Deref`. Beside each reference sits the
/// canonical signature of its function (`CanonicalSignature::NULL` for the null reference,
/// `FOREIGN` where it has none), which an indirect call's check compares: it is written with the
/// element, so a signature is read from its function once per write, not per call.
#[derive(Clone, Debug, Default)]
pub(crate) struct WasmTableElements {
    pub(crate) values: Vec<Value>,
    pub(crate) maximum: Option<u64>,
    signatures: Vec<CanonicalSignature>,
}

impl WasmTableElements {
    /// `length` elements of `value`, whose function's signature is `signature`; none where the
    /// allocation fails.
    pub(crate) fn filled(
        length: usize,
        maximum: Option<u64>,
        value: Value,
        signature: CanonicalSignature,
    ) -> Option<Self> {
        let mut elements = Self {
            maximum,
            ..Self::default()
        };
        elements.try_grow(length, value, signature).then_some(elements)
    }

    /// The signature of each element's function, in element order.
    pub(crate) fn signatures(&self) -> &[CanonicalSignature] {
        &self.signatures
    }

    /// The bytes of heap the references and their signatures hold.
    pub(crate) fn external_bytes(&self) -> usize {
        self.values.capacity() * size_of::<Value>()
            + self.signatures.capacity() * size_of::<CanonicalSignature>()
    }

    /// Grows to `length` elements of `value`; false, with nothing changed, where memory is
    /// exhausted.
    pub(crate) fn try_grow(
        &mut self,
        length: usize,
        value: Value,
        signature: CanonicalSignature,
    ) -> bool {
        let extra = length.saturating_sub(self.values.len());
        if self.values.try_reserve_exact(extra).is_err()
            || self.signatures.try_reserve_exact(extra).is_err()
        {
            return false;
        }
        self.resize(length, value, signature);
        true
    }

    pub(crate) fn resize(&mut self, length: usize, value: Value, signature: CanonicalSignature) {
        self.values.resize(length, value);
        self.signatures.resize(length, signature);
    }

    pub(crate) fn fill(&mut self, range: Range<usize>, value: Value, signature: CanonicalSignature) {
        self.values[range.clone()].fill(value);
        self.signatures[range].fill(signature);
    }

    /// Copies the elements `input` to `output..`, which may overlap.
    pub(crate) fn copy_within(&mut self, input: Range<usize>, output: usize) {
        self.values.copy_within(input.clone(), output);
        self.signatures.copy_within(input, output);
    }

    /// Copies the elements `input` of `source` to `output`.
    pub(crate) fn copy_from(&mut self, output: Range<usize>, source: &Self, input: Range<usize>) {
        self.values[output.clone()].copy_from_slice(&source.values[input.clone()]);
        self.signatures[output].copy_from_slice(&source.signatures[input]);
    }

    /// Writes `values`, whose functions have `signatures`, from element `output`.
    pub(crate) fn copy_in(
        &mut self,
        output: usize,
        values: &[Value],
        signatures: &[CanonicalSignature],
    ) {
        self.values[output..output + values.len()].copy_from_slice(values);
        self.signatures[output..output + signatures.len()].copy_from_slice(signatures);
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

    const LEFT: CanonicalSignature = CanonicalSignature::NULL;
    const RIGHT: CanonicalSignature = CanonicalSignature::FOREIGN;

    fn elements(left: usize) -> WasmTableElements {
        let mut elements =
            WasmTableElements::filled(left, None, Value::from_bits(1), LEFT).unwrap();
        elements.resize(left + 2, Value::from_bits(2), RIGHT);
        elements
    }

    fn pairs(elements: &WasmTableElements) -> Vec<(u64, CanonicalSignature)> {
        elements
            .values
            .iter()
            .map(|value| value.to_bits())
            .zip(elements.signatures().iter().copied())
            .collect()
    }

    #[test]
    fn a_signature_stays_with_its_reference_through_every_write() {
        let mut table = elements(2);
        assert_eq!(table.signatures().len(), table.values.len());
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
        table.copy_in(2, &[Value::from_bits(4)], &[RIGHT]);
        assert_eq!(table.values[2].to_bits(), 4);
        assert_eq!(table.signatures()[2], RIGHT);
    }

    #[test]
    fn growth_extends_both_arrays_and_counts_both() {
        let mut table = elements(1);
        let before = table.external_bytes();
        assert!(table.try_grow(64, Value::from_bits(5), LEFT));
        assert_eq!(table.signatures().len(), 64);
        assert_eq!(table.values.len(), 64);
        assert_eq!(table.signatures()[63], LEFT);
        assert!(table.external_bytes() > before);
        assert!(!table.try_grow(usize::MAX, Value::from_bits(5), LEFT));
        assert_eq!(table.signatures().len(), table.values.len());
    }
}
