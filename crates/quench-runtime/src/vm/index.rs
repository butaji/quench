use super::*;

const MAX_DENSE_ARRAY_HOLE_GAP: usize = 1024;
#[cfg(feature = "profile-aggregate")]
const ARRAY_INDEX_GET_DENSE: usize = 0;
#[cfg(feature = "profile-aggregate")]
const ARRAY_INDEX_GET_SPARSE: usize = 1;

#[inline(always)]
pub(super) fn mutable_array_elements(
    elements: &mut crate::heap::ArrayElements,
) -> &mut crate::heap::UniqueElements {
    elements.make_mut()
}

impl<H: Host> Vm<H> {
    pub(super) fn primitive_prototype(&self, value: Value) -> Option<Value> {
        if self.heap.is_string(value) {
            return Some(self.string_proto);
        }
        let native = match self.heap.get(value) {
            Some(Cell::Symbol(_)) => Native::Symbol,
            Some(Cell::BigInt(_)) => Native::BigInt,
            _ if value.as_bool().is_some() => Native::Boolean,
            _ if value.as_number().is_some() => Native::Number,
            _ => return None,
        };
        self.realm.intrinsics.builtin_prototypes.get(&(self.realm.globals, native)).copied()
    }

    pub(super) fn require_object_coercible(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<(), JsError> {
        if value.is_null() || value.is_undefined() {
            return Err(self.type_error(p, "cannot convert null or undefined to object".into()));
        }
        Ok(())
    }

    #[inline(always)]
    /// A string's own integer-indexed property (String exotic [[GetOwnProperty]]): the
    /// one-unit string at `index`, or `None` past its end, where lookup continues on the
    /// prototype.
    pub(super) fn string_index_value(&mut self, string: Value, index: usize) -> Option<Value> {
        let Some(text) = self.heap.string(string) else {
            return None;
        };
        let unit = text.units().get(index)?;
        Some(self.heap.alloc_string(JsString::from_units(&[unit])))
    }

    /// `get_index` of a string at an integer key inside its length: the one-unit string
    /// there (`string_index_value`). `None` for any other read.
    pub(super) fn string_element(&mut self, string: Value, key: Value) -> Option<Value> {
        let index = key
            .as_number()
            .filter(|number| number.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(number))?;
        self.string_index_value(string, index as usize)
    }

    /// `get_index` of a typed array at an index key where it allocates nothing: an element
    /// of a non-BigInt kind, or `undefined` past the end. `None` for any other read.
    pub(super) fn plain_typed_element(&mut self, object: Value, key: Value) -> Option<Value> {
        let index = self.array_index_key(key)? as usize;
        match self.heap.get(object) {
            Some(Cell::TypedArray { kind, .. })
                if !matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64) => {}
            _ => return None,
        }
        self.typed_array_get(object, index)
    }

    pub(super) fn get_index(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
    ) -> Result<Value, JsError> {
        if let Some(index) = self.array_index_key(key) {
            let index = index as usize;
            // Accessor indices are stored as holes, so a present dense element is a data property.
            if let Some(array_object) = self.heap.array(object)
                && let Some(value) = array_object
                    .elements()
                    .get(index)
                    .copied()
                    .filter(|value| !value.is_deleted())
            {
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_get(ARRAY_INDEX_GET_DENSE);
                return Ok(value);
            }
            if let Some(value) = self.absent_array_element(object, key) {
                return Ok(value);
            }
            if let Some(value) = self.typed_array_get(object, index) {
                return Ok(value);
            }
            if let Some(unit) = self.string_index_value(object, index) {
                return Ok(unit);
            }
            if let Some((atom, attributes)) = self.array_descriptor_entry(object, index)
                && attributes.accessor
            {
                return self.get_property(p, object, atom);
            }
            if self.heap.array(object).is_some()
                && let Some(value) = self
                    .heap
                    .sparse_get(object, index)
                    .filter(|value| !value.is_deleted())
            {
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_get(ARRAY_INDEX_GET_SPARSE);
                return Ok(value);
            }
        }
        // A symbol or an existing non-index name that the plain chain walk answers, as the
        // lane reads it (`plain_keyed_get`): no guest code runs before the slow path would.
        if let Some(value) = self.plain_keyed_get(object, key) {
            return Ok(value);
        }
        self.get_index_slow(p, object, key)
    }

