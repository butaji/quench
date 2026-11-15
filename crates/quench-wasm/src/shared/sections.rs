//! The section-by-section decoding of a shared module, and its lowering once every section is read.

use super::*;

/// The sections of one shared module as they decode, before the module is lowered.
#[derive(Default)]
pub(super) struct Sections<'a> {
    types: WasmTypes,
    functions: Vec<u32>,
    bodies: Vec<wasmparser::FunctionBody<'a>>,
    function_imports: Vec<quench_runtime::WasmFunctionImport>,
    tags: Vec<quench_runtime::WasmTag>,
    globals: Vec<WasmGlobal>,
    memories: Vec<WasmMemory>,
    tables: Vec<quench_runtime::WasmTable>,
    elements: Vec<quench_runtime::WasmElement>,
    data: Vec<WasmData>,
    exports: Vec<quench_runtime::WasmExport>,
    custom_sections: Vec<quench_runtime::WasmCustomSection>,
    start: Option<u32>,
    selected: Option<u32>,
    code_section: Option<std::ops::Range<usize>>,
}

impl<'a> Sections<'a> {
    /// Records one payload of the module: its section's entries, in module order.
    pub(super) fn record(&mut self, payload: Payload<'a>, export: Option<&str>) -> Result<(), Error> {
        match payload {
            Payload::Version { encoding: Encoding::Module, .. } | Payload::End(_) => Ok(()),
            Payload::CodeSectionStart { range, .. } => Ok(self.code_section = Some(range)),
            Payload::CustomSection(reader) => {
                self.custom_sections.push(quench_runtime::WasmCustomSection {
                    name: reader.name().into(),
                    bytes: reader.data().into(),
                });
                Ok(())
            }
            Payload::StartSection { func, .. } => Ok(self.start = Some(func)),
            Payload::DataCountSection { .. } => Ok(()),
            Payload::CodeSectionEntry(body) => Ok(self.bodies.push(body)),
            Payload::TypeSection(reader) => {
                self.types = WasmTypes::from_groups(
                    reader.into_iter().collect::<Result<Vec<_>, _>>().map_err(parse_error)?,
                );
                Ok(())
            }
            other => self.entries(other, export),
        }
    }

    /// Records a payload of a section whose entries the module defines or imports.
    fn entries(&mut self, payload: Payload<'a>, export: Option<&str>) -> Result<(), Error> {
        match payload {
            Payload::ImportSection(reader) => self.imports(reader),
            Payload::TagSection(reader) => self.tags_section(reader),
            Payload::FunctionSection(reader) => self.function_section(reader),
            Payload::ExportSection(reader) => self.exports_section(reader, export),
            Payload::GlobalSection(reader) => decode_globals(reader, &mut self.globals, &self.types),
            Payload::ElementSection(reader) => {
                self.elements = decode_elements(reader, &self.globals, &self.tables, &self.types)?;
                Ok(())
            }
            Payload::TableSection(reader) => self.table_section(reader),
            Payload::MemorySection(reader) => self.memory_section(reader),
            Payload::DataSection(reader) => self.data_section(reader),
            _ => Err(Error::Unsupported(
                "module requires state, imports, or an unsupported section".into(),
            )),
        }
    }

    fn tags_section(&mut self, reader: wasmparser::TagSectionReader<'a>) -> Result<(), Error> {
        for tag in reader {
            self.tags.push(quench_runtime::WasmTag { ty: tag.map_err(parse_error)?.func_type_idx, import: None });
        }
        Ok(())
    }

    fn function_section(&mut self, reader: wasmparser::FunctionSectionReader<'a>) -> Result<(), Error> {
        for ty in reader {
            self.functions.push(ty.map_err(parse_error)?);
        }
        Ok(())
    }

    fn memory_section(&mut self, reader: wasmparser::MemorySectionReader<'a>) -> Result<(), Error> {
        for memory in reader {
            let memory = memory.map_err(parse_error)?;
            self.memories.push(WasmMemory { ty: memory, import: None });
        }
        Ok(())
    }

    /// The entries of the import section: a function, tag, table, memory or immutable global.
    fn imports(&mut self, reader: wasmparser::ImportSectionReader<'a>) -> Result<(), Error> {
        for (index, import) in reader.into_imports().enumerate() {
            let import = import.map_err(parse_error)?;
            let name = quench_runtime::WasmImportName {
                index: u32::try_from(index)
                    .map_err(|_| Error::Unsupported("too many Wasm imports".into()))?,
                module: import.module.to_owned(),
                name: import.name.to_owned(),
            };
            self.import(import.ty, name)?;
        }
        Ok(())
    }

