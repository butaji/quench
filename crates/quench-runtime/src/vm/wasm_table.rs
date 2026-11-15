use super::*;

pub(super) fn table_range(
    start: u64,
    length: u64,
    available: usize,
) -> Result<std::ops::Range<usize>, JsError> {
    crate::wasm::memory::checked_range(start, usize::try_from(length).map_err(|_| out_of_bounds_table())?, available)
        .map_err(|_| out_of_bounds_table())
}

/// The trap of a table access outside its elements.
fn out_of_bounds_table() -> JsError {
    JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsTable)
}

impl<H: Host> Vm<H> {
    pub(super) fn wasm_reference_valid(&self, value: Value, ty: wasmparser::RefType) -> bool {
        if value.is_null() {
            return ty.is_nullable();
        }
        // Externrefs can carry any live host value, including an opaque GC object.
        if ty.heap_type() == wasmparser::HeapType::EXTERN {
            return !value.is_heap() || self.heap.is_live(value);
        }
        if matches!(self.heap.get(value), Some(Cell::WasmExtern(_))) {
            return matches!(
                ty.heap_type(),
                wasmparser::HeapType::Abstract {
                    shared: false,
                    ty: wasmparser::AbstractHeapType::Any
                }
            );
        }
        if let Some(Cell::WasmGc {
            declarations,
            ty: index,
            ..
        }) = self.heap.get(value)
        {
            return declarations.reference_subtype(
                wasmparser::RefType::new(
                    false,
                    wasmparser::HeapType::Exact(wasmparser::UnpackedIndex::Module(*index)),
                )
                .unwrap(),
                declarations,
                ty,
            );
        }
        match ty.heap_type() {
            wasmparser::HeapType::Abstract {
                shared: false,
                ty:
                    wasmparser::AbstractHeapType::I31
                    | wasmparser::AbstractHeapType::Eq
                    | wasmparser::AbstractHeapType::Any,
            } => crate::wasm::i31::bits(value).is_some(),
            wasmparser::HeapType::Abstract {
                shared: false,
                ty: wasmparser::AbstractHeapType::Exn,
            } => matches!(self.heap.get(value), Some(Cell::WasmException { .. })),
            wasmparser::HeapType::FUNC => self.wasm_callable_signature(value).is_some(),
            _ => false,
        }
    }

    fn wasm_table_accepts(&self, table: Value, value: Value) -> bool {
        let Some(Cell::WasmTable {
            element_type,
            declarations,
            ..
        }) = self.heap.get(table)
        else {
            return false;
        };
        declarations
            .callable_value_type(wasmparser::ValType::Ref(*element_type))
            .is_some_and(|ty| {
                self.decode_wasm_value_in(value, ty, Some(declarations))
                    .is_ok()
            })
    }

    pub(super) fn wasm_function_reference(
        &mut self,
        p: &ResidualProgram,
        function: u32,
        env: Value,
    ) -> Result<Value, JsError> {
        let signatures = self
            .programs
            .wasm_signatures(self.active_program)
            .ok_or_else(|| JsError::validation("missing Wasm function facts".into()))?;
        signatures
            .get(function as usize)
            .ok_or_else(|| JsError::validation("Wasm function index out of bounds".into()))?;
        let Some(function) = signatures.defined_index(function) else {
            let Some(Cell::Environment { slots, .. }) = self.heap.get(env) else {
                return Err(JsError::validation(
                    "invalid Wasm import environment".into(),
                ));
            };
            let slot = slots
                .len()
                .checked_sub(signatures.imports().len())
                .and_then(|base| base.checked_add(function as usize))
                .ok_or_else(|| JsError::validation("invalid Wasm function import slot".into()))?;
            return self
                .heap
                .environment_slot(env, slot)
                .ok_or_else(|| JsError::validation("invalid Wasm function import binding".into()));
        };
        let cached = self
            .cached_functions_in_environment(self.active_program, function, env)
            .next();
        match cached {
            Some(value) => Ok(value),
            None => self.closure(p, function, env),
        }
    }

    pub(super) fn wasm_table_index_type(&self, table: Value) -> Result<crate::WasmType, JsError> {
        match self.heap.get(table) {
            Some(Cell::WasmTable { table64, .. }) => Ok(if *table64 {
                crate::WasmType::I64
            } else {
                crate::WasmType::I32
            }),
            _ => Err(JsError::validation("invalid Wasm table binding".into())),
        }
    }

    pub(super) fn wasm_table_index(&self, table: Value, value: Value) -> Result<u64, JsError> {
        let ty = self.wasm_table_index_type(table)?;
        self.wasm_index_operand(value, ty)
    }

    #[inline]
    pub(super) fn wasm_index_operand(
        &self,
        value: Value,
        ty: crate::WasmType,
    ) -> Result<u64, JsError> {
        match (ty, value.as_int(), value.bits64()) {
            (crate::WasmType::I32, Some(bits), _) => return Ok(u64::from(bits as u32)),
            (crate::WasmType::I64, _, Some(bits)) => return Ok(bits),
            _ => {}
        }
        match self.decode_wasm_value(value, ty)? {
            crate::WasmValue::I32(value) => Ok(u64::from(value as u32)),
            crate::WasmValue::I64(value) => Ok(value as u64),
            _ => Err(JsError::validation("invalid Wasm index type".into())),
        }
    }

