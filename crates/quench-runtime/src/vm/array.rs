use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn indexed_at_index(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        length: usize,
    ) -> Result<Option<usize>, JsError> {
        let number = self.to_number(p, value)?;
        let integer = if number.is_nan() { 0.0 } else { number.trunc() };
        let relative = if integer < 0.0 {
            length as f64 + integer
        } else {
            integer
        };
        if relative < 0.0 || relative >= length as f64 {
            Ok(None)
        } else {
            Ok(Some(relative as usize))
        }
    }

    pub(super) fn array_value_at(&self, array: Value, index: usize) -> Value {
        let value = self.heap.array_elements(array).and_then(|elements| {
            elements
                .get(index)
                .copied()
                .filter(|value| !value.is_deleted())
                .or_else(|| {
                    self.heap
                        .sparse_get(array, index)
                        .filter(|value| !value.is_deleted())
                })
        });
        value.unwrap_or(Value::UNDEFINED)
    }

    pub(super) fn array_reverse_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let (mut lower, mut upper) = (0, length.saturating_sub(1));
        while lower < upper {
            let lower_key = Value::number(lower as f64);
            let upper_key = Value::number(upper as f64);
            let lower_present = self.has_property(p, object, lower_key)?;
            let lower_value = lower_present
                .then(|| self.get_index(p, object, lower_key))
                .transpose()?;
            let upper_present = self.has_property(p, object, upper_key)?;
            let upper_value = upper_present
                .then(|| self.get_index(p, object, upper_key))
                .transpose()?;
            match (lower_value, upper_value) {
                (Some(lower_value), Some(upper_value)) => {
                    self.set_index_mode(p, object, lower_key, upper_value, true)?;
                    self.set_index_mode(p, object, upper_key, lower_value, true)?;
                }
                (None, Some(upper_value)) => {
                    self.set_index_mode(p, object, lower_key, upper_value, true)?;
                    self.delete_array_like_property(p, object, upper_key)?;
                }
                (Some(lower_value), None) => {
                    self.delete_array_like_property(p, object, lower_key)?;
                    self.set_index_mode(p, object, upper_key, lower_value, true)?;
                }
                (None, None) => {}
            }
            lower += 1;
            upper -= 1;
        }
        Ok(object)
    }

    pub(super) fn array_shift_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        if length == 0 {
            self.set_array_like_length(p, object, 0)?;
            return Ok(Value::UNDEFINED);
        }
        if let Some(first) = self.shift_dense(object, length) {
            return Ok(first);
        }
        let first = self.get_index(p, object, Value::number(0.0))?;
        for index in 1..length {
            let source = Value::number(index as f64);
            let destination = Value::number((index - 1) as f64);
            if self.has_property(p, object, source)? {
                let value = self.get_index(p, object, source)?;
                self.set_index_mode(p, object, destination, value, true)?;
            } else {
                self.delete_array_like_property(p, object, destination)?;
            }
        }
        self.delete_array_like_property(p, object, Value::number((length - 1) as f64))?;
        self.set_array_like_length(p, object, length - 1)?;
        Ok(first)
    }

    pub(super) fn array_unshift_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let new_length = length
            .checked_add(args.len())
            .filter(|length| *length as f64 <= MAX_SAFE_INTEGER)
            .ok_or_else(|| self.type_error(p, "array-like length exceeds safe integer".into()))?;
        if self.splice_dense_elements(object, length, 0, 0, args).is_some() {
            return Ok(Value::number(new_length as f64));
        }
        if !args.is_empty() {
            for index in (1..=length).rev() {
                let source = Value::number((index - 1) as f64);
                let destination = Value::number((index + args.len() - 1) as f64);
                if self.has_property(p, object, source)? {
                    let value = self.get_index(p, object, source)?;
                    self.set_index_mode(p, object, destination, value, true)?;
                } else {
                    self.delete_array_like_property(p, object, destination)?;
                }
            }
            for (index, value) in args.iter().copied().enumerate() {
                self.set_index_mode(p, object, Value::number(index as f64), value, true)?;
            }
        }
        self.set_array_like_length(p, object, new_length)?;
        Ok(Value::number(new_length as f64))
    }

    /// `shift` of a non-empty array of `length` whose every index is an ordinary own data
    /// element and whose `length` is writable, so shifting consults no prototype accessor
    /// and runs no guest code: one vector removal. Exceptional descriptors, holes, mapped
    /// arguments and integrity states leave `None`, for the ordered property algorithm.
    fn shift_dense(&mut self, object: Value, length: usize) -> Option<Value> {
        let dense = length > 0
            && self.heap.sparse_length(object).is_none()
            && self.heap.array(object).is_some_and(|data| {
                let elements = data.elements();
                data.is_extensible()
                    && !data.is_arguments_object()
                    && !data.has_indexed_descriptors()
                    && elements.len() == length
                    && elements.iter().all(|value| !value.is_deleted())
            });
        let writable_length = self
            .property_attributes(object, super::property_key::PropertyKey::string(self.length_atom))
            .is_some_and(|attributes| attributes.writable);
        if !dense || !writable_length {
            return None;
        }
        let elements = self.heap.array_elements_mut(object).expect("dense array guard");
        Some(super::index::mutable_array_elements(elements).remove(0))
    }

    /// `shift` of an array as `shift_dense` makes it, which observes nothing.
    pub(super) fn plain_array_shift(&mut self, this: Value) -> Option<Value> {
        let length = self.own_array_length(this)?;
        self.shift_dense(this, length)
    }

    pub(super) fn delete_array_like_property(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
    ) -> Result<(), JsError> {
        let deleted = self.object_delete_property(p, &[object, key])?;
        if self.truthy(deleted) {
            Ok(())
        } else {
            Err(self.type_error(p, "cannot delete array-like property".into()))
        }
    }

    pub(super) fn array_splice_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let (start, delete_count) = self.splice_window(p, args, length)?;
        let removed = self.array_species_create(p, object, delete_count)?;
        let items = args.get(2..).unwrap_or_default();
        if self.array_splice_dense(object, length, start, delete_count, items, removed) {
            return Ok(removed);
        }
        // `removed` is held only here while guest code (getters, traps) can collect.
        self.with_call_roots([removed], |vm| {
            for offset in 0..delete_count {
                let source_index = start + offset;
                let source = Value::number(source_index as f64);
                if vm.has_property(p, object, source)? {
                    let value = vm.get_index(p, object, source)?;
                    vm.create_data_property_or_throw(p, removed, offset, value)?;
                }
            }
            vm.set_array_like_length(p, removed, delete_count)?;

            let items = args.iter().copied().skip(2).collect::<Vec<_>>();
            let new_length = length
                .checked_sub(delete_count)
                .and_then(|length| length.checked_add(items.len()))
                .filter(|length| *length as f64 <= MAX_SAFE_INTEGER)
                .ok_or_else(|| vm.type_error(p, "splice result exceeds safe integer".into()))?;
            if items.len() < delete_count {
                for index in start..(length - delete_count) {
                    vm.move_array_like_property(
                        p,
                        object,
                        index + items.len(),
                        index + delete_count,
                    )?;
                }
                for index in (new_length..length).rev() {
                    vm.delete_array_like_property(p, object, Value::number(index as f64))?;
                }
            } else if items.len() > delete_count {
                for index in (start..(length - delete_count)).rev() {
                    vm.move_array_like_property(
                        p,
                        object,
                        index + items.len(),
                        index + delete_count,
                    )?;
                }
            }
            for (offset, value) in items.into_iter().enumerate() {
                vm.set_index_mode(
                    p,
                    object,
                    Value::number((start + offset) as f64),
                    value,
                    true,
                )?;
            }
            vm.set_array_like_length(p, object, new_length)?;
            Ok(removed)
        })
    }

    /// `splice` of a dense array (`dense_array_length`) as one vector splice, once every
    /// guest-visible step before the moves has run: its length is still the one the
    /// arguments were clamped to, each element it moves or removes is present, `removed` is
    /// another dense array holding only holes, and no prototype accessor sees a grown index.
    /// Each generic step then reads, writes or deletes an own writable data element.
    fn array_splice_dense(
        &mut self,
        this: Value,
        length: usize,
        start: usize,
        delete_count: usize,
        items: &[Value],
        removed: Value,
    ) -> bool {
        if this == removed {
            return false;
        }
        let Some(removed_length) = self.dense_array_length(removed) else {
            return false;
        };
        let removed_holds_holes = removed_length <= delete_count
            && self
                .heap
                .array_elements(removed)
                .is_some_and(|elements| elements.iter().all(|value| value.is_deleted()));
        if !removed_holds_holes {
            return false;
        }
        let Some(taken) = self.splice_dense_elements(this, length, start, delete_count, items) else {
            return false;
        };
        let Some(removed_object) = self.heap.array_mut(removed) else {
            unreachable!("matched an array above");
        };
        *removed_object.elements_mut() = taken.into();
        true
    }

    /// The receiver's half of `array_splice_dense`, which `unshift` also is, as
    /// `splice(0, 0, ...items)`: `this` is still a dense array of `length`
    /// (`dense_array_length`), each element from `start` on is present, and each index it
    /// grows into stores plainly, so every generic step reads, writes or deletes an own
    /// writable data element. The elements it removed, or `None` with nothing changed.
    fn splice_dense_elements(
        &mut self,
        this: Value,
        length: usize,
        start: usize,
        delete_count: usize,
        items: &[Value],
    ) -> Option<Vec<Value>> {
        if self.dense_array_length(this) != Some(length) {
            return None;
        }
        let moved_present = self
            .heap
            .array_elements(this)
            .is_some_and(|elements| elements[start..].iter().all(|value| !value.is_deleted()));
        let new_length = (length - delete_count)
            .checked_add(items.len())
            .filter(|length| *length <= MAX_ARRAY_LENGTH)?;
        if !moved_present || !self.array_indices_store_plainly(this, length..new_length) {
            return None;
        }
        let Some(object) = self.heap.array_mut(this) else {
            unreachable!("matched an array above");
        };
        let elements = super::index::mutable_array_elements(object.elements_mut());
        let taken = elements[start..start + delete_count].to_vec();
        elements.splice(start..start + delete_count, items);
        Some(taken)
    }

    /// `splice`'s start and delete count over an array-like of `length`: its first two
    /// arguments as relative index and clamped count, which convert Numbers without
    /// observing anything.
    fn splice_window(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        length: usize,
    ) -> Result<(usize, usize), JsError> {
        let start = args
            .first()
            .copied()
            .map(|value| self.array_relative_index(p, value, length))
            .transpose()?
            .unwrap_or(0);
        let available = length - start;
        let delete_count = match args.get(1).copied() {
            None if args.len() == 1 => available,
            None => 0,
            Some(value) => {
                let number = self.to_number(p, value)?;
                if number.is_nan() || number <= 0.0 {
                    0
                } else if number.is_infinite() {
                    available
                } else {
                    (number.trunc() as usize).min(available)
                }
            }
        };
        Ok((start, delete_count))
    }

    /// `splice` of this realm (`callee`) on a dense array whose steps run no guest code:
    /// Number window arguments (`splice_window`), an ArraySpeciesCreate that reads plain
    /// data to the intrinsic `%Array%` (`array_species_is_intrinsic`) and so makes a fresh
    /// array, and the dense splice (`array_splice_dense`). `None` leaves the call, with
    /// nothing observable done, to the general path.
    pub(super) fn plain_array_splice(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        this: Value,
        args: &[Value],
    ) -> Option<Value> {
        if !self.is_current_realm_native(callee, Native::ArraySplice)
            || args.iter().take(2).any(|value| value.as_number().is_none())
            || !self.array_species_is_intrinsic(this)
        {
            return None;
        }
        let length = self.dense_array_length(this)?;
        let (start, delete_count) = self.splice_window(p, args, length).ok()?;
        let removed = self.array_create(p, delete_count).ok()?;
        let items = args.get(2..).unwrap_or_default();
        self.array_splice_dense(this, length, start, delete_count, items, removed)
            .then_some(removed)
    }

    /// `object`'s own data property `atom` (`Some(None)` when it has no own `atom`), read
    /// from its shape without a call; `None` for an accessor, or for an object keeping
    /// descriptors outside its shape, which its shape alone does not answer.
    fn own_plain_data(&self, object: Value, atom: Atom) -> Option<Option<Value>> {
        let data = self.object_data(object)?;
        if data.has_side_descriptors() {
            return None;
        }
        let Some(slot) = self.shape_slot(data.shape(), atom) else {
            return Some(None);
        };
        if self.slot_attributes(data, slot)?.accessor {
            return None;
        }
        Some(Some(self.heap.property_get(data, slot)?))
    }

    /// `unshift` of a dense array as `splice_dense_elements` makes it, which observes
    /// nothing; `None` leaves the general path's steps.
    pub(super) fn plain_array_unshift(&mut self, this: Value, items: &[Value]) -> Option<Value> {
        let length = self.dense_array_length(this)?;
        self.splice_dense_elements(this, length, 0, 0, items)?;
        Some(Value::number((length + items.len()) as f64))
    }

    /// Whether ArraySpeciesCreate of the array `array` provably makes a plain array of this
    /// realm: its `constructor` is an own data property, or this realm's `Array.prototype`
    /// holds it as one (`own_plain_data`), with the value `undefined` or this realm's
    /// `%Array%`, whose `@@species` getter is intrinsic. The reads observe nothing.
    fn array_species_is_intrinsic(&self, array: Value) -> bool {
        let atom = self.runtime_atoms.constructor;
        if self.heap.array(array).is_none() {
            return false;
        }
        let constructor = match self.own_plain_data(array, atom) {
            Some(Some(constructor)) => constructor,
            Some(None) => {
                let prototype = self.array_prototype_for_realm(self.realm.globals);
                if self.object_data(array).map(Object::proto) != Some(prototype) {
                    return false;
                }
                match self.own_plain_data(prototype, atom) {
                    Some(Some(constructor)) => constructor,
                    _ => return false,
                }
            }
            None => return false,
        };
        constructor.is_undefined()
            || (matches!(
                self.heap.function(constructor),
                Some(FunctionRecord { kind: FunctionKind::Native(Native::Array), realm, .. })
                    if *realm == self.realm.globals
            ) && self.has_intrinsic_species_getter(constructor))
    }

    fn move_array_like_property(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        destination: usize,
        source: usize,
    ) -> Result<(), JsError> {
        let source_key = Value::number(source as f64);
        let destination_key = Value::number(destination as f64);
        if self.has_property(p, object, source_key)? {
            let value = self.get_index(p, object, source_key)?;
            self.set_index_mode(p, object, destination_key, value, true)
        } else {
            self.delete_array_like_property(p, object, destination_key)
        }
    }

    pub(super) fn array_fill_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let start = args
            .get(1)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.array_relative_index(p, value, length))
            .transpose()?
            .unwrap_or(0);
        let end = args
            .get(2)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.array_relative_index(p, value, length))
            .transpose()?
            .unwrap_or(length);
        if self.fill_plain_array_elements(object, start..end, value) {
            return Ok(object);
        }
        for index in start..end {
            self.set_index_mode(p, object, Value::number(index as f64), value, true)?;
        }
        Ok(object)
    }

    /// Stores `value` at every index of `indices`, all below the array's length, where each
    /// Set is the plain element store: an extensible, unfrozen array whose elements carry
    /// default attributes and whose prototypes hold no index (`prototypes_hold_no_index`), so
    /// a present element is replaced and a hole defined in place. `false` leaves every index
    /// to the generic Set, having stored none.
    fn fill_plain_array_elements(
        &mut self,
        array: Value,
        indices: std::ops::Range<usize>,
        value: Value,
    ) -> bool {
        let Some(dense) = self.heap.array(array).map(|array| array.elements().len()) else {
            return false;
        };
        let plain = self.object_data(array).is_some_and(|object| {
            object.is_extensible()
                && !object.is_frozen()
                && !object.has_indexed_descriptors()
                && !object.is_arguments_object()
        }) && self.prototypes_hold_no_index(array);
        if !plain {
            return false;
        }
        let in_dense = indices.start..indices.end.min(dense);
        if !in_dense.is_empty() {
            let Some(array_object) = self.heap.array_store_mut(array, value) else {
                unreachable!("matched an array above");
            };
            super::index::mutable_array_elements(array_object.elements_mut())[in_dense].fill(value);
        }
        for index in dense.max(indices.start)..indices.end {
            self.store_validated_array_element(array, index, value);
        }
        true
    }

    pub(super) fn array_concat_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = self.box_object_or_type_error(p, this)?;
        let target = self.array_species_create(p, source, 0)?;
        // `target` is held only here while guest code (getters, traps) can collect.
        self.with_call_roots([target], |vm| {
            let spreadable_atom = vm
                .well_known_symbols
                .get(WellKnownSymbol::IsConcatSpreadable)
                .ok_or_else(|| JsError("Symbol.isConcatSpreadable is not initialized".into()))?;
            let mut next = 0usize;
            for item in std::iter::once(source).chain(args.iter().copied()) {
                let spreadable = if vm.is_object_like(item) {
                    let value = vm.get_index(p, item, spreadable_atom)?;
                    if value.is_undefined() {
                        vm.is_array(p, item)?
                    } else {
                        vm.truthy(value)
                    }
                } else {
                    false
                };
                if !spreadable {
                    if next as f64 >= MAX_SAFE_INTEGER {
                        return Err(
                            vm.type_error(p, "array concat index exceeds safe integer".into())
                        );
                    }
                    vm.create_data_property_or_throw(p, target, next, item)?;
                    next += 1;
                    continue;
                }
                let length = vm.array_like_length(p, item)?;
                if next.saturating_add(length) as f64 > MAX_SAFE_INTEGER {
                    return Err(vm.type_error(p, "array concat length exceeds safe integer".into()));
                }
                // Present own data elements read without observing anything, and append as
                // the definitions would.
                if let Some(values) = vm.present_dense_elements(item, 0..length).map(<[Value]>::to_vec)
                    && vm.append_dense_definitions(target, next, &values)
                {
                    next += length;
                    continue;
                }
                for index in 0..length {
                    let source_key = Value::number(index as f64);
                    if vm.has_property(p, item, source_key)? {
                        let value = vm.get_index(p, item, source_key)?;
                        vm.create_data_property_or_throw(p, target, next, value)?;
                    }
                    next += 1;
                }
            }
            vm.set_array_like_length(p, target, next)?;
            Ok(target)
        })
    }

    pub(super) fn array_push_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if let Some(length) = self.array_push_dense(this, args) {
            return Ok(length);
        }
        self.array_push_with_array_like_semantics(p, this, args)
    }

    /// The length of a plain dense array whose `push` and `pop` reduce to appending to and
    /// removing from its element vector: it is extensible, unfrozen, holds no indexed
    /// descriptors or sparse elements, is no arguments object and has a writable `length`.
    /// `None` leaves every other receiver to the generic algorithms.
    pub(super) fn dense_array_length(&self, this: Value) -> Option<usize> {
        let object = self.heap.array(this)?;
        if object.has_indexed_descriptors()
            || object.is_frozen()
            || !object.is_extensible()
            || object.is_arguments_object()
            || self.heap.sparse_length(this).is_some()
        {
            return None;
        }
        let length = object.elements().len();
        // An array's `length` keeps non-default attributes only as a recorded descriptor.
        if object.has_length_descriptor()
            && self
                .property_attributes(this, PropertyKey::string(self.length_atom))
                .is_some_and(|attributes| !attributes.writable)
        {
            return None;
        }
        Some(length)
    }

    /// The elements `range` of an array whose indices there are present own data elements:
    /// it holds no indexed descriptors, sparse storage or arguments mapping, and no hole in
    /// `range`. HasProperty and Get of each such index read the element and observe nothing.
    pub(super) fn present_dense_elements(
        &self,
        this: Value,
        range: std::ops::Range<usize>,
    ) -> Option<&[Value]> {
        let object = self.heap.array(this)?;
        if object.has_indexed_descriptors()
            || object.is_arguments_object()
            || self.heap.sparse_length(this).is_some()
        {
            return None;
        }
        let elements = object.elements().get(range)?;
        elements.iter().all(|value| !value.is_deleted()).then_some(elements)
    }

    /// CreateDataPropertyOrThrow of `values` at indices `start..` of `array`, a dense array
    /// (`dense_array_length`) of length `start`: each definition appends a plain element,
    /// whatever its prototypes hold, as a definition consults none. `false` leaves `array`
    /// untouched.
    pub(super) fn append_dense_definitions(&mut self, array: Value, start: usize, values: &[Value]) -> bool {
        if self.dense_array_length(array) != Some(start)
            || start.checked_add(values.len()).is_none_or(|end| end > MAX_ARRAY_LENGTH)
        {
            return false;
        }
        let Some(object) = self.heap.array_mut(array) else {
            unreachable!("matched an array above");
        };
        let elements = super::index::mutable_array_elements(object.elements_mut());
        let capacity = elements.capacity();
        elements.extend_from_slice(values);
        let grown = elements.capacity() - capacity;
        self.heap.charge_payload_growth(grown * size_of::<Value>());
        true
    }

    /// CreateDataPropertyOrThrow of `values` at indices `0..` of `array`, a dense array
    /// (`dense_array_length`) of exactly their count holding only holes, as ArrayCreate
    /// makes one: each definition fills its hole. `false` leaves `array` untouched.
    pub(super) fn fill_dense_holes(&mut self, array: Value, values: &[Value]) -> bool {
        if self.dense_array_length(array) != Some(values.len())
            || !self
                .heap
                .array(array)
                .is_some_and(|object| object.elements().iter().all(|value| value.is_deleted()))
        {
            return false;
        }
        let Some(object) = self.heap.array_mut(array) else {
            unreachable!("matched an array above");
        };
        super::index::mutable_array_elements(object.elements_mut()).copy_from_slice(values);
        true
    }

    /// `push` onto a dense array (`dense_array_length`) as one append, when each appended
    /// index stores plainly (`array_indices_store_plainly`).
    pub(super) fn array_push_dense(&mut self, this: Value, args: &[Value]) -> Option<Value> {
        let length = self.dense_array_length(this)?;
        let final_length = length.checked_add(args.len())?;
        if final_length > MAX_ARRAY_LENGTH {
            return None;
        }
        if !self.array_indices_store_plainly(this, length..final_length) {
            return None;
        }
        let Some(object) = self.heap.array_mut(this) else {
            unreachable!("matched an array above");
        };
        let elements = super::index::mutable_array_elements(object.elements_mut());
        let capacity = elements.capacity();
        elements.extend_from_slice(args);
        let grown = elements.capacity() - capacity;
        self.heap.charge_payload_growth(grown * size_of::<Value>());
        Some(Value::number(final_length as f64))
    }

    /// `array[length] = value` on a dense array (`dense_array_length`) as one append, when
    /// no prototype claims the index (`inherited_index_unclaimed`).
    pub(super) fn append_dense_element(&mut self, this: Value, index: usize, value: Value) -> bool {
        if self.dense_array_length(this) != Some(index)
            || index >= MAX_ARRAY_LENGTH
            || !self.inherited_index_unclaimed(this, index)
        {
            return false;
        }
        let Some(object) = self.heap.array_mut(this) else {
            unreachable!("matched an array above");
        };
        super::index::mutable_array_elements(object.elements_mut()).push(value);
        true
    }

    /// `define_array_literal_element` into a literal's dense storage: the hole `MakeArray`
    /// left at `index` (`Vm::hole_array`), or the next element at the literal's length. A
    /// literal is unreachable from guest code until it is complete, so no prototype observes
    /// the index, and CreateDataProperty stores the value as its own element. `false` leaves
    /// any other definition to `define_array_literal_element`.
    pub(super) fn define_literal_element(&mut self, array: Value, index: usize, value: Value) -> bool {
        let Some(length) = self.dense_array_length(array) else {
            return false;
        };
        if index > length || index >= MAX_ARRAY_LENGTH {
            return false;
        }
        let Some(object) = self.heap.array_store_mut(array, value) else {
            return false;
        };
        let elements = super::index::mutable_array_elements(object.elements_mut());
        if index == length {
            elements.push(value);
        } else {
            elements[index] = value;
        }
        true
    }

    /// ArrayAccumulation's CreateDataPropertyOrThrow of `values` at a dense literal's next
    /// indices (`define_literal_element` of each at the literal's length) as one append.
    /// `false` leaves the literal untouched.
    pub(super) fn append_literal_elements(&mut self, array: Value, values: &[Value]) -> bool {
        let Some(length) = self.dense_array_length(array) else {
            return false;
        };
        if length.checked_add(values.len()).is_none_or(|end| end > MAX_ARRAY_LENGTH) {
            return false;
        }
        let Some(object) = self.heap.array_mut(array) else {
            unreachable!("matched an array above");
        };
        let elements = super::index::mutable_array_elements(object.elements_mut());
        let capacity = elements.capacity();
        elements.extend_from_slice(values);
        let grown = elements.capacity() - capacity;
        self.heap.charge_payload_growth(grown * size_of::<Value>());
        true
    }

    /// `this[index] = value` into a hole below a dense array's length (`dense_array_length`):
    /// OrdinarySet finds no own element there and, while no prototype claims the index
    /// (`inherited_index_unclaimed`), defines a writable, enumerable, configurable data
    /// element, which the hole's slot then holds.
    pub(super) fn fill_dense_hole(&mut self, this: Value, index: usize, value: Value) -> bool {
        if self.dense_array_length(this).is_none_or(|length| index >= length)
            || !self
                .heap
                .array(this)
                .is_some_and(|object| object.elements()[index].is_deleted())
            || !self.inherited_index_unclaimed(this, index)
        {
            return false;
        }
        let Some(object) = self.heap.array_mut(this) else {
            unreachable!("matched an array above");
        };
        super::index::mutable_array_elements(object.elements_mut())[index] = value;
        true
    }

    /// `pop` of a dense array (`dense_array_length`) as one removal, when its last element
    /// is present: the generic steps read that own data element, delete it and shorten
    /// `length` by one.
    pub(super) fn array_pop_dense(&mut self, this: Value) -> Option<Value> {
        let length = self.dense_array_length(this)?;
        let Some(last) = length.checked_sub(1) else {
            return Some(Value::UNDEFINED);
        };
        let object = self.heap.array_mut(this)?;
        if object.elements()[last].is_deleted() {
            return None;
        }
        super::index::mutable_array_elements(object.elements_mut()).pop()
    }

    fn array_push_with_array_like_semantics(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let final_length = length
            .checked_add(args.len())
            .filter(|length| *length as f64 <= MAX_SAFE_INTEGER)
            .ok_or_else(|| self.type_error(p, "array-like length exceeds safe integer".into()))?;
        for (offset, value) in args.iter().copied().enumerate() {
            self.set_index_mode(
                p,
                object,
                Value::number((length + offset) as f64),
                value,
                true,
            )?;
        }
        self.set_array_like_length(p, object, final_length)?;
        Ok(Value::number(final_length as f64))
    }

    pub(super) fn array_pop_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        if let Some(value) = self.array_pop_dense(this) {
            return Ok(value);
        }
        self.array_pop_with_array_like_semantics(p, this)
    }

    fn array_pop_with_array_like_semantics(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        if length == 0 {
            self.set_array_like_length(p, object, 0)?;
            return Ok(Value::UNDEFINED);
        }
        let last = length - 1;
        let value = self.get_index(p, object, Value::number(last as f64))?;
        let deleted = self.object_delete_property(p, &[object, Value::number(last as f64)])?;
        if !self.truthy(deleted) {
            return Err(self.type_error(p, "cannot delete array-like element".into()));
        }
        self.set_array_like_length(p, object, last)?;
        Ok(value)
    }

    pub(super) fn set_array_like_length(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        length: usize,
    ) -> Result<(), JsError> {
        // A dense array's writable `length` already holding `length` stays as it is.
        if self.dense_array_length(object) == Some(length) {
            return Ok(());
        }
        let atom = self.length_atom;
        if self.heap.array(object).is_some()
            && !self
                .object_data(object)
                .is_some_and(Object::is_arguments_object)
        {
            if self
                .property_attributes(object, PropertyKey::string(atom))
                .is_some_and(|attributes| !attributes.writable)
            {
                return Err(self.type_error(p, "array length is not writable".into()));
            }
            if !self.set_array_length(p, object, Value::number(length as f64))? {
                return Err(self.type_error(p, "cannot set array length".into()));
            }
            return Ok(());
        }
        let success =
            self.set_property_with_receiver(p, object, atom, Value::number(length as f64), object)?;
        if success {
            Ok(())
        } else {
            Err(self.type_error(p, "cannot set array-like length".into()))
        }
    }

    pub(super) fn array_slice_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let start = args
            .first()
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.array_relative_index(p, value, length))
            .transpose()?
            .unwrap_or(0);
        let end = args
            .get(1)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.array_relative_index(p, value, length))
            .transpose()?
            .unwrap_or(length);
        let count = end.saturating_sub(start);
        let result = self.array_species_create(p, object, count)?;
        if let Some(values) = self.present_dense_elements(object, start..end).map(<[Value]>::to_vec)
            && self.fill_dense_holes(result, &values)
        {
            // Each CreateDataPropertyOrThrow filled a hole; the final `length` Set still runs.
            self.set_array_like_length(p, result, count)?;
            return Ok(result);
        }
        // `result` is held only here while guest code (getters, traps) can collect.
        self.with_call_roots([result], |vm| {
            for (destination, source) in (start..end).enumerate() {
                let key = Value::number(source as f64);
                if vm.has_property(p, object, key)? {
                    let value = vm.get_index(p, object, key)?;
                    vm.create_data_property_or_throw(p, result, destination, value)?;
                }
            }
            vm.set_array_like_length(p, result, count)?;
            Ok(result)
        })
    }

    pub(super) fn array_species_create(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        length: usize,
    ) -> Result<Value, JsError> {
        let mut constructor = if self.is_array(p, source)? {
            // An array's `constructor` is a data property of its chain where none runs guest code
            // (`plain_property_lookup`); a Proxy or an accessor takes the general read.
            let key = PropertyKey::string(self.runtime_atoms.constructor);
            match self.plain_property_lookup(source, key) {
                Some(found) => found.unwrap_or(Value::UNDEFINED),
                None => self.get_property(p, source, self.runtime_atoms.constructor)?,
            }
        } else {
            Value::UNDEFINED
        };
        let constructor_is_foreign_intrinsic_array = matches!(
            self.heap.function(constructor),
            Some(FunctionRecord {
                kind: FunctionKind::Native(Native::Array),
                realm,
                ..
            }) if *realm != self.realm.globals
        );
        if constructor_is_foreign_intrinsic_array {
            constructor = Value::UNDEFINED;
        }
        if self.is_object_like(constructor) {
            constructor = self.species_of(p, constructor)?;
            if constructor.is_null() {
                constructor = Value::UNDEFINED;
            }
        }
        // `new Array(length)` of this realm is ArrayCreate: Array.prototype is fixed.
        let constructor_is_intrinsic_array = matches!(
            self.heap.function(constructor),
            Some(FunctionRecord {
                kind: FunctionKind::Native(Native::Array),
                realm,
                ..
            }) if *realm == self.realm.globals
        );
        if constructor.is_undefined() || constructor_is_intrinsic_array {
            return self.array_create(p, length);
        }
        if !self.is_constructable(p, constructor) {
            return Err(self.type_error(p, "array species is not a constructor".into()));
        }
        self.construct_value(p, constructor, &[Value::number(length as f64)])
    }

    pub(super) fn is_array(&mut self, p: &ResidualProgram, value: Value) -> Result<bool, JsError> {
        if let Some(answer) = self.steady_is_array(value) {
            return Ok(answer);
        }
        match self.heap.get(value) {
            Some(Cell::Proxy { handler, .. }) if handler.is_null() => {
                Err(self.type_error(p, "revoked proxy".into()))
            }
            Some(Cell::Proxy { target, .. }) => self.is_array(p, *target),
            _ => Ok(false),
        }
    }

    /// IsArray of any value but a Proxy, which reads no state beyond the value's kind; `None`
    /// for a Proxy, whose target (or revocation) `is_array` follows.
    pub(super) fn steady_is_array(&self, value: Value) -> Option<bool> {
        if let Some(object) = self.heap.array(value) {
            return Some(!object.is_arguments_object());
        }
        (!matches!(self.heap.get(value), Some(Cell::Proxy { .. }))).then_some(false)
    }

    pub(super) fn array_relative_index(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        length: usize,
    ) -> Result<usize, JsError> {
        let number = self.to_number(p, value)?;
        if number.is_nan() || number == 0.0 {
            return Ok(0);
        }
        if number == f64::NEG_INFINITY {
            return Ok(0);
        }
        if number == f64::INFINITY {
            return Ok(length);
        }
        let integer = number.trunc();
        Ok(if integer < 0.0 {
            length.saturating_sub((-integer) as usize)
        } else {
            (integer as usize).min(length)
        })
    }

    pub(super) fn array_includes_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        if length == 0 {
            return Ok(Value::FALSE);
        }
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        let from = args
            .get(1)
            .copied()
            .map(|value| self.to_number(p, value))
            .transpose()?
            .unwrap_or(0.0);
        let start = if from.is_nan() || from == 0.0 || from == f64::NEG_INFINITY {
            0
        } else if from == f64::INFINITY {
            length
        } else if from.is_sign_negative() {
            length.saturating_sub(from.abs().trunc() as usize)
        } else {
            (from.trunc() as usize).min(length)
        };
        for index in start..length {
            let value = self.get_index(p, object, Value::number(index as f64))?;
            if self.same_value_zero(value, search) {
                return Ok(Value::TRUE);
            }
        }
        Ok(Value::FALSE)
    }

    pub(super) fn array_join_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(std::iter::once(this).chain(args.iter().copied()), |vm| {
            let object = vm.box_object_or_type_error(p, this)?;
            vm.with_call_roots([object], |vm| {
                let length = vm.array_like_length(p, object)?;
                let separator = match args.first().copied() {
                    None | Some(Value::UNDEFINED) => JsString::from_str(","),
                    Some(value) => vm.coerce_js_string(p, value)?,
                };
                let mut output = super::wtf16::JsStringBuilder::default();
                for index in 0..length {
                    if index != 0 {
                        output.append(&separator);
                    }
                    let value = vm.get_index(p, object, Value::number(index as f64))?;
                    if value.is_null() || value.is_undefined() {
                        continue;
                    }
                    output.append(&vm.coerce_js_string(p, value)?);
                }
                let output = vm.string_build_result(p, output.finish())?;
                Ok(vm.heap.alloc_string(output))
            })
        })
    }
}