    /// One imported entity, recorded in the list its kind belongs to.
    fn import(&mut self, ty: wasmparser::TypeRef, name: quench_runtime::WasmImportName) -> Result<(), Error> {
        match ty {
            wasmparser::TypeRef::Func(index) | wasmparser::TypeRef::FuncExact(index) => {
                self.function_imports.push(quench_runtime::WasmFunctionImport {
                    exact: matches!(ty, wasmparser::TypeRef::FuncExact(_)),
                    ty: quench_runtime::WasmCallableType::Declared(index),
                    name,
                })
            }
            wasmparser::TypeRef::Tag(ty) => self.tags.push(quench_runtime::WasmTag {
                ty: ty.func_type_idx,
                import: Some(name),
            }),
            wasmparser::TypeRef::Table(ty) => self.tables.push(quench_runtime::WasmTable {
                ty,
                initializer: quench_runtime::WasmTableInitializer::Import(name),
            }),
            wasmparser::TypeRef::Memory(ty) => self.memories.push(WasmMemory {
                ty,
                import: Some(name),
            }),
            wasmparser::TypeRef::Global(ty) if !ty.shared => {
                let content = self.types.callable_value_type(ty.content_type).ok_or_else(|| {
                    Error::Unsupported("unsupported Wasm global type".into())
                })?;
                self.globals.push(WasmGlobal {
                    initial: WasmGlobalInitializer::Import { ty: content, name },
                    mutable: ty.mutable,
                })
            }
            _ => {
                return Err(Error::Unsupported(
                    "unsupported shared Wasm import kind".into(),
                ));
            }
        }
        Ok(())
    }

    fn exports_section(&mut self, reader: wasmparser::ExportSectionReader<'a>, export: Option<&str>) -> Result<(), Error> {
        for item in reader {
            let item = item.map_err(parse_error)?;
            if Some(item.name) == export && item.kind == ExternalKind::Func {
                self.selected = Some(item.index);
            }
            self.exports.push(quench_runtime::WasmExport {
                name: item.name.to_owned(),
                kind: item.kind,
                index: item.index,
            });
        }
        Ok(())
    }

    fn table_section(&mut self, reader: wasmparser::TableSectionReader<'a>) -> Result<(), Error> {
        for table in reader {
            let table = table.map_err(parse_error)?;
            let initializer = match table.init {
                wasmparser::TableInit::RefNull => quench_runtime::WasmReferenceInitializer::Null,
                wasmparser::TableInit::Expr(expression) => reference_initializer(
                    table.ty.element_type,
                    expression,
                    &self.globals,
                    &self.types,
                )?,
            };
            self.tables.push(quench_runtime::WasmTable {
                ty: table.ty,
                initializer: initializer.into(),
            });
        }
        Ok(())
    }

    fn data_section(&mut self, reader: wasmparser::DataSectionReader<'a>) -> Result<(), Error> {
        for segment in reader {
            let segment = segment.map_err(parse_error)?;
            let mode = self.data_mode(segment.kind)?;
            self.data.push(WasmData {
                mode,
                bytes: std::rc::Rc::new(segment.data.to_vec()),
            });
        }
        Ok(())
    }

    /// Where a data segment's bytes go: nowhere until used, or into a memory at a constant offset.
    fn data_mode(&self, kind: wasmparser::DataKind<'a>) -> Result<WasmDataMode, Error> {
        Ok(match kind {
            wasmparser::DataKind::Passive => WasmDataMode::Passive,
            wasmparser::DataKind::Active {
                memory_index,
                offset_expr,
            } => {
                let index_type = self
                    .memories
                    .get(memory_index as usize)
                    .ok_or_else(|| Error::Unsupported("unknown data memory".into()))?
                    .ty
                    .index_type();
                let offset = Engine::lower_wasm_constant_expression(
                    "wasm-data-offset",
                    value_type(index_type)?,
                    &self.globals,
                    offset_expr.get_operators_reader(),
                )
                .map_err(|error| Error::Unsupported(error.to_string()))?;
                WasmDataMode::Active {
                    memory: memory_index,
                    offset,
                }
            }
        })
    }

