use super::*;

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
    pub(super) fn array_reverse_native(&mut self, this: Value) -> Result<Value, JsError> {
        let values = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => {
                let length = self.heap.sparse_length(this).unwrap_or(elements.len());
                (0..length)
                    .map(|index| {
                        elements
                            .get(index)
                            .copied()
                            .or_else(|| self.heap.sparse_get(this, index))
                            .unwrap_or(Value::UNDEFINED)
                    })
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

    pub(super) fn array_shift_native(&mut self, this: Value) -> Result<Value, JsError> {
        let values = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => {
                let length = self.heap.sparse_length(this).unwrap_or(elements.len());
                (0..length)
                    .map(|index| {
                        elements
                            .get(index)
                            .copied()
                            .or_else(|| self.heap.sparse_get(this, index))
                            .unwrap_or(Value::UNDEFINED)
                    })
                    .collect::<Vec<_>>()
            }
            _ => return Err(JsError("shift receiver is not array".into())),
        };
        let first = values.first().copied().unwrap_or(Value::UNDEFINED);
        if !values.is_empty() {
            self.check_array_mutation(this, false, false, true)?;
        }
        if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(this) {
            *elements = Rc::new(values.into_iter().skip(1).collect());
        }
        Ok(first)
    }

    pub(super) fn array_unshift_native(
        &mut self,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let values = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => {
                let length = self.heap.sparse_length(this).unwrap_or(elements.len());
                (0..length)
                    .map(|index| {
                        elements
                            .get(index)
                            .copied()
                            .or_else(|| self.heap.sparse_get(this, index))
                            .unwrap_or(Value::UNDEFINED)
                    })
                    .collect::<Vec<_>>()
            }
            _ => return Err(JsError("unshift receiver is not array".into())),
        };
        let mut updated = args.to_vec();
        updated.extend(values);
        let length = updated.len();
        self.check_array_mutation(this, true, !args.is_empty(), true)?;
        if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(this) {
            *elements = Rc::new(updated);
        }
        Ok(Value::number(length as f64))
    }

    pub(super) fn array_splice_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let values = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => {
                let length = self.heap.sparse_length(this).unwrap_or(elements.len());
                (0..length)
                    .map(|index| {
                        elements
                            .get(index)
                            .copied()
                            .or_else(|| self.heap.sparse_get(this, index))
                            .unwrap_or(Value::UNDEFINED)
                    })
                    .collect::<Vec<_>>()
            }
            _ => return Err(JsError("splice receiver is not array".into())),
        };
        let length = values.len();
        let start_number =
            self.to_number(p, args.first().copied().unwrap_or(Value::number(0.0)))?;
        let start = if start_number.is_nan() {
            0
        } else if start_number.is_sign_negative() {
            length.saturating_sub(start_number.abs().trunc() as usize)
        } else {
            (start_number.trunc() as usize).min(length)
        };
        let delete_count = args
            .get(1)
            .map(|value| self.to_number(p, *value))
            .transpose()?
            .unwrap_or((length - start) as f64)
            .max(0.0)
            .trunc() as usize;
        let delete_count = delete_count.min(length - start);
        let removed = values[start..start + delete_count].to_vec();
        let mut updated = values[..start].to_vec();
        updated.extend(args.iter().copied().skip(2));
        updated.extend(values[start + delete_count..].iter().copied());
        self.check_array_mutation(
            this,
            !updated.is_empty(),
            updated.len() > length,
            updated.len() < length,
        )?;
        if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(this) {
            *elements = Rc::new(updated);
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(removed),
        }))
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
            .map(|index| {
                elements
                    .get(index)
                    .copied()
                    .or_else(|| self.heap.sparse_get(value, index))
                    .unwrap_or(Value::UNDEFINED)
            })
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
                values.extend((0..length).map(|index| {
                    elements
                        .get(index)
                        .copied()
                        .or_else(|| self.heap.sparse_get(value, index))
                        .unwrap_or(Value::UNDEFINED)
                }));
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
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(this) else {
            return Err(JsError("push receiver is not array".into()));
        };
        let mut length = self.heap.sparse_length(this).unwrap_or(elements.len());
        if !args.is_empty() {
            self.check_array_mutation(this, false, true, false)?;
        }
        for value in args {
            self.set_array_element(this, length, *value);
            length += 1;
        }
        Ok(Value::number(length as f64))
    }

    pub(super) fn array_pop_native(&mut self, this: Value) -> Result<Value, JsError> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(this) else {
            return Err(JsError("pop receiver is not array".into()));
        };
        let dense_len = elements.len();
        if self.heap.sparse_length(this).unwrap_or(dense_len) > 0 {
            self.check_array_mutation(this, false, false, true)?;
        }
        if self.heap.sparse_length(this).is_some() {
            return Ok(self.heap.sparse_pop(this, dense_len));
        }
        let Some(Cell::Array { elements, .. }) = self.heap.get_mut(this) else {
            unreachable!()
        };
        Ok(super::index::mutable_array_elements(elements)
            .pop()
            .unwrap_or(Value::UNDEFINED))
    }

    pub(super) fn array_slice_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let elements = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => Rc::clone(elements),
            _ => return Err(JsError("slice receiver is not array".into())),
        };
        let length = self.heap.sparse_length(this).unwrap_or(elements.len());
        let mut relative = |value: Value| -> Result<usize, JsError> {
            let number = self.to_number(p, value)?;
            if number.is_nan() {
                return Ok(0);
            }
            if number.is_infinite() {
                return Ok(if number.is_sign_negative() { 0 } else { length });
            }
            let number = number.trunc();
            if number.is_sign_negative() {
                Ok(length.saturating_sub((-number) as usize))
            } else {
                Ok((number as usize).min(length))
            }
        };
        let start = relative(args.first().copied().unwrap_or(Value::number(0.0)))?;
        let end = args
            .get(1)
            .copied()
            .map(relative)
            .transpose()?
            .unwrap_or(length);
        let values = (start.min(end)..end)
            .map(|index| {
                elements
                    .get(index)
                    .copied()
                    .or_else(|| self.heap.sparse_get(this, index))
                    .unwrap_or(Value::UNDEFINED)
            })
            .collect();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
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
            let value = elements
                .get(index)
                .copied()
                .or_else(|| self.heap.sparse_get(this, index))
                .unwrap_or(Value::UNDEFINED);
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
            let Some(value) = elements
                .get(index)
                .copied()
                .or_else(|| self.heap.sparse_get(this, index))
            else {
                continue;
            };
            if value.is_null() || value.is_undefined() {
                continue;
            }
            output.push_str(&self.to_string(p, value)?);
        }
        Ok(self.heap.alloc(Cell::String(output)))
    }
}
