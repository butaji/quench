//! The target of an indirect Wasm call, resolved by the staged rule
//! (`staged::wasm_call::indirect_target`) on the VM's tables.

use super::*;
use crate::staged::wasm_call::indirect_target;
use crate::staged::wasm_machine::ConcreteTable;

impl<H: Host> Vm<H> {
    /// The function reference at `index` an indirect call of signature type `expected_index`
    /// calls (`staged::wasm_call::indirect_target`): a validation error where the reference's
    /// signature cannot be compared, which the rule meets only after the element checks.
    pub(super) fn wasm_indirect_target(
        &self,
        caller: ProgramId,
        table: Value,
        index: u64,
        expected_index: u32,
    ) -> Result<Value, JsError> {
        let elements = self.wasm_table_elements(table)?;
        let mut failure = None;
        let mut matches = |reference: Value| {
            let verdict = self
                .programs
                .wasm_signatures(caller)
                .filter(|pool| pool.type_signature(expected_index).is_some())
                .ok_or_else(|| JsError::validation("invalid Wasm signature index".into()))
                .and_then(|expected| {
                    self.wasm_callable_matches(reference, expected, expected_index, false)
                        .ok_or_else(|| {
                            JsError::validation("invalid Wasm function signature".into())
                        })
                });
            verdict.unwrap_or_else(|error| {
                failure = Some(error);
                true
            })
        };
        let mut stage = ConcreteTable {
            elements,
            matches: &mut matches,
        };
        let reference =
            indirect_target(&mut stage, index, expected_index).map_err(JsError::wasm_trap_error)?;
        failure.map_or(Ok(Value::from_bits(reference)), Err)
    }
}
