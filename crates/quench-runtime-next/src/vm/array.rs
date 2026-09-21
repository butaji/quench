use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn array_push_native(
        &mut self,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(this) else {
            return Err(JsError("push receiver is not array".into()));
        };
        let mut length = self.heap.sparse_length(this).unwrap_or(elements.len());
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
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(this) else {
            return Err(JsError("slice receiver is not array".into()));
        };
        let length = self.heap.sparse_length(this).unwrap_or(elements.len());
        let relative = |value: Value, default: usize| {
            let number = value.as_number().unwrap_or(default as f64);
            if number.is_nan() {
                return 0;
            }
            if number.is_sign_negative() {
                length.saturating_sub((-number) as usize)
            } else {
                (number as usize).min(length)
            }
        };
        let start = relative(args.first().copied().unwrap_or(Value::number(0.0)), 0);
        let end = args
            .get(1)
            .copied()
            .map(|value| relative(value, length))
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
}
