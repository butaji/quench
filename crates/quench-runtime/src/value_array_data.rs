#[derive(Debug, Clone, PartialEq)]
pub struct ArrayData {
    values: Vec<Value>,
    length: usize,
    properties: Vec<(String, Value)>,
    descriptors: Vec<(String, Value)>,
    arguments: bool,
    strict_arguments: bool,
    mapped: Vec<Option<Rc<RefCell<Value>>>>,
    deleted: Vec<bool>,
    prototype: std::cell::RefCell<Option<Value>>,
    argument_live: Option<Rc<RefCell<ArgumentLive>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArgumentLive {
    pub values: Vec<Value>,
    pub length: usize,
    pub mapped: Vec<Option<Rc<RefCell<Value>>>>,
    pub deleted: Vec<bool>,
    /// Optional override for `arguments.length`. Per spec 10.6 the property
    /// is writable: a plain value-property assignment stores the value
    /// here so that subsequent reads return it verbatim instead of
    /// coercing through the array's length slot.
    pub length_override: Option<Value>,
}

impl ArrayData {
    pub fn new(values: Vec<Value>) -> Self {
        let length = values.len();
        let live_values = values.clone();
        Self {
            values,
            length,
            properties: Vec::new(),
            descriptors: Vec::new(),
            arguments: false,
            strict_arguments: false,
            mapped: Vec::new(),
            deleted: Vec::new(),
            prototype: std::cell::RefCell::new(None),
            argument_live: Some(Rc::new(RefCell::new(ArgumentLive {
                values: live_values,
                length,
                mapped: Vec::new(),
                deleted: Vec::new(),
                length_override: None,
            }))),
        }
    }

    pub(crate) fn new_arguments(values: Vec<Value>, strict: bool) -> Self {
        let mut data = Self::new(values);
        data.arguments = true;
        data.strict_arguments = strict;
        data.argument_live = Some(Rc::new(RefCell::new(ArgumentLive {
            values: data.values.clone(),
            length: data.length,
            mapped: data.mapped.clone(),
            deleted: data.deleted.clone(),
            length_override: None,
        })));
        data
    }

    pub(crate) fn is_arguments(&self) -> bool {
        self.arguments
    }

    pub(crate) fn is_strict_arguments(&self) -> bool {
        self.strict_arguments
    }
    /// Borrow the canonical dense storage and its header facts together.
    /// Callers must derive all fast-path decisions from this tuple; no shadow
    /// length or element cache is permitted.
    #[inline]
    pub(crate) fn hot_storage(&self) -> (&[Value], usize, ArrayKind) {
        (&self.values, self.logical_len(), self.kind)
    }


    pub fn logical_len(&self) -> usize {
        self.argument_live
            .as_ref()
            .map_or(self.length, |live| live.borrow().length)
    }

    /// Return the value of `arguments.length` if a plain-value override
    /// was written through `SetProperty`/`define_own_property`. When no
    /// override is set, returns the argument's logical length coerced to
    /// a number.
    pub(crate) fn arguments_length_value(&self) -> Value {
        if let Some(live) = &self.argument_live {
            if let Some(value) = live.borrow().length_override.clone() {
                return value;
            }
        }
        Value::Number(self.logical_len() as f64)
    }

    /// Set the override value of `arguments.length`. Subsequent reads
    /// return this value verbatim, regardless of the array's actual
    /// length slot.
    pub(crate) fn set_arguments_length_override(&self, value: Value) {
        if let Some(live) = &self.argument_live {
            live.borrow_mut().length_override = Some(value);
        }
    }

    /// Number of physical values currently held. The array's logical
    /// length may be larger when the runtime has accepted a length
    /// write that exceeds the existing element count.
    pub(crate) fn physical_len(&self) -> usize {
        self.values.len()
    }


    #[inline]
    pub(crate) fn is_sparse(&self) -> bool {
        matches!(self.kind, ArrayKind::Sparse)
    }
    pub(crate) fn is_dense(&self) -> bool {
        !matches!(self.kind, ArrayKind::Sparse)
    }


    #[inline]
    pub(crate) fn is_numeric_packed(&self) -> bool {
        matches!(self.kind, ArrayKind::PackedInt | ArrayKind::PackedDouble)
    }

