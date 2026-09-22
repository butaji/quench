use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn construct_int8_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_typed_array_native(p, args, TypedArrayKind::Int8, "Int8Array")
    }

    pub(super) fn construct_int16_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_typed_array_native(p, args, TypedArrayKind::Int16, "Int16Array")
    }

    pub(super) fn construct_int32_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_typed_array_native(p, args, TypedArrayKind::Int32, "Int32Array")
    }
}
