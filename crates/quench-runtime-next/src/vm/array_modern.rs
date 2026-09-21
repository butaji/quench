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
            _ => unreachable!("non-modern native routed to modern array dispatch"),
        }
    }

    fn array_values(&self, this: Value) -> Result<Vec<Value>, JsError> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(this) else {
            return Err(JsError("modern array method receiver is not array".into()));
        };
        let length = self.heap.sparse_length(this).unwrap_or(elements.len());
        Ok((0..length)
            .map(|index| {
                elements
                    .get(index)
                    .copied()
                    .or_else(|| self.heap.sparse_get(this, index))
                    .unwrap_or(Value::UNDEFINED)
            })
            .collect())
    }

    fn new_array(&mut self, values: Vec<Value>) -> Value {
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
}