    /// Whether mutation may use the dense backing store without consulting
    /// indexed properties, descriptors, prototypes, or argument mappings.
    #[inline]
    pub(crate) fn is_packed_ordinary(&self) -> bool {
        self.is_packed()
            && self.logical_len() == self.physical_len()
            && self.properties.is_empty()
            && self.descriptors.is_empty()
            && self.prototype.borrow().is_none()
            && !self.arguments
            && self.argument_live.is_none()
            && self.deleted.iter().all(|deleted| !deleted)
            && self.mapped.iter().all(Option::is_none)
    }
    /// Borrow the live argument data without consuming `self`.
    /// `argument_live` field is shared between the original and any
    /// `Rc::make_mut` clones of this data, so overrides stored via
    /// `set_arguments_length_override` are visible to all references.
    pub(crate) fn argument_live_view(&self) -> Option<std::cell::Ref<'_, ArgumentLive>> {
        self.argument_live.as_ref().map(|live| live.borrow())
    }

    pub(crate) fn prototype(&self) -> Option<Value> {
        self.prototype.borrow().clone()
    }

    pub(crate) fn set_prototype(&self, prototype: Value) {
        self.prototype.replace(Some(prototype));
    }

    pub fn set_length(&mut self, length: usize) {
        if let Some(live) = &self.argument_live {
            let mut live = live.borrow_mut();
            live.values.truncate(length);
            live.deleted.truncate(length);
            live.mapped.truncate(length);
            live.length = length;
        }
        if length < self.length {
            self.values.truncate(length);
            self.deleted.truncate(length);
            self.mapped.truncate(length);
            self.properties.retain(|(key, _)| keep_index(key, length));
            self.descriptors.retain(|(key, _)| keep_index(key, length));
        }
        self.length = length;
        self.kind = monotonic_kind(self.kind, classify_kind_with_holes(&self.values, &self.deleted, length));
    }

    pub fn set_index(&mut self, index: usize, value: Value) {
        // Sparse arrays keep indexed entries in the property store even when
        // a later write happens to be adjacent to the dense prefix. Once a
        // representation has transitioned, never re-expand its backing Vec.
        if self.is_sparse() && index >= self.values.len() {
            self.set_sparse_index(index, value);
            return;
        }
        if index > self.values.len() {
            self.set_sparse_index(index, value);
            return;
        }
        if let Some(live) = &self.argument_live {
            let mut live = live.borrow_mut();
            set_live_index(&mut live, index, value.clone());
        }
        if let Some(Some(binding)) = self.mapped.get(index) {
            *binding.borrow_mut() = value.clone();
        }
        if self.values.len() <= index {
            self.values
                .resize(index.saturating_add(1), Value::Undefined);
        }
        self.values[index] = value;
        if self.deleted.len() <= index {
            self.deleted.resize(index.saturating_add(1), false);
        }
        self.deleted[index] = false;
        self.length = self.length.max(index.saturating_add(1));
        self.kind = monotonic_kind(
            self.kind,
            classify_kind_with_holes(&self.values, &self.deleted, self.length),
        );
    }

    /// Grow dense storage geometrically so sequential appends do not
    /// repeatedly reallocate, while preserving undefined holes.
    fn grow_dense_storage(&mut self, required: usize) {
        let current = self.values.len();
        if required <= self.values.capacity() {
            self.values.resize(required, Value::Undefined);
            return;
        }
        // Base the next boundary on capacity, not only on the current prefix:
        // allocators are permitted to return more than requested.
        let target = self
            .values
            .capacity()
            .saturating_mul(2)
            .max(4)
            .max(required);
        self.values.reserve(target.saturating_sub(current));
        self.values.resize(required, Value::Undefined);
        debug_assert!(self.values.len() >= required);
        debug_assert!(self.values.capacity() >= self.values.len());
    }

    fn set_sparse_index(&mut self, index: usize, value: Value) {
        self.set_property(&index.to_string(), value);
        let length = index.saturating_add(1);
        if let Some(live) = &self.argument_live {
            let mut live = live.borrow_mut();
            live.length = live.length.max(length);
        }
        self.length = self.length.max(length);
    }

    pub(crate) fn append_live(&self, values: &[Value]) {
        let Some(live) = &self.argument_live else {
            return;
        };
        let mut live = live.borrow_mut();
        for value in values {
            let index = live.length;
            set_live_index(&mut live, index, value.clone());
        }
    }

    pub(crate) fn values_mut(&mut self) -> &mut [Value] {
        &mut self.values
    }

