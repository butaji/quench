//! `call_indirect`'s target: the table element, checked against the type the call expects.
use super::*;

impl<H: Host> Vm<H> {
    /// The function at `index` of `table` that a `call_indirect` expecting type
    /// `expected_index` of `caller`'s module calls, or the trap it raises.
    pub(super) fn wasm_indirect_target(
        &self,
        caller: ProgramId,
        table: Value,
        index: u64,
        expected_index: u32,
    ) -> Result<Value, JsError> {
        let elements = self.wasm_table_elements(table)?;
        let reference = usize::try_from(index)
            .ok()
            .and_then(|index| elements.get(index))
            .copied()
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::UndefinedElement))?;
        if reference.is_null() {
            return Err(JsError::wasm_trap_error(crate::WasmTrap::UninitializedElement));
        }
        // A function of the caller's own module declared with the expected type, the common
        // indirect call, is the interned type itself (`WasmSignatures::accepts`'s first case).
        if let Some(FunctionRecord { kind: FunctionKind::User(program, defined), .. }) = self.heap.function(reference)
            && *program == caller
            && self.programs.wasm_signatures(caller).and_then(|pool| pool.defined_type_index(*defined)) == Some(expected_index)
        {
            return Ok(reference);
        }
        self.wasm_callable_accepted(caller, reference, expected_index).map(|()| reference)
    }

    /// Whether the callable `reference` has a type that type `expected_index` of `caller`'s
    /// module accepts, as the trap a mismatch raises.
    #[inline(never)]
    fn wasm_callable_accepted(&self, caller: ProgramId, reference: Value, expected_index: u32) -> Result<(), JsError> {
        let expected = self
            .programs
            .wasm_signatures(caller)
            .filter(|pool| pool.type_signature(expected_index).is_some())
            .ok_or_else(|| JsError::validation("invalid Wasm signature index".into()))?;
        let matches = self
            .wasm_callable_matches(reference, expected, expected_index, false)
            .ok_or_else(|| JsError::validation("invalid Wasm function signature".into()))?;
        if matches { Ok(()) } else { Err(JsError::wasm_trap_error(crate::WasmTrap::IndirectCallTypeMismatch)) }
    }
}
