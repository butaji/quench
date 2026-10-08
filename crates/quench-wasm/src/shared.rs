//! Decode validated modules for the shared VM. There is no executor here.

use crate::{Error, Module};
use rqj::WasmValue;
use rqj::{
    Engine, WasmElementSegment, WasmFunction, WasmFunctionBody, WasmFunctionRef, WasmGcDescriptor,
    WasmGcField, WasmGcInitialObject, WasmGcType, WasmI32Function, WasmMemoryId, WasmMemoryInit,
    WasmSignature, WasmTableId, WasmTableInit, WasmType,
};
use std::collections::{HashMap, HashSet};
use wasmparser::{
    CompositeInnerType, Encoding, ExternalKind, FuncType, Operator, Parser, Payload, TypeRef,
    ValType,
};

impl Module {
    pub(crate) fn export_names(&self) -> Result<HashSet<String>, Error> {
        let mut names = HashSet::new();
        for payload in Parser::new(0).parse_all(self.bytes()) {
            if let Payload::ExportSection(reader) = payload.map_err(parse_error)? {
                for item in reader {
                    names.insert(item.map_err(parse_error)?.name.to_owned());
                }
            }
        }
        Ok(names)
    }

    pub(crate) fn tag_link_metadata(
        &self,
    ) -> Result<
        (
            HashMap<String, WasmSignature>,
            Vec<(String, String, WasmSignature)>,
            HashMap<String, String>,
            Vec<(String, String, String)>,
        ),
        Error,
    > {
        let mut types = Vec::new();
        let mut type_fingerprints = Vec::new();
        let mut tag_types = Vec::new();
        let mut imports = Vec::new();
        let mut exports = Vec::new();
        for payload in Parser::new(0).parse_all(self.bytes()) {
            match payload.map_err(parse_error)? {
                Payload::TypeSection(reader) => {
                    let (slots, _, _, _, _, fingerprints) = function_type_slots(reader)?;
                    types.extend(slots.iter().map(shared_signature));
                    type_fingerprints.extend(fingerprints);
                }
                Payload::ImportSection(reader) => {
                    for item in reader.into_imports() {
                        let item = item.map_err(parse_error)?;
                        if let TypeRef::Tag(tag) = item.ty {
                            imports.push((
                                item.module.to_owned(),
                                item.name.to_owned(),
                                tag.func_type_idx,
                            ));
                            tag_types.push(tag.func_type_idx);
                        }
                    }
                }
                Payload::TagSection(reader) => {
                    for tag in reader {
                        tag_types.push(tag.map_err(parse_error)?.func_type_idx);
                    }
                }
                Payload::ExportSection(reader) => {
                    for item in reader {
                        let item = item.map_err(parse_error)?;
                        if item.kind == ExternalKind::Tag {
                            exports.push((item.name.to_owned(), item.index));
                        }
                    }
                }
                _ => {}
            }
        }
        let tag_export_fingerprints = exports
            .iter()
            .filter_map(|(name, index)| {
                let type_index = *tag_types.get(*index as usize)?;
                Some((
                    name.clone(),
                    type_fingerprints.get(type_index as usize)?.clone(),
                ))
            })
            .collect();
        let tag_import_fingerprints = imports
            .iter()
            .filter_map(|(module, name, type_index)| {
                Some((
                    module.clone(),
                    name.clone(),
                    type_fingerprints.get(*type_index as usize)?.clone(),
                ))
            })
            .collect();
        let tag_exports = exports
            .into_iter()
            .filter_map(|(name, index)| {
                let type_index = *tag_types.get(index as usize)?;
                Some((name, types.get(type_index as usize)?.as_ref()?.clone()))
            })
            .collect();
        let tag_imports = imports
            .into_iter()
            .filter_map(|(module, name, type_index)| {
                Some((
                    module,
                    name,
                    types.get(type_index as usize)?.as_ref()?.clone(),
                ))
            })
            .collect();
        Ok((
            tag_exports,
            tag_imports,
            tag_export_fingerprints,
            tag_import_fingerprints,
        ))
    }

