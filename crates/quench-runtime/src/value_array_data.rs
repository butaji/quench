#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ArrayKind {
    PackedLimb28,
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
            Self::PackedLimb28 | Self::PackedInt | Self::PackedDouble | Self::PackedValue
        )
    }
}

#[derive(Debug, Clone)]
pub struct ArrayData {
    identity: u64,
    values: DenseElements,
    length: std::cell::Cell<usize>,
    kind: std::cell::Cell<ArrayKind>,
    properties: Vec<(String, Value)>,
    descriptors: Vec<(String, Value)>,
    arguments: bool,
    strict_arguments: bool,
    mapped: Vec<Option<Rc<crate::value::BindingCell>>>,
    deleted: Vec<bool>,
    prototype: std::cell::RefCell<Option<Value>>,
    argument_live: Option<Rc<RefCell<ArgumentLive>>>,
}

impl PartialEq for ArrayData {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
            && self.length.get() == other.length.get()
            && self.kind == other.kind
            && self.properties == other.properties
            && self.descriptors == other.descriptors
            && self.arguments == other.arguments
            && self.strict_arguments == other.strict_arguments
            && self.mapped == other.mapped
            && self.deleted == other.deleted
            && self.prototype == other.prototype
            && self.argument_live == other.argument_live
    }
}

/// The one authoritative dense element store. Numeric arrays carry only IEEE
/// words; generic JavaScript values appear only after a semantic transition.
/// `Cell<f64>` permits value-only mutation through shared array identity while
/// preserving structural COW for growth, holes, descriptors, and mappings.
#[derive(Debug, Clone, PartialEq)]
enum DenseElements {
    Numbers(Rc<RefCell<Vec<std::cell::Cell<f64>>>>),
    Values(Rc<RefCell<Vec<Value>>>),
}

impl DenseElements {
    fn from_values(values: Vec<Value>) -> Self {
        if values.iter().all(|value| matches!(value, Value::Number(_))) {
            return Self::Numbers(Rc::new(RefCell::new(
                values
                    .into_iter()
                    .map(|value| match value {
                        Value::Number(number) => std::cell::Cell::new(number),
                        _ => unreachable!(),
                    })
                    .collect(),
            )));
        }
        Self::Values(Rc::new(RefCell::new(values)))
    }

    fn len(&self) -> usize {
        match self {
            Self::Numbers(values) => values.borrow().len(),
            Self::Values(values) => values.borrow().len(),
        }
    }

    fn capacity(&self) -> usize {
        match self {
            Self::Numbers(values) => values.borrow().capacity(),
            Self::Values(values) => values.borrow().capacity(),
        }
    }

    fn truncate(&mut self, length: usize) {
        match self {
            Self::Numbers(values) => values.borrow_mut().truncate(length),
            Self::Values(values) => values.borrow_mut().truncate(length),
        }
    }

    fn reserve(&mut self, additional: usize) {
        match self {
            Self::Numbers(values) => values.borrow_mut().reserve(additional),
            Self::Values(values) => values.borrow_mut().reserve(additional),
        }
    }

    fn resize_undefined(&mut self, length: usize) {
        self.materialize_values().resize(length, Value::Undefined);
    }

    fn resize_numeric(&mut self, length: usize) {
        match self {
            Self::Numbers(values) => values
                .borrow_mut()
                .resize_with(length, || std::cell::Cell::new(0.0)),
            Self::Values(values) => values.borrow_mut().resize(length, Value::Number(0.0)),
        }
    }

    fn set(&mut self, index: usize, value: Value) {
        if let (Self::Numbers(values), Value::Number(number)) = (&*self, &value) {
            values.borrow()[index].set(*number);
            return;
        }
        if let Self::Values(values) = self {
            values.borrow_mut()[index] = value;
            return;
        }
        self.materialize_values()[index] = value;
    }

    fn kind_with_holes(&self, deleted: &[bool], length: usize) -> ArrayKind {
        if length > self.len().saturating_mul(2).max(32) {
            return ArrayKind::Sparse;
        }
        if deleted.iter().any(|deleted| *deleted) || length > self.len() {
            return ArrayKind::Holey;
        }
        match self {
            Self::Numbers(values) if values.borrow().iter().all(|value| is_limb28(value.get())) => {
                ArrayKind::PackedLimb28
            }
            Self::Numbers(values)
                if values
                    .borrow()
                    .iter()
                    .all(|value| value.get().fract() == 0.0) =>
            {
                ArrayKind::PackedInt
            }
            Self::Numbers(_) => ArrayKind::PackedDouble,
            Self::Values(_) => ArrayKind::PackedValue,
        }
    }

    fn number_at(&self, index: usize) -> Option<f64> {
        match self {
            Self::Numbers(values) => values.borrow().get(index).map(std::cell::Cell::get),
            Self::Values(values) => match values.borrow().get(index)? {
                Value::Number(number) => Some(*number),
                _ => None,
            },
        }
    }

    fn value_at(&self, index: usize) -> Option<Value> {
        match self {
            Self::Numbers(values) => values
                .borrow()
                .get(index)
                .map(|value| Value::Number(value.get())),
            Self::Values(values) => values.borrow().get(index).cloned(),
        }
    }

    fn set_existing_number(&self, index: usize, number: f64) -> bool {
        let Self::Numbers(values) = self else {
            return false;
        };
        let values = values.borrow();
        let Some(slot) = values.get(index) else {
            return false;
        };
        slot.set(number);
        true
    }

