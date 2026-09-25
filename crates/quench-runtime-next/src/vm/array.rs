use super::*;

const MAX_ARRAY_LENGTH: usize = u32::MAX as usize;

pub(super) fn normalized_array_values(elements: &[Value]) -> Vec<Value> {
    elements
        .iter()
        .copied()
        .map(|value| {
            if value.is_deleted() {
                Value::UNDEFINED
            } else {
                value
            }
        })
        .collect()
}

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

    pub(super) fn array_reverse_native(&mut self, this: Value) -> Result<Value, JsError> {
        let values = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => {
                let length = self.heap.sparse_length(this).unwrap_or(elements.len());
                (0..length)
                    .map(|index| self.array_value_at(this, index))
                    .collect::<Vec<_>>()
            }
            _ => return Err(JsError("reverse receiver is not array".into())),
        };
        self.check_array_mutation(this, true, false, false)?;
        for (index, value) in values.into_iter().rev().enumerate() {
            self.set_array_element(this, index, value);
        }
        Ok(this)
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

    fn delete_array_like_property(
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
        let length = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => {
                self.heap.sparse_length(this).unwrap_or(elements.len())
            }
            _ => return Err(JsError("fill receiver is not array".into())),
        };
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let relative = |number: f64| {
            if number.is_nan() {
                0
            } else if number.is_infinite() {
                if number.is_sign_negative() { 0 } else { length }
            } else if number.is_sign_negative() {
                length.saturating_sub(number.abs().trunc() as usize)
            } else {
                (number.trunc() as usize).min(length)
            }
        };
        let start = args
            .get(1)
            .map(|value| self.to_number(p, *value))
            .transpose()?
            .map(relative)
            .unwrap_or(0);
        let end = args
            .get(2)
            .map(|value| self.to_number(p, *value))
            .transpose()?
            .map(relative)
            .unwrap_or(length);
        if start < end {
            self.check_array_mutation(this, true, false, false)?;
        }
        for index in start.min(end)..end {
            self.set_array_element(this, index, value);
        }
        Ok(this)
    }

    pub(super) fn array_flat_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !matches!(self.heap.get(this), Some(Cell::Array { .. })) {
            return Err(JsError("flat receiver is not array".into()));
        }
        let depth = match self.to_number(p, args.first().copied().unwrap_or(Value::number(1.0)))? {
            value if value.is_nan() || value <= 0.0 => 0,
            value if value.is_infinite() => usize::MAX,
            value => value.trunc() as usize,
        };
        let mut values = Vec::new();
        self.flatten_array(this, depth, &mut values);
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    fn flatten_array(&self, value: Value, depth: usize, output: &mut Vec<Value>) {
        let Some(Cell::Array { elements, .. }) = self.heap.get(value) else {
            output.push(value);
            return;
        };
        let length = self.heap.sparse_length(value).unwrap_or(elements.len());
        let items = (0..length)
            .map(|index| self.array_value_at(value, index))
            .collect::<Vec<_>>();
        for item in items {
            if depth > 0 && matches!(self.heap.get(item), Some(Cell::Array { .. })) {
                self.flatten_array(item, depth - 1, output);
            } else {
                output.push(item);
            }
        }
    }

    pub(super) fn array_concat_native(
        &mut self,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::Array { .. }) = self.heap.get(this) else {
            return Err(JsError("concat receiver is not array".into()));
        };
        let mut values = Vec::new();
        let append = |value: Value, values: &mut Vec<Value>| {
            if let Some(Cell::Array { elements, .. }) = self.heap.get(value) {
                let length = self.heap.sparse_length(value).unwrap_or(elements.len());
                values.extend((0..length).map(|index| self.array_value_at(value, index)));
            } else {
                values.push(value);
            }
        };
        append(this, &mut values);
        for value in args.iter().copied() {
            append(value, &mut values);
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
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

    fn set_array_like_length(
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

    fn array_species_create(
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

    fn is_array(&mut self, p: &ResidualProgram, value: Value) -> Result<bool, JsError> {
        match self.heap.get(value) {
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
        let elements = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => Rc::clone(elements),
            _ => return Err(JsError("includes receiver is not array".into())),
        };
        let length = self.heap.sparse_length(this).unwrap_or(elements.len());
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        let from = args
            .get(1)
            .copied()
            .map(|value| self.to_number(p, value))
            .transpose()?
            .unwrap_or(0.0);
        if from.is_infinite() && from.is_sign_positive() {
            return Ok(Value::FALSE);
        }
        let start = if from.is_sign_negative() {
            length.saturating_sub((-from.trunc()) as usize)
        } else {
            from.max(0.0).trunc() as usize
        };
        for index in start..length {
            let value = self.array_value_at(this, index);
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
        let elements = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => Rc::clone(elements),
            _ => return Err(JsError("join receiver is not array".into())),
        };
        let length = self.heap.sparse_length(this).unwrap_or(elements.len());
        let separator = match args.first().copied() {
            None | Some(Value::UNDEFINED) => ",".to_owned(),
            Some(value) => self.to_string(p, value)?,
        };
        let mut output = String::new();
        for index in 0..length {
            if index != 0 {
                output.push_str(&separator);
            }
            let value = self.array_value_at(this, index);
            if value.is_undefined() {
                continue;
            }
            if value.is_null() || value.is_undefined() {
                continue;
            }
            output.push_str(&self.to_string(p, value)?);
        }
        Ok(self.heap.alloc(Cell::String(output.into())))
    }
}
