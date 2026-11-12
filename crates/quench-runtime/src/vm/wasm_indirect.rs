//! The target of an indirect Wasm call, resolved by the staged rule
//! (`staged::wasm_call::indirect_target`) on the VM's tables, and the signature a table element
//! carries beside its reference for that rule.

use super::*;
use crate::staged::wasm_call::indirect_target;
use crate::staged::wasm_machine::ConcreteTable;
use crate::wasm::CanonicalSignature;

impl<H: Host> Vm<H> {
    /// What a table element holding `value` presents to the check of an indirect call: the
    /// canonical signature of a Wasm function, the null id for the null reference, the foreign
    /// id for anything the VM compares against the expected type (a host function).
    pub(super) fn wasm_element_signature(&self, value: Value) -> CanonicalSignature {
        if value.is_null() {
            return CanonicalSignature::NULL;
        }
        match self.heap.function(value) {
            Some(FunctionRecord {
                kind: FunctionKind::User(program, function),
                ..
            }) => self
                .programs
                .wasm_signatures(*program)
                .and_then(|pool| pool.defined_canonical(*function))
                .unwrap_or(CanonicalSignature::FOREIGN),
            _ => CanonicalSignature::FOREIGN,
        }
    }

    /// The function reference at `index` an indirect call of signature type `expected_index`
    /// calls (`staged::wasm_call::indirect_target`): a validation error where the program has
    /// no such type, or where a reference whose id is not the expected one cannot be compared,
    /// which the rule meets only after the element checks.
    pub(super) fn wasm_indirect_target(
        &self,
        caller: ProgramId,
        table: Value,
        index: u64,
        expected_index: u32,
    ) -> Result<Value, JsError> {
        let Some(Cell::WasmTable { elements, .. }) = self.heap.get(table) else {
            return Err(JsError::validation("invalid Wasm table binding".into()));
        };
        let (pool, expected) = self
            .programs
            .wasm_signatures(caller)
            .and_then(|pool| Some((pool, pool.type_canonical(expected_index)?)))
            .ok_or_else(|| JsError::validation("invalid Wasm signature index".into()))?;
        let mut failure = None;
        let mut related = |reference: Value| {
            self.wasm_callable_matches(reference, pool, expected_index, false)
                .unwrap_or_else(|| {
                    failure = Some(JsError::validation("invalid Wasm function signature".into()));
                    true
                })
        };
        let mut stage = ConcreteTable {
            elements: &elements.values,
            signatures: elements.signatures(),
            related: &mut related,
        };
        let reference =
            indirect_target(&mut stage, index, expected).map_err(JsError::wasm_trap_error)?;
        failure.map_or(Ok(Value::from_bits(reference)), Err)
    }
}

#[cfg(test)]
mod tests;
