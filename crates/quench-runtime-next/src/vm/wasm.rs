use super::*;

impl<H: Host> Vm<H> {
    pub(crate) fn execute_wasm_i32(
        &mut self,
        function: &crate::WasmI32Function,
        args: &[i32],
    ) -> Result<Option<i32>, JsError> {
        let program = &function.program;
        program.validate().map_err(JsError::validation)?;
        if args.len() != usize::from(program.functions[0].params) {
            return Err(JsError::validation("Wasm argument count mismatch".into()));
        }
        self.initialize(program)?;
        let args = args.iter().copied().map(Value::integer).collect::<Vec<_>>();
        let result = self.call_user(program, 0, Value::NULL, Value::UNDEFINED, &args)?;
        if function.has_result {
            result
                .as_int()
                .map(Some)
                .ok_or_else(|| JsError::validation("invalid Wasm i32 result".into()))
        } else {
            Ok(None)
        }
    }
}