    #[inline(never)]
    fn get_index_slow(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
    ) -> Result<Value, JsError> {
        if object.is_null() || object.is_undefined() {
            // A rejected base never invokes a property-key object's conversion hooks.
            let property = if self.is_object_like(key) { None }
                else if let Some(Cell::Symbol(description)) = self.heap.get(key) { Some(format!("Symbol({})", description.as_deref().unwrap_or(""))) }
                else { Some(self.to_string(p, key)?) };
            return Err(self.nullish_property_error(p, object, property.as_deref()));
        }
        let key = if self.heap.is_string(key) || matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            key
        } else {
            self.to_property_key(p, key)?
        };
        if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            return self.get_symbol_property_with_receiver(p, object, key, object);
        }

        let atom = match self.string_key_atom(key) {
            Some(atom) => atom,
            None => {
                let key = self.coerce_js_string(p, key)?;
                self.intern_js_atom(&key)
            }
        };
        self.get_property(p, object, atom)
    }

    #[inline(never)]
    pub(super) fn set_index(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        value: Value,
    ) -> Result<(), JsError> {
        self.set_index_mode(p, object, key, value, false)
    }

    pub(super) fn set_index_mode(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        value: Value,
        strict: bool,
    ) -> Result<(), JsError> {
        if object.is_null() || object.is_undefined() {
            let property = self.detail_string(key);
            return Err(self.nullish_assignment_error(p, object, &property));
        }
        if let Some(index) = self.array_index_key(key) {
            let index = index as usize;
            // Descriptors, accessors and frozen state are the only things that make replacing a
            // present dense element differ from a plain store, and a plain array has none.
            if let Some(data) = self.heap.array(object)
                && !data.has_indexed_descriptors()
                && !data.is_frozen()
                && data
                    .elements()
                    .get(index)
                    .is_some_and(|value| !value.is_deleted())
            {
                self.replace_dense_element(object, index, value);
                return Ok(());
            }
            if self.append_dense_element(object, index, value) {
                return Ok(());
            }
            // A typed array's integer-indexed [[Set]] is TypedArraySetElement: its indices
            // carry no descriptors, accessors or integrity state for the checks below.
            if self.typed_array_set(p, object, index, value)? {
                return Ok(());
            }
            if let Some((atom, attributes)) = self.array_descriptor_entry(object, index)
                && attributes.accessor
            {
                if strict && attributes.setter.is_none() {
                    return Err(self.type_error(p, "array index has no setter".into()));
                }
                return self.set_property_with_program(p, object, atom, value);
            }
            // An index the array does not hold is OrdinarySet's to answer whenever a prototype
            // claims it.
            if self.heap.array(object).is_some()
                && !self.has_own_array_index(object, index)
                && !self.inherited_index_unclaimed(object, index)
            {
                let atom = self.intern_atom(&index.to_string());
                return self.set_property_with_program_mode(p, object, atom, value, strict);
            }
            if self
                .array_descriptor(object, index)
                .is_some_and(|attributes| !attributes.writable)
            {
                return if strict {
                    Err(self.type_error(p, "array index is not writable".into()))
                } else {
                    Ok(())
                };
            }
            if let Some(array_object) = self.heap.array(object) {
                let elements = array_object.elements();
                let existing = index < elements.len()
                    && elements.get(index).is_some_and(|value| !value.is_deleted())
                    || self
                        .heap
                        .sparse_get(object, index)
                        .is_some_and(|value| !value.is_deleted());
                let integrity = self.object_data(object);
                if integrity.is_some_and(Object::is_frozen)
                    || integrity.is_some_and(|object| !object.is_extensible()) && !existing
                {
                    let atom = self
                        .lookup_array_index_atom(index)
                        .unwrap_or_else(|| self.intern_atom(&index.to_string()));
                    return self.set_property_with_program_mode(p, object, atom, value, strict);
                }
                // An array cannot gain an index at or past a non-writable length
                // (ArrayDefineOwnProperty step 3.f), so the store fails.
                if !existing
                    && self
                        .own_array_length(object)
                        .is_some_and(|length| index >= length)
                    && self
                        .property_attributes(object, PropertyKey::string(self.length_atom))
                        .is_some_and(|attributes| !attributes.writable)
                {
                    return if strict {
                        Err(self.type_error(p, "array length is not writable".into()))
                    } else {
                        Ok(())
                    };
                }
            }
            if self.heap.array(object).is_some()
                && !self.has_own_array_index(object, index)
                && self.set_array_element(object, index, value)
            {
                return Ok(());
            }
            let replaces = self.heap.array_elements(object).is_some_and(|elements| {
                elements.get(index).is_some_and(|value| !value.is_deleted())
            });
            if replaces {
                self.replace_dense_element(object, index, value);
                return Ok(());
            }
        }
        // A symbol or an existing non-index name that the plain chain walk decides, as the
        // lane stores it: no guest code runs and nothing fails before the slow path would.
        if self.try_plain_symbol_set(object, key, value) || self.try_plain_string_key_set(object, key, value) {
            return Ok(());
        }
        self.set_index_slow(p, object, key, value, strict)
    }

    /// `dense_array_element` of an array whose object is a record; `None` for any other base.
    #[inline(always)]
    pub(super) fn direct_dense_element(&self, base: Value, key: Value) -> Option<Value> {
        let index = usize::try_from(key.as_int()?).ok()?;
        self.heap
            .record_array(base)?
            .elements()
            .get(index)
            .copied()
            .filter(|value| !value.is_deleted())
    }

    /// `base[key]` for a non-negative integer key that neither an ordinary object or array
    /// `base` nor anything on its prototype chain holds: `undefined`, as [[Get]] finds it. An
    /// object on the chain holds the index as an element of an array, as a property keyed by
    /// the index's canonical atom (none exists without one, `lookup_array_index_atom`), or
    /// through exotic behaviour, where this declines.
    pub(super) fn absent_array_element(&self, base: Value, key: Value) -> Option<Value> {
        let index = usize::try_from(key.as_int()?).ok()?;
        let atom = self.lookup_array_index_atom(index).map(PropertyKey::string);
        let mut current = base;
        loop {
            let data = self.object_data(current)?;
            if data.has_indexed_descriptors()
                || data.is_arguments_object()
                || data.boxed_primitive().is_some()
                || data.is_module_namespace()
                || data.deferred_module().is_some()
                || self.heap.sparse_length(current).is_some()
                || atom.is_some_and(|key| self.property_shape_slot(data.shape(), key).is_some())
            {
                return None;
            }
            match self.heap.array(current) {
                Some(array) if array.elements().get(index).is_some_and(|value| !value.is_deleted()) => {
                    return None;
                }
                Some(_) => {}
                None => {
                    self.heap.ordinary_object(current)?;
                }
            }
            current = data.proto();
            if current.is_null() {
                return Some(Value::UNDEFINED);
            }
        }
    }

    /// The present dense element `base[key]` for a non-negative integer key: a data property.
    #[inline(always)]
    /// What a keyed read of `base` gives for a string key naming an array index (`"0"`), as
    /// the integer key reads it (`dense_array_element`, `absent_array_element`,
    /// `plain_index_get`), or an array's `"length"`: `None` for any other key or read, which the
    /// caller's other paths decide.
    pub(super) fn string_index_element(&self, base: Value, key: Value) -> Option<Value> {
        let atom = self.lookup_string_atom(key)?;
        if atom == self.length_atom {
            return self.own_array_length(base).map(Value::length);
        }
        if !self.atom_class(atom).intersects(super::object::AtomClass::ARRAY_INDEX) {
            return None;
        }
        let index = self.array_index_key(key)?;
        let integer = Value::number(f64::from(index));
        self.dense_array_element(base, integer)
            .or_else(|| self.absent_array_element(base, integer))
            .or_else(|| self.plain_index_get(base, index as usize))
    }

    pub(super) fn dense_array_element(&self, base: Value, key: Value) -> Option<Value> {
        let index = usize::try_from(key.as_int()?).ok()?;
        let Some(object) = self.heap.array(base) else {
            return None;
        };
        object
            .elements()
            .get(index)
            .copied()
            .filter(|value| !value.is_deleted())
    }

    /// A present own data index on an ordinary array, whether it lives in the dense
    /// vector or sparse backing. Accessors and absent indices stay on the full lookup path.
    pub(super) fn direct_array_index_data(&self, base: Value, key: Value) -> Option<Value> {
        let index = usize::try_from(self.array_index_key(key)?).ok()?;
        let array = self.heap.array(base)?;
        if let Some(value) = array
            .elements()
            .get(index)
            .copied()
            .filter(|value| !value.is_deleted())
        {
            return Some(value);
        }
        if self
            .array_descriptor_entry(base, index)
            .is_some_and(|(_, attributes)| attributes.accessor)
        {
            return None;
        }
        self.heap
            .sparse_get(base, index)
            .filter(|value| !value.is_deleted())
    }

    /// Stores `value` at `key` of `object` when that replaces a present dense element of a
    /// plain array (no indexed descriptors, not frozen) with no mapped `arguments` to mirror:
    /// one array lookup serves the guards and the write. `false` leaves the store, unobserved,
    /// to the general path.
    #[inline(always)]
    pub(super) fn try_replace_dense_element(
        &mut self,
        object: Value,
        key: Value,
        value: Value,
    ) -> bool {
        self.try_replace_element_of::<true>(object, key, value, crate::heap::Heap::array_store_mut)
    }

    /// `object[key] = value` that appends to a dense array at its length
    /// (`append_dense_element`); `false` leaves the store, unobserved, to the general path.
    pub(super) fn try_append_dense_element(&mut self, object: Value, key: Value, value: Value) -> bool {
        key.as_int()
            .and_then(|index| usize::try_from(index).ok())
            .is_some_and(|index| self.append_dense_element(object, index, value))
    }

    /// `object[key] = value` that fills a hole of a dense array (`fill_dense_hole`); `false`
    /// leaves the store, unobserved, to the general path.
    pub(super) fn try_fill_dense_hole(&mut self, object: Value, key: Value, value: Value) -> bool {
        key.as_int()
            .and_then(|index| usize::try_from(index).ok())
            .is_some_and(|index| self.fill_dense_hole(object, index, value))
    }

    /// `try_replace_dense_element` of an array whose object is a record and whose elements no
    /// other array shares, for a value the write barrier passes without logging; `false` for
    /// any other write, so the write calls nothing.
    #[inline(always)]
    pub(super) fn try_replace_record_element(
        &mut self,
        object: Value,
        key: Value,
        value: Value,
    ) -> bool {
        self.try_replace_element_of::<false>(
            object,
            key,
            value,
            crate::heap::Heap::record_array_store_mut_unlogged,
        )
    }

    /// `try_replace_dense_element` of the array `array` finds for `object`; with `DETACH`, also
    /// of shared elements, which the write first copies.
    #[inline(always)]
    fn try_replace_element_of<const DETACH: bool>(
        &mut self,
        object: Value,
        key: Value,
        value: Value,
        array: fn(&mut crate::heap::Heap, Value, Value) -> Option<&mut crate::heap::ArrayObject>,
    ) -> bool {
        let Some(index) = key.as_int().and_then(|index| usize::try_from(index).ok()) else {
            return false;
        };
        let Some(array) = array(&mut self.heap, object, value) else {
            return false;
        };
        if array.has_indexed_descriptors() || array.is_frozen() || array.arguments_map().is_some() {
            return false;
        }
        let elements = array.elements_mut();
        if !elements.get(index).is_some_and(|value| !value.is_deleted())
            || (!DETACH && elements.owner_count() != 1)
        {
            return false;
        }
        #[cfg(feature = "profile-aggregate")]
        self.profile
            .array_write_ownership(elements.owner_count() != 1);
        mutable_array_elements(elements)[index] = value;
        #[cfg(feature = "profile-aggregate")]
        self.profile.index_set(0);
        true
    }

    /// Overwrites a present dense array element after every guard has passed.
    #[inline(always)]
    pub(super) fn replace_dense_element(&mut self, object: Value, index: usize, value: Value) {
        let Some(array_object) = self.heap.array_store_mut(object, value) else {
            unreachable!("dense element replacement requires an array")
        };
        // Only a mapped arguments object mirrors its elements into parameter bindings.
        let mapped = array_object.arguments_map().is_some();
        let elements = array_object.elements_mut();
        #[cfg(feature = "profile-aggregate")]
        self.profile
            .array_write_ownership(elements.owner_count() != 1);
        mutable_array_elements(elements)[index] = value;
        if mapped {
            self.sync_mapped_argument(object, index, value);
        }
        #[cfg(feature = "profile-aggregate")]
        self.profile.index_set(0);
    }

    #[inline(never)]
    fn set_index_slow(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        value: Value,
        strict: bool,
    ) -> Result<(), JsError> {
        let key = if self.heap.is_string(key) || matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            key
        } else {
            self.to_property_key(p, key)?
        };
        if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            let succeeded =
                self.set_symbol_property_with_receiver(p, object, key, value, object)?;
            return if succeeded || !strict {
                Ok(())
            } else {
                Err(self.type_error(p, "cannot assign symbol property".into()))
            };
        }
        if self.heap.array(object).is_some() && self.prototype_chain_contains_proxy(object) {
            let key = self.coerce_js_string(p, key)?;
            let atom = self.intern_js_atom(&key);
            return self.set_property_with_program(p, object, atom, value);
        }

        let key_value = key;
        let key = self.coerce_js_string(p, key)?;
        if let Some(index) = super::object_static::array_index(key.host_string())
            && self.heap.array(object).is_some()
            && !self.has_own_array_index(object, index as usize)
            && !self.inherited_index_unclaimed(object, index as usize)
        {
            let atom = self.intern_js_atom(&key);
            return self.set_property_with_program_mode(p, object, atom, value, strict);
        }
        if let Some(index) = super::object_static::array_index(key.host_string())
            && self.heap.array(object).is_some()
        {
            if self
                .array_descriptor(object, index as usize)
                .is_some_and(|attributes| attributes.accessor)
            {
                if strict
                    && self
                        .array_descriptor(object, index as usize)
                        .is_some_and(|attributes| attributes.setter.is_none())
                {
                    return Err(self.type_error(p, "array index has no setter".into()));
                }
                let atom = self.intern_js_atom(&key);
                return self.set_property_with_program(p, object, atom, value);
            }
            if self
                .array_descriptor(object, index as usize)
                .is_some_and(|attributes| !attributes.writable)
            {
                return if strict {
                    Err(self.type_error(p, "array index is not writable".into()))
                } else {
                    Ok(())
                };
            }
            if self.set_array_element(object, index as usize, value) {
                return Ok(());
            }
        }
        let atom = match self.string_key_atom(key_value) {
            Some(atom) => atom,
            None => self.intern_js_atom(&key),
        };
        self.set_property_with_program_mode(p, object, atom, value, strict)
    }

    pub(super) fn array_index_key(&self, key: Value) -> Option<u32> {
        if let Some(index) = key.as_int() {
            return u32::try_from(index).ok();
        }
        let Some(key) = self.heap.string(key) else {
            return None;
        };
        super::object_static::array_index(key.host_string())
    }

    pub(super) fn set_array_element(&mut self, object: Value, index: usize, value: Value) -> bool {
        if self.check_array_element_write(object, index).is_err() {
            return false;
        }
        self.store_validated_array_element(object, index, value)
    }

    /// Storage after either assignment checks or ValidateAndApplyPropertyDescriptor.
    /// A compatible definition can replace an accessor hole on a non-extensible
    /// array, or preserve the same value on a frozen array.
    pub(super) fn store_validated_array_element(&mut self, object: Value, index: usize, value: Value) -> bool {
        let Some(array_object) = self.heap.array(object) else {
            return false;
        };
        let elements = array_object.elements();
        let dense_len = elements.len();
        if index < dense_len {
            let Some(array_object) = self.heap.array_store_mut(object, value) else {
                unreachable!()
            };
            let elements = array_object.elements_mut();
            #[cfg(feature = "profile-aggregate")]
            self.profile
                .array_write_ownership(elements.owner_count() != 1);
            mutable_array_elements(elements)[index] = value;
            self.sync_mapped_argument(object, index, value);
            return true;
        }
        let sparse = self.array_index_uses_sparse_storage(object, index, dense_len);
        if sparse {
            self.heap.sparse_set(object, index, value);
        } else if let Some(array_object) = self.heap.array_mut(object) {
            let elements = array_object.elements_mut();
            #[cfg(feature = "profile-aggregate")]
            self.profile
                .array_write_ownership(elements.owner_count() != 1);
            let elements = mutable_array_elements(elements);
            let capacity = elements.capacity();
            elements.resize(index + 1, Value::DELETED);
            elements[index] = value;
            let grown = elements.capacity() - capacity;
            self.heap.charge_payload_growth(grown * size_of::<Value>());
        }
        self.sync_mapped_argument(object, index, value);
        true
    }

    pub(super) fn define_array_literal_element(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        index: usize,
        value: Value,
    ) -> Result<(), JsError> {
        if self.set_array_element(object, index, value) {
            Ok(())
        } else {
            Err(self.type_error(p, "cannot define array literal element".into()))
        }
    }

    /// ArrayAccumulation's CreateDataPropertyOrThrow at the literal's next index, which is its
    /// length: the literal is still unreachable from user code, so no hook observes it.
    pub(super) fn append_array_literal_elements(
        &mut self,
        p: &ResidualProgram,
        array: Value,
        values: impl IntoIterator<Item = Value>,
    ) -> Result<(), JsError> {
        let mut index = self
            .own_array_length(array)
            .ok_or_else(|| JsError("array literal is not an array".into()))?;
        for value in values {
            self.define_array_literal_element(p, array, index, value)?;
            index += 1;
        }
        Ok(())
    }

    /// ArrayAccumulation of elisions: each advances the literal's next index without an element.
    pub(super) fn append_array_literal_holes(
        &mut self,
        array: Value,
        count: usize,
    ) -> Result<(), JsError> {
        let length = self
            .own_array_length(array)
            .ok_or_else(|| JsError("array literal is not an array".into()))?;
        if count > 0 {
            self.heap.sparse_set_length(array, length + count);
        }
        Ok(())
    }

    fn array_index_uses_sparse_storage(
        &self,
        object: Value,
        index: usize,
        dense_length: usize,
    ) -> bool {
        self.heap.sparse_length(object).is_some()
            || index.saturating_sub(dense_length) > MAX_DENSE_ARRAY_HOLE_GAP
    }

    pub(super) fn check_array_element_write(
        &self,
        object: Value,
        index: usize,
    ) -> Result<(), JsError> {
        let Some(array_object) = self.heap.array(object) else {
            return Err(JsError("array receiver is not array".into()));
        };
        let elements = array_object.elements();
        let existing = index < elements.len()
            && elements.get(index).is_some_and(|value| !value.is_deleted())
            || self
                .heap
                .sparse_get(object, index)
                .is_some_and(|value| !value.is_deleted());
        let integrity = self.object_data(object);
        if integrity.is_some_and(Object::is_frozen)
            || integrity.is_some_and(|object| !object.is_extensible()) && !existing
        {
            return Err(JsError("cannot write sealed or frozen array".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_host::TestHost;


    #[test]
    fn high_array_index_stays_out_of_dense_storage() {
        let mut vm = Vm::new(TestHost::default());
        let array = vm.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Vm::<TestHost>::empty_object(Value::NULL),
                vec![],
            ),
        });
        assert!(vm.set_array_element(array, 1_000_000, Value::TRUE));
        let Some(array_object) = vm.heap.array(array) else {
            panic!("array cell")
        };
        let elements = array_object.elements();
        assert!(elements.is_empty());
        assert_eq!(vm.heap.sparse_length(array), Some(1_000_001));
    }

    #[test]
    fn bounded_holey_array_length_uses_dense_storage() {
        let program = crate::Engine::specialize("", "dense-array-holes.js").unwrap();
        let mut vm = Vm::new(TestHost::default());
        let array = vm.array_create(&program, 16_900).unwrap();

        let Some(array_object) = vm.heap.array(array) else {
            panic!("array cell")
        };
        let elements = array_object.elements();
        assert_eq!(elements.len(), 16_900);
        assert!(elements.iter().all(|value| value.is_deleted()));
        assert_eq!(vm.heap.sparse_length(array), None);

        assert!(vm.set_array_element(array, 16_899, Value::TRUE));
        let Some(array_object) = vm.heap.array(array) else {
            panic!("array cell")
        };
        let elements = array_object.elements();
        assert_eq!(elements[16_899], Value::TRUE);
        assert!(elements[16_898].is_deleted());
        assert_eq!(vm.heap.sparse_length(array), None);
    }

    #[test]
    fn bounded_sparse_write_promotes_to_dense_storage() {
        let mut vm = Vm::new(TestHost::default());
        let array = vm.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Vm::<TestHost>::empty_object(Value::NULL),
                Vec::new(),
            ),
        });

        assert!(vm.set_array_element(array, 2_048, Value::TRUE));

        let Some(array_object) = vm.heap.array(array) else {
            panic!("array cell")
        };
        let elements = array_object.elements();
        assert_eq!(elements.len(), 2_049);
        assert!(elements[..2_048].iter().all(|value| value.is_deleted()));
        assert_eq!(elements[2_048], Value::TRUE);
        assert_eq!(vm.heap.sparse_length(array), None);
    }

    #[test]
    fn shrinking_large_sparse_array_promotes_and_preserves_values() {
        let mut vm = Vm::new(TestHost::default());
        let array = vm.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Vm::<TestHost>::empty_object(Value::NULL),
                Vec::new(),
            ),
        });
        vm.heap.sparse_set_length(array, 1_000_000);
        vm.heap.sparse_set(array, 3, Value::TRUE);
        vm.heap.sparse_set(array, 999_999, Value::FALSE);

        vm.heap.sparse_set_length(array, 16_900);

        let Some(array_object) = vm.heap.array(array) else {
            panic!("array cell")
        };
        let elements = array_object.elements();
        assert_eq!(elements.len(), 16_900);
        assert_eq!(elements[3], Value::TRUE);
        assert!(elements[4].is_deleted());
        assert_eq!(vm.heap.sparse_length(array), None);
    }

    #[test]
    fn ordinary_indexed_writes_do_not_intern_property_names() {
        let program = crate::Engine::specialize("", "array-index-atoms.js").unwrap();
        let mut vm = Vm::new(TestHost::default());
        let array = vm.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Vm::<TestHost>::empty_object(Value::NULL),
                Vec::new(),
            ),
        });
        let atom_count = vm.atom_sequence.len();
        vm.heap.retain_allocations_for_test();
        let cell_count = vm.heap.occupied_cell_count_for_test();

        for index in 0..1024 {
            vm.set_index(
                &program,
                array,
                Value::integer(index),
                Value::integer(index),
            )
            .unwrap();
        }

        assert_eq!(vm.atom_sequence.len(), atom_count);
        assert!(vm.lookup_array_index_atom(1023).is_none());
        assert_eq!(vm.heap.occupied_cell_count_for_test(), cell_count);
    }

    #[test]
    fn deleting_an_uninterned_present_index_does_not_create_an_atom() {
        let mut vm = Vm::new(TestHost::default());
        let array = vm.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Vm::<TestHost>::empty_object(Value::NULL),
                vec![Value::FALSE; 1024],
            ),
        });
        let atom_count = vm.atom_sequence.len();

        assert_eq!(vm.delete_array_index(array, 1023), Value::TRUE);

        let Some(array_object) = vm.heap.array(array) else {
            panic!("array cell")
        };
        let elements = array_object.elements();
        assert!(elements[1023].is_deleted());
        assert_eq!(vm.atom_sequence.len(), atom_count);
        assert!(vm.lookup_array_index_atom(1023).is_none());
    }

    #[test]
    fn array_index_atom_lookup_uses_canonical_decimal_text() {
        let mut vm = Vm::new(TestHost::default());
        for index in [0, 9, 10, 99, 100, 1023, u32::MAX as usize, usize::MAX] {
            let expected = vm.intern_atom(&index.to_string());
            assert_eq!(vm.lookup_array_index_atom(index), Some(expected));
        }
    }

    #[test]
    fn indexed_write_still_calls_an_inherited_numeric_setter() {
        let source = r#"
            var observed = -1;
            Object.defineProperty(Array.prototype, "5", {
                configurable: true,
                set: function(value) { observed = value; }
            });
            var array = [];
            array[5] = 17;
            if (observed !== 17 || Object.prototype.hasOwnProperty.call(array, "5")) {
                throw new Error("inherited numeric setter was skipped");
            }
        "#;
        let program = crate::Engine::specialize(source, "array-index-setter.js").unwrap();
        let mut vm = Vm::new(TestHost::default());
        vm.execute(&program).unwrap();
    }

    #[test]
    fn indexed_write_keeps_default_inherited_data_semantics_without_atoms() {
        let source = r#"
            var prototype = [41];
            var array = [];
            Object.setPrototypeOf(array, prototype);
            array[0] = 9;
            if (array[0] !== 9 || prototype[0] !== 41 || !array.hasOwnProperty(0)) {
                throw new Error("indexed write did not create an own element");
            }
        "#;
        let program = crate::Engine::specialize(source, "array-index-prototype-data.js").unwrap();
        let mut vm = Vm::new(TestHost::default());
        assert!(vm.lookup_array_index_atom(0).is_none());
        vm.execute(&program).unwrap();
    }

    #[test]
    fn unique_array_mutation_keeps_the_allocation() {
        let mut elements = crate::heap::ArrayElements::from(vec![Value::FALSE]);
        let allocation = elements.as_ptr();
        mutable_array_elements(&mut elements)[0] = Value::TRUE;
        assert_eq!(elements.as_ptr(), allocation);
        assert_eq!(elements[0], Value::TRUE);
    }

    #[test]
    fn shared_array_mutation_detaches_the_template() {
        let template = crate::heap::ArrayElements::from(vec![Value::FALSE]);
        let mut elements = template.clone();
        mutable_array_elements(&mut elements)[0] = Value::TRUE;
        assert_eq!(template[0], Value::FALSE);
        assert_eq!(elements[0], Value::TRUE);
        assert_ne!(template.as_ptr(), elements.as_ptr());
    }
}