    pub(crate) fn get_index(&self, index: usize) -> Option<Value> {
        if index >= self.logical_len() {
            return None;
        }
        if let Some(live) = &self.argument_live {
            return live_index(&live.borrow(), index).or_else(|| self.property(&index.to_string()));
        }
        if self.deleted.get(index) == Some(&true) {
            return None;
        }
        self.mapped
            .get(index)
            .and_then(Option::as_ref)
            .map(|binding| binding.borrow().clone())
            .or_else(|| self.values.get(index).cloned())
            .or_else(|| self.property(&index.to_string()))
    }

    /// Read a packed numeric slot without materializing an owned `Value`.
    ///
    /// The returned scalar is the unboxed representation used by numeric
    /// array fast paths. Callers that need JavaScript semantics should use
    /// `get_index`, which remains the authoritative Value-producing path.
    #[inline]
    pub(crate) fn dense_number_at(&self, index: usize) -> Option<f64> {
        let value = self.dense_value_at(index)?;
        match value {
            Value::Number(number) => Some(*number),
            _ => None,
        }
    }

    /// Store an unboxed numeric slot while retaining canonical `Value`
    /// semantics at the storage boundary.
    #[inline]
    pub(crate) fn set_numeric_index(&mut self, index: usize, number: f64) {
        self.set_index(index, Value::Number(number));
    }

    #[inline]
    pub(crate) fn dense_value_at(&self, index: usize) -> Option<&Value> {
        if index >= self.logical_len()
            || index >= self.values.len()
            || self.deleted.get(index) == Some(&true)
        {
            return None;
        }
        // SAFETY: the explicit checks above prove `index < self.values.len()`.
        Some(unsafe { self.values.get_unchecked(index) })
    }

    #[inline]
    pub(crate) fn last_dense_value(&self) -> Option<&Value> {
        self.values
            .len()
            .checked_sub(1)
            .and_then(|index| self.dense_value_at(index))
    }

    pub(crate) fn dense_value_at_mut(&mut self, index: usize) -> Option<&mut Value> {
        if index >= self.logical_len()
            || index >= self.values.len()
            || self.deleted.get(index) == Some(&true)
        {
            return None;
        }
        // SAFETY: the explicit checks above prove `index < self.values.len()`.
        Some(unsafe { self.values.get_unchecked_mut(index) })
    }


    pub(crate) fn has_index(&self, index: usize) -> bool {
        if let Some(live) = &self.argument_live {
            let live = live.borrow();
            return (index < live.length
                && live.deleted.get(index) != Some(&true)
                && (index < live.values.len()
                    || live.mapped.get(index).and_then(Option::as_ref).is_some()))
                || self.property(&index.to_string()).is_some();
        }
        let logical_len = self.logical_len();
        index < logical_len
            && self.deleted.get(index) != Some(&true)
            && (index < self.values.len()
                || self.mapped.get(index).and_then(Option::as_ref).is_some()
                || self.property(&index.to_string()).is_some())
    }
    /// Copy a fully dense range within the backing store using memmove ordering.
    ///
    /// This fast path is deliberately conservative: a hole in either range
    /// means the caller must use the property-aware slow path.  Keeping the
    /// check here makes the no-allocation copy safe even when callers are
    /// changed independently of the array representation.
    pub(crate) fn copy_dense_within(&mut self, src: usize, dst: usize, len: usize) -> bool {
        let Some(src_end) = src.checked_add(len) else {
            return false;
        };
        let Some(dst_end) = dst.checked_add(len) else {
            return false;
        };
        if src_end > self.values.len() || dst_end > self.values.len() {
            return false;
        }
        if self.deleted.get(src..src_end).is_some_and(|range| range.iter().any(|&hole| hole))
            || self
                .deleted
                .get(dst..dst_end)
                .is_some_and(|range| range.iter().any(|&hole| hole))
        {
            return false;
        }

        // Value is not Copy, so Vec::copy_within is unavailable.  Clone in
        // memmove order instead, avoiding a temporary allocation while still
        // preserving the source values for overlapping ranges.
        if dst > src && dst < src_end {
            for offset in (0..len).rev() {
                self.values[dst + offset] = self.values[src + offset].clone();
            }
        } else {
            for offset in 0..len {
                self.values[dst + offset] = self.values[src + offset].clone();
            }
        }
        true
    }

