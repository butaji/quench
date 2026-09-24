use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn array_modern_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::ArrayToReversed => self.array_to_reversed_native(this),
            Native::ArrayToSpliced => self.array_to_spliced_native(p, this, args),
            Native::ArraySort => self.array_sort_native(p, this, args, true),
            Native::ArrayToSorted => self.array_sort_native(p, this, args, false),
            Native::ArrayToString => self.array_to_string_native(p, this),
            Native::ArrayFrom => self.array_from_native(p, args),
            Native::ArrayOf => Ok(self.new_array(args.to_vec())),
            _ => unreachable!("non-modern native routed to modern array dispatch"),
        }
    }

    pub(super) fn array_values(&self, this: Value) -> Result<Vec<Value>, JsError> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(this) else {
            return Err(JsError("modern array method receiver is not array".into()));
        };
        let length = self.heap.sparse_length(this).unwrap_or(elements.len());
        Ok((0..length)
            .map(|index| self.array_value_at(this, index))
            .collect())
    }

    pub(super) fn new_array(&mut self, values: Vec<Value>) -> Value {
        self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        })
    }

    fn array_to_reversed_native(&mut self, this: Value) -> Result<Value, JsError> {
        let mut values = self.array_values(this)?;
        values.reverse();
        Ok(self.new_array(values))
    }

    fn array_to_string_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        self.array_join_native(p, this, &[])
    }

    fn array_from_native(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let done_atom = self.intern_atom("done");
        let value_atom = self.intern_atom("value");
        let mapfn = args.get(1).copied().filter(|value| !value.is_undefined());
        if let Some(mapfn) = mapfn
            && !matches!(self.heap.get(mapfn), Some(Cell::Function { .. }))
        {
            return Err(JsError("Array.from map function is not callable".into()));
        }
        let map_this = args.get(2).copied().unwrap_or(Value::UNDEFINED);
        let mut values = Vec::new();
        match self.get_iterator(p, source) {
            Ok(iterator) => loop {
                let step = match self.iterator_next(p, iterator) {
                    Ok(step) => step,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                let done = match self.get_property(p, step, done_atom) {
                    Ok(done) => done,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                if self.truthy(done) {
                    break;
                }
                let mut value = match self.get_property(p, step, value_atom) {
                    Ok(value) => value,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                if let Some(mapfn) = mapfn {
                    let index = Value::number(values.len() as f64);
                    value = match self.call_value(p, mapfn, map_this, &[value, index]) {
                        Ok(value) => value,
                        Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                    };
                }
                values.push(value);
            },
            Err(error) if error.to_string() == "value is not iterable" => {
                let length_atom = self.intern_atom("length");
                let length_value = self.get_property(p, source, length_atom)?;
                let length = self.to_number(p, length_value)?;
                let length = if !length.is_finite() || length <= 0.0 {
                    if length.is_infinite() && length.is_sign_positive() {
                        return Err(JsError("Array.from length is too large".into()));
                    }
                    0
                } else {
                    length.floor().min(usize::MAX as f64) as usize
                };
                values.reserve(length);
                for index in 0..length {
                    let mut value = self.get_index(p, source, Value::number(index as f64))?;
                    if let Some(mapfn) = mapfn {
                        value = self.call_value(
                            p,
                            mapfn,
                            map_this,
                            &[value, Value::number(index as f64)],
                        )?;
                    }
                    values.push(value);
                }
            }
            Err(error) => return Err(error),
        }
        Ok(self.new_array(values))
    }

    pub(super) fn iterator_abrupt(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        error: JsError,
    ) -> JsError {
        self.iterator_close(p, iterator).err().unwrap_or(error)
    }

    fn array_to_spliced_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let values = self.array_values(this)?;
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
        let mut updated = values[..start].to_vec();
        updated.extend(args.iter().copied().skip(2));
        updated.extend(values[start + delete_count..].iter().copied());
        Ok(self.new_array(updated))
    }

    fn array_sort_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
        mutate: bool,
    ) -> Result<Value, JsError> {
        let mut values = self.array_values(this)?;
        let comparator = args.first().copied().filter(|value| !value.is_undefined());
        if let Some(value) = comparator
            && !matches!(self.heap.get(value), Some(Cell::Function { .. }))
        {
            return Err(JsError("sort comparator is not callable".into()));
        }
        for index in 1..values.len() {
            let value = values[index];
            let mut position = index;
            while position > 0
                && self.sort_compare(p, comparator, values[position - 1], value)? > 0.0
            {
                values[position] = values[position - 1];
                position -= 1;
            }
            values[position] = value;
        }
        if mutate {
            if !values.is_empty() {
                self.check_array_mutation(this, true, false, false)?;
            }
            for (index, value) in values.into_iter().enumerate() {
                self.set_array_element(this, index, value);
            }
            Ok(this)
        } else {
            Ok(self.new_array(values))
        }
    }

    fn sort_compare(
        &mut self,
        p: &ResidualProgram,
        comparator: Option<Value>,
        left: Value,
        right: Value,
    ) -> Result<f64, JsError> {
        if left.is_undefined() || right.is_undefined() {
            return Ok(match (left.is_undefined(), right.is_undefined()) {
                (true, true) => 0.0,
                (true, false) => 1.0,
                (false, true) => -1.0,
                _ => unreachable!(),
            });
        }
        if let Some(comparator) = comparator {
            let result = self.call_value(p, comparator, Value::UNDEFINED, &[left, right])?;
            let number = self.to_number(p, result)?;
            return Ok(if number.is_nan() { 0.0 } else { number });
        }
        let left = self.to_string(p, left)?;
        let right = self.to_string(p, right)?;
        Ok(match left.cmp(&right) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        })
    }
}