    fn set_existing_numeric_value(&self, index: usize, number: f64) -> bool {
        match self {
            Self::Numbers(_) => self.set_existing_number(index, number),
            Self::Values(values) => {
                let mut values = values.borrow_mut();
                let Some(Value::Number(value)) = values.get_mut(index) else { return false };
                *value = number;
                true
            }
        }
    }

    fn append_number(&mut self, number: f64) -> bool {
        let Self::Numbers(values) = self else {
            return false;
        };
        values.borrow_mut().push(std::cell::Cell::new(number));
        true
    }

    fn detach_numbers(&mut self) -> bool {
        let Self::Numbers(values) = self else {
            return false;
        };
        let detached = values
            .borrow()
            .iter()
            .map(|value| std::cell::Cell::new(value.get()))
            .collect();
        *values = Rc::new(RefCell::new(detached));
        true
    }

    fn append_number_shared(&self, number: f64) -> bool {
        let Self::Numbers(values) = self else {
            return false;
        };
        values.borrow_mut().push(std::cell::Cell::new(number));
        true
    }

    fn materialize_values(&mut self) -> &mut Vec<Value> {
        if let Self::Numbers(numbers) = self {
            let values = numbers
                .borrow()
                .iter()
                .map(|number| Value::Number(number.get()))
                .collect();
            *self = Self::Values(Rc::new(RefCell::new(values)));
        }
        let Self::Values(values) = self else {
            unreachable!()
        };
        if Rc::strong_count(values) > 1 {
            let cloned = values.borrow().clone();
            *self = Self::Values(Rc::new(RefCell::new(cloned)));
        }
        let Self::Values(values) = self else {
            unreachable!()
        };
        Rc::get_mut(values).expect("detached values").get_mut()
    }

