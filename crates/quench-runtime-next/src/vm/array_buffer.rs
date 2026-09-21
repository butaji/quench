use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn construct_array_buffer_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let number = self.to_number(p, args.first().copied().unwrap_or(Value::number(0.0)))?;
        let length = if number.is_nan() || number.is_sign_negative() {
            0
        } else {
            number.trunc() as usize
        };
        let buffer = self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes: Rc::new(vec![0; length]),
        });
        self.intern_atom("byteLength");
        Ok(buffer)
    }

    pub(super) fn array_buffer_slice_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let bytes = match self.heap.get(this) {
            Some(Cell::ArrayBuffer { bytes, .. }) => Rc::clone(bytes),
            _ => return Err(JsError("ArrayBuffer.slice receiver is invalid".into())),
        };
        let length = bytes.len();
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
            .first()
            .map(|value| self.to_number(p, *value))
            .transpose()?
            .map(relative)
            .unwrap_or(0);
        let end = args
            .get(1)
            .map(|value| self.to_number(p, *value))
            .transpose()?
            .map(relative)
            .unwrap_or(length);
        Ok(self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes: Rc::new(bytes[start.min(end)..end].to_vec()),
        }))
    }
}
