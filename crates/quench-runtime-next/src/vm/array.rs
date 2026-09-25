use super::*;

const DEFAULT_FLAT_DEPTH: usize = 1;

impl<H: Host> Vm<H> {
    pub(super) fn array_length(&self, array: Value) -> Option<usize> {
        match self.heap.get(array) {
            Some(Cell::Array { elements, .. }) => {
                Some(self.heap.sparse_length(array).unwrap_or(elements.len()))
            }
            _ => None,
        }
    }

    pub(super) fn array_value_at(&self, array: Value, index: usize) -> Value {
        let value = match self.heap.get(array) {
            Some(Cell::Array { elements, .. }) => elements
                .get(index)
                .copied()
                .filter(|value| !value.is_deleted())
                .or_else(|| {
                    self.heap
                        .sparse_get(array, index)
                        .filter(|value| !value.is_deleted())
                }),
            _ => None,
        };
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
        let removed = self.array_species_create(p, object, delete_count)?;
        for offset in 0..delete_count {
            let source_index = start + offset;
            let source = Value::number(source_index as f64);
            if self.has_property(p, object, source)? {
                let value = self.get_index(p, object, source)?;
                self.create_data_property_or_throw(p, removed, offset, value)?;
            }
        }
        self.set_array_like_length(p, removed, delete_count)?;

        let items = args.iter().copied().skip(2).collect::<Vec<_>>();
        let new_length = length
            .checked_sub(delete_count)
            .and_then(|length| length.checked_add(items.len()))
            .filter(|length| *length as f64 <= MAX_SAFE_INTEGER)
            .ok_or_else(|| self.type_error(p, "splice result exceeds safe integer".into()))?;
        if items.len() < delete_count {
            for index in start..(length - delete_count) {
                self.move_array_like_property(
                    p,
                    object,
                    index + items.len(),
                    index + delete_count,
                )?;
            }
            for index in (new_length..length).rev() {
                self.delete_array_like_property(p, object, Value::number(index as f64))?;
            }
        } else if items.len() > delete_count {
            for index in (start..(length - delete_count)).rev() {
                self.move_array_like_property(
                    p,
                    object,
                    index + items.len(),
                    index + delete_count,
                )?;
            }
        }
        for (offset, value) in items.into_iter().enumerate() {
            self.set_index_mode(
                p,
                object,
                Value::number((start + offset) as f64),
                value,
                true,
            )?;
        }
        self.set_array_like_length(p, object, new_length)?;
        Ok(removed)
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
        for index in start..end {
            self.set_index_mode(p, object, Value::number(index as f64), value, true)?;
        }
        Ok(object)
    }