    pub(crate) fn next_index(&self, start: usize, length: usize) -> Option<usize> {
        let dense_end = self.values.len().min(length);
        (start..dense_end)
            .find(|&index| self.has_index(index))
            .or_else(|| {
                self.properties
                    .iter()
                    .filter_map(|(key, _)| {
                        let index = crate::arrays::array_index(key)? as usize;
                        (index >= start && index < length && self.has_index(index)).then_some(index)
                    })
                    .min()
            })
    }

    pub(crate) fn snapshot(&self) -> Vec<Value> {
        (0..self.length)
            .map(|index| self.get_index(index).unwrap_or(Value::Undefined))
            .collect()
    }

    pub(crate) fn map_index(&mut self, index: usize, binding: Rc<RefCell<Value>>) {
        if let Some(live) = &self.argument_live {
            let mut live = live.borrow_mut();
            live.mapped.resize(index.saturating_add(1), None);
            live.mapped[index] = Some(Rc::clone(&binding));
        }
        self.mapped.resize(index.saturating_add(1), None);
        self.mapped[index] = Some(binding);
    }

    pub(crate) fn disconnect_index(&mut self, index: usize) {
        if let Some(live) = &self.argument_live {
            if let Some(mapping) = live.borrow_mut().mapped.get_mut(index) {
                *mapping = None;
            }
        }
        if let Some(mapping) = self.mapped.get_mut(index) {
            *mapping = None;
        }
    }

    pub(crate) fn descriptor(&self, key: &str) -> Option<Value> {
        self.descriptors
            .iter()
            .rev()
            .find_map(|(name, value)| (name == key).then(|| value.clone()))
    }

    pub(crate) fn define_descriptor(&mut self, key: &str, descriptor: Value) {
        if let Some((_, current)) = self
            .descriptors
            .iter_mut()
            .rev()
            .find(|(name, _)| name == key)
        {
            *current = descriptor;
            return;
        }
        self.descriptors.push((key.to_string(), descriptor));
    }

    pub(crate) fn descriptor_keys(&self) -> Vec<String> {
        self.descriptors
            .iter()
            .map(|(key, _)| key.clone())
            .collect()
    }

    pub(crate) fn property(&self, key: &str) -> Option<Value> {
        self.properties
            .iter()
            .rev()
            .find_map(|(name, value)| (name == key).then(|| value.clone()))
    }

    pub(crate) fn property_keys(&self) -> Vec<String> {
        self.properties.iter().map(|(key, _)| key.clone()).collect()
    }

    pub(crate) fn set_property(&mut self, key: &str, value: Value) {
        if let Some((_, current)) = self
            .properties
            .iter_mut()
            .rev()
            .find(|(name, _)| name == key)
        {
            *current = value;
        } else {
            self.properties.push((key.to_string(), value));
        }
        self.sync_descriptor_value(key);
    }

    fn sync_descriptor_value(&mut self, key: &str) {
        let value = self.property(key);
        let Some((_, Value::Object(descriptor))) = self
            .descriptors
            .iter_mut()
            .rev()
            .find(|(name, _)| name == key)
        else {
            return;
        };
        if let Some((_, current)) = Rc::make_mut(descriptor)
            .iter_mut()
            .find(|(name, _)| name == "value")
        {
            *current = value.unwrap_or(Value::Undefined);
        }
    }

    pub(crate) fn delete_property(&mut self, key: &str) {
        self.properties.retain(|(name, _)| name != key);
        self.descriptors.retain(|(name, _)| name != key);
        if let Some(index) = crate::arrays::array_index(key) {
            let index = index as usize;
            if index >= self.values.len() {
                return;
            }
            self.disconnect_index(index);
            self.deleted.resize(index.saturating_add(1), false);
            self.deleted[index] = true;
            if let Some(live) = &self.argument_live {
                let mut live = live.borrow_mut();
                live.deleted.resize(index.saturating_add(1), false);
                live.deleted[index] = true;
            }
        }
    }
}

/// Element kinds only become less specialized as an array is mutated.
fn monotonic_kind(previous: ArrayKind, candidate: ArrayKind) -> ArrayKind {
    if kind_rank(candidate) >= kind_rank(previous) { candidate } else { previous }
}

fn kind_rank(kind: ArrayKind) -> u8 {
    match kind {
        ArrayKind::PackedInt => 0,
        ArrayKind::PackedDouble => 1,
        ArrayKind::PackedValue => 2,
        ArrayKind::Holey => 3,
        ArrayKind::Sparse => 4,
    }
}

