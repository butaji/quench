//! Linking a module's imports at instantiation: each import's root names a value that must be of
//! the external kind the module declares, with a type that matches the declaration. A kind's
//! list holds the linked value where the module imports that entry and `None` where it defines it.
use super::*;

/// The error of an import whose type does not match its declaration.
fn incompatible_import() -> JsError {
    JsError::wasm_link("incompatible import type")
}

/// Whether an imported global of type `actual` under `declarations` satisfies the type `ty` the
/// module declares under `declared`: a subtype, and for a mutable global also a supertype.
fn global_types_match(declared: &crate::WasmTypes, (actual, declarations): (crate::WasmType, &crate::WasmTypes), (ty, mutable): (crate::WasmType, bool)) -> bool {
    declarations.value_subtype(actual, declared, ty) && (!mutable || declared.value_subtype(ty, declarations, actual))
}

impl Vm {
    /// The value import `index` of `imports` names, for an import of `kind`.
    fn wasm_import_value(&self, imports: &[RootId], index: u32, kind: &str) -> Result<Value, JsError> {
        self.root_value(imports[index as usize])
            .ok_or_else(|| JsError::wasm_link(format!("invalid Wasm {kind} import root")))
    }

    pub(super) fn link_wasm_table_imports(&self, module: &crate::WasmModule, imports: &[RootId]) -> Result<Vec<Option<Value>>, JsError> {
        module
            .tables
            .iter()
            .map(|table| {
                let crate::WasmTableInitializer::Import(name) = &table.initializer else {
                    return Ok(None);
                };
                let value = self.wasm_import_value(imports, name.index, "table")?;
                let Some(Cell::WasmTable { elements, element_type, declarations, table64, .. }) = self.heap.get(value) else {
                    return Err(JsError::wasm_link("Wasm table import has wrong external kind"));
                };
                let declared = &module.signatures.declarations;
                let reference_types_match = declarations.reference_subtype(*element_type, declared, table.ty.element_type)
                    && declared.reference_subtype(table.ty.element_type, declarations, *element_type);
                let limits_match = crate::wasm::import_limits_match(elements.len() as u64, elements.maximum, table.ty.initial, table.ty.maximum);
                if *table64 != table.ty.table64 || !reference_types_match || !limits_match {
                    return Err(incompatible_import());
                }
                Ok(Some(value))
            })
            .collect()
    }

    pub(super) fn link_wasm_memory_imports(&self, module: &crate::WasmModule, imports: &[RootId]) -> Result<Vec<Option<Value>>, JsError> {
        module
            .memories
            .iter()
            .map(|memory| {
                let Some(name) = &memory.import else {
                    return Ok(None);
                };
                let value = self.wasm_import_value(imports, name.index, "memory")?;
                let Some(Cell::WasmMemory { bytes, header, .. }) = self.heap.get(value) else {
                    return Err(JsError::wasm_link("Wasm memory import has wrong external kind"));
                };
                let ty = &header.ty;
                let limits_match = crate::wasm::import_limits_match((bytes.len() / ty.page_size() as usize) as u64, ty.maximum, memory.ty.initial, memory.ty.maximum);
                let same_kind = ty.shared == memory.ty.shared && ty.index_type() == memory.ty.index_type() && ty.page_size() == memory.ty.page_size();
                if !same_kind || !limits_match {
                    return Err(incompatible_import());
                }
                Ok(Some(value))
            })
            .collect()
    }

    pub(super) fn link_wasm_global_imports(&self, module: &crate::WasmModule, imports: &[RootId]) -> Result<Vec<Option<Value>>, JsError> {
        module
            .globals
            .iter()
            .map(|global| {
                let crate::WasmGlobalInitializer::Import { ty, name } = &global.initial else {
                    return Ok(None);
                };
                let value = self.wasm_import_value(imports, name.index, "global")?;
                match self.heap.get(value) {
                    Some(Cell::WasmGlobal { ty: actual, header, mutable, .. })
                        if *mutable == global.mutable && global_types_match(&module.signatures.declarations, (*actual, &header.declarations), (*ty, *mutable)) =>
                    {
                        Ok(Some(value))
                    }
                    _ => Err(incompatible_import()),
                }
            })
            .collect()
    }

    pub(super) fn link_wasm_function_imports(&self, module: &crate::WasmModule, imports: &[RootId]) -> Result<Vec<Value>, JsError> {
        module
            .signatures
            .imports()
            .iter()
            .enumerate()
            .map(|(index, import)| {
                let value = self.wasm_import_value(imports, import.name.index, "function")?;
                let expected = module
                    .signatures
                    .function_type_index(index)
                    .ok_or_else(|| JsError::validation("invalid Wasm import type index".into()))?;
                if self.wasm_callable_matches(value, &module.signatures, expected, import.exact) != Some(true) {
                    return Err(incompatible_import());
                }
                Ok(value)
            })
            .collect()
    }

    pub(super) fn link_wasm_tag_imports(&self, module: &crate::WasmModule, imports: &[RootId]) -> Result<Vec<Option<Value>>, JsError> {
        module
            .tags
            .iter()
            .map(|tag| {
                let Some(name) = &tag.import else {
                    return Ok(None);
                };
                let value = self.wasm_import_value(imports, name.index, "tag")?;
                let Some(Cell::WasmTag { declarations, ty, .. }) = self.heap.get(value) else {
                    return Err(incompatible_import());
                };
                if !declarations.equivalent(*ty as usize, &module.signatures.declarations, tag.ty as usize) {
                    return Err(incompatible_import());
                }
                Ok(Some(value))
            })
            .collect()
    }
}
