use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn construct_float32_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_typed_array_native(p, args, TypedArrayKind::Float32, "Float32Array")
    }

    pub(super) fn construct_float64_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_typed_array_native(p, args, TypedArrayKind::Float64, "Float64Array")
    }
}