fn keep_index(key: &str, length: usize) -> bool {
    crate::arrays::array_index(key).map_or(true, |index| (index as usize) < length)
}

fn set_live_index(live: &mut ArgumentLive, index: usize, value: Value) {
    if let Some(Some(binding)) = live.mapped.get(index) {
        *binding.borrow_mut() = value.clone();
    }
    if live.values.len() <= index {
        live.values
            .resize(index.saturating_add(1), Value::Undefined);
    }
    live.values[index] = value;
    if live.deleted.len() <= index {
        live.deleted.resize(index.saturating_add(1), false);
    }
    live.deleted[index] = false;
    live.length = live.length.max(index.saturating_add(1));
}

fn live_index(live: &ArgumentLive, index: usize) -> Option<Value> {
    if live.deleted.get(index) == Some(&true) {
        return None;
    }
    live.mapped
        .get(index)
        .and_then(Option::as_ref)
        .map(|binding| binding.borrow().clone())
        .or_else(|| live.values.get(index).cloned())
}

impl std::ops::Deref for ArrayData {
    type Target = [Value];

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

#[cfg(test)]
mod array_data_tests {
    use super::{ArrayData, ArrayKind};
    use crate::value::Value;

    #[test]
    fn classifies_numeric_and_holey_storage() {
        let ints = ArrayData::new(vec![Value::Number(1.0), Value::Number(2.0)]);
        assert_eq!(std::mem::size_of::<ArrayKind>(), 1);
        assert_eq!(ints.kind(), ArrayKind::PackedInt);
        assert!(ints.kind().is_packed());
        let doubles = ArrayData::new(vec![Value::Number(1.5)]);
        assert_eq!(doubles.kind(), ArrayKind::PackedDouble);
        assert!(doubles.kind().is_packed());
        let mut holey = ArrayData::new(vec![Value::Number(1.0), Value::Number(2.0)]);
        holey.delete_property("0");
        assert_eq!(holey.kind(), ArrayKind::Holey);
        assert!(!holey.kind().is_packed());
    }
    #[test]
    fn kind_transitions_preserve_monotonic_holes_and_sparse_boundary() {
        let mut data = ArrayData::new(vec![Value::Number(1.0)]);
        assert_eq!(data.kind(), ArrayKind::PackedInt);

        data.set_index(0, Value::Number(1.25));
        assert_eq!(data.kind(), ArrayKind::PackedDouble);

        data.set_index(0, Value::Boolean(true));
        assert_eq!(data.kind(), ArrayKind::PackedValue);

        data.delete_property("0");
        assert_eq!(data.kind(), ArrayKind::Holey);
        data.set_index(0, Value::Number(2.0));
        assert_eq!(data.kind(), ArrayKind::Holey);

        let mut boundary = ArrayData::new(
            (0..32).map(|index| Value::Number(index as f64)).collect(),
        );
        boundary.set_length(33);
        assert_eq!(boundary.kind(), ArrayKind::Holey);
        boundary.set_length(65);
        assert_eq!(boundary.kind(), ArrayKind::Sparse);
        boundary.set_length(1);
        assert_eq!(boundary.kind(), ArrayKind::Sparse);
    }

