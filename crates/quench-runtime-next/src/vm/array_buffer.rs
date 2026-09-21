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
            object: Self::empty_object(self.object_proto),
            bytes: Rc::new(vec![0; length]),
        });
        self.intern_atom("byteLength");
        Ok(buffer)
    }
}