    pub(super) fn encode_wasm_table_size(
        &mut self,
        table: Value,
        size: Option<u64>,
    ) -> Result<Value, JsError> {
        let ty = self.wasm_table_index_type(table)?;
        Ok(self.encode_wasm_index_size(ty, size))
    }

    pub(super) fn wasm_table_elements(&self, table: Value) -> Result<&[Value], JsError> {
        match self.heap.get(table) {
            Some(Cell::WasmTable { elements, .. }) => Ok(elements),
            _ => Err(JsError::validation("invalid Wasm table binding".into())),
        }
    }

    pub(super) fn wasm_table_get(&self, table: Value, index: u64) -> Result<Value, JsError> {
        self.wasm_table_elements(table)?
            .get(usize::try_from(index).map_err(|_| out_of_bounds_table())?)
            .copied()
            .ok_or_else(out_of_bounds_table)
    }

    pub(super) fn wasm_table_fill(
        &mut self,
        table: Value,
        index: u64,
        value: Value,
        length: u64,
    ) -> Result<(), JsError> {
        if !self.wasm_table_accepts(table, value) {
            return Err(JsError::validation("invalid Wasm table reference".into()));
        }
        let target = self.wasm_element_target(value);
        let Some(Cell::WasmTable { elements, .. }) = self.heap.get_mut(table) else {
            unreachable!()
        };
        let range = table_range(index, length, elements.len())?;
        elements.fill(range, value, target);
        Ok(())
    }

    pub(super) fn wasm_table_grow(
        &mut self,
        table: Value,
        initial: Value,
        delta: u64,
    ) -> Result<Option<u64>, JsError> {
        if !self.wasm_table_accepts(table, initial) {
            return Err(JsError::validation("invalid Wasm table reference".into()));
        }
        let target = self.wasm_element_target(initial);
        let Some(Cell::WasmTable {
            elements, table64, ..
        }) = self.heap.get_mut(table)
        else {
            unreachable!()
        };
        let old_size = elements.len();
        let limit = elements
            .maximum
            .unwrap_or_else(|| crate::wasm::table::table_index_limit(*table64));
        let Some(length) = usize::try_from(delta)
            .ok()
            .and_then(|delta| old_size.checked_add(delta))
            .filter(|length| (*length as u64) <= limit)
        else {
            return Ok(None);
        };
        let before = elements.external_bytes();
        if !elements.try_grow(length, initial, target) {
            return Ok(None);
        }
        let after = elements.external_bytes();
        self.heap.adjust_external_bytes(before, after);
        Ok(Some(old_size as u64))
    }

    pub(super) fn wasm_table_init(
        &mut self,
        table: Value,
        source: Value,
        output: u64,
        input: u64,
        length: u64,
    ) -> Result<(), JsError> {
        let values = if source == Value::UNDEFINED {
            &[][..]
        } else {
            match self.heap.get(source) {
                Some(Cell::WasmElements(values)) => values.as_slice(),
                _ => return Err(JsError::validation("invalid Wasm element binding".into())),
            }
        };
        let input = table_range(input, length, values.len())?;
        let output = table_range(output, length, self.wasm_table_elements(table)?.len())?;
        let values = values[input].to_vec();
        let targets: Vec<_> = values
            .iter()
            .map(|value| self.wasm_element_target(*value))
            .collect();
        let Some(Cell::WasmTable { elements, .. }) = self.heap.get_mut(table) else {
            unreachable!()
        };
        elements.copy_in(output.start, &values, &targets);
        Ok(())
    }

    pub(super) fn wasm_table_copy(
        &mut self,
        destination: Value,
        source: Value,
        output: u64,
        input: u64,
        length: u64,
    ) -> Result<(), JsError> {
        let facts = |table| match self.heap.get(table) {
            Some(Cell::WasmTable {
                element_type,
                declarations,
                ..
            }) => Ok((*element_type, declarations)),
            _ => Err(JsError::validation("invalid Wasm table binding".into())),
        };
        let (source_type, source_owner) = facts(source)?;
        let (target_type, target_owner) = facts(destination)?;
        if !source_owner.reference_subtype(source_type, target_owner, target_type) {
            return Err(JsError::validation(
                "incompatible Wasm table copy types".into(),
            ));
        }
        // Both ranges are checked before writes, including empty copies.
        let output = table_range(output, length, self.wasm_table_elements(destination)?.len())?;
        let input = table_range(input, length, self.wasm_table_elements(source)?.len())?;
        if destination == source {
            let Some(Cell::WasmTable { elements, .. }) = self.heap.get_mut(destination) else {
                unreachable!()
            };
            elements.copy_within(input, output.start);
        } else {
            let Some((
                Cell::WasmTable {
                    elements: destination_elements,
                    ..
                },
                Cell::WasmTable {
                    elements: source_elements,
                    ..
                },
            )) = self.heap.get_mut_and_ref(destination, source)
            else {
                unreachable!()
            };
            destination_elements.copy_from(output, source_elements, input);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod reference_boundary_tests;

#[cfg(test)]
mod element_owner_tests;
