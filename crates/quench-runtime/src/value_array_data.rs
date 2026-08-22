#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ArrayKind {
    PackedInt,
    PackedDouble,
    PackedValue,
    Holey,
    Sparse,
}
impl ArrayKind {
    #[inline]
    pub fn is_packed(self) -> bool {
        matches!(
            self,
            Self::PackedInt | Self::PackedDouble | Self::PackedValue
        )
    }
}


#[derive(Debug, Clone, PartialEq)]
pub struct ArrayData {
    values: Vec<Value>,
    length: usize,
    kind: ArrayKind,
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
        Self {
            kind: classify_kind(&values),
            values,
            length,
            properties: Vec::new(),
            descriptors: Vec::new(),
            arguments: false,
            strict_arguments: false,
            mapped: Vec::new(),
            deleted: Vec::new(),
            prototype: std::cell::RefCell::new(None),
            argument_live: None,
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
    pub(crate) fn kind(&self) -> ArrayKind {
        self.kind
    }

    pub(crate) fn is_arguments(&self) -> bool {
        self.arguments
    }

    pub(crate) fn is_strict_arguments(&self) -> bool {
        self.strict_arguments
    }

    #[inline]
    pub(crate) fn is_packed(&self) -> bool {
        self.kind.is_packed()
    }

    pub fn logical_len(&self) -> usize {
        self.argument_live
            .as_ref()
            .map_or(self.length, |live| live.borrow().length)
    }

    #[inline]
    pub(crate) fn is_holey(&self) -> bool {
        matches!(self.kind, ArrayKind::Holey)
    }
    /// Header-resident logical length; does not traverse element storage.
    #[inline]
    pub(crate) fn header_length(&self) -> usize {
        self.logical_len()
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
    /// Capacity of the dense backing store, exposed for focused allocation
    /// checks without exposing ownership of the storage itself.
    #[cfg(test)]
    pub(crate) fn storage_capacity(&self) -> usize {
        self.values.capacity()
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
        self.kind = classify_kind_with_holes(&self.values, &self.deleted, length);
    }

    pub fn set_index(&mut self, index: usize, value: Value) {
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
            self.grow_dense_storage(index.saturating_add(1));
        }
        self.values[index] = value;
        if self.deleted.len() <= index {
            self.deleted.resize(index.saturating_add(1), false);
        }
        self.deleted[index] = false;
        self.length = self.length.max(index.saturating_add(1));
        self.kind = classify_kind_with_holes(&self.values, &self.deleted, self.length);
    }

    /// Grow dense storage geometrically so sequential appends do not
    /// repeatedly reallocate, while preserving undefined holes.
    fn grow_dense_storage(&mut self, required: usize) {
        let current = self.values.len();
        if required <= current {
            return;
        }
        let doubled = current.saturating_mul(2).max(4);
        let capacity = doubled.max(required);
        self.values.reserve(capacity.saturating_sub(current));
        self.values.resize(required, Value::Undefined);
    }

    fn set_sparse_index(&mut self, index: usize, value: Value) {
        self.set_property(&index.to_string(), value);
        let length = index.saturating_add(1);
        if let Some(live) = &self.argument_live {
            let mut live = live.borrow_mut();
            live.length = live.length.max(length);
        }
        self.length = self.length.max(length);
        self.kind = ArrayKind::Sparse;
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

    #[inline]
    pub(crate) fn dense_value_at(&self, index: usize) -> Option<&Value> {
        (index < self.values.len() && self.deleted.get(index) != Some(&true))
            .then(|| self.values.get(index))?
    }


    #[inline]
    pub(crate) fn last_dense_value(&self) -> Option<&Value> {
        self.values
            .len()
            .checked_sub(1)
            .and_then(|index| self.dense_value_at(index))
    }
    pub(crate) fn dense_value_at_mut(&mut self, index: usize) -> Option<&mut Value> {
        (index < self.values.len() && self.deleted.get(index) != Some(&true))
            .then(|| self.values.get_mut(index))?
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
        index < self.length
            && self.deleted.get(index) != Some(&true)
            && (index < self.values.len()
                || self.mapped.get(index).and_then(Option::as_ref).is_some())
    }
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
        let moved = self.values[src..src_end].to_vec();
        self.values[dst..dst_end].clone_from_slice(&moved);
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
            self.kind = ArrayKind::Holey;
            if let Some(live) = &self.argument_live {
                let mut live = live.borrow_mut();
                live.deleted.resize(index.saturating_add(1), false);
                live.deleted[index] = true;
            }
        }
    }
}
fn classify_kind(values: &[Value]) -> ArrayKind {
    classify_kind_with_holes(values, &[], values.len())
}

fn classify_kind_with_holes(values: &[Value], deleted: &[bool], length: usize) -> ArrayKind {
    if length > values.len().saturating_mul(2).max(32) {
        return ArrayKind::Sparse;
    }
    if deleted.iter().any(|deleted| *deleted) || length > values.len() {
        return ArrayKind::Holey;
    }
    if values
        .iter()
        .all(|value| matches!(value, Value::Number(number) if number.fract() == 0.0))
    {
        ArrayKind::PackedInt
    } else if values.iter().all(|value| matches!(value, Value::Number(_))) {
        ArrayKind::PackedDouble
    } else {
        ArrayKind::PackedValue
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
    fn dense_growth_is_geometric_and_sparse_length_is_separate() {
        let mut data = ArrayData::new(Vec::new());
        let mut previous = data.storage_capacity();
        for index in 0..64 {
            data.set_index(index, Value::Number(index as f64));
            let capacity = data.storage_capacity();
            assert!(capacity >= data.physical_len());
            if capacity != previous {
                previous = capacity;
            }
        }
        data.set_index(10_000, Value::Boolean(true));
        assert!(data.is_sparse());
        assert!(!data.is_dense());
        assert_eq!(data.logical_len(), 10_001);
        assert_eq!(data.get_index(10_000), Some(Value::Boolean(true)));
    }

    #[test]
    fn ordinary_arrays_do_not_duplicate_argument_storage() {
        let ordinary = ArrayData::new(vec![Value::Number(1.0)]);
        assert!(ordinary.argument_live_view().is_none());
        let arguments = ArrayData::new_arguments(vec![Value::Number(1.0)], false);
        assert!(arguments.argument_live_view().is_some());
    }
}

impl Value {
    /// Create an ordinary JavaScript object from own data properties.
    pub fn object(properties: ObjectProperties) -> Self {
        Self::Object(Rc::new(ObjectData::new(properties)))
    }

    pub(crate) fn array(values: Vec<Value>) -> Self {
        Self::Array(Rc::new(ArrayData::new(values)))
    }
}