    fn snapshot(&self) -> Vec<Value> {
        (0..self.len())
            .filter_map(|index| self.value_at(index))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArgumentLive {
    pub values: Vec<Value>,
    pub length: usize,
    pub mapped: Vec<Option<Rc<crate::value::BindingCell>>>,
    pub deleted: Vec<bool>,
    /// Optional override for `arguments.length`. Per spec 10.6 the property
    /// is writable: a plain value-property assignment stores the value
    /// here so that subsequent reads return it verbatim instead of
    /// coercing through the array's length slot.
    pub length_override: Option<Value>,
}

impl ArrayData {
    pub fn new(values: Vec<Value>) -> Self {
        crate::execution_trace::array_lifecycle(true);
        let length = values.len();
        let kind = classify_kind(&values);
        Self {
            identity: next_array_identity(),
            kind: std::cell::Cell::new(kind),
            values: DenseElements::from_values(values),
            length: std::cell::Cell::new(length),
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

    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }

    pub(crate) fn new_arguments(values: Vec<Value>, strict: bool) -> Self {
        let mut data = Self::new(values);
        data.arguments = true;
        data.strict_arguments = strict;
        data.argument_live = Some(Rc::new(RefCell::new(ArgumentLive {
            values: data.values.snapshot(),
            length: data.length.get(),
            mapped: data.mapped.clone(),
            deleted: data.deleted.clone(),
            length_override: None,
        })));
        data
    }
    pub(crate) fn kind(&self) -> ArrayKind {
        self.kind.get()
    }

    pub(crate) fn is_arguments(&self) -> bool {
        self.arguments
    }

    pub(crate) fn has_argument_live(&self) -> bool {
        self.argument_live.is_some()
    }

    pub(crate) fn is_strict_arguments(&self) -> bool {
        self.strict_arguments
    }
    /// Borrow the canonical dense storage and its header facts together.
    /// Callers must derive all fast-path decisions from this tuple; no shadow
    /// length or element cache is permitted.
    #[inline]
    pub(crate) fn hot_storage(&self) -> (Vec<Value>, usize, ArrayKind) {
        (self.values.snapshot(), self.logical_len(), self.kind.get())
    }

    #[inline]
    pub(crate) fn is_packed(&self) -> bool {
        self.kind.get().is_packed()
    }

    pub fn logical_len(&self) -> usize {
        self.argument_live.as_ref().map_or_else(
            || {
                // Packed ordinary arrays have no separate hole/descriptor
                // state. Their shared dense store is authoritative so a
                // value-only append remains visible through all references.
                if self.kind.get().is_packed()
                    && self.deleted.is_empty()
                    && self.properties.is_empty()
                    && self.descriptors.is_empty()
                    && self.prototype.borrow().is_none()
                    && !self.arguments
                {
                    self.length.get().max(self.values.len())
                } else {
                    self.length.get()
                }
            },
            |live| {
                let live = live.borrow();
                live.length_override
                    .as_ref()
                    .and_then(argument_length)
                    .unwrap_or(live.length)
            },
        )
    }

    pub fn len(&self) -> usize {
        self.logical_len()
    }

    pub fn is_empty(&self) -> bool {
        self.logical_len() == 0
    }

    pub fn get(&self, index: usize) -> Option<Value> {
        self.get_index(index)
    }

    pub fn first(&self) -> Option<Value> {
        self.get_index(0)
    }

    #[inline]
    pub(crate) fn is_holey(&self) -> bool {
        matches!(self.kind.get(), ArrayKind::Holey)
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

    pub(crate) fn sync_length_to_storage(&mut self) {
        self.length.set(self.length.get().max(self.values.len()));
    }
    /// Capacity of the dense backing store, exposed for focused allocation
    /// checks without exposing ownership of the storage itself.
    #[cfg(test)]
    pub(crate) fn storage_capacity(&self) -> usize {
        self.values.capacity()
    }

    #[inline]
    pub(crate) fn is_sparse(&self) -> bool {
        matches!(self.kind.get(), ArrayKind::Sparse)
    }
    pub(crate) fn is_dense(&self) -> bool {
        !matches!(self.kind.get(), ArrayKind::Sparse)
    }

    #[inline]
    pub(crate) fn is_numeric_packed(&self) -> bool {
        matches!(
            self.kind.get(),
            ArrayKind::PackedLimb28 | ArrayKind::PackedInt | ArrayKind::PackedDouble
        )
    }

    /// Whether mutation may use the dense backing store without consulting
    /// indexed properties, descriptors, prototypes, or argument mappings.
    #[inline]
    pub(crate) fn is_packed_ordinary(&self) -> bool {
        self.is_packed()
            && self.logical_len() == self.physical_len()
            && self.properties.is_empty()
            && self.indexed_descriptors_plain()
            && self.has_default_array_prototype()
            && !self.arguments
            && self.argument_live.is_none()
    }

    /// Whether every logical index is an own packed data element. Unlike
    /// `is_packed_ordinary`, this read-only proof permits an inherited
    /// prototype because existing own indices cannot be intercepted by it.
    #[inline]
    pub(crate) fn is_packed_data(&self) -> bool {
        self.is_packed()
            && self.logical_len() == self.physical_len()
            && self.properties.is_empty()
            && self.indexed_descriptors_plain()
            && !self.arguments
            && self.argument_live.is_none()
    }

    /// Prove that indexed reads/writes have no prototype, descriptor, or
    /// side-property interception. Unlike `is_packed_data`, this also admits
    /// a holey dense store: a missing dense slot simply observes `undefined`.
    #[inline]
    pub(crate) fn is_plain_dense_access(&self) -> bool {
        !self.is_sparse()
            && self.properties.is_empty()
            && self.indexed_descriptors_plain()
            && self.has_default_array_prototype()
            && !self.arguments
            && self.argument_live.is_none()
    }

    /// Prove that indexed writes cannot be intercepted by array metadata.
    /// Holes are allowed because fill materializes every slot in its range.
    #[inline]
    pub(crate) fn can_fast_fill(&self) -> bool {
        self.properties.is_empty()
            && self.indexed_descriptors_plain()
            && self.has_default_array_prototype()
            && !self.arguments
            && self.argument_live.is_none()
            && self.mapped.is_empty()
    }

    /// Whether every logical slot is an own numeric value, even when the
    /// monotonic kind is conservatively marked holey after a length reset.
    /// The deleted bitmap remains the authoritative hole proof.
    #[inline]
    pub(crate) fn is_dense_numeric_data(&self) -> bool {
        self.logical_len() == self.physical_len()
            && self.deleted.iter().all(|deleted| !deleted)
            && self.properties.is_empty()
            && self.indexed_descriptors_plain()
            && !self.arguments
            && self.argument_live.is_none()
            && matches!(self.values, DenseElements::Numbers(_))
    }

    pub(crate) fn has_indexed_accessor(&self) -> bool {
        self.descriptors.iter().any(|(key, value)| {
            crate::arrays::array_index(key).is_some_and(|_| {
                matches!(value, Value::Object(fields) if fields.iter().any(|(name, _)| name == "get" || name == "set"))
            })
        })
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
        if length < self.length.get() || length < self.values.len() {
            self.values.truncate(length);
            self.deleted.truncate(length);
            self.mapped.truncate(length);
            self.properties.retain(|(key, _)| keep_index(key, length));
            self.descriptors.retain(|(key, _)| keep_index(key, length));
        }
        self.length.set(length);
        self.kind.set(monotonic_kind(
            self.kind.get(),
            self.values.kind_with_holes(&self.deleted, length),
        ));
    }

    pub fn set_index(&mut self, index: usize, value: Value) {
        if self.is_sparse() && index >= self.values.len() {
            self.set_sparse_index(index, value);
            return;
        }
        if index == self.length.get()
            && index == self.values.len()
            && self.properties.is_empty()
            && self.indexed_descriptors_plain()
            && self.has_default_array_prototype()
            && !self.arguments
            && self.argument_live.is_none()
            && matches!(&value, Value::Number(number) if self.values.append_number(*number))
        {
            self.length.set(self.length.get() + 1);
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
            binding.store(value.clone());
        }
        let appended_number = index == self.values.len()
            && matches!(&value, Value::Number(number) if self.values.append_number(*number));
        let appended_kind = appended_number.then(|| match &value {
            Value::Number(number) => number_kind(*number),
            _ => unreachable!(),
        });
        if self.values.len() <= index {
            self.grow_dense_storage(index.saturating_add(1));
        }
        if !appended_number {
            self.values.set(index, value);
        }
        if self.deleted.len() <= index {
            self.deleted.resize(index.saturating_add(1), false);
        }
        self.deleted[index] = false;
        self.length
            .set(self.length.get().max(index.saturating_add(1)));
        let candidate = if self.kind.get().is_packed() {
            appended_kind.unwrap_or_else(|| {
                self.values
                    .kind_with_holes(&self.deleted, self.length.get())
            })
        } else {
            self.values
                .kind_with_holes(&self.deleted, self.length.get())
        };
        self.kind.set(monotonic_kind(self.kind.get(), candidate));
    }

    /// Grow dense storage geometrically so sequential appends do not
    /// repeatedly reallocate, while preserving undefined holes.
    fn grow_dense_storage(&mut self, required: usize) {
        let current = self.values.len();
        if required <= self.values.capacity() {
            self.values.resize_undefined(required);
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
        self.values.resize_undefined(required);
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
        self.length.set(self.length.get().max(length));
        self.kind.set(ArrayKind::Sparse);
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

    pub(crate) fn append_physical(&mut self, values: &[Value]) {
        match &mut self.values {
            DenseElements::Numbers(numbers)
                if values.iter().all(|value| matches!(value, Value::Number(_))) =>
            {
                numbers.borrow_mut().extend(values.iter().map(|value| {
                    let Value::Number(number) = value else {
                        unreachable!()
                    };
                    std::cell::Cell::new(*number)
                }));
            }
            DenseElements::Numbers(_) => {
                let mut current = self.values.materialize_values().to_vec();
                current.extend_from_slice(values);
                self.values = DenseElements::Values(Rc::new(RefCell::new(current)));
            }
            DenseElements::Values(current) => current.borrow_mut().extend_from_slice(values),
        }
        self.length
            .set(self.length.get().saturating_add(values.len()));
    }

    pub(crate) fn values_mut(&mut self) -> &mut [Value] {
        self.kind
            .set(monotonic_kind(self.kind.get(), ArrayKind::PackedValue));
        self.values.materialize_values()
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
            .or_else(|| self.values.value_at(index))
            .or_else(|| self.property(&index.to_string()))
    }

    /// Public host-facing indexed read for API adapters that consume arrays as data.
    pub fn index_value(&self, index: usize) -> Value {
        self.get_index(index).unwrap_or(Value::Undefined)
    }

    /// Read a packed numeric slot without materializing an owned `Value`.
    ///
    /// The returned scalar is the unboxed representation used by numeric
    /// array fast paths. Callers that need JavaScript semantics should use
    /// `get_index`, which remains the authoritative Value-producing path.
    #[inline]
    pub(crate) fn dense_number_at(&self, index: usize) -> Option<f64> {
        (index < self.logical_len() && self.deleted.get(index) != Some(&true))
            .then(|| self.values.number_at(index))
            .flatten()
    }

    pub(crate) fn dense_numeric_snapshot(&self) -> Option<Vec<f64>> {
        self.is_dense_numeric_data().then(|| {
            (0..self.logical_len())
                .map(|index| self.dense_number_at(index).unwrap_or(0.0))
                .collect()
        })
    }

    #[inline]
    pub(crate) fn numeric_cells(&self) -> Option<std::cell::Ref<'_, [std::cell::Cell<f64>]>> {
        self.widen_mutable_numeric_kind();
        let DenseElements::Numbers(values) = &self.values else {
            return None;
        };
        Some(std::cell::Ref::map(values.borrow(), Vec::as_slice))
    }

    /// Borrow the canonical numeric payload as immutable IEEE words for a
    /// guarded native kernel. No JavaScript can run while this borrow lives.
    pub(crate) fn numeric_kernel_words(&self) -> Option<std::cell::Ref<'_, [f64]>> {
        let DenseElements::Numbers(values) = &self.values else {
            return None;
        };
        Some(std::cell::Ref::map(values.borrow(), |values| {
            // SAFETY: `Cell<f64>` has the same layout as `f64`; the RefCell
            // borrow prevents structural mutation for the returned lifetime.
            unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<f64>(), values.len()) }
        }))
    }

    /// Borrow the one canonical numeric payload exclusively. Admission must
    /// prove this array distinct from every input before taking this view.
    pub(crate) fn numeric_kernel_words_mut(&self) -> Option<std::cell::RefMut<'_, [f64]>> {
        self.widen_mutable_numeric_kind();
        let DenseElements::Numbers(values) = &self.values else {
            return None;
        };
        Some(std::cell::RefMut::map(values.borrow_mut(), |values| {
            // SAFETY: `Cell<f64>` is layout-compatible with `f64`, and the
            // exclusive RefCell borrow prevents every competing live view.
            unsafe {
                std::slice::from_raw_parts_mut(values.as_mut_ptr().cast::<f64>(), values.len())
            }
        }))
    }

    /// Borrow a base-2^28 limb payload after one header guard. The element
    /// kind is the canonical proof that every word is an exact limb; callers
    /// therefore execute load/ALU/store without per-element float checks.
    pub(crate) fn limb28_kernel_words(&self) -> Option<std::cell::Ref<'_, [f64]>> {
        (self.kind.get() == ArrayKind::PackedLimb28 && self.is_packed_ordinary()).then_some(())?;
        self.numeric_kernel_words()
    }

