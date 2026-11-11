//! A JavaScript call of an exported Wasm function whose code is compiled: arguments convert
//! straight to the raw words native code takes and the raw result word straight back, without
//! the interpreter's value encoding in between (a boxed word per 64-bit scalar each way).
use super::*;

/// Whether values of `ty` cross the native boundary as one raw word.
fn raw_scalar(ty: &WasmType) -> bool {
    matches!(ty, WasmType::I32 | WasmType::I64 | WasmType::F32 | WasmType::F64)
}

/// The raw word native code holds for a scalar.
fn raw_word(value: WasmValue) -> Option<u64> {
    match value.bits()? {
        crate::wasm::ScalarBits::Bits32(bits) => Some(u64::from(bits)),
        crate::wasm::ScalarBits::Bits64(bits) => Some(bits),
        crate::wasm::ScalarBits::Bits128(_) => None,
    }
}

impl<H: Host> Vm<H> {
    /// Call compiled export `index` of `program` in instance `environment` on `args`; `None`
    /// when it has no compiled code yet or a type that does not cross as a raw word, before any
    /// argument converted.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call_compiled_wasm_export(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
        environment: Value,
        index: u32,
        signature: &crate::WasmSignature,
        declarations: Option<&crate::WasmTypes>,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        if signature.results.len() > 1 || !signature.params.iter().chain(&signature.results).all(raw_scalar) {
            return None;
        }
        let compiled = self.compiled_wasm_holding(program, index)?;
        Some((|| {
            let raw = self.raw_wasm_arguments(p, &signature.params, declarations, args)?;
            let previous = self.replace_active_program(program);
            let result = match self.enter_compiled_wasm(&compiled, program, environment, index, &raw) {
                Some(out) => out.map(|bits| signature.results.first().map(|&ty| WasmValue::from_bits(ty, bits))),
                // This instance's memory does not suit the artifact: the interpreter runs it.
                None => self.call_wasm_export_raw_interpreted(previous, program, environment, index, signature, declarations, &raw),
            };
            self.set_active_program(previous);
            let result = result.map_err(|error| self.trap_raised_in_javascript(p, error))?;
            match (signature.results.first(), result) {
                (Some(&ty), Some(value)) => self.wasm_value_to_js(p, value, ty),
                _ => Ok(Value::UNDEFINED),
            }
        })())
    }

    /// `args` converted to the raw words of parameters `params`, in order.
    fn raw_wasm_arguments(
        &mut self,
        p: &ResidualProgram,
        params: &[WasmType],
        declarations: Option<&crate::WasmTypes>,
        args: &[Value],
    ) -> Result<smallvec::SmallVec<[u64; WASM_INLINE_ARGUMENTS]>, JsError> {
        let mut raw = smallvec::SmallVec::new();
        for (position, &ty) in params.iter().enumerate() {
            let value = args.get(position).copied().unwrap_or(Value::UNDEFINED);
            let value = self.js_to_wasm_value(p, value, ty, declarations)?;
            raw.push(raw_word(value).ok_or_else(|| JsError::validation("non-scalar Wasm argument".into()))?);
        }
        Ok(raw)
    }

    /// `call_wasm_export_interpreted` on raw argument words, with the result decoded.
    #[allow(clippy::too_many_arguments)]
    fn call_wasm_export_raw_interpreted(
        &mut self,
        previous: ProgramId,
        program: ProgramId,
        environment: Value,
        index: u32,
        signature: &crate::WasmSignature,
        declarations: Option<&crate::WasmTypes>,
        raw: &[u64],
    ) -> Result<Option<WasmValue>, JsError> {
        let encoded: smallvec::SmallVec<[Value; WASM_INLINE_ARGUMENTS]> = signature.params.iter().zip(raw)
            .map(|(&ty, &bits)| self.encode_wasm_value(WasmValue::from_bits(ty, bits))).collect();
        let value = self.call_wasm_export_interpreted(previous, program, environment, index, &encoded)?;
        signature.results.first().map(|&ty| self.decode_wasm_value_in(value, ty, declarations)).transpose()
    }

    /// Run export `index` in the interpreter on `encoded` arguments, with `program` active and
    /// `previous` the program active before the call.
    pub(super) fn call_wasm_export_interpreted(
        &mut self,
        previous: ProgramId,
        program: ProgramId,
        environment: Value,
        index: u32,
        encoded: &[Value],
    ) -> Result<Value, JsError> {
        let residual = self.programs.get(program).ok_or_else(|| JsError::validation("missing Wasm program".into()))?;
        // The interpreted call pushes a frame, which observes the frame stack: a lane's
        // deferred activations become frames first, from their caller's program, as a call of
        // guest code makes them (`runs_beside_activations`).
        if self.lane_deferred.is_some() {
            self.set_active_program(previous);
            self.materialize_deferred_lane();
            self.replace_active_program(program);
        }
        self.call_user(&residual, index, environment, Value::UNDEFINED, encoded, CallContext::Internal)
    }
}
