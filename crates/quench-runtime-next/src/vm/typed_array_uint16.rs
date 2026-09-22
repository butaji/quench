use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn construct_uint8_clamped_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_typed_array_native(
            p,
            args,
            TypedArrayKind::Uint8Clamped,
            "Uint8ClampedArray",
        )
    }

    pub(super) fn construct_uint16_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_typed_array_native(p, args, TypedArrayKind::Uint16, "Uint16Array")
    }

    pub(super) fn construct_uint32_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_typed_array_native(p, args, TypedArrayKind::Uint32, "Uint32Array")
    }
}