    /// The code section's byte range when the module is large enough, and kept whole, to lower
    /// its functions lazily: the interpreter lowers them on their first calls, from one kept
    /// copy of the code. Most of a toolchain's output never runs.
    pub(super) fn lazy_range(&self, export: Option<&str>, mode: quench_runtime::ExecutionMode) -> Option<std::ops::Range<usize>> {
        self.code_section.clone().filter(|_| {
            export.is_none()
                && mode == quench_runtime::ExecutionMode::Interpreted
                && self.bodies.len() >= LAZY_LOWERING_FUNCTIONS
                && self.functions.len() == self.bodies.len()
        })
    }

    /// The code of the lazily lowered module: its bytes in `range`, each function's type and
    /// the span of its body.
    fn lazy_code(&self, bytes: &[u8], range: std::ops::Range<usize>) -> quench_runtime::WasmLazyCode {
        let bodies = self.functions.iter().zip(&self.bodies).map(|(ty, body)| {
            let span = body.range();
            quench_runtime::WasmLazyBody {
                ty: *ty,
                start: span.start as u32,
                end: span.end as u32,
            }
        });
        quench_runtime::WasmLazyCode::new(bytes[range.clone()].into(), range.start, bodies.collect())
    }

    pub(super) fn lower_lazily(
        self,
        bytes: &[u8],
        range: std::ops::Range<usize>,
        export: Option<&str>,
    ) -> Result<(WasmModule, Option<u32>), Error> {
        let code = self.lazy_code(bytes, range);
        let Self { types, function_imports, tags, globals, memories, tables, elements, data, exports, custom_sections, start, selected, .. } = self;
        Engine::lower_wasm_module_lazily(
            export.unwrap_or("wasm-instance"),
            code,
            &types,
            &globals,
            &memories,
            &data,
            &tables,
            &elements,
            &function_imports,
            &tags,
        )
        .and_then(|module| match start {
            Some(index) => module.with_start(index),
            None => Ok(module),
        })
        .map(|module| module.with_exports(exports).with_custom_sections(custom_sections))
        .map(|module| (module, selected))
        .map_err(|error| Error::Unsupported(error.to_string()))
    }

    pub(super) fn lower_eagerly(
        self,
        mode: quench_runtime::ExecutionMode,
        export: Option<&str>,
    ) -> Result<(WasmModule, Option<u32>), Error> {
        let Self { types, functions, bodies, function_imports, tags, globals, memories, tables, elements, data, exports, custom_sections, start, selected, .. } = self;
        let inputs = function_inputs(functions, bodies)?;
        Engine::lower_wasm_module_for(
            mode,
            export.unwrap_or("wasm-instance"),
            inputs,
            &types,
            &globals,
            &memories,
            &data,
            &tables,
            &elements,
            &function_imports,
            &tags,
        )
        .and_then(|module| match start {
            Some(index) => module.with_start(index),
            None => Ok(module),
        })
        .map(|module| module.with_exports(exports).with_custom_sections(custom_sections))
        .map(|module| (module, selected))
        .map_err(|error| Error::Unsupported(error.to_string()))
    }
}

/// Each defined function's type and body, its locals expanded, ready to lower.
pub(super) fn function_inputs(functions: Vec<u32>, bodies: Vec<wasmparser::FunctionBody<'_>>) -> Result<Vec<WasmFunctionBody>, Error> {
    let mut inputs = Vec::with_capacity(bodies.len());
    for (type_index, body) in functions.into_iter().zip(bodies) {
        let mut locals = Vec::new();
        for local in body.get_locals_reader().map_err(parse_error)? {
            let (count, ty) = local.map_err(parse_error)?;
            let count = usize::try_from(count)
                .map_err(|_| Error::Unsupported("too many locals".into()))?;
            let length = locals
                .len()
                .checked_add(count)
                .filter(|length| *length <= usize::from(u16::MAX))
                .ok_or_else(|| Error::Unsupported("too many locals".into()))?;
            locals.resize(length, ty);
        }
        inputs.push(WasmFunctionBody {
            ty: quench_runtime::WasmCallableType::Declared(type_index),
            locals,
            operators: body.get_operators_reader().map_err(parse_error)?,
        });
    }
    Ok(inputs)
}
