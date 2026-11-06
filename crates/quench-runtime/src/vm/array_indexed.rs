use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn array_indexed_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::ArrayAt => self.array_at_native(p, this, args),
            Native::ArrayLastIndexOf => self.array_last_index_of_native(p, this, args),
            Native::ArrayIndexOf => self.array_index_of_native(p, this, args),
            Native::ArrayCopyWithin => self.array_copy_within_native(p, this, args),
            Native::ArrayWith => self.array_copy_native(p, native, this, args),
            native if native.is_array_callback() => self.array_callback_native(p, native, this, args),
            Native::ArrayGroup | Native::ArrayGroupToMap => {
                self.array_group_native(p, native, this, args)
            }
            Native::ArrayFlatMap => self.array_flatten_native(p, native, this, args),
            Native::ArrayReduce | Native::ArrayReduceRight => {
                self.array_reduce_native(p, native, this, args)
            }
            _ => unreachable!("non-indexed native routed to array index dispatch"),
        }
    }

    pub(super) fn array_at_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let Some(index) =
            self.indexed_at_index(p, args.first().copied().unwrap_or(Value::UNDEFINED), length)?
        else {
            return Ok(Value::UNDEFINED);
        };
        self.get_index(p, object, Value::number(index as f64))
    }

    pub(super) fn array_last_index_of_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        self.array_last_index_of_with_length(p, object, args, length)
    }

    pub(super) fn typed_array_last_index_of_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(this) else {
            return Err(self.type_error(p, "typed array lastIndexOf receiver is invalid".into()));
        };
        if self.typed_array_out_of_bounds(this) || self.array_buffer_detached(*buffer) {
            return Err(self.type_error(p, "typed array lastIndexOf receiver is invalid".into()));
        }
        let length = self.typed_array_length(this).unwrap_or(0);
        self.array_last_index_of_with_length(p, this, args, length)
    }

    fn array_last_index_of_with_length(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        args: &[Value],
        length: usize,
    ) -> Result<Value, JsError> {
        if length == 0 {
            return Ok(Value::number(-1.0));
        }
        let from = match args.get(1) {
            None => length as isize - 1,
            Some(value) => {
                let number = self.to_number(p, *value)?;
                if number.is_nan() || number == 0.0 {
                    0
                } else if number == f64::NEG_INFINITY {
                    return Ok(Value::number(-1.0));
                } else if number == f64::INFINITY {
                    length as isize - 1
                } else if number < 0.0 {
                    (length as f64 + number.trunc()).floor() as isize
                } else {
                    (number.trunc() as usize).min(length - 1) as isize
                }
            }
        };
        if from < 0 {
            return Ok(Value::number(-1.0));
        }
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        for index in (0..=from as usize).rev() {
            let key = Value::number(index as f64);
            if self.has_property(p, object, key)? {
                let value = self.get_index(p, object, key)?;
                if self.strict_equal(value, search) {
                    return Ok(Value::number(index as f64));
                }
            }
        }
        Ok(Value::number(-1.0))
    }

    pub(super) fn array_index_of_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if let Some(found) = self.dense_index_of(this, args) {
            return Ok(found);
        }
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        if length == 0 {
            return Ok(Value::number(-1.0));
        }
        let start = match args.get(1) {
            None => 0,
            Some(value) => {
                let number = self.to_number(p, *value)?;
                if number.is_nan() || number == 0.0 || number == f64::NEG_INFINITY {
                    0
                } else if number.is_infinite() {
                    return Ok(Value::number(-1.0));
                } else if number < 0.0 {
                    length.saturating_sub(number.abs().trunc() as usize)
                } else {
                    (number.trunc() as usize).min(length)
                }
            }
        };
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        for index in start..length {
            let key = Value::number(index as f64);
            if self.has_property(p, object, key)? {
                let value = self.get_index(p, object, key)?;
                if self.strict_equal(value, search) {
                    return Ok(Value::number(index as f64));
                }
            }
        }
        Ok(Value::number(-1.0))
    }

    /// `Array.prototype.indexOf` of a dense array (`dense_array_length`) from an integer or
    /// absent `fromIndex`: each element is its own data property, compared by strict
    /// equality, and a hole no prototype holds (`inherited_index_absent`) is skipped, so no
    /// guest code runs. `None` for any other receiver or start, or a hole a prototype holds.
    pub(super) fn dense_index_of(&self, array: Value, args: &[Value]) -> Option<Value> {
        let length = self.dense_array_length(array)?;
        let start = match args.get(1) {
            None => 0,
            Some(from) => {
                let from = from.as_int()?;
                if from < 0 {
                    length.saturating_sub(from.unsigned_abs() as usize)
                } else {
                    (from as usize).min(length)
                }
            }
        };
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        let elements = self.heap.array(array)?.elements();
        for (index, value) in elements.iter().enumerate().take(length).skip(start) {
            if value.is_deleted() {
                if !self.inherited_index_absent(array, index) {
                    return None;
                }
            } else if self.strict_equal(*value, search) {
                return Some(Value::number(index as f64));
            }
        }
        Some(Value::number(-1.0))
    }

    pub(super) fn array_copy_within_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let target = args
            .first()
            .map(|value| self.array_relative_index(p, *value, length))
            .transpose()?
            .unwrap_or(0);
        let source = args
            .get(1)
            .map(|value| self.array_relative_index(p, *value, length))
            .transpose()?
            .unwrap_or(0);
        let end = args
            .get(2)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.array_relative_index(p, value, length))
            .transpose()?
            .unwrap_or(length);
        let count = end
            .saturating_sub(source)
            .min(length.saturating_sub(target));
        for offset in 0..count {
            let from = if target <= source {
                source + offset
            } else {
                source + count - offset - 1
            };
            let to = if target <= source {
                target + offset
            } else {
                target + count - offset - 1
            };
            let from_key = Value::number(from as f64);
            let to_key = Value::number(to as f64);
            if self.has_property(p, object, from_key)? {
                let value = self.get_index(p, object, from_key)?;
                self.set_index_mode(p, object, to_key, value, true)?;
            } else {
                self.delete_array_like_property(p, object, to_key)?;
            }
        }
        Ok(object)
    }

    pub(super) fn array_callback_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if this.is_null() || this.is_undefined() {
            return Err(self.type_error(p, "array callback receiver is nullish".into()));
        }
        let this = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, this)?;
        self.array_callback_with_length(p, native, this, args, length, false)
    }

    pub(super) fn typed_array_callback_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(this) else {
            return Err(self.type_error(p, "typed array callback receiver is invalid".into()));
        };
        if self.typed_array_out_of_bounds(this) || self.array_buffer_detached(*buffer) {
            return Err(self.type_error(p, "typed array callback receiver is invalid".into()));
        }
        let length = self.typed_array_length(this).unwrap_or(0);
        if matches!(native, Native::ArrayReduce | Native::ArrayReduceRight) {
            return self.array_reduce_with_length(p, native, this, args, length, true);
        }
        self.array_callback_with_length(p, native, this, args, length, true)
    }

    fn array_callback_with_length(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
        length: usize,
        typed_array: bool,
    ) -> Result<Value, JsError> {
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(self.type_error(p, "array callback is not callable".into()));
        }
        // A result ArrayCreate makes is unreachable from guest code until it is returned, so
        // its elements are stored as its own dense storage (`store_fresh_array_element`): a
        // map's result over a dense receiver, which bounds its length, or a filter's.
        let array_create = !typed_array
            && matches!(native, Native::ArrayMap | Native::ArrayFilter)
            && self.array_species_is_array_create(this);
        let dense_receiver = self.heap.array_elements(this).is_some_and(|elements| elements.len() == length);
        let fresh = array_create && (native == Native::ArrayFilter || dense_receiver);
        let result = match native {
            Native::ArrayMap if typed_array => {
                Some(self.typed_array_species_create_for_writing(p, this, length)?)
            }
            Native::ArrayMap if fresh => Some(self.hole_array(length)),
            Native::ArrayMap => Some(self.array_species_create(p, this, length)?),
            Native::ArrayFilter if typed_array => None,
            Native::ArrayFilter if fresh => Some(self.hole_array(0)),
            Native::ArrayFilter => Some(self.array_species_create(p, this, 0)?),
            _ => None,
        };
        let result_root = result.map(|result| self.heap.root(result));
        let mut filtered_value_roots = Vec::new();
        let this_arg = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let outcome = self.with_prepared_callback(callback, this_arg, |vm, prepared| {
            let mut result_length = 0;
            let reverse = matches!(native, Native::ArrayFindLast | Native::ArrayFindLastIndex);
            let finds_holes = matches!(
                native,
                Native::ArrayFind | Native::ArrayFindIndex | Native::ArrayFindLast | Native::ArrayFindLastIndex
            );
            for offset in 0..length {
                let index = if reverse { length - offset - 1 } else { offset };
                let key = Value::length(index);
                // A present dense element is an own data property: HasProperty holds and Get
                // reads it, observing nothing.
                let value = match vm.dense_array_element(this, key) {
                    Some(value) if !typed_array => value,
                    _ => {
                        if !typed_array && !finds_holes && !vm.has_property(p, this, key)? {
                            continue;
                        }
                        vm.get_index(p, this, key)?
                    }
                };
                let callback_args = [value, key, this];
                let mapped = vm.call_prepared(p, prepared, &callback_args)?;
                match native {
                    Native::ArrayForEach => {}
                    Native::ArrayMap => {
                        let target = vm
                            .heap
                            .root_value(result_root.expect("map result is rooted"))
                            .unwrap();
                        if typed_array {
                            vm.typed_array_set(p, target, index, mapped)?;
                        } else if fresh {
                            vm.store_fresh_array_element(target, index, mapped);
                        } else {
                            vm.create_data_property_or_throw(p, target, index, mapped)?;
                        }
                    }
                    Native::ArrayFilter if vm.truthy(mapped) => {
                        if typed_array {
                            filtered_value_roots.push(vm.heap.root(value));
                        } else {
                            let target = vm
                                .heap
                                .root_value(result_root.expect("filter result is rooted"))
                                .unwrap();
                            if fresh {
                                vm.store_fresh_array_element(target, result_length, value);
                            } else {
                                vm.create_data_property_or_throw(p, target, result_length, value)?;
                            }
                        }
                        result_length += 1;
                    }
                    Native::ArraySome if vm.truthy(mapped) => return Ok(Value::TRUE),
                    Native::ArrayEvery if !vm.truthy(mapped) => return Ok(Value::FALSE),
                    Native::ArrayFind if vm.truthy(mapped) => return Ok(value),
                    Native::ArrayFindIndex if vm.truthy(mapped) => {
                        return Ok(Value::number(index as f64));
                    }
                    Native::ArrayFindLast if vm.truthy(mapped) => return Ok(value),
                    Native::ArrayFindLastIndex if vm.truthy(mapped) => {
                        return Ok(Value::number(index as f64));
                    }
                    _ => {}
                }
            }
            match native {
                Native::ArrayForEach => Ok(Value::UNDEFINED),
                Native::ArrayFilter if typed_array => {
                    let target =
                        vm.typed_array_species_create_for_writing(p, this, result_length)?;
                    let target_root = vm.heap.root(target);
                    let result = (|| {
                        for (index, value_root) in filtered_value_roots.iter().enumerate() {
                            let value = vm.heap.root_value(*value_root).unwrap();
                            vm.typed_array_set(p, target, index, value)?;
                        }
                        Ok(vm.heap.root_value(target_root).unwrap())
                    })();
                    vm.heap.release_root(target_root);
                    result
                }
                Native::ArrayMap | Native::ArrayFilter => Ok(vm
                    .heap
                    .root_value(result_root.expect("callback result is rooted"))
                    .unwrap()),
                Native::ArraySome => Ok(Value::FALSE),
                Native::ArrayEvery => Ok(Value::TRUE),
                Native::ArrayFind => Ok(Value::UNDEFINED),
                Native::ArrayFindIndex => Ok(Value::number(-1.0)),
                Native::ArrayFindLast => Ok(Value::UNDEFINED),
                Native::ArrayFindLastIndex => Ok(Value::number(-1.0)),
                _ => unreachable!("non-callback native routed to callback dispatch"),
            }
        });
        if let Some(root) = result_root {
            self.heap.release_root(root);
        }
        for root in filtered_value_roots {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn array_like_length(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<usize, JsError> {
        if let Some(length) = self.own_array_length(object) {
            return Ok(length);
        }
        let length_atom = self.length_atom;
        let value = self.get_property(p, object, length_atom)?;
        let number = self.to_number(p, value)?;
        if number.is_nan() || number <= 0.0 {
            return Ok(0);
        }
        Ok(number.floor().min(MAX_SAFE_INTEGER).min(usize::MAX as f64) as usize)
    }

    pub(super) fn array_reduce_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        self.array_reduce_with_length(p, native, object, args, length, false)
    }

    fn array_reduce_with_length(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        object: Value,
        args: &[Value],
        length: usize,
        typed_array: bool,
    ) -> Result<Value, JsError> {
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(self.type_error(p, "reduce callback is not callable".into()));
        }
        let object_root = self.heap.root(object);
        let callback_root = self.heap.root(callback);
        let mut accumulator_root = args.get(1).map(|value| self.heap.root(*value));
        let reverse = matches!(native, Native::ArrayReduceRight);
        let mut indices = 0..length;
        let outcome = self.with_prepared_callback(callback, Value::UNDEFINED, |vm, prepared| {
            while let Some(index) = if reverse {
                indices.next_back()
            } else {
                indices.next()
            } {
                let key = Value::length(index);
                let object = vm.heap.root_value(object_root).unwrap();
                if !typed_array && !vm.has_property(p, object, key)? {
                    continue;
                }
                let object = vm.heap.root_value(object_root).unwrap();
                let value = vm.get_index(p, object, key)?;
                match accumulator_root {
                    None => accumulator_root = Some(vm.heap.root(value)),
                    Some(root) => {
                        let accumulator = vm.heap.root_value(root).unwrap();
                        let object = vm.heap.root_value(object_root).unwrap();
                        let result = vm.call_prepared(p, prepared, &[accumulator, value, key, object])?;
                        vm.heap.update_root(root, result);
                    }
                }
            }
            match accumulator_root {
                Some(root) => Ok(vm.heap.root_value(root).unwrap()),
                None => {
                    Err(vm.type_error(p, "reduce of empty array with no initial value".into()))
                }
            }
        });
        if let Some(root) = accumulator_root {
            self.heap.release_root(root);
        }
        self.heap.release_root(callback_root);
        self.heap.release_root(object_root);
        outcome
    }
}