    pub(super) fn array_flat_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = self.box_object_or_type_error(p, this)?;
        let source_length = self.array_like_length(p, source)?;
        let depth = match args.first().copied() {
            None | Some(Value::UNDEFINED) => DEFAULT_FLAT_DEPTH,
            Some(value) => match self.to_number(p, value)? {
                value if value.is_nan() || value <= 0.0 => 0,
                value if value.is_infinite() => usize::MAX,
                value => value.trunc() as usize,
            },
        };
        let target = self.array_species_create(p, source, 0)?;
        let mut values = Vec::new();
        self.flatten_into(p, source, source_length, depth, &mut values)?;
        for (index, value) in values.into_iter().enumerate() {
            self.create_data_property_or_throw(p, target, index, value)?;
        }
        Ok(target)
    }

    pub(super) fn flatten_into(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        source_length: usize,
        depth: usize,
        output: &mut Vec<Value>,
    ) -> Result<(), JsError> {
        for index in 0..source_length {
            let key = Value::number(index as f64);
            if !self.has_property(p, source, key)? {
                continue;
            }
            let value = self.get_index(p, source, key)?;
            if depth > 0 && self.is_array(p, value)? {
                let nested_length = self.array_like_length(p, value)?;
                self.flatten_into(p, value, nested_length, depth - 1, output)?;
            } else {
                output.push(value);
            }
        }
        Ok(())
    }

    pub(super) fn array_concat_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = self.box_object_or_type_error(p, this)?;
        let target = self.array_species_create(p, source, 0)?;
        let spreadable_atom = self
            .well_known_symbols
            .get("isConcatSpreadable")
            .copied()
            .ok_or_else(|| JsError("Symbol.isConcatSpreadable is not initialized".into()))?;
        let mut next = 0usize;
        for item in std::iter::once(source).chain(args.iter().copied()) {
            let spreadable = if self.is_object_like(item) {
                let value = self.get_index(p, item, spreadable_atom)?;
                if value.is_undefined() {
                    self.is_array(p, item)?
                } else {
                    self.truthy(value)
                }
            } else {
                false
            };
            if !spreadable {
                if next as f64 >= MAX_SAFE_INTEGER {
                    return Err(
                        self.type_error(p, "array concat index exceeds safe integer".into())
                    );
                }
                self.create_data_property_or_throw(p, target, next, item)?;
                next += 1;
                continue;
            }
            let length = self.array_like_length(p, item)?;
            if next.saturating_add(length) as f64 > MAX_SAFE_INTEGER {
                return Err(self.type_error(p, "array concat length exceeds safe integer".into()));
            }
            for index in 0..length {
                let source_key = Value::number(index as f64);
                if self.has_property(p, item, source_key)? {
                    let value = self.get_index(p, item, source_key)?;
                    self.create_data_property_or_throw(p, target, next, value)?;
                }
                next += 1;
            }
        }
        self.set_array_like_length(p, target, next)?;
        Ok(target)
    }

    pub(super) fn array_push_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.array_push_with_array_like_semantics(p, this, args)
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
        let atom = self.intern_atom("length");
        if matches!(self.heap.get(object), Some(Cell::Array { .. }))
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
        if count > MAX_ARRAY_LENGTH {
            return Err(self.range_error(p, "invalid array length".into()));
        }
        let result = self.array_species_create(p, object, count)?;
        for (destination, source) in (start..end).enumerate() {
            let key = Value::number(source as f64);
            if self.has_property(p, object, key)? {
                let value = self.get_index(p, object, key)?;
                self.create_data_property_or_throw(p, result, destination, value)?;
            }
        }
        Ok(result)
    }

    pub(super) fn array_species_create(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        length: usize,
    ) -> Result<Value, JsError> {
        let mut constructor = if self.is_array(p, source)? {
            let atom = self.intern_atom("constructor");
            self.get_property(p, source, atom)?
        } else {
            Value::UNDEFINED
        };
        let constructor_is_foreign_intrinsic_array = matches!(
            self.heap.get(constructor),
            Some(Cell::Function {
                kind: FunctionKind::Native(Native::Array),
                realm,
                ..
            }) if *realm != self.realm.globals
        );
        if constructor_is_foreign_intrinsic_array {
            constructor = Value::UNDEFINED;
        }
        if self.is_object_like(constructor) {
            let species = self
                .well_known_symbols
                .get("species")
                .copied()
                .ok_or_else(|| JsError("Symbol.species is not initialized".into()))?;
            constructor = self.get_index(p, constructor, species)?;
            if constructor.is_null() {
                constructor = Value::UNDEFINED;
            }
        }
        if constructor.is_undefined() {
            constructor = self.native_value(Native::Array);
        }
        if !self.is_constructable(p, constructor) {
            return Err(self.type_error(p, "array species is not a constructor".into()));
        }
        self.construct_value(p, constructor, &[Value::number(length as f64)])
    }

    pub(super) fn is_array(&mut self, p: &ResidualProgram, value: Value) -> Result<bool, JsError> {
        match self.heap.get(value) {
            Some(Cell::Array { .. })
                if self
                    .object_data(value)
                    .is_some_and(Object::is_arguments_object) =>
            {
                Ok(false)
            }
            Some(Cell::Array { .. }) => Ok(true),
            Some(Cell::Proxy { handler, .. }) if handler.is_null() => {
                Err(self.type_error(p, "revoked proxy".into()))
            }
            Some(Cell::Proxy { target, .. }) => self.is_array(p, *target),
            _ => Ok(false),
        }
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
        Ok(if integer.is_sign_negative() {
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
        let object = self.box_object_or_type_error(p, this)?;
        let length = self.array_like_length(p, object)?;
        let separator = match args.first().copied() {
            None | Some(Value::UNDEFINED) => ",".to_owned(),
            Some(value) => self.to_string(p, value)?,
        };
        let mut output = String::new();
        for index in 0..length {
            if index != 0 {
                output.push_str(&separator);
            }
            let value = self.get_index(p, object, Value::number(index as f64))?;
            if value.is_null() || value.is_undefined() {
                continue;
            }
            output.push_str(&self.to_string(p, value)?);
        }
        Ok(self.heap.alloc(Cell::String(output.into())))
    }
}