    pub(crate) fn tag_link_indices(
        &self,
    ) -> Result<(Vec<(String, String, u32)>, HashMap<String, u32>), Error> {
        let mut imports = Vec::new();
        let mut exports = HashMap::new();
        let mut next_tag = 0u32;
        for payload in Parser::new(0).parse_all(self.bytes()) {
            match payload.map_err(parse_error)? {
                Payload::ImportSection(reader) => {
                    for item in reader.into_imports() {
                        let item = item.map_err(parse_error)?;
                        if matches!(item.ty, TypeRef::Tag(_)) {
                            imports.push((item.module.to_owned(), item.name.to_owned(), next_tag));
                            next_tag = next_tag.checked_add(1).ok_or_else(|| {
                                Error::Unsupported("too many Wasm exception tags".into())
                            })?;
                        }
                    }
                }
                Payload::TagSection(reader) => {
                    let count = u32::try_from(reader.count())
                        .map_err(|_| Error::Unsupported("too many Wasm exception tags".into()))?;
                    next_tag = next_tag
                        .checked_add(count)
                        .ok_or_else(|| Error::Unsupported("too many Wasm exception tags".into()))?;
                }
                Payload::ExportSection(reader) => {
                    for item in reader {
                        let item = item.map_err(parse_error)?;
                        if item.kind == ExternalKind::Tag {
                            exports.insert(item.name.to_owned(), item.index);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok((imports, exports))
    }

    pub(crate) fn table_link_metadata(
        &self,
    ) -> Result<
        (
            Vec<TableImport>,
            HashMap<String, u32>,
            HashMap<String, TableImport>,
        ),
        Error,
    > {
        let mut imports = Vec::new();
        let mut exports = HashMap::new();
        let mut tables = Vec::new();
        let mut type_canonicals = Vec::new();
        for payload in Parser::new(0).parse_all(self.bytes()) {
            match payload.map_err(parse_error)? {
                Payload::TypeSection(reader) => {
                    let (_, _, canonicals, _, _, _) = function_type_slots(reader)?;
                    type_canonicals.extend(canonicals);
                }
                Payload::ImportSection(reader) => {
                    for item in reader.into_imports() {
                        let item = item.map_err(parse_error)?;
                        if let TypeRef::Table(ty) = item.ty {
                            let mut table = table_link_type(ty, &type_canonicals)?;
                            table.module = item.module.to_owned();
                            table.name = item.name.to_owned();
                            imports.push(table.clone());
                            tables.push(table);
                        }
                    }
                }
                Payload::TableSection(reader) => {
                    for table in reader {
                        tables.push(table_link_type(
                            table.map_err(parse_error)?.ty,
                            &type_canonicals,
                        )?);
                    }
                }
                Payload::ExportSection(reader) => {
                    for item in reader {
                        let item = item.map_err(parse_error)?;
                        if item.kind == ExternalKind::Table {
                            exports.insert(item.name.to_owned(), item.index);
                        }
                    }
                }
                _ => {}
            }
        }
        let export_types = exports
            .iter()
            .filter_map(|(name, index)| Some((name.clone(), tables.get(*index as usize)?.clone())))
            .collect();
        Ok((imports, exports, export_types))
    }

    pub(crate) fn memory_link_metadata(
        &self,
    ) -> Result<(Vec<MemoryImport>, HashMap<String, u32>), Error> {
        let mut imports = Vec::new();
        let mut exports = HashMap::new();
        for payload in Parser::new(0).parse_all(self.bytes()) {
            match payload.map_err(parse_error)? {
                Payload::ImportSection(reader) => {
                    for item in reader.into_imports() {
                        let item = item.map_err(parse_error)?;
                        if let TypeRef::Memory(ty) = item.ty {
                            let limits = memory_type(ty)?;
                            imports.push(MemoryImport {
                                module: item.module.to_owned(),
                                name: item.name.to_owned(),
                                initial_pages: limits.initial_pages,
                                maximum_pages: limits.maximum_pages,
                                memory64: limits.memory64,
                                shared: limits.shared,
                                page_size_log2: limits.page_size_log2,
                            });
                        }
                    }
                }
                Payload::ExportSection(reader) => {
                    for item in reader {
                        let item = item.map_err(parse_error)?;
                        if item.kind == ExternalKind::Memory {
                            exports.insert(item.name.to_owned(), item.index);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok((imports, exports))
    }

    pub(crate) fn global_link_metadata(
        &self,
    ) -> Result<(Vec<GlobalImport>, HashMap<String, String>), Error> {
        let mut type_fingerprints = Vec::new();
        let mut globals = Vec::new();
        let mut imports = Vec::new();
        let mut exports = Vec::new();
        for payload in Parser::new(0).parse_all(self.bytes()) {
            match payload.map_err(parse_error)? {
                Payload::TypeSection(reader) => {
                    let (_, _, _, _, _, fingerprints) = function_type_slots(reader)?;
                    type_fingerprints.extend(fingerprints);
                }
                Payload::ImportSection(reader) => {
                    for import in reader.into_imports() {
                        let import = import.map_err(parse_error)?;
                        if let TypeRef::Global(ty) = import.ty {
                            let index = globals.len();
                            globals.push((ty.content_type, ty.mutable));
                            imports.push((import.module.to_owned(), import.name.to_owned(), index));
                        }
                    }
                }
                Payload::GlobalSection(reader) => {
                    for global in reader {
                        let global = global.map_err(parse_error)?;
                        globals.push((global.ty.content_type, global.ty.mutable));
                    }
                }
                Payload::ExportSection(reader) => {
                    for export in reader {
                        let export = export.map_err(parse_error)?;
                        if export.kind == ExternalKind::Global {
                            exports.push((export.name.to_owned(), export.index));
                        }
                    }
                }
                _ => {}
            }
        }
        let fingerprint = |index: usize| -> Option<String> {
            let (ty, mutable) = *globals.get(index)?;
            Some(format!(
                "{};mutable:{mutable}",
                value_type_link_fingerprint(ty, &type_fingerprints)
            ))
        };
        let imports = imports
            .into_iter()
            .filter_map(|(module, name, index)| {
                Some(GlobalImport {
                    module,
                    name,
                    ty: scalar_type(globals.get(index)?.0).ok()?,
                    mutable: globals[index].1,
                    fingerprint: fingerprint(index)?,
                })
            })
            .collect();
        let exports = exports
            .into_iter()
            .filter_map(|(name, index)| Some((name, fingerprint(index as usize)?)))
            .collect();
        Ok((imports, exports))
    }

    /// Return scalar function link metadata without requiring the module to be
    /// executable by the current shared loader. The WAST linker uses this to
    /// diagnose missing and type-incompatible function imports precisely.
    pub(crate) fn function_link_metadata(
        &self,
    ) -> Result<
        (
            HashMap<String, WasmSignature>,
            Vec<(String, String, WasmSignature)>,
            HashMap<String, String>,
            Vec<(String, String, String)>,
        ),
        Error,
    > {
        let mut types = Vec::new();
        let mut type_fingerprints = Vec::new();
        let mut function_types = Vec::new();
        let mut function_imports = Vec::new();
        let mut exports = Vec::new();
        for payload in Parser::new(0).parse_all(self.bytes()) {
            match payload.map_err(parse_error)? {
                Payload::TypeSection(reader) => {
                    let (slots, _, _, _, _, fingerprints) = function_type_slots(reader)?;
                    for ty in slots {
                        types.push(shared_signature(&ty));
                    }
                    type_fingerprints.extend(fingerprints);
                }
                Payload::ImportSection(reader) => {
                    for item in reader.into_imports() {
                        let item = item.map_err(parse_error)?;
                        if let TypeRef::Func(type_index) | TypeRef::FuncExact(type_index) = item.ty
                        {
                            function_types.push(type_index);
                            function_imports.push((
                                item.module.to_owned(),
                                item.name.to_owned(),
                                type_index,
                            ));
                        }
                    }
                }
                Payload::FunctionSection(reader) => {
                    for type_index in reader {
                        function_types.push(type_index.map_err(parse_error)?);
                    }
                }
                Payload::ExportSection(reader) => {
                    for item in reader {
                        let item = item.map_err(parse_error)?;
                        if item.kind == ExternalKind::Func {
                            exports.push((item.name.to_owned(), item.index));
                        }
                    }
                }
                _ => {}
            }
        }
        let function_export_fingerprints = exports
            .iter()
            .filter_map(|(name, function_index)| {
                let type_index = *function_types.get(*function_index as usize)?;
                Some((
                    name.clone(),
                    type_fingerprints.get(type_index as usize)?.clone(),
                ))
            })
            .collect();
        let function_import_fingerprints = function_imports
            .iter()
            .filter_map(|(module, name, type_index)| {
                Some((
                    module.clone(),
                    name.clone(),
                    type_fingerprints.get(*type_index as usize)?.clone(),
                ))
            })
            .collect();
        let function_exports = exports
            .into_iter()
            .filter_map(|(name, function_index)| {
                let type_index = *function_types.get(function_index as usize)?;
                let signature = types.get(type_index as usize)?.as_ref()?.clone();
                Some((name, signature))
            })
            .collect();
        let function_imports = function_imports
            .into_iter()
            .filter_map(|(module, name, type_index)| {
                Some((
                    module,
                    name,
                    types.get(type_index as usize)?.as_ref()?.clone(),
                ))
            })
            .collect();
        Ok((
            function_exports,
            function_imports,
            function_export_fingerprints,
            function_import_fingerprints,
        ))
    }

    /// Lower i32 module functions with one selected exported entry into the
    /// JavaScript VM's residual bytecode. Stateful sections and unsupported operators fail
    /// explicitly until their shared lowering is implemented.
    pub fn lower_shared_i32(&self, export: &str) -> Result<WasmI32Function, Error> {
        let function = self.lower_shared(export)?;
        if function
            .signature()
            .params
            .iter()
            .any(|ty| *ty != WasmType::I32)
            || function
                .signature()
                .result
                .is_some_and(|ty| ty != WasmType::I32)
            || !function.signature().additional_results.is_empty()
        {
            return Err(Error::Unsupported(
                "function requires the typed Wasm boundary".into(),
            ));
        }
        Ok(function)
    }

    /// Lower validated scalar module functions into the single shared VM.
    pub fn lower_shared(&self, export: &str) -> Result<WasmFunction, Error> {
        self.lower_shared_with_entry(Some(export), &[], &[], &[], &[], &[])
            .map(|(module, _, _)| module)
    }

    /// Lower a stateless scalar module and retain its function export map.
    pub fn lower_shared_module(&self) -> Result<(WasmFunction, HashMap<String, u32>), Error> {
        let (module, exports, _) = self.lower_shared_with_entry(None, &[], &[], &[], &[], &[])?;
        Ok((module, exports))
    }

    pub(crate) fn lower_shared_module_with_imports(
        &self,
        imported_globals: &[(WasmValue, bool)],
        imported_functions: &[Option<WasmFunctionRef>],
        imported_tables: &[Option<WasmTableId>],
        imported_memories: &[Option<WasmMemoryId>],
        imported_tags: &[Option<rqj::WasmTagId>],
    ) -> Result<
        (
            WasmFunction,
            HashMap<String, u32>,
            HashMap<String, (u32, WasmType, bool)>,
        ),
        Error,
    > {
        self.lower_shared_with_entry(
            None,
            imported_globals,
            imported_functions,
            imported_tables,
            imported_memories,
            imported_tags,
        )
    }

    fn lower_shared_with_entry(
        &self,
        entry_name: Option<&str>,
        imported_globals: &[(WasmValue, bool)],
        imported_function_refs: &[Option<WasmFunctionRef>],
        imported_table_refs: &[Option<WasmTableId>],
        imported_memory_refs: &[Option<WasmMemoryId>],
        imported_tag_refs: &[Option<rqj::WasmTagId>],
    ) -> Result<
        (
            WasmFunction,
            HashMap<String, u32>,
            HashMap<String, (u32, WasmType, bool)>,
        ),
        Error,
    > {
        let mut types = Vec::new();
        let mut gc_supertypes = Vec::new();
        let mut gc_type_canonicals = Vec::new();
        let mut gc_type_fingerprints = Vec::new();
        let mut gc_types = Vec::new();
        let mut gc_descriptors = Vec::new();
        let mut gc_initial_objects = Vec::new();
        let mut functions = Vec::new();
        let mut function_import_types = Vec::new();
        let mut tag_signatures = Vec::new();
        let mut tag_import_count = 0usize;
        let mut bodies = Vec::new();
        let mut globals = Vec::new();
        let mut memories: Vec<WasmMemoryInit> = Vec::new();
        let mut tables: Vec<WasmTableInit> = Vec::new();
        let mut table_import_count = 0usize;
        let mut memory_import_count = 0usize;
        let mut element_segments = Vec::new();
        let mut data_segments = Vec::new();
        let mut global_import_count = 0usize;
        let mut exports = HashMap::new();
        let mut global_exports = Vec::new();
        let mut start_function = None;
        for payload in Parser::new(0).parse_all(self.bytes()) {
            match payload.map_err(parse_error)? {
                Payload::Version {
                    encoding: Encoding::Module,
                    ..
                }
                | Payload::CodeSectionStart { .. }
                | Payload::CustomSection(_)
                | Payload::End(_) => {}
                Payload::TypeSection(reader) => {
                    let (slots, supertypes, canonicals, layouts, descriptors, fingerprints) =
                        function_type_slots(reader)?;
                    types.extend(slots);
                    gc_supertypes.extend(supertypes);
                    gc_type_canonicals.extend(canonicals);
                    gc_types.extend(layouts);
                    gc_descriptors.extend(descriptors);
                    gc_type_fingerprints.extend(fingerprints);
                }
                Payload::FunctionSection(reader) => {
                    for ty in reader {
                        functions.push(ty.map_err(parse_error)?);
                    }
                }
                Payload::ImportSection(reader) => {
                    for import in reader.into_imports() {
                        let import = import.map_err(parse_error)?;
                        match import.ty {
                            TypeRef::Func(type_index) | TypeRef::FuncExact(type_index) => {
                                function_import_types.push(type_index);
                            }
                            TypeRef::Table(ty) => {
                                let (element_type, initial_size, maximum_size, table64) =
                                    table_type(ty)?;
                                tables.push(WasmTableInit {
                                    initial_size,
                                    maximum_size,
                                    table64,
                                    element_type,
                                    initial_value: null_reference(element_type),
                                    elements: Vec::new(),
                                });
                                table_import_count += 1;
                            }
                            TypeRef::Memory(ty) => {
                                memories.push(memory_type(ty)?);
                                memory_import_count += 1;
                            }
                            TypeRef::Global(ty) => {
                                let (value, mutable) = imported_globals
                                    .get(global_import_count)
                                    .copied()
                                    .ok_or_else(|| {
                                        Error::Unsupported(
                                            "missing imported Wasm global value".into(),
                                        )
                                    })?;
                                if value.ty() != scalar_type(ty.content_type)?
                                    || mutable != ty.mutable
                                {
                                    return Err(Error::Unsupported(
                                        "incompatible imported Wasm global".into(),
                                    ));
                                }
                                global_import_count += 1;
                                globals.push((value, mutable));
                            }
                            TypeRef::Tag(tag) => {
                                let signature = types
                                    .get(tag.func_type_idx as usize)
                                    .and_then(shared_signature)
                                    .ok_or_else(|| {
                                        Error::Unsupported(
                                            "unsupported Wasm exception tag type".into(),
                                        )
                                    })?;
                                tag_signatures.push(signature);
                                tag_import_count += 1;
                            }
                        }
                    }
                }
                Payload::GlobalSection(reader) => {
                    for global in reader {
                        let global = global.map_err(parse_error)?;
                        let ty = scalar_type(global.ty.content_type)?;
                        let value = evaluate_const_expr(
                            &global.init_expr,
                            &globals,
                            &gc_types,
                            &mut gc_initial_objects,
                        )?;
                        let value = coerce_null_reference(value, ty);
                        if !const_value_assignable(value, ty) {
                            return Err(Error::Unsupported(
                                "Wasm global initializer type mismatch".into(),
                            ));
                        }
                        globals.push((value, global.ty.mutable));
                    }
                }
                Payload::TagSection(reader) => {
                    for item in reader {
                        let tag = item.map_err(parse_error)?;
                        let signature = types
                            .get(tag.func_type_idx as usize)
                            .and_then(shared_signature)
                            .ok_or_else(|| {
                                Error::Unsupported("unsupported Wasm exception tag type".into())
                            })?;
                        tag_signatures.push(signature);
                    }
                }
                Payload::MemorySection(reader) => {
                    for memory in reader {
                        let memory = memory.map_err(parse_error)?;
                        memories.push(memory_type(memory)?);
                    }
                }
                Payload::TableSection(reader) => {
                    for item in reader {
                        let table = item.map_err(parse_error)?;
                        let element_type = WasmType::from_ref_type(table.ty.element_type)
                            .ok_or_else(|| {
                                Error::Unsupported(
                                    "module requires an unsupported Wasm table type".into(),
                                )
                            })?;
                        let initial_size = table.ty.initial;
                        let maximum_size = table.ty.maximum;
                        let initial_value = match table.init {
                            wasmparser::TableInit::RefNull => null_reference(element_type),
                            wasmparser::TableInit::Expr(expression) => {
                                let value = evaluate_const_expr(
                                    &expression,
                                    &globals,
                                    &gc_types,
                                    &mut gc_initial_objects,
                                )?;
                                let value = coerce_null_reference(value, element_type);
                                if !const_value_assignable(value, element_type) {
                                    return Err(Error::Unsupported(
                                        "unsupported Wasm table initializer".into(),
                                    ));
                                }
                                value
                            }
                        };
                        tables.push(WasmTableInit {
                            initial_size,
                            maximum_size,
                            table64: table.ty.table64,
                            element_type,
                            initial_value,
                            elements: Vec::new(),
                        });
                    }
                }
                Payload::ElementSection(reader) => {
                    for segment in reader {
                        let segment = segment.map_err(parse_error)?;
                        let (element_type, values) = evaluate_element_items(
                            segment.items,
                            &globals,
                            &gc_types,
                            &mut gc_initial_objects,
                        )?;
                        match segment.kind {
                            wasmparser::ElementKind::Active {
                                table_index,
                                offset_expr,
                            } => {
                                let table_index = table_index.unwrap_or(0);
                                let table =
                                    tables.get_mut(table_index as usize).ok_or_else(|| {
                                        Error::Unsupported(
                                            "Wasm element table index out of bounds".into(),
                                        )
                                    })?;
                                if !reference_type_assignable(element_type, table.element_type) {
                                    return Err(Error::Unsupported(
                                        "Wasm element type does not match its table".into(),
                                    ));
                                }
                                let offset_value = evaluate_const_expr(
                                    &offset_expr,
                                    &globals,
                                    &gc_types,
                                    &mut gc_initial_objects,
                                )?;
                                let offset = match (table.table64, offset_value) {
                                    (false, WasmValue::I32(offset)) => u64::from(offset as u32),
                                    (true, WasmValue::I64(offset)) => offset as u64,
                                    _ => {
                                        return Err(Error::Unsupported(
                                            "Wasm table segment offset has the wrong address width"
                                                .into(),
                                        ));
                                    }
                                };
                                table.elements.push((offset, values));
                                element_segments.push(WasmElementSegment {
                                    element_type,
                                    values: None,
                                });
                            }
                            wasmparser::ElementKind::Passive => {
                                element_segments.push(WasmElementSegment {
                                    element_type,
                                    values: Some(values),
                                });
                            }
                            wasmparser::ElementKind::Declared => {
                                element_segments.push(WasmElementSegment {
                                    element_type,
                                    values: None,
                                });
                            }
                        }
                    }
                }
                Payload::DataSection(reader) => {
                    for segment in reader {
                        let segment = segment.map_err(parse_error)?;
                        match segment.kind {
                            wasmparser::DataKind::Active {
                                memory_index,
                                offset_expr,
                            } => {
                                let offset_value = evaluate_const_expr(
                                    &offset_expr,
                                    &globals,
                                    &gc_types,
                                    &mut gc_initial_objects,
                                )?;
                                let memory =
                                    memories.get_mut(memory_index as usize).ok_or_else(|| {
                                        Error::Unsupported(
                                            "Wasm data segment memory index out of bounds".into(),
                                        )
                                    })?;
                                let offset = match (memory.memory64, offset_value) {
                                    (false, WasmValue::I32(offset)) => u64::from(offset as u32),
                                    (true, WasmValue::I64(offset)) => offset as u64,
                                    _ => {
                                        return Err(Error::Unsupported(
                                            "Wasm data offset has the wrong address width".into(),
                                        ));
                                    }
                                };
                                memory.data_segments.push((offset, segment.data.to_vec()));
                                data_segments.push(None);
                            }
                            wasmparser::DataKind::Passive => {
                                data_segments.push(Some(segment.data.to_vec()));
                            }
                        }
                    }
                }
                Payload::ExportSection(reader) => {
                    for item in reader {
                        let item = item.map_err(parse_error)?;
                        match item.kind {
                            ExternalKind::Func => {
                                exports.insert(item.name.to_owned(), item.index);
                            }
                            ExternalKind::Global => {
                                global_exports.push((item.name.to_owned(), item.index));
                            }
                            ExternalKind::Memory => {
                                // WAST only invokes functions and reads globals;
                                // exported memory remains runtime-owned state.
                            }
                            ExternalKind::Table => {
                                // Tables remain available to exported functions in the VM.
                            }
                            ExternalKind::Tag => {}
                            _ => {
                                return Err(Error::Unsupported(
                                    "module requires stateful exports".into(),
                                ));
                            }
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => bodies.push(body),
                Payload::StartSection { func, .. } => start_function = Some(func),
                // Declarations and initialization segments are inert in the
                // scalar subset when no supported function references or
                // exports the corresponding state. Memory/table operators
                // and state exports still fail at their specific lowering
                // boundary below; the shared VM never pretends to execute
                // those instructions.
                Payload::DataCountSection { .. } => {}
                _ => {
                    return Err(Error::Unsupported(
                        "module requires state, imports, or an unsupported section".into(),
                    ));
                }
            }
        }
        if global_import_count != imported_globals.len() {
            return Err(Error::Unsupported(
                "unused imported Wasm global value".into(),
            ));
        }
        if function_import_types.len() != imported_function_refs.len() {
            return Err(Error::Unsupported(
                "missing or unused imported Wasm function binding".into(),
            ));
        }
        if table_import_count != imported_table_refs.len() {
            return Err(Error::Unsupported(
                "missing or unused imported Wasm table binding".into(),
            ));
        }
        if memory_import_count != imported_memory_refs.len() {
            return Err(Error::Unsupported(
                "missing or unused imported Wasm memory binding".into(),
            ));
        }
        if tag_import_count != imported_tag_refs.len() {
            return Err(Error::Unsupported(
                "missing or unused imported Wasm exception tag binding".into(),
            ));
        }
        let entry = if let Some(name) = entry_name {
            *exports
                .get(name)
                .ok_or_else(|| Error::Unsupported(format!("unknown function export: {name}")))?
                as usize
        } else {
            exports.values().next().copied().unwrap_or(0) as usize
        };
        let mut inputs = Vec::with_capacity(function_import_types.len() + bodies.len());
        for type_index in &function_import_types {
            let signature = types
                .get(*type_index as usize)
                .ok_or_else(|| Error::Unsupported("function import type out of bounds".into()))?;
            let result_types = signature
                .results()
                .iter()
                .copied()
                .map(scalar_type)
                .collect::<Result<Vec<_>, _>>()?;
            let function_signature = WasmSignature {
                params: signature
                    .params()
                    .iter()
                    .copied()
                    .map(scalar_type)
                    .collect::<Result<_, _>>()?,
                result: result_types.first().copied(),
                additional_results: result_types.iter().copied().skip(1).collect(),
            };
            inputs.push(WasmFunctionBody {
                signature: function_signature,
                locals: Vec::new(),
                operators: vec![Ok(Operator::Unreachable), Ok(Operator::End)],
            });
        }
        for (&type_index, body) in functions.iter().zip(bodies) {
            let signature = &types[type_index as usize];
            u16::try_from(signature.params().len())
                .map_err(|_| Error::Unsupported("too many parameters".into()))?;
            let result_types = signature
                .results()
                .iter()
                .copied()
                .map(scalar_type)
                .collect::<Result<Vec<_>, _>>()?;
            let signature = WasmSignature {
                params: signature
                    .params()
                    .iter()
                    .copied()
                    .map(scalar_type)
                    .collect::<Result<_, _>>()?,
                result: result_types.first().copied(),
                additional_results: result_types.iter().copied().skip(1).collect(),
            };
            let mut locals = Vec::new();
            for local in body.get_locals_reader().map_err(parse_error)? {
                let (count, ty) = local.map_err(parse_error)?;
                let ty = scalar_type(ty)?;
                let count = usize::try_from(count)
                    .map_err(|_| Error::Unsupported("too many locals".into()))?;
                let length = locals
                    .len()
                    .checked_add(count)
                    .filter(|length| *length <= usize::from(u16::MAX))
                    .ok_or_else(|| Error::Unsupported("too many locals".into()))?;
                locals.resize(length, ty);
            }
            let operators = body
                .get_operators_reader()
                .map_err(parse_error)?
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .map_err(parse_error)?
                .into_iter()
                .map(Ok)
                .collect::<Vec<Result<_, wasmparser::BinaryReaderError>>>();
            inputs.push(WasmFunctionBody {
                signature,
                locals,
                operators,
            });
        }
        if inputs.is_empty() {
            inputs.push(WasmFunctionBody {
                signature: WasmSignature {
                    params: Vec::new(),
                    result: None,
                    additional_results: Vec::new(),
                },
                locals: Vec::new(),
                operators: vec![Ok(Operator::End)],
            });
        }
        let type_signatures = types
            .iter()
            .map(shared_signature)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| Error::Unsupported("module has non-scalar function types".into()))?;
        let function_type_indices = function_import_types
            .iter()
            .copied()
            .chain(functions.iter().copied())
            .collect();
        let mut module = Engine::lower_wasm_module_with_module_state(
            "wast module",
            entry as u32,
            inputs,
            &globals,
            &memories,
            &tables,
            &type_signatures,
            &gc_types,
            &element_segments,
            &data_segments,
            imported_function_refs,
            imported_table_refs,
            &tag_signatures,
        )
        .map_err(|error| Error::Unsupported(error.to_string()))?;
        if let Some(start_function) = start_function {
            module.set_start_function(start_function);
        }
        module.set_gc_type_metadata(gc_supertypes, gc_type_canonicals, gc_type_fingerprints);
        module.set_gc_descriptor_metadata(gc_descriptors);
        module.set_gc_initial_objects(gc_initial_objects);
        module.set_function_type_metadata(function_type_indices);
        module.set_memory_imports(imported_memory_refs.to_vec());
        module.set_tag_imports(imported_tag_refs.to_vec());
        let global_exports = global_exports
            .into_iter()
            .filter_map(|(name, index)| {
                globals
                    .get(index as usize)
                    .copied()
                    .map(|(value, mutable)| (name, (index, value.ty(), mutable)))
            })
            .collect();
        Ok((module, exports, global_exports))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GlobalImport {
    pub module: String,
    pub name: String,
    pub ty: WasmType,
    pub mutable: bool,
    pub fingerprint: String,
}

#[derive(Clone, Debug)]
pub(crate) struct TableImport {
    pub module: String,
    pub name: String,
    pub element_type: WasmType,
    pub element_fingerprint: String,
    pub initial_size: u64,
    pub maximum_size: Option<u64>,
    pub table64: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct MemoryImport {
    pub module: String,
    pub name: String,
    pub initial_pages: u64,
    pub maximum_pages: Option<u64>,
    pub memory64: bool,
    pub shared: bool,
    pub page_size_log2: u32,
}

fn table_type(ty: wasmparser::TableType) -> Result<(WasmType, u64, Option<u64>, bool), Error> {
    let element_type = WasmType::from_ref_type(ty.element_type).ok_or_else(|| {
        Error::Unsupported("module requires an unsupported Wasm table type".into())
    })?;
    Ok((element_type, ty.initial, ty.maximum, ty.table64))
}

fn table_link_type(
    ty: wasmparser::TableType,
    type_canonicals: &[u32],
) -> Result<TableImport, Error> {
    let element_type = WasmType::from_ref_type(ty.element_type).ok_or_else(|| {
        Error::Unsupported("module requires an unsupported Wasm table type".into())
    })?;
    Ok(TableImport {
        module: String::new(),
        name: String::new(),
        element_type,
        element_fingerprint: value_type_fingerprint(
            ValType::Ref(ty.element_type),
            0,
            0,
            type_canonicals,
        ),
        initial_size: ty.initial,
        maximum_size: ty.maximum,
        table64: ty.table64,
    })
}

fn memory_type(ty: wasmparser::MemoryType) -> Result<WasmMemoryInit, Error> {
    let page_size_log2 = ty.page_size_log2.unwrap_or(16);
    let address_bits = if ty.memory64 { 64 } else { 32 };
    let max_pages = (1u128 << address_bits) / (1u128 << page_size_log2);
    let max_pages = u64::try_from(max_pages).unwrap_or(u64::MAX);
    if ty.initial > max_pages || ty.maximum.is_some_and(|maximum| maximum > max_pages) {
        return Err(Error::Unsupported(
            "Wasm memory exceeds its address-width limit".into(),
        ));
    }
    Ok(WasmMemoryInit {
        initial_pages: ty.initial,
        maximum_pages: ty.maximum,
        memory64: ty.memory64,
        shared: ty.shared,
        page_size_log2,
        data_segments: Vec::new(),
    })
}

fn evaluate_element_items(
    items: wasmparser::ElementItems<'_>,
    globals: &[(WasmValue, bool)],
    gc_types: &[Option<WasmGcType>],
    gc_initial_objects: &mut Vec<WasmGcInitialObject>,
) -> Result<(WasmType, Vec<WasmValue>), Error> {
    match items {
        wasmparser::ElementItems::Functions(reader) => Ok((
            WasmType::FuncRef,
            reader
                .into_iter()
                .map(|index| {
                    index
                        .map(|index| WasmValue::FuncRef(Some(index)))
                        .map_err(parse_error)
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        wasmparser::ElementItems::Expressions(ref_type, reader) => {
            let element_type = WasmType::from_wasm(wasmparser::ValType::Ref(ref_type))
                .ok_or_else(|| Error::Unsupported("unsupported Wasm element type".into()))?;
            let values = reader
                .into_iter()
                .map(|expression| {
                    let value = evaluate_const_expr(
                        &expression.map_err(parse_error)?,
                        globals,
                        gc_types,
                        gc_initial_objects,
                    )?;
                    if const_value_assignable(value, element_type) {
                        Ok(value)
                    } else {
                        Err(Error::Unsupported(
                            "unsupported Wasm element expression".into(),
                        ))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok((element_type, values))
        }
    }
}

fn null_reference(ty: WasmType) -> WasmValue {
    ty.zero()
}

fn coerce_null_reference(value: WasmValue, destination: WasmType) -> WasmValue {
    if matches!(
        value,
        WasmValue::FuncRef(None)
            | WasmValue::ExternRef(None)
            | WasmValue::I31Ref(None)
            | WasmValue::ExnRef(None)
    ) && matches!(
        destination,
        WasmType::FuncRef | WasmType::ExternRef | WasmType::I31Ref | WasmType::ExnRef
    ) {
        destination.zero()
    } else {
        value
    }
}

fn reference_type_assignable(source: WasmType, destination: WasmType) -> bool {
    source == destination || (source == WasmType::I31Ref && destination == WasmType::ExternRef)
}

fn const_value_assignable(value: WasmValue, destination: WasmType) -> bool {
    reference_type_assignable(value.ty(), destination)
        || (matches!(
            value,
            WasmValue::FuncRef(None)
                | WasmValue::ExternRef(None)
                | WasmValue::I31Ref(None)
                | WasmValue::ExnRef(None)
        ) && matches!(
            destination,
            WasmType::FuncRef | WasmType::ExternRef | WasmType::I31Ref | WasmType::ExnRef
        ))
        || matches!(
            (value, destination),
            (WasmValue::FuncRef(Some(reference)), WasmType::ExternRef)
                if reference & !rqj::WASM_GC_INITIAL_REFERENCE_ID_MASK
                    == rqj::WASM_GC_INITIAL_REFERENCE_TAG
        )
}

fn evaluate_const_expr(
    expression: &wasmparser::ConstExpr,
    globals: &[(WasmValue, bool)],
    gc_types: &[Option<WasmGcType>],
    gc_initial_objects: &mut Vec<WasmGcInitialObject>,
) -> Result<WasmValue, Error> {
    let mut stack = Vec::new();
    for operator in expression.get_operators_reader() {
        let operator = operator.map_err(parse_error)?;
        match operator {
            Operator::I32Const { value } => stack.push(WasmValue::I32(value)),
            Operator::I64Const { value } => stack.push(WasmValue::I64(value)),
            Operator::F32Const { value } => stack.push(WasmValue::F32(value.bits())),
            Operator::F64Const { value } => stack.push(WasmValue::F64(value.bits())),
            Operator::V128Const { value } => stack.push(WasmValue::V128(u128::from(value))),
            Operator::RefNull { hty } => stack.push(match hty {
                wasmparser::HeapType::Abstract {
                    ty: wasmparser::AbstractHeapType::Func | wasmparser::AbstractHeapType::NoFunc,
                    ..
                }
                | wasmparser::HeapType::Concrete(_)
                | wasmparser::HeapType::Exact(_) => WasmValue::FuncRef(None),
                wasmparser::HeapType::Abstract {
                    ty: wasmparser::AbstractHeapType::Cont | wasmparser::AbstractHeapType::NoCont,
                    ..
                } => {
                    return Err(Error::Unsupported(
                        "unsupported Wasm reference constant".into(),
                    ));
                }
                wasmparser::HeapType::Abstract {
                    ty: wasmparser::AbstractHeapType::I31,
                    ..
                } => WasmValue::I31Ref(None),
                wasmparser::HeapType::Abstract { .. } => WasmValue::ExternRef(None),
            }),
            Operator::RefFunc { function_index } => {
                stack.push(WasmValue::FuncRef(Some(function_index)))
            }
            Operator::RefI31 => {
                let value = stack.pop().ok_or_else(const_expr_stack_error)?;
                let WasmValue::I32(value) = value else {
                    return Err(Error::Unsupported(
                        "invalid Wasm i31 constant expression operand".into(),
                    ));
                };
                stack.push(WasmValue::I31Ref(Some(value as u32 & 0x7fff_ffff)));
            }
            Operator::AnyConvertExtern | Operator::ExternConvertAny => {
                let value = stack.pop().ok_or_else(const_expr_stack_error)?;
                stack.push(value);
            }
            Operator::StructNew { struct_type_index } => {
                let Some(Some(WasmGcType::Struct(fields))) =
                    gc_types.get(struct_type_index as usize)
                else {
                    return Err(Error::Unsupported("invalid constant struct type".into()));
                };
                let mut values = Vec::new();
                values
                    .try_reserve_exact(fields.len())
                    .map_err(|_| Error::Unsupported("constant struct is too large".into()))?;
                for _ in fields {
                    values.push(stack.pop().ok_or_else(const_expr_stack_error)?);
                }
                values.reverse();
                stack.push(allocate_gc_constant(
                    gc_initial_objects,
                    struct_type_index,
                    values,
                )?);
            }
            Operator::StructNewDesc { struct_type_index } => {
                let descriptor = stack.pop().ok_or_else(const_expr_stack_error)?;
                let Some(Some(WasmGcType::Struct(fields))) =
                    gc_types.get(struct_type_index as usize)
                else {
                    return Err(Error::Unsupported("invalid constant struct type".into()));
                };
                let mut values = Vec::new();
                values
                    .try_reserve_exact(fields.len())
                    .map_err(|_| Error::Unsupported("constant struct is too large".into()))?;
                for _ in fields {
                    values.push(stack.pop().ok_or_else(const_expr_stack_error)?);
                }
                values.reverse();
                stack.push(allocate_gc_constant(
                    gc_initial_objects,
                    struct_type_index,
                    values,
                )?);
                gc_initial_objects
                    .last_mut()
                    .expect("allocated GC constant")
                    .descriptor = Some(descriptor);
            }
            Operator::StructNewDefault { struct_type_index } => {
                let Some(Some(WasmGcType::Struct(fields))) =
                    gc_types.get(struct_type_index as usize)
                else {
                    return Err(Error::Unsupported("invalid constant struct type".into()));
                };
                let values = fields.iter().map(|field| field.ty.zero()).collect();
                stack.push(allocate_gc_constant(
                    gc_initial_objects,
                    struct_type_index,
                    values,
                )?);
            }
            Operator::StructNewDefaultDesc { struct_type_index } => {
                let descriptor = stack.pop().ok_or_else(const_expr_stack_error)?;
                let Some(Some(WasmGcType::Struct(fields))) =
                    gc_types.get(struct_type_index as usize)
                else {
                    return Err(Error::Unsupported("invalid constant struct type".into()));
                };
                let values = fields.iter().map(|field| field.ty.zero()).collect();
                stack.push(allocate_gc_constant(
                    gc_initial_objects,
                    struct_type_index,
                    values,
                )?);
                gc_initial_objects
                    .last_mut()
                    .expect("allocated GC constant")
                    .descriptor = Some(descriptor);
            }
            Operator::ArrayNew { array_type_index } => {
                let length = const_array_length(stack.pop().ok_or_else(const_expr_stack_error)?)?;
                let initial = stack.pop().ok_or_else(const_expr_stack_error)?;
                let mut values = Vec::new();
                values
                    .try_reserve_exact(length)
                    .map_err(|_| Error::Unsupported("constant array is too large".into()))?;
                values.resize(length, initial);
                stack.push(allocate_gc_constant(
                    gc_initial_objects,
                    array_type_index,
                    values,
                )?);
            }
            Operator::ArrayNewDefault { array_type_index } => {
                let length = const_array_length(stack.pop().ok_or_else(const_expr_stack_error)?)?;
                let Some(Some(WasmGcType::Array(field))) = gc_types.get(array_type_index as usize)
                else {
                    return Err(Error::Unsupported("invalid constant array type".into()));
                };
                let mut values = Vec::new();
                values
                    .try_reserve_exact(length)
                    .map_err(|_| Error::Unsupported("constant array is too large".into()))?;
                values.resize(length, field.ty.zero());
                stack.push(allocate_gc_constant(
                    gc_initial_objects,
                    array_type_index,
                    values,
                )?);
            }
            Operator::ArrayNewFixed {
                array_type_index,
                array_size,
            } => {
                let count = usize::try_from(array_size)
                    .map_err(|_| Error::Unsupported("constant array is too large".into()))?;
                let mut values = Vec::new();
                values
                    .try_reserve_exact(count)
                    .map_err(|_| Error::Unsupported("constant array is too large".into()))?;
                for _ in 0..count {
                    values.push(stack.pop().ok_or_else(const_expr_stack_error)?);
                }
                values.reverse();
                stack.push(allocate_gc_constant(
                    gc_initial_objects,
                    array_type_index,
                    values,
                )?);
            }
            Operator::GlobalGet { global_index } => {
                let (value, mutable) =
                    globals.get(global_index as usize).copied().ok_or_else(|| {
                        Error::Unsupported(
                            "Wasm constant expression global is out of bounds".into(),
                        )
                    })?;
                if mutable {
                    return Err(Error::Unsupported(
                        "Wasm constant expression reads a mutable global".into(),
                    ));
                }
                stack.push(value);
            }
            Operator::I32Add
            | Operator::I32Sub
            | Operator::I32Mul
            | Operator::I64Add
            | Operator::I64Sub
            | Operator::I64Mul
            | Operator::F32Add
            | Operator::F64Add => {
                let right = stack.pop().ok_or_else(const_expr_stack_error)?;
                let left = stack.pop().ok_or_else(const_expr_stack_error)?;
                stack.push(evaluate_const_binary(operator, left, right)?);
            }
            Operator::End => break,
            _ => {
                return Err(Error::Unsupported(
                    "unsupported Wasm constant expression".into(),
                ));
            }
        }
    }
    match stack.as_slice() {
        [value] => Ok(*value),
        _ => Err(Error::Unsupported(
            "invalid Wasm constant expression stack".into(),
        )),
    }
}

fn const_array_length(value: WasmValue) -> Result<usize, Error> {
    let WasmValue::I32(length) = value else {
        return Err(Error::Unsupported("invalid constant array length".into()));
    };
    usize::try_from(length as u32)
        .map_err(|_| Error::Unsupported("constant array is too large".into()))
}

fn allocate_gc_constant(
    gc_initial_objects: &mut Vec<WasmGcInitialObject>,
    type_index: u32,
    values: Vec<WasmValue>,
) -> Result<WasmValue, Error> {
    let id = u32::try_from(gc_initial_objects.len() + 1)
        .ok()
        .filter(|id| *id <= rqj::WASM_GC_INITIAL_REFERENCE_ID_MASK)
        .ok_or_else(|| Error::Unsupported("too many constant Wasm GC objects".into()))?;
    let reference = rqj::WASM_GC_INITIAL_REFERENCE_TAG | id;
    gc_initial_objects.push(WasmGcInitialObject {
        reference,
        type_index,
        values,
        descriptor: None,
    });
    Ok(WasmValue::FuncRef(Some(reference)))
}

fn evaluate_const_binary(
    operator: Operator<'_>,
    left: WasmValue,
    right: WasmValue,
) -> Result<WasmValue, Error> {
    let result = match (operator, left, right) {
        (Operator::I32Add, WasmValue::I32(left), WasmValue::I32(right)) => {
            WasmValue::I32(left.wrapping_add(right))
        }
        (Operator::I32Sub, WasmValue::I32(left), WasmValue::I32(right)) => {
            WasmValue::I32(left.wrapping_sub(right))
        }
        (Operator::I32Mul, WasmValue::I32(left), WasmValue::I32(right)) => {
            WasmValue::I32(left.wrapping_mul(right))
        }
        (Operator::I64Add, WasmValue::I64(left), WasmValue::I64(right)) => {
            WasmValue::I64(left.wrapping_add(right))
        }
        (Operator::I64Sub, WasmValue::I64(left), WasmValue::I64(right)) => {
            WasmValue::I64(left.wrapping_sub(right))
        }
        (Operator::I64Mul, WasmValue::I64(left), WasmValue::I64(right)) => {
            WasmValue::I64(left.wrapping_mul(right))
        }
        (Operator::F32Add, WasmValue::F32(left), WasmValue::F32(right)) => {
            WasmValue::F32((f32::from_bits(left) + f32::from_bits(right)).to_bits())
        }
        (Operator::F64Add, WasmValue::F64(left), WasmValue::F64(right)) => {
            WasmValue::F64((f64::from_bits(left) + f64::from_bits(right)).to_bits())
        }
        _ => {
            return Err(Error::Unsupported(
                "invalid Wasm constant expression operand types".into(),
            ));
        }
    };
    Ok(result)
}

fn const_expr_stack_error() -> Error {
    Error::Unsupported("Wasm constant expression stack underflow".into())
}

fn shared_signature(ty: &wasmparser::FuncType) -> Option<WasmSignature> {
    let params = ty
        .params()
        .iter()
        .copied()
        .map(scalar_type)
        .collect::<Result<_, _>>()
        .ok()?;
    let results = ty
        .results()
        .iter()
        .copied()
        .map(scalar_type)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some(WasmSignature {
        params,
        result: results.first().copied(),
        additional_results: results.into_iter().skip(1).collect(),
    })
}

fn function_type_slots(
    reader: wasmparser::TypeSectionReader<'_>,
) -> Result<
    (
        Vec<FuncType>,
        Vec<Option<u32>>,
        Vec<u32>,
        Vec<Option<WasmGcType>>,
        Vec<WasmGcDescriptor>,
        Vec<String>,
    ),
    Error,
> {
    let mut types = Vec::new();
    let mut supertypes: Vec<Option<u32>> = Vec::new();
    let mut canonicals: Vec<u32> = Vec::new();
    let mut gc_types = Vec::new();
    let mut gc_descriptors = Vec::new();
    let mut fingerprints = Vec::new();
    let mut canonical_groups: Vec<(String, Vec<u32>)> = Vec::new();
    for group in reader {
        let group = group.map_err(parse_error)?;
        let group_types = group.into_types().collect::<Vec<_>>();
        let group_start = types.len() as u32;
        let group_end = group_start + group_types.len() as u32;
        let group_gc_types = group_types
            .iter()
            .map(|subtype| gc_type_definition(&subtype.composite_type.inner))
            .collect::<Result<Vec<_>, _>>()?;
        let group_gc_descriptors = group_types
            .iter()
            .map(|subtype| WasmGcDescriptor {
                descriptor_type: subtype
                    .composite_type
                    .descriptor_idx
                    .and_then(|index| index.as_module_index()),
                describes_type: subtype
                    .composite_type
                    .describes_idx
                    .and_then(|index| index.as_module_index()),
            })
            .collect::<Vec<_>>();
        let shape = group_types
            .iter()
            .map(|subtype| {
                let parent = subtype
                    .supertype_idx
                    .and_then(|index| index.as_module_index());
                let parent_shape = parent.map_or_else(
                    || "root".to_string(),
                    |parent| {
                        if (group_start..group_end).contains(&parent) {
                            format!("local:{}", parent - group_start)
                        } else {
                            format!(
                                "canonical:{}",
                                canonicals.get(parent as usize).copied().unwrap_or(parent)
                            )
                        }
                    },
                );
                let descriptor_shape = type_index_shape(
                    subtype
                        .composite_type
                        .descriptor_idx
                        .and_then(|index| index.as_module_index()),
                    group_start,
                    group_end,
                    &canonicals,
                );
                let describes_shape = type_index_shape(
                    subtype
                        .composite_type
                        .describes_idx
                        .and_then(|index| index.as_module_index()),
                    group_start,
                    group_end,
                    &canonicals,
                );
                format!(
                    "final:{};parent:{};descriptor:{};describes:{};shared:{};inner:{}",
                    subtype.is_final,
                    parent_shape,
                    descriptor_shape,
                    describes_shape,
                    subtype.composite_type.shared,
                    composite_type_fingerprint(
                        &subtype.composite_type.inner,
                        group_start,
                        group_end,
                        &canonicals,
                    ),
                )
            })
            .collect::<Vec<_>>()
            .join("|");
        let group_canonicals = canonical_groups
            .iter()
            .find_map(|(candidate, candidate_canonicals)| {
                (candidate == &shape).then(|| candidate_canonicals.clone())
            })
            .unwrap_or_else(|| {
                (0..group_types.len())
                    .map(|offset| group_start + offset as u32)
                    .collect()
            });
        fingerprints
            .extend((0..group_types.len()).map(|offset| format!("{shape}::member:{offset}")));
        for (offset, subtype) in group_types.into_iter().enumerate() {
            let parent = subtype
                .supertype_idx
                .and_then(|index| index.as_module_index());
            let ty = match subtype.composite_type.inner {
                CompositeInnerType::Func(ty) => ty,
                CompositeInnerType::Array(_) | CompositeInnerType::Struct(_) => {
                    FuncType::new([], [])
                }
                CompositeInnerType::Cont(_) => {
                    return Err(Error::Unsupported(
                        "stack switching proposal is not supported".into(),
                    ));
                }
            };
            types.push(ty);
            supertypes.push(parent);
            canonicals.push(group_canonicals[offset]);
            gc_types.push(group_gc_types[offset].clone());
            gc_descriptors.push(group_gc_descriptors[offset]);
        }
        canonical_groups.push((shape, group_canonicals));
    }
    Ok((
        types,
        supertypes,
        canonicals,
        gc_types,
        gc_descriptors,
        fingerprints,
    ))
}

fn type_index_shape(
    index: Option<u32>,
    group_start: u32,
    group_end: u32,
    canonicals: &[u32],
) -> String {
    index.map_or_else(
        || "none".to_string(),
        |index| {
            if (group_start..group_end).contains(&index) {
                format!("local:{}", index - group_start)
            } else {
                format!(
                    "canonical:{}",
                    canonicals.get(index as usize).copied().unwrap_or(index)
                )
            }
        },
    )
}

fn gc_type_definition(inner: &wasmparser::CompositeInnerType) -> Result<Option<WasmGcType>, Error> {
    use wasmparser::CompositeInnerType as Inner;
    match inner {
        Inner::Func(_) => Ok(None),
        Inner::Array(array) => Ok(Some(WasmGcType::Array(gc_field(array.0)?))),
        Inner::Struct(structure) => Ok(Some(WasmGcType::Struct(
            structure
                .fields
                .iter()
                .copied()
                .map(gc_field)
                .collect::<Result<_, _>>()?,
        ))),
        Inner::Cont(_) => Err(Error::Unsupported(
            "stack switching proposal is not supported".into(),
        )),
    }
}

fn gc_field(field: wasmparser::FieldType) -> Result<WasmGcField, Error> {
    let (ty, packed_bits) = match field.element_type {
        wasmparser::StorageType::I8 => (WasmType::I32, Some(8)),
        wasmparser::StorageType::I16 => (WasmType::I32, Some(16)),
        wasmparser::StorageType::Val(ty) => (scalar_type(ty)?, None),
    };
    Ok(WasmGcField {
        ty,
        mutable: field.mutable,
        packed_bits,
    })
}

fn composite_type_fingerprint(
    ty: &wasmparser::CompositeInnerType,
    group_start: u32,
    group_end: u32,
    canonicals: &[u32],
) -> String {
    use wasmparser::CompositeInnerType as Inner;
    match ty {
        Inner::Func(ty) => format!(
            "func({})->({})",
            ty.params()
                .iter()
                .map(|ty| value_type_fingerprint(*ty, group_start, group_end, canonicals))
                .collect::<Vec<_>>()
                .join(","),
            ty.results()
                .iter()
                .map(|ty| value_type_fingerprint(*ty, group_start, group_end, canonicals))
                .collect::<Vec<_>>()
                .join(","),
        ),
        Inner::Array(array) => format!(
            "array({})",
            field_type_fingerprint(array.0, group_start, group_end, canonicals)
        ),
        Inner::Struct(structure) => format!(
            "struct({})",
            structure
                .fields
                .iter()
                .map(|field| field_type_fingerprint(*field, group_start, group_end, canonicals))
                .collect::<Vec<_>>()
                .join(","),
        ),
        Inner::Cont(ty) => format!("cont({ty:?})"),
    }
}

fn field_type_fingerprint(
    field: wasmparser::FieldType,
    group_start: u32,
    group_end: u32,
    canonicals: &[u32],
) -> String {
    let value = match field.element_type {
        wasmparser::StorageType::I8 => "i8".to_string(),
        wasmparser::StorageType::I16 => "i16".to_string(),
        wasmparser::StorageType::Val(ty) => {
            value_type_fingerprint(ty, group_start, group_end, canonicals)
        }
    };
    format!("{}:{}", field.mutable, value)
}

fn value_type_fingerprint(
    ty: wasmparser::ValType,
    group_start: u32,
    group_end: u32,
    canonicals: &[u32],
) -> String {
    match ty {
        wasmparser::ValType::I32 => "i32".into(),
        wasmparser::ValType::I64 => "i64".into(),
        wasmparser::ValType::F32 => "f32".into(),
        wasmparser::ValType::F64 => "f64".into(),
        wasmparser::ValType::V128 => "v128".into(),
        wasmparser::ValType::Ref(ty) => format!(
            "ref:{}:{}",
            ty.is_nullable(),
            heap_type_fingerprint(ty.heap_type(), group_start, group_end, canonicals)
        ),
    }
}

fn value_type_link_fingerprint(ty: wasmparser::ValType, type_fingerprints: &[String]) -> String {
    match ty {
        wasmparser::ValType::I32 => "i32".into(),
        wasmparser::ValType::I64 => "i64".into(),
        wasmparser::ValType::F32 => "f32".into(),
        wasmparser::ValType::F64 => "f64".into(),
        wasmparser::ValType::V128 => "v128".into(),
        wasmparser::ValType::Ref(ty) => {
            let heap = match ty.heap_type() {
                wasmparser::HeapType::Abstract { shared, ty } => {
                    format!("abstract:{shared}:{ty:?}")
                }
                wasmparser::HeapType::Concrete(index) | wasmparser::HeapType::Exact(index) => {
                    let exact = matches!(ty.heap_type(), wasmparser::HeapType::Exact(_));
                    let target = index
                        .as_module_index()
                        .and_then(|index| type_fingerprints.get(index as usize))
                        .or_else(|| {
                            index
                                .as_rec_group_index()
                                .and_then(|index| type_fingerprints.get(index as usize))
                        })
                        .cloned()
                        .unwrap_or_else(|| format!("unresolved:{index:?}"));
                    format!("{}:{target}", if exact { "exact" } else { "concrete" })
                }
            };
            format!("ref:{}:{heap}", ty.is_nullable())
        }
    }
}

fn heap_type_fingerprint(
    ty: wasmparser::HeapType,
    group_start: u32,
    group_end: u32,
    canonicals: &[u32],
) -> String {
    match ty {
        wasmparser::HeapType::Abstract { shared, ty } => format!("abstract:{shared}:{ty:?}"),
        wasmparser::HeapType::Concrete(index) | wasmparser::HeapType::Exact(index) => {
            let exact = matches!(ty, wasmparser::HeapType::Exact(_));
            let key = index
                .as_rec_group_index()
                .map(|index| format!("local:{index}"))
                .or_else(|| {
                    index.as_module_index().map(|index| {
                        if (group_start..group_end).contains(&index) {
                            format!("local:{}", index - group_start)
                        } else {
                            format!(
                                "canonical:{}",
                                canonicals.get(index as usize).copied().unwrap_or(index)
                            )
                        }
                    })
                })
                .unwrap_or_else(|| format!("other:{index:?}"));
            format!("{}:{key}", if exact { "exact" } else { "concrete" })
        }
    }
}

fn scalar_type(ty: ValType) -> Result<WasmType, Error> {
    WasmType::from_wasm(ty)
        .ok_or_else(|| Error::Unsupported("function requires non-scalar types".into()))
}

fn parse_error(error: wasmparser::BinaryReaderError) -> Error {
    Error::Parse(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rqj::{ExecutionRequest, Host, Runtime};

    #[derive(Default)]
    struct TestHost;
    impl Host for TestHost {
        fn write_line(&mut self, _: &str) {}
        fn clock_millis(&mut self) -> f64 {
            0.0
        }
    }

    fn lower(wat: &str) -> WasmI32Function {
        crate::Engine::new()
            .compile_wat(wat)
            .unwrap()
            .lower_shared_i32("f")
            .unwrap()
    }

    #[test]
    fn shared_i32_arithmetic_wraps_at_wasm_boundaries() {
        let mut runtime = Runtime::new(TestHost);
        for (op, left, right, expected) in [
            ("add", i32::MAX, 1, i32::MIN),
            ("sub", i32::MIN, 1, i32::MAX),
            ("mul", i32::MAX, 2, -2),
            ("mul", i32::MIN, -1, i32::MIN),
            ("add", -2, 1, -1),
        ] {
            let function = lower(&format!(
                "(module (func (export \"f\") (param i32 i32) (result i32) local.get 0 local.get 1 i32.{op}))"
            ));
            assert_eq!(
                runtime.execute_wasm_i32(&function, &[left, right]).unwrap(),
                Some(expected)
            );
        }
    }

    #[test]
    fn shared_locals_are_zeroed_and_tee_preserves_stack_value() {
        let function = lower(
            "(module (func (export \"f\") (param i32) (result i32) (local i32 i32) local.get 1 local.get 0 i32.add local.tee 2 drop local.get 2))",
        );
        let mut runtime = Runtime::new(TestHost);
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[17]).unwrap(),
            Some(17)
        );
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[-42]).unwrap(),
            Some(-42)
        );
        runtime.collect(function.residual()).unwrap();
    }

    #[test]
    fn shared_void_result_and_argument_count_are_explicit() {
        let function = lower(
            "(module (func (export \"f\") (param i32) (local i32) local.get 0 local.set 1 nop))",
        );
        let mut runtime = Runtime::new(TestHost);
        assert!(runtime.execute_wasm_i32(&function, &[]).is_err());
        assert!(runtime.execute_wasm_i32(&function, &[1, 2]).is_err());
        assert_eq!(runtime.execute_wasm_i32(&function, &[1]).unwrap(), None);
    }

    #[test]
    fn shared_wasm_and_javascript_use_the_same_runtime() {
        let mut runtime = Runtime::new(TestHost);
        let function = lower("(module (func (export \"f\") (result i32) i32.const -2147483648))");
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "if (2147483647 + 1 !== 2147483648) throw 'JS overflow';",
                "arithmetic.js",
            ))
            .unwrap();
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[]).unwrap(),
            Some(i32::MIN)
        );
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "if (2 * 3 !== 6) throw 'JS multiply';",
                "arithmetic.js",
            ))
            .unwrap();
    }

    #[test]
    fn shared_i32_entry_rejects_non_i32_results() {
        let module = crate::Engine::new()
            .compile_wat("(module (func (export \"f\") (result i64) i64.const 1))")
            .unwrap();
        assert!(matches!(
            module.lower_shared_i32("f"),
            Err(Error::Unsupported(_))
        ));
        let module = crate::Engine::new().compile_wat("(module)").unwrap();
        assert!(module.lower_shared_i32("missing").is_err());
    }

    #[test]
    fn shared_lowering_uses_wide_encoding_for_deep_stacks() {
        let wat = format!(
            "(module (func (export \"f\") (result i32) {} {}))",
            "i32.const 1 ".repeat(300),
            "i32.add ".repeat(299)
        );
        let function = lower(&wat);
        let mut runtime = Runtime::new(TestHost);
        assert_eq!(runtime.execute_wasm_i32(&function, &[]).unwrap(), Some(300));
    }

    #[test]
    fn shared_lowering_resolves_the_exported_function_index() {
        let function = lower(
            "(module (func (result i32) i32.const 99) (func (export \"f\") (param i32) (result i32) local.get 0 i32.const 3 i32.mul))",
        );
        let mut runtime = Runtime::new(TestHost);
        assert_eq!(runtime.execute_wasm_i32(&function, &[7]).unwrap(), Some(21));
    }

    #[test]
    fn shared_integer_traps_are_typed_and_runtime_recovers() {
        use rqj::WasmTrap;

        let divide = lower(
            "(module (func (export \"f\") (param i32 i32) (result i32) local.get 0 local.get 1 i32.div_s))",
        );
        let remainder = lower(
            "(module (func (export \"f\") (param i32 i32) (result i32) local.get 0 local.get 1 i32.rem_s))",
        );
        let mut runtime = Runtime::new(TestHost);
        for (args, trap) in [
            ([i32::MIN, 0], WasmTrap::IntegerDivideByZero),
            ([i32::MIN, -1], WasmTrap::IntegerOverflow),
        ] {
            let error = runtime.execute_wasm_i32(&divide, &args).unwrap_err();
            assert_eq!(error.wasm_trap(), Some(trap));
            assert_eq!(error.to_string(), trap.to_string());
            runtime.collect(divide.residual()).unwrap();
            assert_eq!(runtime.execute_wasm_i32(&divide, &[7, 2]).unwrap(), Some(3));
        }
        assert_eq!(
            runtime
                .execute_wasm_i32(&remainder, &[i32::MIN, -1])
                .unwrap(),
            Some(0)
        );
        let error = runtime.execute_wasm_i32(&divide, &[]).unwrap_err();
        assert_eq!(error.wasm_trap(), None);

        let program = rqj::Engine::specialize("throw new Error('guest');", "throw.js").unwrap();
        assert_eq!(runtime.execute(&program).unwrap_err().wasm_trap(), None);
    }
}

#[cfg(test)]
mod spec;

#[cfg(test)]
mod control_tests;

#[cfg(test)]
mod call_tests;

#[cfg(test)]
mod scalar_tests;

#[cfg(test)]
mod integer_tests;

#[cfg(test)]
mod float_tests;
