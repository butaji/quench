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
            Native::ArrayWith => self.array_with_native(p, this, args),
            Native::ArrayForEach
            | Native::ArrayMap
            | Native::ArrayFilter
            | Native::ArraySome
            | Native::ArrayEvery
            | Native::ArrayFind
            | Native::ArrayFindIndex
            | Native::ArrayFindLast
            | Native::ArrayFindLastIndex => self.array_callback_native(p, native, this, args),
            Native::ArrayGroup | Native::ArrayGroupToMap => {
                self.array_group_native(p, native, this, args)
            }
            Native::ArrayFlatMap => self.array_flat_map_native(p, this, args),
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
        let index = self.array_relative_index(
            p,
            args.first().copied().unwrap_or(Value::UNDEFINED),
            length,
        )?;
        if index >= length {
            return Ok(Value::UNDEFINED);
        }
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
                if self.array_strict_equal(value, search) {
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
                if self.array_strict_equal(value, search) {
                    return Ok(Value::number(index as f64));
                }
            }
        }
        Ok(Value::number(-1.0))
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

    pub(super) fn array_with_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        if length > u32::MAX as usize {
            return Err(self.range_error(p, "invalid array length".into()));
        }
        let number = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let index = if number.is_nan() || number == 0.0 {
            0.0
        } else if number.is_infinite() {
            number
        } else {
            number.trunc()
        };
        let actual_index = if index < 0.0 {
            length as f64 + index
        } else {
            index
        };
        if actual_index < 0.0 || actual_index >= length as f64 {
            return Err(self.range_error(p, "array index out of range".into()));
        }
        let index = actual_index as usize;
        let mut values = Vec::with_capacity(length);
        for offset in 0..length {
            values.push(if offset == index {
                args.get(1).copied().unwrap_or(Value::UNDEFINED)
            } else {
                self.get_index(p, object, Value::number(offset as f64))?
            });
        }
        Ok(self.new_array(values))
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
        if native == Native::ArrayMap && length > u32::MAX as usize {
            return Err(self.range_error(p, "invalid array length".into()));
        }
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !matches!(self.heap.get(callback), Some(Cell::Function { .. })) {
            return Err(JsError("array callback is not callable".into()));
        }
        let result = match native {
            Native::ArrayMap => Some(self.array_species_create(p, this, length)?),
            Native::ArrayFilter => Some(self.array_species_create(p, this, 0)?),
            _ => None,
        };
        let result_root = result.map(|result| self.heap.root(result));
        let outcome = (|| {
            let this_arg = args.get(1).copied().unwrap_or(Value::UNDEFINED);
            let mut result_length = 0;
            let reverse = matches!(native, Native::ArrayFindLast | Native::ArrayFindLastIndex);
            for offset in 0..length {
                let index = if reverse { length - offset - 1 } else { offset };
                let key = Value::number(index as f64);
                let finds_holes = matches!(
                    native,
                    Native::ArrayFind
                        | Native::ArrayFindIndex
                        | Native::ArrayFindLast
                        | Native::ArrayFindLastIndex
                );
                if !finds_holes && !self.has_property(p, this, key)? {
                    continue;
                }
                let value = self.get_index(p, this, key)?;
                let callback_args = [value, Value::number(index as f64), this];
                let mapped = self.call_value(p, callback, this_arg, &callback_args)?;
                match native {
                    Native::ArrayForEach => {}
                    Native::ArrayMap => {
                        let target = self
                            .heap
                            .root_value(result_root.expect("map result is rooted"))
                            .unwrap();
                        self.create_data_property_or_throw(p, target, index, mapped)?;
                    }
                    Native::ArrayFilter if self.truthy(mapped) => {
                        let target = self
                            .heap
                            .root_value(result_root.expect("filter result is rooted"))
                            .unwrap();
                        self.create_data_property_or_throw(p, target, result_length, value)?;
                        result_length += 1;
                    }
                    Native::ArraySome if self.truthy(mapped) => return Ok(Value::TRUE),
                    Native::ArrayEvery if !self.truthy(mapped) => return Ok(Value::FALSE),
                    Native::ArrayFind if self.truthy(mapped) => return Ok(value),
                    Native::ArrayFindIndex if self.truthy(mapped) => {
                        return Ok(Value::number(index as f64));
                    }
                    Native::ArrayFindLast if self.truthy(mapped) => return Ok(value),
                    Native::ArrayFindLastIndex if self.truthy(mapped) => {
                        return Ok(Value::number(index as f64));
                    }
                    _ => {}
                }
            }
            match native {
                Native::ArrayForEach => Ok(Value::UNDEFINED),
                Native::ArrayMap | Native::ArrayFilter => Ok(self
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
        })();
        if let Some(root) = result_root {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn array_like_length(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<usize, JsError> {
        if let Some(Cell::Array { elements, .. }) = self.heap.get(object)
            && !self
                .object_data(object)
                .is_some_and(Object::is_arguments_object)
        {
            return Ok(self
                .heap
                .sparse_length(object)
                .unwrap_or(0)
                .max(elements.len()));
        }
        let length_atom = self.intern_atom("length");
        let value = self.get_property(p, object, length_atom)?;
        let number = self.to_number(p, value)?;
        if number.is_nan() || number <= 0.0 {
            return Ok(0);
        }
        Ok(number.floor().min(MAX_SAFE_INTEGER).min(usize::MAX as f64) as usize)
    }

    pub(super) fn array_flat_map_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, source)?;
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(JsError("flatMap callback is not callable".into()));
        }
        let target = self.array_species_create(p, source, 0)?;
        let mut output = Vec::new();
        for index in 0..length {
            let key = Value::number(index as f64);
            if !self.has_property(p, source, key)? {
                continue;
            }
            let value = self.get_index(p, source, key)?;
            let callback_args = [value, key, source];
            let result = self.call_value(p, callback, Value::UNDEFINED, &callback_args)?;
            if self.is_array(p, result)? {
                self.flatten_into(p, result, 0, &mut output)?;
            } else {
                output.push(result);
            }
        }
        for (index, value) in output.into_iter().enumerate() {
            self.create_data_property_or_throw(p, target, index, value)?;
        }
        Ok(target)
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
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !matches!(self.heap.get(callback), Some(Cell::Function { .. })) {
            return Err(self.type_error(p, "reduce callback is not callable".into()));
        }
        let reverse = matches!(native, Native::ArrayReduceRight);
        let mut index = if reverse { length } else { 0 };
        let mut accumulator = args.get(1).copied();
        while accumulator.is_none() && if reverse { index > 0 } else { index < length } {
            if reverse {
                index -= 1;
            }
            let key = Value::number(index as f64);
            if self.has_property(p, object, key)? {
                accumulator = Some(self.get_index(p, object, key)?);
            }
            if !reverse {
                index += 1;
            }
        }
        let Some(mut accumulator) = accumulator else {
            return Err(self.type_error(p, "reduce of empty array with no initial value".into()));
        };
        while if reverse { index > 0 } else { index < length } {
            if reverse {
                index -= 1;
            }
            let key = Value::number(index as f64);
            if self.has_property(p, object, key)? {
                let value = self.get_index(p, object, key)?;
                let callback_args = [accumulator, value, key, object];
                accumulator = self.call_value(p, callback, Value::UNDEFINED, &callback_args)?;
            }
            if !reverse {
                index += 1;
            }
        }
        Ok(accumulator)
    }

    fn array_strict_equal(&self, left: Value, right: Value) -> bool {
        self.strict_equal(left, right)
    }
}
