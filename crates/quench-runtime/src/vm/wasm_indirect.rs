//! The target of an indirect Wasm call, resolved by the staged rule
//! (`staged::wasm_call::indirect_target`) on the VM's tables, and the signature a table element
//! carries beside its reference for that rule.

use super::*;
use crate::staged::wasm_call::indirect_target;
use super::wasm_table::table_index_type;
use crate::staged::wasm_machine::ConcreteTable;
use crate::heap::{TableTarget, WasmTableElements};
use crate::vm::engine_types::InstanceContext;

impl Vm {
    /// What a table element holding `value` presents to an indirect call (`TableTarget`): the
    /// null row for the null reference; for a function of a module its canonical signature, its
    /// index and its instance's record (`InstanceContext::address`); the foreign row for anything the VM compares against
    /// the expected type (a host function).
    pub(super) fn wasm_element_target(&self, value: Value) -> TableTarget {
        if value.is_null() {
            return TableTarget::NULL;
        }
        let Some(FunctionRecord {
            kind: FunctionKind::User(program, function),
            env,
            ..
        }) = self.heap.function(value)
        else {
            return TableTarget::FOREIGN;
        };
        let Some(signature) = self
            .programs
            .wasm_signatures(*program)
            .and_then(|pool| pool.defined_canonical(*function))
        else {
            return TableTarget::FOREIGN;
        };
        let Some(instance) = self.wasm_instance_address(*env, *program) else {
            return TableTarget::FOREIGN;
        };
        TableTarget {
            signature,
            entry: *function,
            instance: instance as u64,
            body: InstanceContext::published_body(instance, *function),
        }
    }

    /// The function reference at `index` of `table` an indirect call of signature type
    /// `expected_index` of `caller`'s module calls (`wasm_indirect_element`).
    pub fn wasm_indirect_target(
        &self,
        caller: ProgramId,
        table: Value,
        index: u64,
        expected_index: u32,
    ) -> Result<Value, JsError> {
        let Some(Cell::WasmTable { elements, .. }) = self.heap.get(table) else {
            return Err(JsError::validation("invalid Wasm table binding".into()));
        };
        self.wasm_indirect_element(self.programs.wasm_signatures(caller), elements, index, expected_index).map(|(value, _)| value)
    }

    /// `wasm_indirect_target` of the index operand a register holds, in the table's index
    /// type, which one read of the table's cell answers with its elements.
    pub(super) fn wasm_indirect_target_of_operand(
        &self,
        caller: ProgramId,
        table: Value,
        operand: Value,
        expected_index: u32,
    ) -> Result<Value, JsError> {
        self.wasm_indirect_row_of_operand(caller, table, operand, expected_index).map(|(value, _)| value)
    }

    /// `wasm_indirect_target_of_operand` with the matched row of the element (`TableTarget`): the
    /// reference, and the row a lane's reference call takes its callee's entry and instance from.
    pub(super) fn wasm_indirect_row_of_operand(
        &self,
        caller: ProgramId,
        table: Value,
        operand: Value,
        expected_index: u32,
    ) -> Result<(Value, TableTarget), JsError> {
        self.wasm_indirect_row_with(self.programs.wasm_signatures(caller), table, operand, expected_index)
    }

    /// `wasm_indirect_row_of_operand` over the caller's signature pool, `pool` (None where the
    /// program has none, which the element check reports after the table and index).
    pub fn wasm_indirect_row_with(
        &self,
        pool: Option<&crate::wasm::WasmSignatures>,
        table: Value,
        operand: Value,
        expected_index: u32,
    ) -> Result<(Value, TableTarget), JsError> {
        let Some(Cell::WasmTable { elements, table64, .. }) = self.heap.get(table) else {
            return Err(JsError::validation("invalid Wasm table binding".into()));
        };
        let index = self.wasm_index_operand(operand, table_index_type(*table64))?;
        self.wasm_indirect_element(pool, elements, index, expected_index)
    }

    /// The element at `index` of `elements` an indirect call of signature type `expected_index`
    /// calls (`staged::wasm_call::indirect_target`): a validation error where the program has
    /// no such type, or where a reference whose id is not the expected one cannot be compared,
    /// which the rule meets only after the element checks.
    #[inline(always)]
    fn wasm_indirect_element(
        &self,
        pool: Option<&crate::wasm::WasmSignatures>,
        elements: &WasmTableElements,
        index: u64,
        expected_index: u32,
    ) -> Result<(Value, TableTarget), JsError> {
        let (pool, expected) = pool
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
            targets: elements.targets(),
            related: &mut related,
        };
        let (reference, row) =
            indirect_target(&mut stage, index, expected).map_err(JsError::wasm_trap_error)?;
        failure.map_or(Ok((Value::from_bits(reference), row)), Err)
    }
}

#[cfg(test)]
mod tests;