    /// Mutable limb view for kernels whose stores are proven masked to 28
    /// bits. General mutable numeric views widen the kind before returning.
    pub(crate) fn limb28_kernel_words_mut(&self) -> Option<std::cell::RefMut<'_, [f64]>> {
        (self.kind.get() == ArrayKind::PackedLimb28 && self.is_packed_ordinary()).then_some(())?;
        let DenseElements::Numbers(values) = &self.values else {
            return None;
        };
        Some(std::cell::RefMut::map(values.borrow_mut(), |values| {
            // SAFETY: identical to `numeric_kernel_words_mut`; this narrower
            // view additionally carries the proven limb element kind.
            unsafe {
                std::slice::from_raw_parts_mut(values.as_mut_ptr().cast::<f64>(), values.len())
            }
        }))
    }

    #[inline]
    fn widen_mutable_numeric_kind(&self) {
        if self.kind.get() == ArrayKind::PackedLimb28 {
            self.kind.set(ArrayKind::PackedInt);
        }
    }

    /// Snapshot an own-data numeric range after proving that no indexed
    /// descriptor, argument mapping, hole, or sparse tail can intercept it.
    pub(crate) fn numeric_kernel_range(&self, start: usize, end: usize) -> Option<Vec<f64>> {
        (start <= end
            && end <= self.logical_len()
            && self.descriptors.is_empty()
            && !self.arguments
            && self.argument_live.is_none())
        .then_some(())?;
        (start..end)
            .map(|index| match self.get_index(index)? {
                Value::Number(value) => Some(value),
                _ => None,
            })
            .collect()
    }

    /// Convert a fully numeric sparse tail into the canonical dense numeric
    /// store. This is a representation-only transition: every logical index
    /// must already exist as ordinary own data and no observable metadata may
    /// intercept indexed access.
    pub(crate) fn promote_sparse_numeric(&mut self) -> bool {
        if !self.is_sparse()
            || !self.descriptors.is_empty()
            || self.prototype.borrow().is_some()
            || self.arguments
            || self.argument_live.is_some()
            || self.deleted.iter().any(|deleted| *deleted)
            || self.mapped.iter().any(Option::is_some)
        {
            return false;
        }
        let start = self.values.len();
        let Some(mut tail) = self.numeric_sparse_tail(start) else {
            return false;
        };
        if !self.values.detach_numbers() {
            return false;
        }
        for number in tail.drain(..) {
            if !self.values.append_number(number) {
                return false;
            }
        }
        self.properties.clear();
        self.kind.set(
            self.values
                .kind_with_holes(&self.deleted, self.length.get()),
        );
        self.is_packed_ordinary()
    }

    fn numeric_sparse_tail(&self, start: usize) -> Option<Vec<f64>> {
        (start <= self.length.get()).then_some(())?;
        let mut tail = vec![None; self.length.get() - start];
        for (key, value) in &self.properties {
            let index = usize::try_from(crate::arrays::array_index(key)?).ok()?;
            let Value::Number(number) = value else {
                return None;
            };
            let slot = index.checked_sub(start)?;
            *tail.get_mut(slot)? = Some(*number);
        }
        tail.into_iter().collect()
    }

    /// Store an unboxed numeric slot while retaining canonical `Value`
    /// semantics at the storage boundary.
    #[inline]
    pub(crate) fn set_numeric_index(&mut self, index: usize, number: f64) {
        self.set_index(index, Value::Number(number));
    }

    pub(crate) fn fill_numeric_constant(&mut self, start: usize, end: usize, number: f64) {
        debug_assert_eq!(start, 0);
        self.values.resize_numeric(end);
        if let DenseElements::Numbers(values) = &self.values {
            for slot in values.borrow()[start..end].iter() {
                slot.set(number);
            }
        } else {
            for index in start..end {
                self.values.set(index, Value::Number(number));
            }
        }
        if let DenseElements::Values(values) = &self.values {
            let numeric = values
                .borrow()
                .iter()
                .map(|value| match value {
                    Value::Number(number) => Some(std::cell::Cell::new(*number)),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>();
            if let Some(numeric) = numeric {
                self.values = DenseElements::Numbers(Rc::new(RefCell::new(numeric)));
            }
        }
        self.deleted.clear();
        self.length.set(end);
        self.kind
            .set(self.values.kind_with_holes(&self.deleted, end));
    }

    pub(crate) fn fill_numeric_range(&mut self, start: usize, end: usize, first: f64) {
        if self.values.len() < end {
            self.values.resize_numeric(end);
        }
        if let DenseElements::Numbers(values) = &self.values {
            let values = values.borrow();
            for (offset, slot) in values[start..end].iter().enumerate() {
                slot.set(first + offset as f64);
            }
            self.deleted.resize(end, false);
            self.deleted[start..end].fill(false);
            self.length.set(self.length.get().max(end));
            self.kind.set(
                self.values
                    .kind_with_holes(&self.deleted, self.length.get()),
            );
            return;
        }
        for index in start..end {
            self.set_numeric_index(index, first + (index - start) as f64);
        }
    }

    pub(crate) fn fill_numeric_constant_range(&mut self, start: usize, end: usize, number: f64) {
        if start >= end {
            return;
        }
        if self.values.len() < end {
            self.values.resize_numeric(end);
        }
        if let DenseElements::Numbers(values) = &self.values {
            for slot in values.borrow()[start..end].iter() {
                slot.set(number);
            }
        } else {
            for index in start..end {
                self.values.set(index, Value::Number(number));
            }
        }
        if let DenseElements::Values(values) = &self.values {
            let numeric = values
                .borrow()
                .iter()
                .map(|value| match value {
                    Value::Number(number) => Some(std::cell::Cell::new(*number)),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>();
            if let Some(numeric) = numeric {
                self.values = DenseElements::Numbers(Rc::new(RefCell::new(numeric)));
            }
        }
        self.deleted.resize(end, false);
        self.deleted[start..end].fill(false);
        self.length.set(self.length.get().max(end));
        self.kind.set(
            self.values
                .kind_with_holes(&self.deleted, self.length.get()),
        );
    }

    /// Mutate an existing packed numeric slot through shared JS array
    /// identity. This changes no structural fact and performs one checked
    /// pointer-plus-index store.
    #[inline(always)]
    pub(crate) fn set_existing_number(&self, index: usize, value: &Value) -> bool {
        let Value::Number(number) = value else {
            return false;
        };
        self.set_existing_f64(index, *number)
    }

    #[inline(always)]
    pub(crate) fn set_existing_f64(&self, index: usize, number: f64) -> bool {
        let stored = self.is_packed_data()
            && self.has_default_array_prototype()
            && index < self.logical_len()
            && self.values.set_existing_number(index, number);
        if stored {
            self.kind
                .set(monotonic_kind(self.kind.get(), number_kind(number)));
        }
        stored
    }

    #[inline(always)]
    pub(crate) fn set_plain_existing_f64(&self, index: usize, number: f64) -> bool {
        let stored = self.is_plain_dense_access()
            && index < self.logical_len()
            && self.deleted.get(index) != Some(&true)
            && self.values.set_existing_number(index, number);
        if stored {
            self.kind
                .set(monotonic_kind(self.kind.get(), number_kind(number)));
        }
        stored
    }

    /// Mutate a preflighted ordinary own numeric element even when unrelated
    /// sparse properties keep the array's monotonic kind at `Sparse`.
    #[inline(always)]
    pub(crate) fn set_proven_existing_f64(&self, index: usize, number: f64) -> bool {
        let stored = self.has_plain_dense_index(index)
            && self.values.set_existing_number(index, number);
        if stored {
            self.kind
                .set(monotonic_kind(self.kind.get(), number_kind(number)));
        }
        stored
    }

    /// Prove that an existing indexed value is ordinary numeric data, whether
    /// it lives in the dense payload or the sparse own-property tail.
    #[inline(always)]
    pub(crate) fn has_kernel_numeric_index(&self, index: usize) -> bool {
        if self.has_plain_dense_index(index) && self.dense_number_at(index).is_some() {
            return true;
        }
        !self.arguments
            && self.argument_live.is_none()
            && self.indexed_descriptors_plain()
            && index < self.logical_len()
            && self.deleted.get(index) != Some(&true)
            && self.mapped.get(index).and_then(Option::as_ref).is_none()
            && matches!(self.property(&index.to_string()), Some(Value::Number(_)))
    }

    /// Store into a preflighted ordinary numeric index without changing array
    /// structure. Sparse tails use the same single-threaded interior-mutation
    /// rule as ordinary object data properties.
    #[inline(always)]
    pub(crate) fn set_kernel_existing_f64(
        array: &Rc<Self>,
        index: usize,
        number: f64,
    ) -> bool {
        if !array.has_kernel_numeric_index(index) {
            return false;
        }
        let stored = if array.has_plain_dense_index(index) {
            array.values.set_existing_numeric_value(index, number)
        } else {
            let key = index.to_string();
            // SAFETY: realm execution is single-threaded; admission proved an
            // existing ordinary data property, and this changes only its value.
            let array = unsafe { &mut *(Rc::as_ptr(array) as *mut Self) };
            match array.properties.iter_mut().rev().find(|(name, _)| name == &key) {
                Some((_, Value::Number(value))) => { *value = number; true }
                _ => false,
            }
        };
        if stored {
            array.kind
                .set(monotonic_kind(array.kind.get(), number_kind(number)));
        }
        stored
    }

    /// Extend a pre-sized holey array in index order without cloning its
    /// identity. Structural uncertainty remains slow; this accepts only the
    /// next missing slot of the canonical numeric prefix.
    #[inline]
    pub(crate) fn append_preallocated_number(&self, index: usize, value: &Value) -> bool {
        let Value::Number(number) = value else {
            return false;
        };
        self.append_preallocated_f64(index, *number)
    }

    #[inline(always)]
    pub(crate) fn append_preallocated_f64(&self, index: usize, number: f64) -> bool {
        let plain = self.properties.is_empty()
            && self.indexed_descriptors_plain()
            && self.has_default_array_prototype()
            && !self.arguments
            && self.argument_live.is_none();
        // A sequential append is the common ASetI shape (for example the
        // RegExp callers may build million-code-point strings this way). Extend
        // the canonical numeric store and logical length in O(1), preserving
        // array identity even when several VM words retain the same Rc.
        if plain && index == self.physical_len() && index == self.logical_len() {
            if !self.values.append_number_shared(number) {
                return false;
            }
            self.length.set(self.length.get() + 1);
            self.kind
                .set(monotonic_kind(self.kind.get(), number_kind(number)));
            return true;
        }
        let rejected = index != self.physical_len() || index >= self.logical_len() || !plain;
        if rejected {
            return false;
        }
        if !self.values.append_number_shared(number) {
            return false;
        }
        let derived = self
            .values
            .kind_with_holes(&self.deleted, self.length.get());
        self.kind.set(derived);
        true
    }

    #[inline]
    fn has_default_array_prototype(&self) -> bool {
        crate::builtins::array_prototype_is_clean()
            && self.prototype.borrow().as_ref().is_none_or(|prototype| {
                matches!(
                    prototype,
                    Value::Builtin(crate::ops::Builtin::ArrayPrototype)
                )
            })
    }

    /// Array length is a mandatory non-enumerable descriptor. It must not
    /// disable indexed packed storage; only an indexed/accessor descriptor
    /// changes the write semantics.
    #[inline]
    fn indexed_descriptors_plain(&self) -> bool {
        self.descriptors.iter().all(|(key, descriptor)| {
            if key != "length" {
                return false;
            }
            let Value::Object(fields) = descriptor else {
                return false;
            };
            fields.iter().rev().find_map(|(name, value)| {
                (name == "writable").then_some(matches!(value, Value::Boolean(true)))
            }) == Some(true)
        })
    }

    /// Append a numeric value to an ordinary packed array without cloning its
    /// backing store. The array's physical length is its logical length in
    /// this state, so the dense store remains the single source of truth.
    #[inline(always)]
    pub(crate) fn append_shared_numbers(&self, values: &[Value]) -> bool {
        if !self.is_packed_ordinary()
            || !values.iter().all(|value| matches!(value, Value::Number(_)))
        {
            return false;
        }
        let DenseElements::Numbers(numbers) = &self.values else {
            return false;
        };
        numbers.borrow_mut().extend(values.iter().map(|value| {
            let Value::Number(number) = value else {
                unreachable!()
            };
            std::cell::Cell::new(*number)
        }));
        self.length
            .set(self.length.get().saturating_add(values.len()));
        self.kind.set(monotonic_kind(
            self.kind.get(),
            values.iter().fold(self.kind.get(), |kind, value| {
                let Value::Number(number) = value else {
                    unreachable!()
                };
                monotonic_kind(kind, number_kind(*number))
            }),
        ));
        true
    }

    #[inline(always)]
    pub(crate) fn append_shared_values(&self, values: &[Value]) -> bool {
        if !self.is_packed_ordinary() || values.is_empty() {
            return values.is_empty() && self.is_packed_ordinary();
        }
        let DenseElements::Values(current) = &self.values else {
            return false;
        };
        current.borrow_mut().extend_from_slice(values);
        self.length
            .set(self.length.get().saturating_add(values.len()));
        true
    }

    #[inline]
    pub(crate) fn dense_value_at(&self, index: usize) -> Option<Value> {
        if index >= self.logical_len()
            || index >= self.values.len()
            || self.deleted.get(index) == Some(&true)
        {
            return None;
        }
        self.values.value_at(index)
    }

    #[inline]
    pub(crate) fn last_dense_value(&self) -> Option<Value> {
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
        self.values.materialize_values().get_mut(index)
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

    /// O(1) proof that an indexed write updates an ordinary own data slot.
    /// Custom descriptors, argument mappings, holes, and sparse properties
    /// remain on the property-aware path.
    #[inline]
    pub(crate) fn has_plain_dense_index(&self, index: usize) -> bool {
        !self.arguments
            && self.argument_live.is_none()
            // The standard writable `length` descriptor is harmless for an
            // indexed data write. Only indexed/accessor descriptors make the
            // direct dense-store path unsafe.
            && self.indexed_descriptors_plain()
            && index < self.logical_len()
            && index < self.values.len()
            && self.deleted.get(index) != Some(&true)
            && self.mapped.get(index).and_then(Option::as_ref).is_none()
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
        if self
            .deleted
            .get(src..src_end)
            .is_some_and(|range| range.iter().any(|&hole| hole))
            || self
                .deleted
                .get(dst..dst_end)
                .is_some_and(|range| range.iter().any(|&hole| hole))
        {
            return false;
        }

        let source: Vec<Value> = (src..src_end)
            .filter_map(|index| self.values.value_at(index))
            .collect();
        for (offset, value) in source.into_iter().enumerate() {
            self.values.set(dst + offset, value);
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
        if self.deleted.iter().all(|deleted| !*deleted) && self.properties.is_empty() {
            if let DenseElements::Numbers(values) = &self.values {
                let values = values.borrow();
                return values
                    .iter()
                    .map(|number| Value::Number(number.get()))
                    .collect();
            }
        }
        (0..self.logical_len())
            .map(|index| self.get_index(index).unwrap_or(Value::Undefined))
            .collect()
    }

    pub fn to_vec(&self) -> Vec<Value> {
        self.snapshot()
    }

    pub(crate) fn map_index(&mut self, index: usize, binding: Rc<crate::value::BindingCell>) {
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
        if let Some((_, mut current)) = Rc::make_mut(descriptor)
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
            if self.deleted.len() <= index {
                self.deleted.resize(index.saturating_add(1), false);
            }
            self.deleted[index] = true;
            self.kind.set(ArrayKind::Holey);
            if let Some(live) = &self.argument_live {
                let mut live = live.borrow_mut();
                if live.deleted.len() <= index {
                    live.deleted.resize(index.saturating_add(1), false);
                }
                live.deleted[index] = true;
            }
        }
    }
}

fn argument_length(value: &Value) -> Option<usize> {
    match value {
        Value::Number(number) if number.is_finite() && *number >= 0.0 => {
            Some(number.floor() as usize)
        }
        _ => None,
    }
}

impl Drop for ArrayData {
    fn drop(&mut self) {
        crate::execution_trace::array_lifecycle(false);
    }
}

fn next_array_identity() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
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
        .all(|value| matches!(value, Value::Number(number) if is_limb28(*number)))
    {
        ArrayKind::PackedLimb28
    } else if values
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

/// Element kinds only become less specialized as an array is mutated.
fn monotonic_kind(previous: ArrayKind, candidate: ArrayKind) -> ArrayKind {
    if kind_rank(candidate) >= kind_rank(previous) {
        candidate
    } else {
        previous
    }
}

fn kind_rank(kind: ArrayKind) -> u8 {
    match kind {
        ArrayKind::PackedLimb28 => 0,
        ArrayKind::PackedInt => 1,
        ArrayKind::PackedDouble => 2,
        ArrayKind::PackedValue => 3,
        ArrayKind::Holey => 4,
        ArrayKind::Sparse => 5,
    }
}

#[inline]
fn is_limb28(number: f64) -> bool {
    number >= 0.0 && number <= 0x0fff_ffff as f64 && number.trunc() == number
}

#[inline]
fn number_kind(number: f64) -> ArrayKind {
    if is_limb28(number) {
        ArrayKind::PackedLimb28
    } else if number.fract() == 0.0 {
        ArrayKind::PackedInt
    } else {
        ArrayKind::PackedDouble
    }
}

fn keep_index(key: &str, length: usize) -> bool {
    crate::arrays::array_index(key).map_or(true, |index| (index as usize) < length)
}

fn set_live_index(live: &mut ArgumentLive, index: usize, value: Value) {
    if let Some(Some(binding)) = live.mapped.get(index) {
        binding.store(value.clone());
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

#[cfg(test)]
mod array_data_tests {
    use super::{ArrayData, ArrayKind};
    use crate::value::Value;

    #[test]
    fn classifies_numeric_and_holey_storage() {
        let ints = ArrayData::new(vec![Value::Number(1.0), Value::Number(2.0)]);
        assert_eq!(std::mem::size_of::<ArrayKind>(), 1);
        assert_eq!(ints.kind(), ArrayKind::PackedLimb28);
        assert!(ints.kind().is_packed());
        let ints = ArrayData::new(vec![Value::Number(-1.0)]);
        assert_eq!(ints.kind(), ArrayKind::PackedInt);
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
        assert_eq!(data.kind(), ArrayKind::PackedLimb28);

        data.set_index(0, Value::Number(1.25));
        assert_eq!(data.kind(), ArrayKind::PackedDouble);

        data.set_index(0, Value::Boolean(true));
        assert_eq!(data.kind(), ArrayKind::PackedValue);

        data.delete_property("0");
        assert_eq!(data.kind(), ArrayKind::Holey);
        data.set_index(0, Value::Number(2.0));
        assert_eq!(data.kind(), ArrayKind::Holey);

        let mut boundary =
            ArrayData::new((0..32).map(|index| Value::Number(index as f64)).collect());
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
        assert!(
            reallocations <= 6,
            "unexpected dense reallocations: {reallocations}"
        );
        data.set_index(10_000, Value::Boolean(true));
        assert!(data.is_sparse());
        assert!(!data.is_dense());
        assert_eq!(data.logical_len(), 10_001);
        assert_eq!(data.get_index(10_000), Some(Value::Boolean(true)));
    }

    #[test]
    fn numeric_sparse_tail_promotes_once_without_changing_values() {
        let mut sparse = ArrayData::new(Vec::new());
        sparse.set_length(64);
        for index in 0..64 {
            sparse.set_index(index, Value::Number(index as f64 + 0.5));
        }
        assert!(sparse.is_sparse());
        let original = sparse.clone();
        assert!(sparse.promote_sparse_numeric());
        assert!(sparse.is_packed_ordinary());
        assert_eq!(
            sparse.numeric_kernel_range(0, 64),
            original.numeric_kernel_range(0, 64)
        );
        assert!(original.is_sparse());
    }

    #[test]
    fn numeric_sparse_promotion_rejects_non_index_properties() {
        let mut sparse = ArrayData::new(Vec::new());
        sparse.set_length(64);
        sparse.set_property("metadata", Value::Number(1.0));
        assert!(!sparse.promote_sparse_numeric());
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
    fn numeric_kernel_range_reads_dense_and_sparse_own_data() {
        let dense = ArrayData::new(vec![Value::Number(1.0), Value::Number(2.0)]);
        assert_eq!(dense.numeric_kernel_range(0, 2), Some(vec![1.0, 2.0]));

        let mut sparse = ArrayData::new(Vec::new());
        sparse.set_index(10_000, Value::Number(3.0));
        sparse.set_index(10_001, Value::Number(4.0));
        assert_eq!(
            sparse.numeric_kernel_range(10_000, 10_002),
            Some(vec![3.0, 4.0])
        );
        assert_eq!(sparse.numeric_kernel_range(9_999, 10_001), None);
    }

    #[test]
    fn shared_numeric_write_updates_one_canonical_ieee_slot() {
        let data = std::rc::Rc::new(ArrayData::new(vec![Value::Number(1.0)]));
        let alias = std::rc::Rc::clone(&data);
        assert!(data.set_existing_number(0, &Value::Number(9.5)));
        assert_eq!(alias.dense_number_at(0), Some(9.5));
        assert_eq!(alias.storage_capacity(), data.storage_capacity());
    }

    #[test]
    fn limb_kind_is_one_guard_and_widens_on_unproven_mutation() {
        let data = ArrayData::new(vec![Value::Number(1.0), Value::Number(0x0fff_ffff as f64)]);
        assert_eq!(data.kind(), ArrayKind::PackedLimb28);
        {
            let mut words = data.limb28_kernel_words_mut().expect("limb words");
            words[0] = 7.0;
        }
        assert_eq!(data.kind(), ArrayKind::PackedLimb28);
        assert!(data.set_existing_f64(0, -1.0));
        assert_eq!(data.kind(), ArrayKind::PackedInt);
        assert!(data.limb28_kernel_words().is_none());

        let general = ArrayData::new(vec![Value::Number(1.0)]);
        drop(general.numeric_kernel_words_mut().expect("numeric words"));
        assert_eq!(general.kind(), ArrayKind::PackedInt);
    }
    #[test]
    fn packed_numeric_storage_is_borrowed_until_value_access() {
        let data = ArrayData::new(vec![Value::Number(1.25), Value::Number(2.5)]);
        assert!(data.is_numeric_packed());
        let capacity = data.storage_capacity();

        // The dense fast path exposes the canonical slot directly; it does
        // not construct a second numeric representation or allocate.
        let slot = data.dense_value_at(1).expect("packed slot");
        assert!(matches!(slot, Value::Number(value) if value == 2.5));
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
        assert_eq!(
            data.next_index(physical, data.logical_len()),
            Some(physical)
        );
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
