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
            _ => unreachable!("non-indexed native routed to array index dispatch"),
        }
    }

    pub(super) fn array_at_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (elements, length) = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => (
                Rc::clone(elements),
                self.heap.sparse_length(this).unwrap_or(elements.len()),
            ),
            _ => return Err(JsError("at receiver is not array".into())),
        };
        let index = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if index.is_nan() || index.is_infinite() {
            return Ok(Value::UNDEFINED);
        }
        let index = index.trunc();
        let index = if index.is_sign_negative() {
            length as f64 + index
        } else {
            index
        };
        if !(0.0..(length as f64)).contains(&index) {
            return Ok(Value::UNDEFINED);
        }
        let index = index as usize;
        Ok(elements
            .get(index)
            .copied()
            .or_else(|| self.heap.sparse_get(this, index))
            .unwrap_or(Value::UNDEFINED))
    }

    pub(super) fn array_last_index_of_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (elements, length) = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => (
                Rc::clone(elements),
                self.heap.sparse_length(this).unwrap_or(elements.len()),
            ),
            _ => return Err(JsError("lastIndexOf receiver is not array".into())),
        };
        if length == 0 {
            return Ok(Value::number(-1.0));
        }
        let from = match args.get(1) {
            None => length as isize - 1,
            Some(value) => {
                let number = self.to_number(p, *value)?;
                if number.is_nan() || number.is_sign_negative() && number.is_infinite() {
                    return Ok(Value::number(-1.0));
                }
                if number.is_infinite() {
                    length as isize - 1
                } else if number.is_sign_negative() {
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
            let value = elements
                .get(index)
                .copied()
                .or_else(|| self.heap.sparse_get(this, index))
                .unwrap_or(Value::UNDEFINED);
            if self.array_strict_equal(value, search) {
                return Ok(Value::number(index as f64));
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
        let (elements, length) = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => (
                Rc::clone(elements),
                self.heap.sparse_length(this).unwrap_or(elements.len()),
            ),
            _ => return Err(JsError("indexOf receiver is not array".into())),
        };
        if length == 0 {
            return Ok(Value::number(-1.0));
        }
        let start = match args.get(1) {
            None => 0,
            Some(value) => {
                let number = self.to_number(p, *value)?;
                if number.is_nan() || number.is_sign_negative() && number.is_infinite() {
                    0
                } else if number.is_infinite() {
                    return Ok(Value::number(-1.0));
                } else if number.is_sign_negative() {
                    length.saturating_sub(number.abs().trunc() as usize)
                } else {
                    (number.trunc() as usize).min(length)
                }
            }
        };
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        for index in start..length {
            let value = elements
                .get(index)
                .copied()
                .or_else(|| self.heap.sparse_get(this, index))
                .unwrap_or(Value::UNDEFINED);
            if self.array_strict_equal(value, search) {
                return Ok(Value::number(index as f64));
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
        let (elements, length) = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => (
                Rc::clone(elements),
                self.heap.sparse_length(this).unwrap_or(elements.len()),
            ),
            _ => return Err(JsError("copyWithin receiver is not array".into())),
        };
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
        let target = args
            .first()
            .map(|value| self.to_number(p, *value))
            .transpose()?
            .map(relative)
            .unwrap_or(0);
        let source = args
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
        let count = end
            .saturating_sub(source)
            .min(length.saturating_sub(target));
        let copied = (0..count)
            .map(|offset| {
                elements
                    .get(source + offset)
                    .copied()
                    .or_else(|| self.heap.sparse_get(this, source + offset))
                    .unwrap_or(Value::UNDEFINED)
            })
            .collect::<Vec<_>>();
        for (offset, value) in copied.into_iter().enumerate() {
            self.set_array_element(this, target + offset, value);
        }
        Ok(this)
    }

    pub(super) fn array_with_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (elements, length) = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => (
                Rc::clone(elements),
                self.heap.sparse_length(this).unwrap_or(elements.len()),
            ),
            _ => return Err(JsError("with receiver is not array".into())),
        };
        let number = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if number.is_infinite() {
            return Err(JsError("with index is out of range".into()));
        }
        let index = if number.is_nan() {
            0
        } else if number.is_sign_negative() {
            length as isize + number.trunc() as isize
        } else {
            number.trunc() as isize
        };
        if index < 0 || index >= length as isize {
            return Err(JsError("with index is out of range".into()));
        }
        let mut values = (0..length)
            .map(|offset| {
                elements
                    .get(offset)
                    .copied()
                    .or_else(|| self.heap.sparse_get(this, offset))
                    .unwrap_or(Value::UNDEFINED)
            })
            .collect::<Vec<_>>();
        values[index as usize] = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    fn array_strict_equal(&self, left: Value, right: Value) -> bool {
        if let (Some(left), Some(right)) = (left.as_number(), right.as_number()) {
            return left == right;
        }
        if left == right {
            return true;
        }
        matches!(
            (self.heap.get(left), self.heap.get(right)),
            (Some(Cell::String(left)), Some(Cell::String(right))) if left == right
        )
    }
}