    #[test]
    fn dense_growth_is_geometric_and_sparse_length_is_separate() {
        let mut data = ArrayData::new(Vec::new());
        let mut previous = data.storage_capacity();
        let mut reallocations = 0;
        for index in 0..64 {
            data.set_index(index, Value::Number(index as f64));
            let capacity = data.storage_capacity();
            assert!(capacity >= data.physical_len());
            if capacity != previous {
                reallocations += 1;
                // Every growth request is at least a doubling (with a
                // four-element minimum for the first allocation).
                assert!(capacity >= previous.saturating_mul(2).max(4));
                previous = capacity;
            }
        }
        // A linear push strategy would allocate on nearly every append.
        assert!(reallocations <= 6, "unexpected dense reallocations: {reallocations}");
        data.set_index(10_000, Value::Boolean(true));
        assert!(data.is_sparse());
        assert!(!data.is_dense());
        assert_eq!(data.logical_len(), 10_001);
        assert_eq!(data.get_index(10_000), Some(Value::Boolean(true)));
    }
    #[test]
    fn numeric_fast_path_reads_scalars_and_preserves_value_boundary() {
        let mut data = ArrayData::new(vec![Value::Number(1.25)]);
        assert_eq!(data.dense_number_at(0), Some(1.25));
        assert_eq!(data.dense_number_at(1), None);

        data.set_numeric_index(1, 2.5);
        assert_eq!(data.dense_number_at(1), Some(2.5));
        assert_eq!(data.get_index(1), Some(Value::Number(2.5)));
        assert_eq!(data.kind(), ArrayKind::PackedDouble);

        data.set_index(0, Value::Boolean(true));
        assert_eq!(data.dense_number_at(0), None);
        assert_eq!(data.get_index(0), Some(Value::Boolean(true)));
    }
    #[test]
    fn packed_numeric_storage_is_borrowed_until_value_access() {
        let data = ArrayData::new(vec![Value::Number(1.25), Value::Number(2.5)]);
        assert!(data.is_numeric_packed());
        let capacity = data.storage_capacity();

        // The dense fast path exposes the canonical slot directly; it does
        // not construct a second numeric representation or allocate.
        let slot = data.dense_value_at(1).expect("packed slot");
        assert!(matches!(slot, Value::Number(value) if *value == 2.5));
        assert_eq!(data.storage_capacity(), capacity);

        // Public element semantics intentionally convert on access by
        // returning an owned Value, while storage remains unchanged.
        assert_eq!(data.get_index(1), Some(Value::Number(2.5)));
        assert_eq!(data.storage_capacity(), capacity);
    }

    #[test]
    fn sparse_entries_keep_dense_values_and_property_semantics_separate() {
        let mut data = ArrayData::new(vec![Value::Number(7.0), Value::Number(8.0)]);
        data.set_index(10_000, Value::Boolean(true));
        data.set_property("label", Value::String("sparse".into()));

        assert_eq!(data.kind(), ArrayKind::Sparse);
        assert_eq!(data.physical_len(), 2);
        assert_eq!(data.get_index(0), Some(Value::Number(7.0)));
        assert_eq!(data.get_index(1), Some(Value::Number(8.0)));
        assert_eq!(data.get_index(9_999), None);
        assert_eq!(data.get_index(10_000), Some(Value::Boolean(true)));
        assert_eq!(data.property("label"), Some(Value::String("sparse".into())));
        assert_eq!(data.next_index(2, data.logical_len()), Some(10_000));

        data.delete_property("10000");
        assert_eq!(data.get_index(10_000), None);
        assert_eq!(data.property("label"), Some(Value::String("sparse".into())));
        assert_eq!(data.next_index(2, data.logical_len()), None);
    }

    #[test]
    fn sparse_length_growth_does_not_allocate_dense_storage() {
        let mut data = ArrayData::new(vec![Value::Number(1.0)]);
        let capacity = data.storage_capacity();
        data.set_index(1_000_000, Value::Number(2.0));

        assert_eq!(data.physical_len(), 1);
        assert_eq!(data.storage_capacity(), capacity);
        assert_eq!(data.logical_len(), 1_000_001);
        assert_eq!(data.get_index(1_000_000), Some(Value::Number(2.0)));
    }

    #[test]
    fn ordinary_arrays_do_not_duplicate_argument_storage() {
        let ordinary = ArrayData::new(vec![Value::Number(1.0)]);
        assert!(ordinary.argument_live_view().is_none());
        let arguments = ArrayData::new_arguments(vec![Value::Number(1.0)], false);
        assert!(arguments.argument_live_view().is_some());
    }
    #[test]
    fn sparse_transition_keeps_adjacent_writes_out_of_dense_storage() {
        let mut data = ArrayData::new(vec![Value::Number(1.0)]);
        data.set_index(10_000, Value::Boolean(true));
        let physical = data.physical_len();
        let capacity = data.storage_capacity();
        data.set_index(physical, Value::Number(2.0));
        assert_eq!(data.physical_len(), physical);
        assert_eq!(data.storage_capacity(), capacity);
        assert_eq!(data.get_index(physical), Some(Value::Number(2.0)));
        assert!(data.has_index(physical));
        assert_eq!(data.next_index(physical, data.logical_len()), Some(physical));
    }
}

impl Value {
    /// Create an ordinary JavaScript object from own data properties.
    pub fn object(properties: Vec<(String, Value)>) -> Self {
        Self::Object(Rc::new(ObjectData::new(properties)))
    }

    pub(crate) fn array(values: Vec<Value>) -> Self {
        Self::Array(Rc::new(ArrayData::new(values)))
    }
}
