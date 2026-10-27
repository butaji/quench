use super::*;

impl<H: Host> Vm<H> {
    pub(crate) fn wasm_value_matches_type(
        &self,
        value: crate::WasmValue,
        ty: crate::WasmType,
    ) -> bool {
        if !value.fits_type(ty) {
            return false;
        }
        match value {
            crate::WasmValue::FuncRef(value)
            | crate::WasmValue::ExternRef(value)
            | crate::WasmValue::GcRef(value) => self.wasm_reference_valid_in(value, ty, None),
            _ => true,
        }
    }

    pub(crate) fn execute_wasm_i32(
        &mut self,
        function: &crate::WasmI32Function,
        args: &[i32],
    ) -> Result<Option<i32>, JsError> {
        if function
            .signature()
            .params
            .iter()
            .any(|ty| *ty != crate::WasmType::I32)
            || function
                .signature()
                .results
                .iter()
                .any(|ty| *ty != crate::WasmType::I32)
        {
            return Err(JsError::validation(
                "function requires the typed Wasm boundary".into(),
            ));
        }
        let args: Vec<_> = args.iter().copied().map(crate::WasmValue::I32).collect();
        self.execute_wasm(function, &args).map(|result| {
            result.map(|value| {
                let crate::WasmValue::I32(value) = value else {
                    unreachable!("checked i32 signature")
                };
                value
            })
        })
    }

    pub(crate) fn execute_wasm(
        &mut self,
        function: &crate::WasmFunction,
        args: &[crate::WasmValue],
    ) -> Result<Option<crate::WasmValue>, JsError> {
        Self::require_single_wasm_result(function)?;
        let result = self.execute_wasm_unbound(function, args)?;
        function
            .signature()
            .results
            .first()
            .map(|ty| {
                self.decode_wasm_value_in(
                    result,
                    *ty,
                    Some(&function.module.signatures.declarations),
                )
            })
            .transpose()
    }

    pub(crate) fn execute_wasm_values(
        &mut self,
        function: &crate::WasmFunction,
        args: &[crate::WasmValue],
    ) -> Result<Vec<crate::WasmValue>, JsError> {
        let result = self.execute_wasm_unbound(function, args)?;
        self.decode_wasm_results(function, result)
    }

    fn execute_wasm_unbound(
        &mut self,
        function: &crate::WasmFunction,
        args: &[crate::WasmValue],
    ) -> Result<Value, JsError> {
        if !function.module.signatures.imports().is_empty()
            || !function.module.globals.is_empty()
            || !function.module.memories.is_empty()
            || !function.module.data.is_empty()
            || !function.module.tables.is_empty()
            || !function.module.elements.is_empty()
            || !function.module.tags.is_empty()
            || function.module.start.is_some()
        {
            return Err(JsError::validation(
                "Wasm state requires a rooted instance".into(),
            ));
        }
        self.execute_wasm_in_environment(function, args, Value::NULL)
    }

    pub(crate) fn instantiate_wasm(
        &mut self,
        function: &crate::WasmFunction,
    ) -> Result<crate::WasmInstance, JsError> {
        self.instantiate_wasm_module(&function.module)
    }

    pub(crate) fn instantiate_wasm_module(
        &mut self,
        module: &crate::WasmModule,
    ) -> Result<crate::WasmInstance, JsError> {
        self.instantiate_wasm_module_with_imports(module, &[])
    }

    pub(crate) fn instantiate_wasm_module_with_imports(
        &mut self,
        module: &crate::WasmModule,
        imports: &[RootId],
    ) -> Result<crate::WasmInstance, JsError> {
        module.program.validate().map_err(JsError::validation)?;
        if imports.len() != module.imports().len() {
            return Err(JsError::wasm_link("Wasm import count mismatch"));
        }
        let tables = module
            .tables
            .iter()
            .map(|table| {
                let crate::WasmTableInitializer::Import(name) = &table.initializer else {
                    return Ok(None);
                };
                let value = self
                    .root_value(imports[name.index as usize])
                    .ok_or_else(|| JsError::wasm_link("invalid Wasm table import root"))?;
                let Some(Cell::WasmTable {
                    elements,
                    element_type,
                    declarations,
                    table64,
                    ..
                }) = self.heap.get(value)
                else {
                    return Err(JsError::wasm_link(
                        "Wasm table import has wrong external kind",
                    ));
                };
                if *table64 != table.ty.table64
                    || !declarations.reference_subtype(
                        *element_type,
                        &module.signatures.declarations,
                        table.ty.element_type,
                    )
                    || !module.signatures.declarations.reference_subtype(
                        table.ty.element_type,
                        declarations,
                        *element_type,
                    )
                    || !crate::wasm::import_limits_match(
                        elements.len() as u64,
                        elements.maximum,
                        table.ty.initial,
                        table.ty.maximum,
                    )
                {
                    return Err(JsError::wasm_link("incompatible import type"));
                }
                Ok(Some(value))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let memories = module
            .memories
            .iter()
            .map(|memory| {
                let Some(name) = &memory.import else {
                    return Ok(None);
                };
                let value = self
                    .root_value(imports[name.index as usize])
                    .ok_or_else(|| JsError::wasm_link("invalid Wasm memory import root"))?;
                let Some(Cell::WasmMemory { bytes, header, .. }) = self.heap.get(value) else {
                    return Err(JsError::wasm_link(
                        "Wasm memory import has wrong external kind",
                    ));
                };
                let ty = &header.ty;
                if ty.shared != memory.ty.shared
                    || ty.index_type() != memory.ty.index_type()
                    || ty.page_size() != memory.ty.page_size()
                    || !crate::wasm::import_limits_match(
                        (bytes.len() / ty.page_size() as usize) as u64,
                        ty.maximum,
                        memory.ty.initial,
                        memory.ty.maximum,
                    )
                {
                    return Err(JsError::wasm_link("incompatible import type"));
                }
                Ok(Some(value))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let imported_globals = module
            .globals
            .iter()
            .map(|global| {
                let crate::WasmGlobalInitializer::Import { ty, name } = &global.initial else {
                    return Ok(None);
                };
                let value = self
                    .root_value(imports[name.index as usize])
                    .ok_or_else(|| JsError::wasm_link("invalid Wasm global import root"))?;
                match self.heap.get(value) {
                    Some(Cell::WasmGlobal {
                        ty: actual,
                        header,
                        mutable,
                        ..
                    }) if let declarations = &header.declarations
                        && *mutable == global.mutable
                        && (if *mutable {
                            declarations.value_subtype(
                                *actual,
                                &module.signatures.declarations,
                                *ty,
                            ) && module.signatures.declarations.value_subtype(
                                *ty,
                                declarations,
                                *actual,
                            )
                        } else {
                            declarations.value_subtype(
                                *actual,
                                &module.signatures.declarations,
                                *ty,
                            )
                        }) =>
                    {
                        Ok(Some(value))
                    }
                    _ => Err(JsError::wasm_link("incompatible import type")),
                }
            })
            .collect::<Result<Vec<_>, JsError>>()?;
        let functions = module
            .signatures
            .imports()
            .iter()
            .enumerate()
            .map(|(index, import)| {
                let name = &import.name;
                let value = self
                    .root_value(imports[name.index as usize])
                    .ok_or_else(|| JsError::wasm_link("invalid Wasm function import root"))?;
                let expected = module
                    .signatures
                    .function_type_index(index)
                    .ok_or_else(|| JsError::validation("invalid Wasm import type index".into()))?;
                if self.wasm_callable_matches(value, &module.signatures, expected, import.exact)
                    != Some(true)
                {
                    return Err(JsError::wasm_link("incompatible import type"));
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>, JsError>>()?;
        let tags = module
            .tags
            .iter()
            .map(|tag| {
                let Some(name) = &tag.import else {
                    return Ok(None);
                };
                let value = self
                    .root_value(imports[name.index as usize])
                    .ok_or_else(|| JsError::wasm_link("invalid Wasm tag import root"))?;
                let Some(Cell::WasmTag { declarations, ty }) = self.heap.get(value) else {
                    return Err(JsError::wasm_link("incompatible import type"));
                };
                if !declarations.equivalent(
                    *ty as usize,
                    &module.signatures.declarations,
                    tag.ty as usize,
                ) {
                    return Err(JsError::wasm_link("incompatible import type"));
                }
                Ok(Some(value))
            })
            .collect::<Result<Vec<_>, JsError>>()?;
        let id = self.register_wasm_program(module)?;
        // A host-driven sequence of instantiations need not enter dispatch.
        // Collect only before constructing the new, not-yet-rooted bindings.
        if self.heap.should_collect() {
            self.collect_now(module.residual());
        }
        // The rooted environment precedes initializer evaluation so ref.func
        // captures this instance. Undefined slots are uninitialized bindings,
        // never guest globals; each declaration installs its real cell in order.
        let mut values: Vec<_> = imported_globals
            .into_iter()
            .map(|binding| binding.unwrap_or(Value::UNDEFINED))
            .collect();
        for (memory, imported) in module.memories.iter().zip(memories) {
            if let Some(value) = imported {
                values.push(value);
                continue;
            }
            values.push(self.alloc_wasm_memory(memory.ty)?);
        }
        for segment in module.data.iter() {
            // All segments are available before initialization effects. A later
            // trap can leave functions reachable through an imported table.
            values.push(self.heap.alloc(Cell::ArrayBuffer {
                object: Box::new(Self::empty_object(Value::NULL)),
                bytes: segment.bytes.clone().into(),
                shared: false,
                detached: false,
                max_byte_length: segment.bytes.len(),
                resizable: false,
                immutable: true,
            }));
        }
        for (table, imported) in module.tables.iter().zip(tables) {
            if let Some(value) = imported {
                values.push(value);
                continue;
            }
            values.push(self.alloc_wasm_table(
                table.ty,
                module.signatures.declarations.clone(),
                Value::NULL,
            )?);
        }
        values.extend(std::iter::repeat_n(Value::UNDEFINED, module.elements.len()));
        for (tag, imported) in module.tags.iter().zip(tags) {
            values.push(imported.unwrap_or_else(|| {
                self.heap.alloc(Cell::WasmTag {
                    declarations: module.signatures.declarations.clone(),
                    ty: tag.ty,
                })
            }));
        }
        // Imported function slots end the resource layout; ref.func derives their base.
        values.extend(functions);
        let environment = self.heap.alloc(Cell::Environment {
            parent: Value::NULL,
            function: 0,
            slots: values.into(),
            scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                program: Some(id.raw()),
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: crate::heap::EnvironmentBindings::Owned(vec![]),
                with_objects: Box::default(),
            }),
        });
        let root = self.root(environment);
        let previous = self.replace_active_program(id);
        let initialized = self
            .initialize_wasm_globals(module, environment)
            .and_then(|()| self.initialize_wasm_references(module, environment))
            .and_then(|()| self.initialize_wasm_data(module, environment))
            .and_then(|()| match module.start {
                Some(index) => self
                    .execute_wasm_in_environment(
                        &module.function(index).expect("validated start entry"),
                        &[],
                        environment,
                    )
                    .map(|_| ()),
                None => Ok(()),
            });
        self.set_active_program(previous);
        if let Err(error) = initialized {
            self.release_root(root);
            return Err(error);
        }
        Ok(crate::WasmInstance {
            module: module.clone(),
            environment: root,
        })
    }

    /// A new zeroed memory of type `ty`, which is also its `WebAssembly.Memory` object.
    pub(super) fn alloc_wasm_memory(
        &mut self,
        ty: wasmparser::MemoryType,
    ) -> Result<Value, JsError> {
        let length = crate::WasmMemory { ty, import: None }
            .byte_length()
            .ok_or_else(|| {
                JsError::validation("Wasm memory allocation exceeds host address space".into())
            })?;
        // An unshared memory32 lives in a guarded reservation in both
        // modes: its pages commit as they are first touched, and growth
        // never copies. Compiled code relies on the guard in place of
        // bounds checks, which covers exactly the bytes past the memory's
        // length only when every length is a whole number of host pages.
        let guarded = (!ty.shared
            && !ty.memory64
            && (ty.page_size() as usize).is_multiple_of(crate::wasm::memory::HOST_PAGE))
        .then(|| crate::wasm::memory::LinearBytes::reserved(length))
        .flatten();
        let bytes = match guarded {
            Some(bytes) => bytes,
            None => {
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(length)
                    .map_err(|_| JsError::validation("Wasm memory allocation failed".into()))?;
                bytes.resize(length, 0);
                crate::wasm::memory::LinearBytes::heap(bytes)
            }
        };
        Ok(self.heap.alloc(Cell::WasmMemory {
            header: Box::new(crate::heap::WasmMemoryHeader {
                object: Self::empty_object(self.wasm_js.memory),
                ty,
            }),
            bytes: std::rc::Rc::new(crate::wasm::memory::MemoryStorage::new(bytes, ty.shared)),
            buffer: Value::UNDEFINED,
        }))
    }

    /// A new table of type `ty` with every element `initial`, which is also its
    /// `WebAssembly.Table` object.
    pub(super) fn alloc_wasm_table(
        &mut self,
        ty: wasmparser::TableType,
        declarations: crate::WasmTypes,
        initial: Value,
    ) -> Result<Value, JsError> {
        let length = usize::try_from(ty.initial).map_err(|_| {
            JsError::validation("Wasm table size exceeds host address space".into())
        })?;
        let mut elements = Vec::new();
        elements
            .try_reserve_exact(length)
            .map_err(|_| JsError::validation("Wasm table allocation failed".into()))?;
        elements.resize(length, initial);
        Ok(self.heap.alloc(Cell::WasmTable {
            object: Box::new(Self::empty_object(self.wasm_js.table)),
            table64: ty.table64,
            elements: Box::new(crate::heap::WasmTableElements {
                values: elements,
                maximum: ty.maximum,
            }),
            element_type: ty.element_type,
            declarations: Box::new(declarations),
        }))
    }

    /// A new global holding the encoded `value`, which is also its `WebAssembly.Global`
    /// object.
    pub(super) fn alloc_wasm_global(
        &mut self,
        value: Value,
        ty: crate::WasmType,
        declarations: crate::WasmTypes,
        mutable: bool,
    ) -> Value {
        self.heap.alloc(Cell::WasmGlobal {
            header: Box::new(crate::heap::WasmGlobalHeader {
                object: Self::empty_object(self.wasm_js.global),
                declarations,
            }),
            value,
            ty,
            mutable,
        })
    }

    fn initialize_wasm_references(
        &mut self,
        module: &crate::WasmModule,
        environment: Value,
    ) -> Result<(), JsError> {
        let table_base = module.globals.len() + module.memories.len() + module.data.len();
        let element_base = table_base + module.tables.len();
        for (index, definition) in module.tables.iter().enumerate() {
            let crate::WasmTableInitializer::Reference(initializer) = &definition.initializer
            else {
                continue;
            };
            let initial = self.wasm_reference_initializer(module, initializer, environment)?;
            let table = self
                .heap
                .environment_slot(environment, table_base + index)
                .unwrap();
            let Some(Cell::WasmTable { elements, .. }) = self.heap.get_mut(table) else {
                unreachable!("allocated table")
            };
            elements.fill(initial);
        }
        for (index, segment) in module.elements.iter().enumerate() {
            let mut values = Vec::new();
            values
                .try_reserve_exact(segment.items.len())
                .map_err(|_| JsError::validation("Wasm element allocation failed".into()))?;
            for value in segment.items.iter() {
                values.push(self.wasm_reference_initializer(module, value, environment)?);
            }
            // Materializing references can allocate. The instance root owns each
            // completed segment until it is installed in a table or kept passive.
            let elements = self.heap.alloc(Cell::WasmElements(values));
            *self
                .heap
                .environment_slot_mut(environment, element_base + index)
                .unwrap() = elements;
        }
        for (index, segment) in module.elements.iter().enumerate() {
            if matches!(segment.mode, crate::WasmElementMode::Passive) {
                continue;
            }
            let slot = element_base + index;
            if let crate::WasmElementMode::Active { table, offset } = &segment.mode {
                let elements = self.heap.environment_slot(environment, slot).unwrap();
                let table = self
                    .heap
                    .environment_slot(environment, table_base + *table as usize)
                    .unwrap();
                let offset = self.wasm_initializer_offset(module, offset, environment)?;
                self.wasm_table_init(table, elements, offset, 0, segment.items.len() as u64)?;
            }
            *self.heap.environment_slot_mut(environment, slot).unwrap() = Value::UNDEFINED;
        }
        Ok(())
    }

    fn initialize_wasm_data(
        &mut self,
        module: &crate::WasmModule,
        environment: Value,
    ) -> Result<(), JsError> {
        let data_base = module.globals.len() + module.memories.len();
        for (index, segment) in module.data.iter().enumerate() {
            let crate::WasmDataMode::Active { memory, offset } = &segment.mode else {
                continue;
            };
            let memory = self
                .heap
                .environment_slot(environment, module.globals.len() + *memory as usize)
                .unwrap();
            let source = self
                .heap
                .environment_slot(environment, data_base + index)
                .unwrap();
            let offset = self.wasm_initializer_offset(module, offset, environment)?;
            self.wasm_copy_bytes(memory, Some(source), offset, 0, segment.bytes.len() as u64)?;
            *self
                .heap
                .environment_slot_mut(environment, data_base + index)
                .unwrap() = Value::UNDEFINED;
        }
        Ok(())
    }

    fn initialize_wasm_globals(
        &mut self,
        module: &crate::WasmModule,
        environment: Value,
    ) -> Result<(), JsError> {
        for (index, global) in module.globals.iter().enumerate() {
            let crate::WasmGlobalInitializer::Expression(expression) = &global.initial else {
                continue;
            };
            let value = self.evaluate_wasm_initializer(module, expression, environment)?;
            let value = self.encode_wasm_value(value);
            let binding = self.alloc_wasm_global(
                value,
                expression.value_type(),
                module.signatures.declarations.clone(),
                global.mutable,
            );
            *self.heap.environment_slot_mut(environment, index).unwrap() = binding;
        }
        Ok(())
    }

    pub(crate) fn create_wasm_host_reference(&mut self) -> crate::RootId {
        let value = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        self.root(value)
    }

    pub(crate) fn wasm_external_conversion(
        &mut self,
        conversion: crate::wasm::reference::ExternalConversion,
        value: Value,
    ) -> Value {
        use crate::wasm::reference::ExternalConversion;
        if value.is_null() {
            return value;
        }
        match conversion {
            ExternalConversion::Internalize => {
                if matches!(self.heap.get(value), Some(Cell::WasmGc { .. })) {
                    return value;
                }
                // JS host numbers in the signed i31 range have the canonical immediate representation.
                if let Some(number) = value.as_number() {
                    if number >= f64::from(crate::wasm::i31::SIGNED_MIN)
                        && number <= f64::from(crate::wasm::i31::SIGNED_MAX)
                        && number.fract() == 0.0
                        && !(number == 0.0 && number.is_sign_negative())
                    {
                        return Value::integer((number as i32) & crate::wasm::i31::I31_MASK);
                    }
                }
                self.heap.alloc(Cell::WasmExtern(value))
            }
            ExternalConversion::Externalize => self.wasm_external_value(value),
        }
    }

    pub(crate) fn wasm_external_value(&self, value: Value) -> Value {
        if let Some(Cell::WasmExtern(payload)) = self.heap.get(value) {
            return *payload;
        }
        if let Some(bits) = crate::wasm::i31::bits(value) {
            return Value::integer(crate::wasm::i31::signed(bits));
        }
        value
    }

    fn evaluate_wasm_initializer(
        &mut self,
        module: &crate::WasmModule,
        expression: &crate::WasmConstantExpression,
        environment: Value,
    ) -> Result<crate::WasmValue, JsError> {
        let name = "wasm-instance-initializer";
        let mut effect_error = None;
        let value = expression
            .evaluate(
                name,
                Some(&module.signatures.declarations),
                |dependency| match dependency {
                    crate::wasm::WasmInitializerEffect::ExternalConversion {
                        conversion,
                        value,
                    } => {
                        let value = self.encode_wasm_value(value);
                        self.decode_wasm_value(value, conversion.input_type())
                            .map_err(|error| {
                                crate::Diagnostic::unsupported(name, error.to_string())
                            })?;
                        let value = self.wasm_external_conversion(conversion, value);
                        self.decode_wasm_value(value, conversion.output_type(true))
                            .map_err(|error| {
                                crate::Diagnostic::unsupported(name, error.to_string())
                            })
                    }
                    crate::wasm::WasmInitializerEffect::Array {
                        index,
                        initialization,
                    } => {
                        let result = (|| {
                            let initialization = match initialization {
                                crate::wasm::WasmArrayInitializer::Default(count) => {
                                    super::wasm_gc::ArrayInitialization::Default(count)
                                }
                                crate::wasm::WasmArrayInitializer::Repeated { count, value } => {
                                    super::wasm_gc::ArrayInitialization::Repeated {
                                        count,
                                        value: self.encode_wasm_value(value),
                                    }
                                }
                                crate::wasm::WasmArrayInitializer::Fixed(fields) => {
                                    let mut values = Vec::new();
                                    values.try_reserve_exact(fields.len()).map_err(|_| {
                                        JsError::wasm_trap_error(crate::WasmTrap::ArrayTooLarge)
                                    })?;
                                    for value in fields {
                                        values.push(self.encode_wasm_value(*value));
                                    }
                                    super::wasm_gc::ArrayInitialization::Fixed(values)
                                }
                            };
                            self.wasm_array_new(
                                &module.signatures.declarations,
                                index,
                                initialization,
                            )
                            .map(crate::WasmValue::GcRef)
                        })();
                        result.map_err(|error| {
                            let diagnostic =
                                crate::Diagnostic::unsupported(name, error.to_string());
                            effect_error = Some(error);
                            diagnostic
                        })
                    }
                    crate::wasm::WasmInitializerEffect::Struct {
                        index,
                        fields,
                        descriptor,
                    } => {
                        let fields = match fields {
                            Some(fields) => {
                                let mut values = Vec::new();
                                values.try_reserve_exact(fields.len()).map_err(|_| {
                                    crate::Diagnostic::unsupported(
                                        name,
                                        "Wasm struct allocation failed",
                                    )
                                })?;
                                for value in fields {
                                    values.push(self.encode_wasm_value(*value));
                                }
                                Some(values)
                            }
                            None => None,
                        };
                        let descriptor = descriptor.map(|value| self.encode_wasm_value(value));
                        self.wasm_struct_new(
                            &module.signatures.declarations,
                            index,
                            fields,
                            descriptor,
                        )
                        .map(crate::WasmValue::GcRef)
                        .map_err(|error| {
                            let diagnostic =
                                crate::Diagnostic::unsupported(name, error.to_string());
                            effect_error = Some(error);
                            diagnostic
                        })
                    }
                    crate::wasm::WasmInitializerEffect::Function(index) => self
                        .wasm_function_reference(module.residual(), index, environment)
                        .map(crate::WasmValue::FuncRef)
                        .map_err(|error| crate::Diagnostic::unsupported(name, error.to_string())),
                    crate::wasm::WasmInitializerEffect::Global(index) => {
                        let binding = self
                            .heap
                            .environment_slot(environment, index as usize)
                            .ok_or_else(|| {
                                crate::Diagnostic::unsupported(
                                    name,
                                    "missing Wasm initializer global",
                                )
                            })?;
                        let Some(Cell::WasmGlobal {
                            value, ty, header, ..
                        }) = self.heap.get(binding)
                        else {
                            return Err(crate::Diagnostic::unsupported(
                                name,
                                "invalid Wasm initializer global binding",
                            ));
                        };
                        self.decode_wasm_value_in(*value, *ty, Some(&header.declarations))
                            .map_err(|error| {
                                crate::Diagnostic::unsupported(name, error.to_string())
                            })
                    }
                },
            )
            .map_err(|error| {
                effect_error.unwrap_or_else(|| JsError::validation(error.to_string()))
            })?;
        if let crate::WasmValue::FuncRef(reference)
        | crate::WasmValue::ExternRef(reference)
        | crate::WasmValue::GcRef(reference) = value
        {
            self.decode_wasm_value_in(
                reference,
                expression.value_type(),
                Some(&module.signatures.declarations),
            )?;
        }
        Ok(value)
    }

    fn wasm_initializer_offset(
        &mut self,
        module: &crate::WasmModule,
        expression: &crate::WasmConstantExpression,
        environment: Value,
    ) -> Result<u64, JsError> {
        match self.evaluate_wasm_initializer(module, expression, environment)? {
            crate::WasmValue::I32(offset) => Ok(u64::from(offset as u32)),
            crate::WasmValue::I64(offset) => Ok(offset as u64),
            _ => Err(JsError::validation(
                "invalid Wasm segment offset type".into(),
            )),
        }
    }

    fn wasm_reference_initializer(
        &mut self,
        module: &crate::WasmModule,
        initializer: &crate::WasmReferenceInitializer,
        environment: Value,
    ) -> Result<Value, JsError> {
        match initializer {
            crate::WasmReferenceInitializer::Expression(expression) => {
                match self.evaluate_wasm_initializer(module, expression, environment)? {
                    crate::WasmValue::FuncRef(value)
                    | crate::WasmValue::ExternRef(value)
                    | crate::WasmValue::GcRef(value) => Ok(value),
                    _ => Err(JsError::validation(
                        "invalid Wasm reference initializer type".into(),
                    )),
                }
            }
            crate::WasmReferenceInitializer::Null => Ok(Value::NULL),
            crate::WasmReferenceInitializer::Function(function) => {
                self.wasm_function_reference(module.residual(), *function, environment)
            }
        }
    }

    pub(crate) fn invoke_wasm(
        &mut self,
        instance: &crate::WasmInstance,
        index: u32,
        args: &[crate::WasmValue],
    ) -> Result<Option<crate::WasmValue>, JsError> {
        let function = instance
            .module
            .function(index)
            .ok_or_else(|| JsError::validation("Wasm function index out of bounds".into()))?;
        Self::require_single_wasm_result(&function)?;
        let environment = self
            .root_value(instance.environment)
            .ok_or_else(|| JsError::validation("invalid Wasm instance root".into()))?;
        let result = self.execute_wasm_in_environment(&function, args, environment)?;
        function
            .signature()
            .results
            .first()
            .map(|ty| {
                self.decode_wasm_value_in(
                    result,
                    *ty,
                    Some(&function.module.signatures.declarations),
                )
            })
            .transpose()
    }

    pub(crate) fn invoke_wasm_values(
        &mut self,
        instance: &crate::WasmInstance,
        index: u32,
        args: &[crate::WasmValue],
    ) -> Result<Vec<crate::WasmValue>, JsError> {
        let environment = self
            .root_value(instance.environment)
            .ok_or_else(|| JsError::validation("invalid Wasm instance root".into()))?;
        let function = instance
            .module
            .function(index)
            .ok_or_else(|| JsError::validation("Wasm function index out of bounds".into()))?;
        let result = self.execute_wasm_in_environment(&function, args, environment)?;
        self.decode_wasm_results(&function, result)
    }

    pub(crate) fn wasm_function(
        &mut self,
        instance: &crate::WasmInstance,
        index: u32,
    ) -> Result<Value, JsError> {
        let environment = self
            .root_value(instance.environment)
            .ok_or_else(|| JsError::wasm_link("invalid Wasm instance root"))?;
        let id = self.register_wasm_program(&instance.module)?;
        let previous = self.replace_active_program(id);
        let result = self.wasm_function_reference(instance.module.residual(), index, environment);
        self.set_active_program(previous);
        result
    }

    pub(crate) fn wasm_memory(
        &self,
        instance: &crate::WasmInstance,
        index: u32,
    ) -> Result<Value, JsError> {
        instance
            .module
            .memories
            .get(index as usize)
            .ok_or_else(|| JsError::wasm_link("unknown Wasm memory export"))?;
        let environment = self
            .root_value(instance.environment)
            .ok_or_else(|| JsError::wasm_link("invalid Wasm instance root"))?;
        self.heap
            .environment_slot(environment, instance.module.globals.len() + index as usize)
            .ok_or_else(|| JsError::wasm_link("invalid Wasm memory export binding"))
    }

    pub(crate) fn wasm_tag(
        &self,
        instance: &crate::WasmInstance,
        index: u32,
    ) -> Result<Value, JsError> {
        if index as usize >= instance.module.tags.len() {
            return Err(JsError::validation("Wasm tag index out of bounds".into()));
        }
        let module = &instance.module;
        let base = module.globals.len()
            + module.memories.len()
            + module.data.len()
            + module.tables.len()
            + module.elements.len();
        let environment = self
            .root_value(instance.environment)
            .ok_or_else(|| JsError::validation("released Wasm instance".into()))?;
        self.heap
            .environment_slot(environment, base + index as usize)
            .filter(|value| matches!(self.heap.get(*value), Some(Cell::WasmTag { .. })))
            .ok_or_else(|| JsError::validation("invalid Wasm tag binding".into()))
    }

    pub(crate) fn wasm_table(
        &self,
        instance: &crate::WasmInstance,
        index: u32,
    ) -> Result<Value, JsError> {
        instance
            .module
            .tables
            .get(index as usize)
            .ok_or_else(|| JsError::wasm_link("unknown Wasm table export"))?;
        let environment = self
            .root_value(instance.environment)
            .ok_or_else(|| JsError::wasm_link("invalid Wasm instance root"))?;
        let slot = instance.module.globals.len()
            + instance.module.memories.len()
            + instance.module.data.len()
            + index as usize;
        self.heap
            .environment_slot(environment, slot)
            .ok_or_else(|| JsError::wasm_link("invalid Wasm table export binding"))
    }

    pub(crate) fn wasm_global(
        &self,
        instance: &crate::WasmInstance,
        index: u32,
    ) -> Result<crate::WasmValue, JsError> {
        let binding = self.wasm_global_binding(instance, index)?;
        let Some(Cell::WasmGlobal {
            value, ty, header, ..
        }) = self.heap.get(binding)
        else {
            return Err(JsError::validation("invalid Wasm global binding".into()));
        };
        self.decode_wasm_value_in(*value, *ty, Some(&header.declarations))
    }

    pub(crate) fn wasm_global_binding(
        &self,
        instance: &crate::WasmInstance,
        index: u32,
    ) -> Result<Value, JsError> {
        instance
            .module
            .globals
            .get(index as usize)
            .ok_or_else(|| JsError::wasm_link("Wasm global index out of bounds"))?;
        let environment = self
            .root_value(instance.environment)
            .ok_or_else(|| JsError::wasm_link("invalid Wasm instance root"))?;
        self.heap
            .environment_slot(environment, index as usize)
            .ok_or_else(|| JsError::wasm_link("invalid Wasm global binding"))
    }

    pub(super) fn wasm_global_load(&self, binding: Value) -> Result<Value, JsError> {
        match self.heap.get(binding) {
            Some(Cell::WasmGlobal { value, .. }) => Ok(*value),
            _ => Err(JsError::validation("invalid Wasm global binding".into())),
        }
    }

    pub(super) fn wasm_global_store(
        &mut self,
        binding: Value,
        value: Value,
    ) -> Result<(), JsError> {
        match self.heap.get_mut(binding) {
            Some(Cell::WasmGlobal {
                value: stored,
                mutable: true,
                ..
            }) => {
                *stored = value;
                Ok(())
            }
            _ => Err(JsError::validation(
                "invalid or immutable Wasm global binding".into(),
            )),
        }
    }

    pub(super) fn wasm_memory_bytes(
        &self,
        memory: Value,
    ) -> Result<crate::wasm::memory::MemoryRead<'_>, JsError> {
        match self.heap.get(memory) {
            Some(Cell::WasmMemory { bytes, .. }) => {
                Ok(crate::wasm::memory::MemoryRead::Memory(bytes.lock()))
            }
            Some(Cell::ArrayBuffer {
                bytes,
                detached: false,
                ..
            }) => Ok(crate::wasm::memory::MemoryRead::Buffer(bytes.read())),
            _ => Err(JsError::validation("invalid Wasm memory binding".into())),
        }
    }

    pub(super) fn wasm_memory_type(
        &self,
        memory: Value,
    ) -> Result<wasmparser::MemoryType, JsError> {
        match self.heap.get(memory) {
            Some(Cell::WasmMemory { header, .. }) => Ok(header.ty),
            _ => Err(JsError::validation("invalid Wasm memory binding".into())),
        }
    }

    pub(super) fn wasm_memory_pages(&self, memory: Value) -> Result<u64, JsError> {
        let Some(Cell::WasmMemory { bytes, header, .. }) = self.heap.get(memory) else {
            return Err(JsError::validation("invalid Wasm memory binding".into()));
        };
        let ty = &header.ty;
        Ok((bytes.len() / ty.page_size() as usize) as u64)
    }

    pub(super) fn wasm_memory_index_type(&self, memory: Value) -> Result<crate::WasmType, JsError> {
        crate::WasmType::from_wasm(self.wasm_memory_type(memory)?.index_type())
            .ok_or_else(|| JsError::validation("invalid Wasm memory index type".into()))
    }

    pub(super) fn wasm_memory_index(&self, memory: Value, value: Value) -> Result<u64, JsError> {
        self.wasm_index_operand(value, self.wasm_memory_index_type(memory)?)
    }

    pub(super) fn encode_wasm_index_size(
        &mut self,
        ty: crate::WasmType,
        size: Option<u64>,
    ) -> Value {
        let value = match ty {
            crate::WasmType::I32 => crate::WasmValue::I32(
                size.map(|v| v as u32 as i32)
                    .unwrap_or(crate::wasm::INDEX_GROW_FAILURE as i32),
            ),
            crate::WasmType::I64 => crate::WasmValue::I64(
                size.map(|v| v as i64)
                    .unwrap_or(crate::wasm::INDEX_GROW_FAILURE),
            ),
            _ => unreachable!("memory/table index type"),
        };
        self.encode_wasm_value(value)
    }

    fn wasm_memory_mut(
        &self,
        memory: Value,
    ) -> Result<(crate::wasm::memory::MemoryGuard<'_>, usize), JsError> {
        match self.heap.get(memory) {
            Some(Cell::WasmMemory { bytes, header, .. }) => {
                Ok((bytes.lock(), crate::wasm::memory::maximum_bytes(&header.ty)))
            }
            _ => Err(JsError::validation(
                "invalid mutable Wasm memory binding".into(),
            )),
        }
    }

    /// An i32 operator on a validated i32 register and an i32 right operand.
    #[inline(always)]
    pub(super) fn wasm_i32_binary(
        &mut self,
        f: usize,
        i: crate::bytecode::WideInstruction,
        operator: crate::wasm::integer::I32BinaryOperator,
        right: i32,
    ) -> Result<(), JsError> {
        let left = self.read(f, i.register_b()).wasm_bits32() as i32;
        let value = operator
            .evaluate(left, right)
            .map_err(JsError::wasm_trap_error)?;
        self.write(f, i.result_register(), Value::integer(value));
        Ok(())
    }

    #[inline(always)]
    pub(super) fn wasm_i32_jump(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: crate::bytecode::WideInstruction,
        pc: &mut usize,
        comparison: crate::wasm::integer::I32BinaryOperator,
    ) -> Result<(), JsError> {
        let left = self.read(f, i.register_a()).wasm_bits32() as i32;
        let right = self.read(f, i.register_b()).wasm_bits32() as i32;
        let taken = comparison
            .evaluate(left, right)
            .map_err(JsError::wasm_trap_error)?
            != 0;
        self.wasm_jump(p, f, i, pc, taken);
        Ok(())
    }

    #[inline(always)]
    pub(super) fn wasm_i32_constant_jump(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: crate::bytecode::WideInstruction,
        pc: &mut usize,
        comparison: crate::wasm::integer::I32BinaryOperator,
    ) -> Result<(), JsError> {
        let left = self.read(f, i.register_a()).wasm_bits32() as i32;
        let right = i32::from(i.b() as i16);
        let taken = comparison
            .evaluate(left, right)
            .map_err(JsError::wasm_trap_error)?
            != 0;
        self.wasm_jump(p, f, i, pc, taken);
        Ok(())
    }

    /// Take a Wasm conditional jump. Backward edges are collection safepoints,
    /// like the unconditional `Jump` that closes every loop iteration.
    #[inline(always)]
    pub(super) fn wasm_jump(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: crate::bytecode::WideInstruction,
        pc: &mut usize,
        taken: bool,
    ) {
        if taken {
            let target = i.jump_target() as usize;
            let backward = target < *pc;
            *pc = target;
            if backward {
                self.frames[f].pc = target;
                self.maybe_collect(p);
            }
        }
    }

    /// Memory32 effective address of a direct access: the i32 operand plus
    /// the static offset, computed without overflow in the 64-bit domain.
    #[inline(always)]
    fn wasm_direct_address(
        &self,
        f: usize,
        i: crate::bytecode::WideInstruction,
    ) -> Result<u64, JsError> {
        let address = Self::wasm_u32_operand(self.read(f, i.register_c()))?;
        Ok(u64::from(address) + u64::from(i.imm()))
    }

    #[inline(always)]
    pub(super) fn wasm_direct_load(
        &mut self,
        f: usize,
        i: crate::bytecode::WideInstruction,
    ) -> Result<(), JsError> {
        let operator = crate::wasm::memory::MemoryLoad::from_direct_op(i.op())
            .expect("validated direct load opcode");
        let address = self.wasm_direct_address(f, i)?;
        let memory = self.read(f, i.register_b());
        let value = operator
            .read(&self.wasm_memory_bytes(memory)?, address)
            .map_err(JsError::wasm_trap_error)?;
        let value = self.encode_wasm_value(value);
        self.write(f, i.result_register(), value);
        Ok(())
    }

    #[inline(always)]
    pub(super) fn wasm_direct_store(
        &mut self,
        f: usize,
        i: crate::bytecode::WideInstruction,
    ) -> Result<(), JsError> {
        let operator = crate::wasm::memory::MemoryStore::from_direct_op(i.op())
            .expect("validated direct store opcode");
        let address = self.wasm_direct_address(f, i)?;
        let memory = self.read(f, i.register_b());
        self.wasm_memory_store(memory, address, operator, self.read(f, i.register_a()))
    }

    pub(super) fn wasm_memory_store(
        &mut self,
        memory: Value,
        address: u64,
        operator: crate::wasm::memory::MemoryStore,
        value: Value,
    ) -> Result<(), JsError> {
        let value = self.decode_wasm_value(value, operator.value_type())?;
        operator
            .write(&mut self.wasm_memory_mut(memory)?.0, address, value)
            .map_err(JsError::wasm_trap_error)
    }

    pub(super) fn wasm_u32_operand(value: Value) -> Result<u32, JsError> {
        value
            .as_int()
            .map(|value| value as u32)
            .ok_or_else(|| JsError::validation("invalid Wasm i32 operand".into()))
    }

    pub(super) fn wasm_memory_fill(
        &mut self,
        memory: Value,
        address: u64,
        byte: u32,
        length: u64,
    ) -> Result<(), JsError> {
        let range = crate::wasm::memory::checked_range(
            address,
            usize::try_from(length)
                .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsMemory))?,
            self.wasm_memory_bytes(memory)?.len(),
        )
        .map_err(JsError::wasm_trap_error)?;
        if range.is_empty() {
            return Ok(());
        }
        self.wasm_memory_mut(memory)?.0[range].fill(byte as u8);
        Ok(())
    }

    /// Copy memory or immutable data bytes; None is the dropped segment state.
    /// Check both ranges before mutation, including zero-length boundary cases.
    pub(super) fn wasm_copy_bytes(
        &mut self,
        destination: Value,
        source: Option<Value>,
        output: u64,
        input: u64,
        length: u64,
    ) -> Result<(), JsError> {
        let length = usize::try_from(length)
            .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsMemory))?;
        let available = source
            .map(|source| self.wasm_memory_bytes(source).map(|bytes| bytes.len()))
            .transpose()?
            .unwrap_or(0);
        let input = crate::wasm::memory::checked_range(input, length, available)
            .map_err(JsError::wasm_trap_error)?;
        let output = crate::wasm::memory::checked_range(
            output,
            length,
            self.wasm_memory_bytes(destination)?.len(),
        )
        .map_err(JsError::wasm_trap_error)?;
        if output.is_empty() {
            return Ok(());
        }
        let source = source.expect("nonempty source range");
        if source == destination {
            self.wasm_memory_mut(destination)?
                .0
                .copy_within(input, output.start);
        } else {
            let Some(Cell::WasmMemory {
                bytes: destination, ..
            }) = self.heap.get(destination)
            else {
                unreachable!("validated destination binding")
            };
            match self.heap.get(source) {
                Some(Cell::WasmMemory { bytes: source, .. }) => {
                    destination.copy_from(source, input, output)
                }
                Some(Cell::ArrayBuffer { bytes: source, .. }) => {
                    destination.lock()[output].copy_from_slice(&source.read()[input])
                }
                _ => unreachable!("validated source binding"),
            }
        }
        Ok(())
    }

    pub(super) fn wasm_memory_grow(
        &mut self,
        memory: Value,
        delta: u64,
    ) -> Result<Option<u64>, JsError> {
        let _order = crate::wasm::atomic::memory_order();
        let page_size = self.wasm_memory_type(memory)?.page_size() as usize;
        let (mut bytes, max_byte_length) = self.wasm_memory_mut(memory)?;
        let old_pages = bytes.len() / page_size;
        let Some(length) = usize::try_from(delta)
            .ok()
            .and_then(|delta| delta.checked_mul(page_size))
            .and_then(|extra| bytes.len().checked_add(extra))
            .filter(|length| *length <= max_byte_length)
        else {
            return Ok(None);
        };
        let before = bytes.capacity();
        if !bytes.grow_to(length) {
            return Ok(None);
        }
        let after = bytes.capacity();
        drop(bytes);
        self.heap.adjust_external_bytes(before, after);
        self.refresh_wasm_memory_buffer(memory);
        Ok(Some(old_pages as u64))
    }

    /// The ArrayBuffer viewing `memory`'s current bytes, created on first request; `None`
    /// for a shared memory, whose buffer is not supported yet.
    pub(super) fn wasm_memory_buffer(&mut self, memory: Value) -> Option<Value> {
        let Some(Cell::WasmMemory { bytes, buffer, .. }) = self.heap.get(memory) else {
            return None;
        };
        if !buffer.is_undefined() {
            return Some(*buffer);
        }
        let view = crate::heap::BufferBytes::wasm_view(bytes)?;
        let byte_length = view.len();
        let buffer = self.heap.alloc(Cell::ArrayBuffer {
            object: Box::new(Self::empty_object(self.array_buffer_proto)),
            bytes: view,
            shared: false,
            detached: false,
            max_byte_length: byte_length,
            resizable: false,
            immutable: false,
        });
        if let Some(Cell::WasmMemory {
            buffer: current, ..
        }) = self.heap.get_mut(memory)
        {
            *current = buffer;
        }
        Some(buffer)
    }

    /// After growth: detaches the buffer viewing the old bytes, so the next request makes
    /// a buffer of the new length.
    fn refresh_wasm_memory_buffer(&mut self, memory: Value) {
        let Some(Cell::WasmMemory { buffer, .. }) = self.heap.get_mut(memory) else {
            return;
        };
        let previous = std::mem::replace(buffer, Value::UNDEFINED);
        if !previous.is_undefined() {
            self.detach_array_buffer(previous);
        }
    }

    fn register_wasm_program(&mut self, module: &crate::WasmModule) -> Result<ProgramId, JsError> {
        let program = &module.program;
        if self.programs.len() == 0 {
            self.initialize_shared(program, super::builtins::RealmScope::Core)?;
        }
        let id = if let Some(id) = self.programs.find_shared(program) {
            id
        } else {
            let id = self
                .programs
                .insert_shared(program.clone())
                .ok_or_else(|| JsError::validation("too many residual programs".into()))?;
            self.append_program_cache_layout(id, program);
            self.materialize_program_constants(id, program);
            id
        };
        if !self.programs.attach_wasm_signatures(id, &module.signatures) {
            return Err(JsError::validation(
                "invalid or conflicting Wasm signature authority".into(),
            ));
        }
        Ok(id)
    }

    fn require_single_wasm_result(function: &crate::WasmFunction) -> Result<(), JsError> {
        if function.signature().results.len() > crate::wasm::SCALAR_RETURN_ARITY {
            return Err(JsError::validation(
                "function requires the multi-result Wasm boundary".into(),
            ));
        }
        Ok(())
    }

    fn execute_wasm_in_environment(
        &mut self,
        function: &crate::WasmFunction,
        args: &[crate::WasmValue],
        environment: Value,
    ) -> Result<Value, JsError> {
        let program = &function.module.program;
        program.validate().map_err(JsError::validation)?;
        if args.len() != function.signature().params.len() {
            return Err(JsError::validation("Wasm argument count mismatch".into()));
        }
        if args
            .iter()
            .zip(&function.signature().params)
            .any(|(value, ty)| !value.fits_type(*ty))
        {
            return Err(JsError::validation("Wasm argument type mismatch".into()));
        }
        for (&value, &ty) in args.iter().zip(&function.signature().params) {
            if let crate::WasmValue::FuncRef(reference)
            | crate::WasmValue::ExternRef(reference)
            | crate::WasmValue::GcRef(reference) = value
            {
                self.decode_wasm_value_in(
                    reference,
                    ty,
                    Some(&function.module.signatures.declarations),
                )?;
            }
        }
        let id = self.register_wasm_program(&function.module)?;
        let defined = function.module.signatures.defined_index(function.entry);
        let imported = if defined.is_none() {
            let previous = self.replace_active_program(id);
            let result = self.wasm_function_reference(program, function.entry, environment);
            self.set_active_program(previous);
            Some(result?)
        } else {
            None
        };
        let args = args
            .iter()
            .copied()
            .map(|value| self.encode_wasm_value(value))
            .collect::<Vec<_>>();
        let previous = self.replace_active_program(id);
        let compiled = defined.and_then(|index| self.run_compiled(id, environment, index, &args));
        let result = match (defined, imported) {
            _ if compiled.is_some() => compiled.expect("checked"),
            (Some(index), _) => self.call_user(
                program,
                index,
                environment,
                Value::UNDEFINED,
                &args,
                CallContext::Internal,
            ),
            (_, Some(callee)) => self.call_value(program, callee, Value::UNDEFINED, &args),
            _ => unreachable!("validated Wasm function target"),
        };
        self.set_active_program(previous);
        #[cfg(feature = "profile-aggregate")]
        if !self.profile.report_dispatch_census_if_enabled(id.raw())
            && let Some(code) = self.programs.get(id)
        {
            self.profile.report(&self.heap, id.raw(), &code);
        }
        result
    }

    fn decode_wasm_results(
        &self,
        function: &crate::WasmFunction,
        result: Value,
    ) -> Result<Vec<crate::WasmValue>, JsError> {
        self.decode_wasm_result_types(
            &function.signature().results,
            result,
            Some(&function.module.signatures.declarations),
        )
    }

    pub(super) fn decode_wasm_result_types(
        &self,
        types: &[crate::WasmType],
        result: Value,
        declarations: Option<&crate::WasmTypes>,
    ) -> Result<Vec<crate::WasmValue>, JsError> {
        match types {
            [] => Ok(vec![]),
            [ty] => self
                .decode_wasm_value_in(result, *ty, declarations)
                .map(|value| vec![value]),
            types => {
                let Some(elements) = self.heap.array_elements(result) else {
                    return Err(JsError::validation("invalid Wasm result bundle".into()));
                };
                if elements.len() != types.len() {
                    return Err(JsError::validation("Wasm result count mismatch".into()));
                }
                elements
                    .iter()
                    .zip(types)
                    .map(|(value, ty)| self.decode_wasm_value_in(*value, *ty, declarations))
                    .collect()
            }
        }
    }

    pub(super) fn wasm_i64_operand(&self, value: Value) -> Result<i64, JsError> {
        let crate::WasmValue::I64(value) = self.decode_wasm_value(value, crate::WasmType::I64)?
        else {
            unreachable!("decoded i64 operand")
        };
        Ok(value)
    }

    pub(super) fn wasm_f32_operand(&self, value: Value) -> Result<f32, JsError> {
        let crate::WasmValue::F32(bits) = self.decode_wasm_value(value, crate::WasmType::F32)?
        else {
            unreachable!("decoded f32 operand")
        };
        Ok(f32::from_bits(bits))
    }

    pub(super) fn wasm_f64_operand(&self, value: Value) -> Result<f64, JsError> {
        let crate::WasmValue::F64(bits) = self.decode_wasm_value(value, crate::WasmType::F64)?
        else {
            unreachable!("decoded f64 operand")
        };
        Ok(f64::from_bits(bits))
    }

    pub(super) fn encode_wasm_value(&mut self, value: crate::WasmValue) -> Value {
        if let crate::WasmValue::FuncRef(value)
        | crate::WasmValue::ExternRef(value)
        | crate::WasmValue::GcRef(value) = value
        {
            return value;
        }
        match value.bits().expect("numeric Wasm slot") {
            crate::wasm::ScalarBits::Bits32(bits) => Value::integer(bits as i32),
            crate::wasm::ScalarBits::Bits64(bits) => self.encode_bits64(bits),
            crate::wasm::ScalarBits::Bits128(bits) => self.heap.alloc_v128(bits),
        }
    }

    /// A 64-bit Wasm payload as a slot value: in place unless it falls in
    /// the tag space (`Value::from_bits64`).
    #[inline(always)]
    pub(crate) fn encode_bits64(&mut self, bits: u64) -> Value {
        Value::from_bits64(bits).unwrap_or_else(|| self.heap.alloc(Cell::WasmBits64(bits)))
    }

    /// The 64-bit Wasm payload of a slot value typed i64 or f64.
    #[inline(always)]
    pub(crate) fn decode_bits64(&self, value: Value) -> Option<u64> {
        value.bits64().or_else(|| match self.heap.get(value) {
            Some(Cell::WasmBits64(bits)) => Some(*bits),
            _ => None,
        })
    }

    pub(crate) fn decode_wasm_value(
        &self,
        value: Value,
        ty: crate::WasmType,
    ) -> Result<crate::WasmValue, JsError> {
        self.decode_wasm_value_in(value, ty, None)
    }

    pub(super) fn wasm_reference_valid_in(
        &self,
        value: Value,
        ty: crate::WasmType,
        declarations: Option<&crate::WasmTypes>,
    ) -> bool {
        match ty {
            crate::WasmType::Reference {
                kind: crate::WasmReferenceKind::DeclaredGc { .. },
                nullable,
            } => {
                let Some(target_owner) = declarations else {
                    return false;
                };
                if value.is_null() {
                    return nullable;
                }
                let Some(Cell::WasmGc {
                    declarations: owner,
                    ty: index,
                    ..
                }) = self.heap.get(value)
                else {
                    return false;
                };
                owner.reference_subtype(
                    wasmparser::RefType::new(
                        false,
                        wasmparser::HeapType::Exact(wasmparser::UnpackedIndex::Module(*index)),
                    )
                    .unwrap(),
                    target_owner,
                    ty.reference_type().unwrap(),
                )
            }
            crate::WasmType::Reference {
                kind: crate::WasmReferenceKind::DeclaredFunction { index, exact },
                nullable,
            } => {
                let Some(declarations) = declarations else {
                    return false;
                };
                if value.is_null() {
                    return nullable;
                }
                match self.heap.function(value) {
                    Some(FunctionRecord {
                        kind: FunctionKind::User(program, function),
                        ..
                    }) => self.programs.wasm_signatures(*program).is_some_and(|pool| {
                        pool.defined_type_index(*function).is_some_and(|actual| {
                            pool.matches_declaration(actual, declarations, index, exact)
                        })
                    }),
                    Some(FunctionRecord {
                        kind: FunctionKind::Native(Native::WasmHost),
                        ..
                    }) => self
                        .wasm_callable_signature(value)
                        .is_some_and(|signature| {
                            declarations.matches_implicit_function(index as usize, signature)
                        }),
                    _ => false,
                }
            }
            _ => ty
                .reference_type()
                .is_some_and(|ty| self.wasm_reference_valid(value, ty)),
        }
    }

    pub(super) fn decode_wasm_value_in(
        &self,
        value: Value,
        ty: crate::WasmType,
        declarations: Option<&crate::WasmTypes>,
    ) -> Result<crate::WasmValue, JsError> {
        // The commonest operands, 32-bit addresses and the 64-bit effective addresses computed
        // from them, held in place: the scalar decode below reaches the same values after the
        // reference and vector checks (an integer value is tagged, an in-place payload is not).
        match ty {
            crate::WasmType::I32 if let Some(bits) = value.as_int() => {
                return Ok(crate::WasmValue::I32(bits));
            }
            crate::WasmType::I64 if let Some(bits) = value.bits64() => {
                return Ok(crate::WasmValue::I64(bits as i64));
            }
            _ => {}
        }
        if let crate::WasmType::Reference { kind, .. } = ty {
            if ty.requires_declarations() && declarations.is_none() {
                return Err(JsError::validation(
                    "Wasm reference requires a declaration owner".into(),
                ));
            }
            if !self.wasm_reference_valid_in(value, ty, declarations) {
                let message = match kind {
                    crate::WasmReferenceKind::DeclaredGc { .. } => {
                        "unsupported Wasm GC reference representation"
                    }
                    crate::WasmReferenceKind::DeclaredFunction { .. } => {
                        "Wasm function reference type mismatch"
                    }
                    _ => "invalid Wasm reference representation",
                };
                return Err(JsError::validation(message.into()));
            }
            return Ok(match kind {
                crate::WasmReferenceKind::Function
                | crate::WasmReferenceKind::BottomFunction
                | crate::WasmReferenceKind::DeclaredFunction { .. } => {
                    crate::WasmValue::FuncRef(value)
                }
                crate::WasmReferenceKind::External | crate::WasmReferenceKind::BottomExternal => {
                    crate::WasmValue::ExternRef(value)
                }
                crate::WasmReferenceKind::Internal(_)
                | crate::WasmReferenceKind::DeclaredGc { .. } => crate::WasmValue::GcRef(value),
            });
        }
        // A vector lives only in its cell, which the scalar probes below would miss first.
        if ty == crate::WasmType::V128 {
            return match self.heap.v128(value) {
                Some(bits) => Ok(crate::WasmValue::V128(u128::from_le_bytes(bits))),
                _ => Err(JsError::validation("invalid Wasm scalar representation".into())),
            };
        }
        let bits = if let Some(bits) = value.as_int() {
            Some(crate::wasm::ScalarBits::Bits32(bits as u32))
        } else if let Some(bits) = self.decode_bits64(value) {
            Some(crate::wasm::ScalarBits::Bits64(bits))
        } else if let Some(bits) = self.heap.v128(value) {
            Some(crate::wasm::ScalarBits::Bits128(bits))
        } else {
            None
        };
        bits.and_then(|bits| ty.decode(bits))
            .ok_or_else(|| JsError::validation("invalid Wasm scalar representation".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmparser::Operator;



    #[cfg(not(quench_profiled))]
    #[test]
    fn javascript_and_wasm_jit_share_one_vm_and_backend() {
        let script = crate::Engine::compile(crate::ExecutionRequest::script(
            "function plusOne(value) { return value + 1; } for (let i = 0; i < 40; i++) plusOne(i);",
            "shared-js-wasm-jit.js",
        ))
        .unwrap();
        let wasm = crate::Engine::lower_wasm_module(
            "shared-js-wasm-jit-wasm",
            0,
            [crate::WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                    params: vec![],
                    results: vec![crate::WasmType::I32],
                }),
                locals: vec![],
                operators: [Operator::I32Const { value: 42 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            }],
        )
        .unwrap();

        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.execute(&script).unwrap();
        assert!(matches!(
            vm.programs
                .js_tier(super::super::program_store::ProgramId::MAIN, 1),
            Some(super::super::program_store::JsTierState::Compiled(_))
        ));
        let js_mapped_bytes = vm.programs.jit_mapped_bytes();
        assert!(js_mapped_bytes > 0);
        assert_eq!(vm.execute_wasm_i32(&wasm, &[]).unwrap(), Some(42));
        assert!(vm.programs.jit_mapped_bytes() > js_mapped_bytes);
    }

    #[test]
    fn compiled_wasm_first_call_compiles_and_enters_its_root() {
        let function = crate::Engine::lower_wasm_module(
            "compiled-wasm-first-root",
            0,
            [crate::WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                    params: vec![],
                    results: vec![crate::WasmType::I32],
                }),
                locals: vec![],
                operators: [Operator::I32Const { value: 42 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);

        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), Some(42));

        let program = vm
            .programs
            .find_shared(&function.module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("first call compiled the Wasm root");
        assert_eq!(compiled.native_entries(0), Some(1));
    }

    /// Compiled code checks for an interrupt on each backward edge and at each function entry:
    /// a run stops whether it repeats through a loop or only through calls.
    #[test]
    fn compiled_wasm_observes_termination_in_loops_and_loop_free_recursion() {
        use wasmparser::BlockType;
        let spin = vec![
            Operator::Loop {
                blockty: BlockType::Empty,
            },
            Operator::Br { relative_depth: 0 },
            Operator::End,
            Operator::I32Const { value: 0 },
            Operator::End,
        ];
        // fib(n) = n < 2 ? n : fib(n - 1) + fib(n - 2), with no loop.
        let fib = vec![
            Operator::LocalGet { local_index: 0 },
            Operator::I32Const { value: 2 },
            Operator::I32LtU,
            Operator::If {
                blockty: BlockType::Type(wasmparser::ValType::I32),
            },
            Operator::LocalGet { local_index: 0 },
            Operator::Else,
            Operator::LocalGet { local_index: 0 },
            Operator::I32Const { value: 1 },
            Operator::I32Sub,
            Operator::Call { function_index: 0 },
            Operator::LocalGet { local_index: 0 },
            Operator::I32Const { value: 2 },
            Operator::I32Sub,
            Operator::Call { function_index: 0 },
            Operator::I32Add,
            Operator::End,
            Operator::End,
        ];
        // block { loop { br_table [0] default 1 (i32.const 0) } }: the loop
        // repeats through the table alone.
        static TABLE_SPIN: &[u8] = &[
            0x02, 0x40, 0x03, 0x40, 0x41, 0x00, 0x0e, 0x01, 0x00, 0x01, 0x0b, 0x0b, 0x41, 0x00,
            0x0b,
        ];
        let table_spin: Vec<_> =
            wasmparser::OperatorsReader::new(wasmparser::BinaryReader::new(TABLE_SPIN, 0))
                .into_iter()
                .map(Result::unwrap)
                .collect();
        // loop { br_if 0 (i32.eqz (local.get 0)) } with a zero argument.
        let zero_spin = vec![
            Operator::Loop {
                blockty: BlockType::Empty,
            },
            Operator::LocalGet { local_index: 0 },
            Operator::I32Eqz,
            Operator::BrIf { relative_depth: 0 },
            Operator::End,
            Operator::I32Const { value: 0 },
            Operator::End,
        ];
        for (name, params, operators, args) in [
            ("compiled-wasm-spin", 0, spin, vec![]),
            ("compiled-wasm-table-spin", 0, table_spin, vec![]),
            ("compiled-wasm-zero-spin", 1, zero_spin, vec![0]),
            ("compiled-wasm-fib", 1, fib, vec![60]),
        ] {
            let function = crate::Engine::lower_wasm_i32_module(
                name,
                0,
                [(params, 0, true, operators.into_iter().map(Ok))],
            )
            .unwrap();
            let mut vm = Vm::new(crate::SystemHost);
            vm.set_execution_mode(crate::ExecutionMode::Compiled);
            let interrupt = vm.execution_interrupt_handle();
            let stopper = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                interrupt.request_termination();
            });
            let error = vm.execute_wasm_i32(&function, &args).unwrap_err();
            assert!(error.is_execution_terminated(), "{name}");
            stopper.join().unwrap();
        }
    }

    #[test]
    fn compiled_wasm_memory_size_and_fill_preserve_bounds_and_results() {
        let memarg = wasmparser::MemArg {
            align: 2,
            max_align: 2,
            offset: 0,
            memory: 0,
        };
        let module = crate::Engine::lower_wasm_module_with_state(
            "compiled-memory-size-fill",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![
                            crate::WasmType::I32,
                            crate::WasmType::I32,
                            crate::WasmType::I32,
                        ],
                        results: vec![crate::WasmType::I32],
                    }),
                    locals: vec![],
                    operators: vec![
                        Operator::LocalGet { local_index: 0 },
                        Operator::LocalGet { local_index: 1 },
                        Operator::LocalGet { local_index: 2 },
                        Operator::MemoryFill { mem: 0 },
                        Operator::I32Const { value: 0 },
                        Operator::I32Load { memarg },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![],
                        results: vec![crate::WasmType::I32],
                    }),
                    locals: vec![],
                    operators: vec![Operator::MemorySize { mem: 0 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
            ],
            &crate::WasmTypes::default(),
            &[],
            &[crate::WasmMemory {
                ty: wasmparser::MemoryType {
                    initial: 1,
                    maximum: Some(1),
                    memory64: false,
                    shared: false,
                    page_size_log2: None,
                },
                import: None,
            }],
            &[],
        )
        .unwrap();

        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        assert_eq!(
            vm.invoke_wasm(
                &instance,
                0,
                &[
                    crate::WasmValue::I32(0),
                    crate::WasmValue::I32(0x2a),
                    crate::WasmValue::I32(4),
                ],
            )
            .unwrap(),
            Some(crate::WasmValue::I32(0x2a2a_2a2a))
        );
        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(crate::WasmValue::I32(1))
        );
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("memory operations compiled");
        assert_eq!(compiled.native_entries(0), Some(1));
        assert_eq!(compiled.native_entries(1), Some(1));
        assert_eq!(
            vm.invoke_wasm(
                &instance,
                0,
                &[
                    crate::WasmValue::I32(65_535),
                    crate::WasmValue::I32(0xff),
                    crate::WasmValue::I32(2),
                ],
            )
            .unwrap_err()
            .wasm_trap(),
            Some(crate::WasmTrap::OutOfBoundsMemory)
        );
    }

    #[test]
    fn compiled_wasm_wrapping_addresses_preserve_load_store_and_traps() {
        let memarg = |offset| wasmparser::MemArg {
            align: 2,
            max_align: 2,
            offset,
            memory: 0,
        };
        let module = crate::Engine::lower_wasm_module_with_state(
            "compiled-wasm-wrapping-addresses",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![crate::WasmType::I32, crate::WasmType::I32],
                        results: vec![crate::WasmType::I32],
                    }),
                    locals: vec![],
                    operators: vec![
                        Operator::LocalGet { local_index: 0 },
                        Operator::LocalGet { local_index: 1 },
                        Operator::I32Add,
                        Operator::I32Load { memarg: memarg(0) },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![
                            crate::WasmType::I32,
                            crate::WasmType::I32,
                            crate::WasmType::I32,
                        ],
                        results: vec![],
                    }),
                    locals: vec![],
                    operators: vec![
                        Operator::LocalGet { local_index: 0 },
                        Operator::LocalGet { local_index: 1 },
                        Operator::I32Add,
                        Operator::LocalGet { local_index: 2 },
                        Operator::I32Store { memarg: memarg(0) },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![crate::WasmType::I32],
                        results: vec![crate::WasmType::I32],
                    }),
                    locals: vec![],
                    operators: vec![
                        Operator::LocalGet { local_index: 0 },
                        Operator::I32Load {
                            memarg: memarg(0x20),
                        },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
            ],
            &crate::WasmTypes::default(),
            &[],
            &[crate::WasmMemory {
                ty: wasmparser::MemoryType {
                    initial: 1,
                    maximum: Some(1),
                    memory64: false,
                    shared: false,
                    page_size_log2: None,
                },
                import: None,
            }],
            &[],
        )
        .unwrap();

        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        let base = crate::WasmValue::I32(-16);
        let index = crate::WasmValue::I32(0x30);
        let stored = crate::WasmValue::I32(0x1234_5678);

        // The i32 addition wraps to 0x20, so both operations name the same
        // low address instead of trapping at the 4 GiB boundary.
        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[base, index, stored])
                .unwrap(),
            None
        );
        assert_eq!(
            vm.invoke_wasm(&instance, 0, &[base, index]).unwrap(),
            Some(crate::WasmValue::I32(0x1234_5678))
        );
        // Writes through the wrapped form of an address read back through its low form,
        // and the other way round.
        assert_eq!(
            vm.invoke_wasm(
                &instance,
                1,
                &[
                    crate::WasmValue::I32(0x20),
                    crate::WasmValue::I32(0),
                    crate::WasmValue::I32(0x2468_1357),
                ],
            )
            .unwrap(),
            None
        );
        assert_eq!(
            vm.invoke_wasm(&instance, 0, &[base, index]).unwrap(),
            Some(crate::WasmValue::I32(0x2468_1357))
        );
        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[base, index, stored])
                .unwrap(),
            None
        );
        assert_eq!(
            vm.invoke_wasm(
                &instance,
                0,
                &[crate::WasmValue::I32(0x20), crate::WasmValue::I32(0)],
            )
            .unwrap(),
            Some(crate::WasmValue::I32(0x1234_5678))
        );

        // An i32 address of 0xffff_ffff still points outside this one-page
        // memory for a four-byte load.
        assert_eq!(
            vm.invoke_wasm(
                &instance,
                0,
                &[crate::WasmValue::I32(-16), crate::WasmValue::I32(0x0f),],
            )
            .unwrap_err()
            .wasm_trap(),
            Some(crate::WasmTrap::OutOfBoundsMemory)
        );

        // A carried i32 addition can wrap to the first byte past the
        // one-page memory; modulo arithmetic must not make it accessible.
        assert_eq!(
            vm.invoke_wasm(
                &instance,
                0,
                &[crate::WasmValue::I32(-16), crate::WasmValue::I32(0x1_0010),],
            )
            .unwrap_err()
            .wasm_trap(),
            Some(crate::WasmTrap::OutOfBoundsMemory)
        );

        // A carried address wrapping to len - 1 still traps for the full
        // four-byte access, even though its first byte is in bounds.
        assert_eq!(
            vm.invoke_wasm(
                &instance,
                0,
                &[crate::WasmValue::I32(-16), crate::WasmValue::I32(0x1_000f),],
            )
            .unwrap_err()
            .wasm_trap(),
            Some(crate::WasmTrap::OutOfBoundsMemory)
        );

        // A memarg offset is added without i32 wrapping; carrying it through
        // 2^32 must trap even though the memory reservation repeats there.
        assert_eq!(
            vm.invoke_wasm(&instance, 2, &[crate::WasmValue::I32(-16)])
                .unwrap_err()
                .wasm_trap(),
            Some(crate::WasmTrap::OutOfBoundsMemory)
        );

        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("wrapping memory accessors compiled");
        assert!(compiled.native_entries(0).unwrap_or_default() >= 4);
        assert!(compiled.native_entries(1).unwrap_or_default() >= 1);
        assert_eq!(compiled.native_entries(2), Some(1));
    }

    #[test]
    fn compiled_wasm_indirect_call_refreshes_guarded_memory_after_growth() {
        let signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let memarg = wasmparser::MemArg {
            align: 2,
            max_align: 2,
            offset: 0,
            memory: 0,
        };
        let module = crate::Engine::lower_wasm_module_with_tables(
            "compiled-indirect-growth",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 1 },
                        Operator::MemoryGrow { mem: 0 },
                        Operator::Drop,
                        Operator::I32Const { value: 73 },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::Drop,
                        Operator::I32Const { value: 65_536 },
                        Operator::I32Const { value: 0x5a },
                        Operator::I32Const { value: 1 },
                        Operator::MemoryFill { mem: 0 },
                        Operator::I32Const { value: 65_536 },
                        Operator::I32Load { memarg },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature),
                    locals: vec![],
                    operators: vec![Operator::MemorySize { mem: 0 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new(
                [],
                [wasmparser::ValType::I32],
            )]),
            &[],
            &[crate::WasmMemory {
                ty: wasmparser::MemoryType {
                    initial: 1,
                    maximum: Some(2),
                    memory64: false,
                    shared: false,
                    page_size_log2: None,
                },
                import: None,
            }],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Function(0).into(),
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(crate::WasmValue::I32(0x5a))
        );
        assert_eq!(
            vm.invoke_wasm(&instance, 2, &[]).unwrap(),
            Some(crate::WasmValue::I32(2))
        );
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("indirect caller compiled");
        assert_eq!(compiled.native_entries(1), Some(1));
        assert_eq!(compiled.native_entries(2), Some(1));
    }

    #[test]
    fn compiled_wasm_indirect_call_spills_large_signature_arguments() {
        let signature = crate::WasmSignature {
            params: vec![crate::WasmType::I32; 9],
            results: vec![crate::WasmType::I32],
        };
        let mut callee_operators = (0..9)
            .map(|local_index| Operator::LocalGet { local_index })
            .collect::<Vec<_>>();
        callee_operators.extend((0..8).map(|_| Operator::I32Add));
        callee_operators.push(Operator::End);
        let mut caller_operators = (1..=9)
            .map(|value| Operator::I32Const { value })
            .collect::<Vec<_>>();
        caller_operators.push(Operator::I32Const { value: 0 });
        caller_operators.push(Operator::CallIndirect {
            type_index: 0,
            table_index: 0,
        });
        caller_operators.push(Operator::End);

        let module = crate::Engine::lower_wasm_module_with_tables(
            "compiled-indirect-large-signature",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature),
                    locals: vec![],
                    operators: callee_operators.into_iter().map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![],
                        results: vec![crate::WasmType::I32],
                    }),
                    locals: vec![],
                    operators: caller_operators.into_iter().map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([
                wasmparser::FuncType::new(
                    std::iter::repeat_n(wasmparser::ValType::I32, 9),
                    [wasmparser::ValType::I32],
                ),
                wasmparser::FuncType::new([], [wasmparser::ValType::I32]),
            ]),
            &[],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Function(0).into(),
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(crate::WasmValue::I32(45))
        );
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("indirect caller compiled");
        // The indirect call enters the eligible nine-parameter callee in
        // native code, exercising the callout's spilled signature and
        // argument buffers on both sides of the boundary.
        assert_eq!(compiled.native_entries(0), Some(1));
        assert_eq!(compiled.native_entries(1), Some(1));
        assert_eq!(compiled.direct_reference_entries(0), Some(1));
    }

    #[test]
    fn compiled_wasm_indirect_call_with_no_result_uses_compiled_callee() {
        let module = crate::Engine::lower_wasm_module_with_tables(
            "compiled-indirect-no-result",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![],
                        results: vec![],
                    }),
                    locals: vec![],
                    operators: vec![Operator::End].into_iter().map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![],
                        results: vec![crate::WasmType::I32],
                    }),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::I32Const { value: 42 },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([
                wasmparser::FuncType::new([], []),
                wasmparser::FuncType::new([], [wasmparser::ValType::I32]),
            ]),
            &[],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Function(0).into(),
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(crate::WasmValue::I32(42))
        );
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("indirect caller compiled");
        assert_eq!(compiled.native_entries(0), Some(1));
        assert_eq!(compiled.native_entries(1), Some(1));
        assert_eq!(compiled.direct_reference_entries(0), Some(1));
    }

    #[test]
    fn compiled_wasm_hot_indirect_target_is_tiered_and_table_stays_dynamic() {
        let signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let module = crate::Engine::lower_wasm_module_with_tables(
            "compiled-hot-indirect-target-tier",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![Operator::Unreachable, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature),
                    locals: vec![],
                    operators: vec![Operator::I32Const { value: 9 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new(
                [],
                [wasmparser::ValType::I32],
            )]),
            &[],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Function(0).into(),
            }],
        )
        .unwrap();
        // With `unreachable` rejected from the eager artifact, the target is the uncompiled
        // function this test promotes.
        super::super::compiled::with_eager_unreachable_rejected_for_test(|| {
            let mut vm = Vm::new(crate::SystemHost);
            vm.set_execution_mode(crate::ExecutionMode::Compiled);
            let instance = vm.instantiate_wasm_module(&module).unwrap();
            let promotion_entries = super::super::program_store::WASM_JIT_PROMOTION_ENTRIES;
            for _ in 0..promotion_entries {
                assert_eq!(
                    vm.invoke_wasm(&instance, 1, &[]).unwrap_err().wasm_trap(),
                    Some(crate::WasmTrap::Unreachable)
                );
            }

            let program = vm
                .programs
                .find_shared(&module.program)
                .expect("registered Wasm program");
            let baseline = vm
                .programs
                .compiled(program)
                .flatten()
                .expect("indirect caller installed the eager artifact");
            let tier = vm
                .programs
                .wasm_tier(program, 0)
                .expect("hot indirect target received a root tier");
            assert_eq!(baseline.native_entries(1), Some(promotion_entries as u64));
            assert_eq!(tier.native_entries(0), Some(1));
            assert_eq!(tier.direct_reference_entries(0), Some(0));
            // The promoted target belongs to a separate artifact, so the caller
            // must keep using the resolver instead of retaining its code pointer.
            assert!(!baseline.reference_fast_cache_matches_target(0, 0));
            assert_eq!(
                vm.invoke_wasm(&instance, 1, &[]).unwrap_err().wasm_trap(),
                Some(crate::WasmTrap::Unreachable)
            );
            assert_eq!(tier.native_entries(0), Some(2));
            assert_eq!(tier.direct_reference_entries(0), Some(1));
            assert_eq!(
                vm.invoke_wasm(&instance, 1, &[]).unwrap_err().wasm_trap(),
                Some(crate::WasmTrap::Unreachable)
            );
            assert_eq!(tier.native_entries(0), Some(3));
            assert_eq!(tier.direct_reference_entries(0), Some(2));

            // The call-site cache has warmed with the exact table target. It must
            // notice replacement before entering native code, then cache the new
            // identity. Null still takes the authoritative resolver's trap path.
            let table = vm.wasm_table(&instance, 0).unwrap();
            let original = vm.wasm_function(&instance, 0).unwrap();
            assert_eq!(baseline.reference_call_cache_target(0), Some(original));
            assert!(baseline.reference_cache_hits() > 0);
            let replacement = vm.wasm_function(&instance, 2).unwrap();
            vm.wasm_table_fill(table, 0, replacement, 1).unwrap();
            assert_eq!(
                vm.invoke_wasm(&instance, 1, &[]).unwrap(),
                Some(crate::WasmValue::I32(9))
            );
            assert_eq!(baseline.reference_call_cache_target(0), Some(replacement));
            vm.wasm_table_fill(table, 0, Value::NULL, 1).unwrap();
            assert_eq!(
                vm.invoke_wasm(&instance, 1, &[]).unwrap_err().wasm_trap(),
                Some(crate::WasmTrap::UninitializedElement)
            );
        });
    }

    #[test]
    fn compiled_wasm_indirect_call_propagates_native_oob_trap() {
        let signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let memarg = wasmparser::MemArg {
            align: 2,
            max_align: 2,
            offset: 0,
            memory: 0,
        };
        let module = crate::Engine::lower_wasm_module_with_tables(
            "compiled-indirect-native-oob-trap",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 65_536 },
                        Operator::I32Load { memarg },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new(
                [],
                [wasmparser::ValType::I32],
            )]),
            &[],
            &[crate::WasmMemory {
                ty: wasmparser::MemoryType {
                    initial: 1,
                    maximum: Some(1),
                    memory64: false,
                    shared: false,
                    page_size_log2: None,
                },
                import: None,
            }],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Function(0).into(),
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::OutOfBoundsMemory)
        );
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("indirect caller compiled");
        assert_eq!(compiled.native_entries(0), Some(1));
        assert_eq!(compiled.native_entries(1), Some(1));
        assert_eq!(compiled.direct_reference_entries(0), Some(1));
    }

    #[test]
    fn compiled_wasm_indirect_call_propagates_nested_callout_error() {
        let signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let module = crate::Engine::lower_wasm_module_with_tables(
            "compiled-indirect-nested-callout-error",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![
                        // The direct native target raises through its own
                        // reference-call helper on the uninitialized slot.
                        Operator::I32Const { value: 1 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new(
                [],
                [wasmparser::ValType::I32],
            )]),
            &[],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Function(0).into(),
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::UndefinedElement)
        );
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("indirect caller and target compiled");
        assert_eq!(compiled.direct_reference_entries(0), Some(1));
    }

    #[test]
    fn compiled_wasm_indirect_call_uses_foreign_instance_memory() {
        let signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let memarg = wasmparser::MemArg {
            align: 2,
            max_align: 2,
            offset: 0,
            memory: 0,
        };
        let module = crate::Engine::lower_wasm_module_with_tables(
            "compiled-indirect-foreign-instance-memory",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::I32Load { memarg },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new(
                [],
                [wasmparser::ValType::I32],
            )]),
            &[],
            &[crate::WasmMemory {
                ty: wasmparser::MemoryType {
                    initial: 1,
                    maximum: Some(1),
                    memory64: false,
                    shared: false,
                    page_size_log2: None,
                },
                import: None,
            }],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Function(0).into(),
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let first = vm.instantiate_wasm_module(&module).unwrap();
        let second = vm.instantiate_wasm_module(&module).unwrap();
        let first_env = vm.root_value(first.environment).unwrap();
        let second_env = vm.root_value(second.environment).unwrap();
        assert_ne!(first_env, second_env);
        let first_memory = vm.wasm_memory(&first, 0).unwrap();
        let second_memory = vm.wasm_memory(&second, 0).unwrap();
        vm.wasm_memory_fill(first_memory, 0, 0x11, 4).unwrap();
        vm.wasm_memory_fill(second_memory, 0, 0x22, 4).unwrap();

        // Point the second instance's table at the first instance's function.
        // It has the same ProgramId and signature, but must retain its own env.
        let first_function = vm.wasm_function(&first, 0).unwrap();
        let second_table = vm.wasm_table(&second, 0).unwrap();
        vm.wasm_table_fill(second_table, 0, first_function, 1)
            .unwrap();
        assert_eq!(
            vm.invoke_wasm(&second, 1, &[]).unwrap(),
            Some(crate::WasmValue::I32(0x1111_1111))
        );

        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("indirect caller and target compiled");
        assert_eq!(compiled.native_entries(0), Some(1));
        assert_eq!(compiled.native_entries(1), Some(1));
        assert_eq!(compiled.direct_reference_entries(0), Some(0));
    }

    #[test]
    fn compiled_wasm_indirect_call_diagnostics_attribute_native_and_fallback_sites() {
        let signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let module = crate::Engine::lower_wasm_module_with_elements(
            "compiled-indirect-call-site-diagnostics",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![Operator::I32Const { value: 42 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    // TableSize stays interpreter-only, preserving a target
                    // that exercises the diagnostic fallback path even when
                    // small unconditional traps compile natively.
                    operators: vec![
                        Operator::TableSize { table: 0 },
                        Operator::Drop,
                        Operator::Unreachable,
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 1 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new(
                [],
                [wasmparser::ValType::I32],
            )]),
            &[],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 2,
                    maximum: Some(2),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Null.into(),
            }],
            &[crate::WasmElement {
                mode: crate::WasmElementMode::Active {
                    table: 0,
                    offset: crate::WasmValue::I32(0).into(),
                },
                element_type: wasmparser::RefType::FUNCREF,
                items: vec![
                    crate::WasmReferenceInitializer::Function(0),
                    crate::WasmReferenceInitializer::Function(1),
                ]
                .into(),
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let env = vm
            .root_value(instance.environment)
            .expect("live Wasm instance environment");
        assert!(vm.install_compiled_diagnostics_for_test(program, env));

        assert_eq!(
            vm.invoke_wasm(&instance, 2, &[]).unwrap(),
            Some(crate::WasmValue::I32(42))
        );
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("diagnostic Wasm program");
        assert!(compiled.reference_call_site_counts().unwrap().iter().any(
            |(function, _, total, native, fallback)| {
                *function == 2 && *total == 1 && *native == 1 && *fallback == 0
            }
        ));
        assert_eq!(compiled.same_artifact_direct_reference_hits(), Some(1));
        assert!(compiled.reference_call_target_counts().unwrap().iter().any(
            |(caller, _, target, total, native, same_artifact)| {
                *caller == 2 && *target == 0 && *total == 1 && *native == 1 && *same_artifact == 1
            }
        ));

        assert_eq!(
            vm.invoke_wasm(&instance, 3, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        assert!(compiled.reference_call_site_counts().unwrap().iter().any(
            |(function, _, total, native, fallback)| {
                *function == 3 && *total == 1 && *native == 0 && *fallback == 1
            }
        ));
        assert_eq!(compiled.same_artifact_direct_reference_hits(), Some(1));
        assert!(compiled.reference_call_target_counts().unwrap().iter().any(
            |(caller, _, target, total, native, same_artifact)| {
                *caller == 3 && *target == 1 && *total == 1 && *native == 0 && *same_artifact == 0
            }
        ));
    }

    #[test]
    fn compiled_wasm_indirect_call_reuses_artifact_local_native_target() {
        let signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let module = crate::Engine::lower_wasm_module_with_elements(
            "compiled-indirect-call-artifact-cache",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![Operator::I32Const { value: 21 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![wasmparser::ValType::I32, wasmparser::ValType::I32],
                    operators: vec![
                        Operator::I32Const { value: 2 },
                        Operator::LocalSet { local_index: 0 },
                        Operator::I32Const { value: 0 },
                        Operator::LocalSet { local_index: 1 },
                        Operator::Loop {
                            blockty: wasmparser::BlockType::Empty,
                        },
                        Operator::Call { function_index: 2 },
                        Operator::LocalGet { local_index: 1 },
                        Operator::I32Add,
                        Operator::LocalSet { local_index: 1 },
                        Operator::LocalGet { local_index: 0 },
                        Operator::I32Const { value: -1 },
                        Operator::I32Add,
                        Operator::LocalTee { local_index: 0 },
                        Operator::BrIf { relative_depth: 0 },
                        Operator::End,
                        Operator::LocalGet { local_index: 1 },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![Operator::I32Const { value: 5 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new(
                [],
                [wasmparser::ValType::I32],
            )]),
            &[],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Function(0).into(),
            }],
            &[crate::WasmElement {
                mode: crate::WasmElementMode::Active {
                    table: 0,
                    offset: crate::WasmValue::I32(0).into(),
                },
                element_type: wasmparser::RefType::FUNCREF,
                items: vec![crate::WasmReferenceInitializer::Function(0)].into(),
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let env = vm
            .root_value(instance.environment)
            .expect("live Wasm instance environment");
        assert!(vm.install_compiled_diagnostics_for_test(program, env));

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(crate::WasmValue::I32(42))
        );
        let indirect_callers = [false, false, true, false];
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("diagnostic Wasm program")
            .clone();
        assert!(compiled.reference_fast_cache_matches_target(0, 0));
        assert_eq!(
            compiled.profiled_indirect_targets(&vm, program, env, &indirect_callers),
            vec![0]
        );
        let table = vm.wasm_table(&instance, 0).unwrap();
        let replacement = vm.wasm_function(&instance, 3).unwrap();
        vm.wasm_table_fill(table, 0, replacement, 1).unwrap();
        assert!(
            compiled
                .profiled_indirect_targets(&vm, program, env, &indirect_callers)
                .is_empty()
        );
        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(crate::WasmValue::I32(10))
        );
        assert!(compiled.reference_fast_cache_matches_target(0, 3));
        assert_eq!(
            compiled.profiled_indirect_targets(&vm, program, env, &indirect_callers),
            vec![3]
        );
        vm.collect_now(module.residual());
        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(crate::WasmValue::I32(10))
        );
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("diagnostic Wasm program");
        assert_eq!(compiled.same_artifact_direct_reference_hits(), Some(3));
        assert!(compiled.reference_fast_cache_matches_target(0, 3));
        assert!(compiled.reference_call_site_counts().unwrap().iter().any(
            |(function, _, total, native, fallback)| {
                *function == 2 && *total == 3 && *native == 3 && *fallback == 0
            }
        ));
    }

    #[test]
    fn compiled_wasm_indirect_target_roots_across_reentrant_table_mutation_and_gc() {
        let signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let module = crate::Engine::lower_wasm_module_with_imports(
            "compiled-indirect-reentrant-mutation-gc",
            [
                // This target mutates its own table slot and collects while its
                // direct native body is still active, then returns the old result.
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![
                        Operator::Call { function_index: 0 },
                        Operator::Drop,
                        Operator::I32Const { value: 42 },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![Operator::I32Const { value: 9 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new(
                [],
                [wasmparser::ValType::I32],
            )]),
            &[],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                // Function index 0 is the import, so the first definition is 1.
                initializer: crate::WasmReferenceInitializer::Function(1).into(),
            }],
            &[],
            &[crate::WasmFunctionImport {
                exact: false,
                ty: crate::WasmCallableType::Embedded(signature.clone()),
                name: crate::WasmImportName {
                    index: 0,
                    module: "env".into(),
                    name: "mutateAndCollect".into(),
                },
            }],
        )
        .unwrap();

        let mut vm = Vm::new(crate::test_host::TestHost::default().with_test262());
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let bootstrap =
            crate::Engine::specialize("void 0", "jit-indirect-gc-bootstrap.js").unwrap();
        vm.execute(&bootstrap).unwrap();
        let helper = vm
            .compile_embedding_host_script(
                "() => { globalThis.__jitIndirectTable.set(0, globalThis.__jitIndirectReplacement); $262.gc(); return 7; }",
                "jit-indirect-reentrant-mutation-gc.js",
                (0, 0),
            )
            .unwrap();
        let callback = vm.evaluate_embedding_compiled_script(&helper).unwrap();
        let imported = vm
            .create_wasm_host_function(
                "mutateAndCollect",
                crate::heap::WasmHostTarget::Js(callback),
                signature,
            )
            .unwrap();
        let imported_root = vm.root(imported);
        let instance = vm
            .instantiate_wasm_module_with_imports(&module, &[imported_root])
            .unwrap();
        let table = vm.wasm_table(&instance, 0).unwrap();
        let replacement = vm.wasm_function(&instance, 2).unwrap();
        let replacement_wrapper = vm.wasm_js_function(replacement, Some(2)).unwrap();
        vm.set_named(
            module.residual(),
            vm.realm.globals,
            "__jitIndirectTable",
            table,
        )
        .unwrap();
        vm.set_named(
            module.residual(),
            vm.realm.globals,
            "__jitIndirectReplacement",
            replacement_wrapper,
        )
        .unwrap();

        let roots_before = vm.heap.root_count_for_test();
        let collections_before = vm.heap.collection_epoch();
        assert_eq!(
            vm.invoke_wasm(&instance, 3, &[]).unwrap(),
            Some(crate::WasmValue::I32(42)),
            "the active native callee must finish after the callback replaces its table entry"
        );
        assert_eq!(vm.heap.collection_epoch(), collections_before + 1);
        assert_eq!(vm.wasm_table_get(table, 0).unwrap(), replacement);
        assert_eq!(vm.heap.root_count_for_test(), roots_before);

        assert_eq!(
            vm.invoke_wasm(&instance, 3, &[]).unwrap(),
            Some(crate::WasmValue::I32(9))
        );
        assert_eq!(vm.heap.root_count_for_test(), roots_before);
        assert_eq!(
            vm.invoke_wasm(&instance, 3, &[]).unwrap(),
            Some(crate::WasmValue::I32(9)),
            "the warmed direct cache must target the replacement"
        );
        assert_eq!(vm.heap.root_count_for_test(), roots_before);
    }

    #[test]
    fn unguarded_memory_indirect_growth_stays_on_the_vm_fallback() {
        let callee_signature = crate::WasmSignature {
            params: vec![],
            results: vec![],
        };
        let caller_signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let memarg = wasmparser::MemArg {
            align: 2,
            max_align: 2,
            offset: 0,
            memory: 0,
        };
        let module = crate::Engine::lower_wasm_module_with_tables(
            "unguarded-indirect-memory-growth",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(callee_signature),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 1 },
                        Operator::MemoryGrow { mem: 0 },
                        Operator::Drop,
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(caller_signature),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::CallIndirect {
                            type_index: 0,
                            table_index: 0,
                        },
                        Operator::I32Const { value: 65_536 },
                        Operator::I32Const { value: 0x5a },
                        Operator::I32Const { value: 1 },
                        Operator::MemoryFill { mem: 0 },
                        Operator::I32Const { value: 65_536 },
                        Operator::I32Load { memarg },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new([], [])]),
            &[],
            &[crate::WasmMemory {
                ty: wasmparser::MemoryType {
                    initial: 1,
                    maximum: Some(2),
                    memory64: false,
                    shared: false,
                    page_size_log2: None,
                },
                import: None,
            }],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Function(0).into(),
            }],
        )
        .unwrap();

        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        let memory = vm.wasm_memory(&instance, 0).unwrap();
        let Cell::WasmMemory { bytes, .. } = vm.heap.get_mut(memory).unwrap() else {
            panic!("memory binding must name a Wasm memory");
        };
        *bytes = std::rc::Rc::new(crate::wasm::memory::MemoryStorage::new(
            crate::wasm::memory::LinearBytes::heap(vec![0; 65_536]),
            false,
        ));
        let Cell::WasmMemory { bytes, .. } = vm.heap.get(memory).unwrap() else {
            unreachable!("memory binding was just checked");
        };
        let crate::wasm::memory::MemoryStorage::Unshared(state) = &**bytes else {
            unreachable!("memory is unshared");
        };
        assert!(state.borrow().bytes.guard().is_none());

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(crate::WasmValue::I32(0x5a))
        );
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        assert!(
            vm.programs
                .compiled(program)
                .flatten()
                .is_none_or(|compiled| compiled.native_entries(1).is_none())
        );
    }

    #[test]
    fn compiled_wasm_unrolled_loop_runs_every_trip_count() {
        // A small program's innermost loop runs as copies of its body; every trip count,
        // a multiple of the copies or not, takes the same values as the loop once.
        let signature = crate::WasmSignature {
            params: vec![crate::WasmType::I32],
            results: vec![crate::WasmType::I32],
        };
        let module = crate::Engine::lower_wasm_module_for(
            crate::ExecutionMode::Compiled,
            "compiled-wasm-unrolled-loop",
            [crate::WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(signature),
                locals: vec![wasmparser::ValType::I32, wasmparser::ValType::I32],
                operators: vec![
                    Operator::Block { blockty: wasmparser::BlockType::Empty },
                    Operator::LocalGet { local_index: 0 },
                    Operator::I32Eqz,
                    Operator::BrIf { relative_depth: 0 },
                    Operator::Loop { blockty: wasmparser::BlockType::Empty },
                    Operator::LocalGet { local_index: 2 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::I32Add,
                    Operator::LocalSet { local_index: 2 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::I32Const { value: 1 },
                    Operator::I32Add,
                    Operator::LocalTee { local_index: 1 },
                    Operator::LocalGet { local_index: 0 },
                    Operator::I32LtU,
                    Operator::BrIf { relative_depth: 0 },
                    Operator::End,
                    Operator::End,
                    Operator::LocalGet { local_index: 2 },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            }],
            &crate::WasmTypes::default(),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        for trips in 0..13 {
            assert_eq!(
                vm.invoke_wasm(&instance, 0, &[crate::WasmValue::I32(trips)]).unwrap(),
                Some(crate::WasmValue::I32(trips * (trips - 1) / 2)),
                "{trips} trips"
            );
        }
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm.programs.compiled(program).flatten().expect("compiled loop");
        assert_eq!(compiled.native_entries(0), Some(13));
    }

    #[test]
    fn compiled_wasm_fallback_caller_keeps_native_callee_entries() {
        crate::wasm::COMPILED_CALLS_KEPT_FOR_TEST.with(|kept| kept.set(true));
        let caller_signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let callee_signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let helper_signature = crate::WasmSignature {
            params: vec![crate::WasmType::I32],
            results: vec![crate::WasmType::I32],
        };
        let module = crate::Engine::lower_wasm_module_for(
            crate::ExecutionMode::Compiled,
            "compiled-wasm-fallback-caller",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(caller_signature),
                    locals: vec![],
                    operators: vec![
                        Operator::Call { function_index: 1 },
                        Operator::I32Const { value: 0 },
                        Operator::I32Add,
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(callee_signature),
                    locals: vec![],
                    operators: vec![
                        // This helper is interpreter-only. Its call to the
                        // compiled leaf must still leave the fast lane.
                        Operator::TableSize { table: 0 },
                        Operator::Call { function_index: 2 },
                        Operator::I32Const { value: 0 },
                        Operator::I32Add,
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(helper_signature),
                    locals: vec![],
                    operators: vec![
                        Operator::LocalGet { local_index: 0 },
                        Operator::I32Const { value: 1 },
                        Operator::I32Add,
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
            ],
            &crate::WasmTypes::default(),
            &[],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Null.into(),
            }],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();

        assert_eq!(
            vm.invoke_wasm(&instance, 0, &[]).unwrap(),
            Some(crate::WasmValue::I32(2))
        );

        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("compiled callee");
        assert_eq!(compiled.native_entries(0), None);
        assert_eq!(compiled.native_entries(1), None);
        assert_eq!(compiled.native_entries(2), Some(1));

        // A selected native root can keep a known call to an unsupported
        // function as a VM boundary; that interpreter frame can still enter
        // a native leaf through the normal dispatcher.
        let mut root_vm = Vm::new(crate::SystemHost);
        root_vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let root_instance = root_vm.instantiate_wasm_module(&module).unwrap();
        let root_environment = root_vm.root_value(root_instance.environment).unwrap();
        let root_program = root_vm
            .programs
            .find_shared(&module.program)
            .expect("registered root-closure Wasm program");
        assert!(root_vm.install_compiled_root_for_test(root_program, root_environment, 0));
        assert_eq!(
            root_vm.invoke_wasm(&root_instance, 0, &[]).unwrap(),
            Some(crate::WasmValue::I32(2))
        );
        let root_compiled = root_vm
            .programs
            .compiled(root_program)
            .flatten()
            .expect("root closure installed");
        assert_eq!(root_compiled.native_entries(0), Some(1));
        assert_eq!(root_compiled.native_entries(1), None);
        assert_eq!(root_compiled.native_entries(2), Some(1));
    }

    #[test]
    fn compiled_wasm_known_fallback_offsets_defined_index_by_import_count() {
        crate::wasm::COMPILED_CALLS_KEPT_FOR_TEST.with(|kept| kept.set(true));
        let signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        };
        let owner = crate::Engine::lower_wasm_i32_module(
            "compiled-wasm-fallback-import-owner",
            0,
            [(
                0,
                0,
                true,
                vec![Operator::I32Const { value: 99 }, Operator::End],
            )]
            .into_iter()
            .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap();
        let module = crate::Engine::lower_wasm_module_for(
            crate::ExecutionMode::Compiled,
            "compiled-wasm-known-fallback-index",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    // The imported function occupies absolute index zero, so
                    // defined function one is absolute index two/local index
                    // one in this residual program.
                    operators: vec![Operator::Call { function_index: 2 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    // TableSize remains interpreter-only. Its known call to
                    // the native leaf must survive the fallback boundary.
                    operators: vec![
                        Operator::TableSize { table: 0 },
                        Operator::Drop,
                        Operator::Call { function_index: 3 },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![Operator::I32Const { value: 42 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
            ],
            &crate::WasmTypes::default(),
            &[],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::FUNCREF,
                    table64: false,
                    initial: 1,
                    maximum: Some(1),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Null.into(),
            }],
            &[],
            &[crate::WasmFunctionImport {
                exact: false,
                ty: crate::WasmCallableType::Embedded(signature),
                name: crate::WasmImportName {
                    index: 0,
                    module: "owner".into(),
                    name: "get".into(),
                },
            }],
            &[],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let owner_instance = vm.instantiate_wasm_module(&owner.module).unwrap();
        let owner_function = vm.wasm_function(&owner_instance, 0).unwrap();
        let owner_function_root = vm.root(owner_function);
        let instance = vm
            .instantiate_wasm_module_with_imports(&module, &[owner_function_root])
            .unwrap();
        let environment = vm.root_value(instance.environment).unwrap();
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered imported Wasm program");
        assert!(vm.install_compiled_root_for_test(program, environment, 0));

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(crate::WasmValue::I32(42))
        );
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("root closure installed");
        assert_eq!(compiled.native_entries(0), Some(1));
        assert_eq!(compiled.native_entries(1), None);
        assert_eq!(compiled.native_entries(2), Some(1));
    }

    #[test]
    fn tag_import_roots_transfer_identity_without_retaining_exporter_environment() {
        type Body = crate::WasmFunctionBody<
            std::vec::IntoIter<Result<Operator<'static>, wasmparser::BinaryReaderError>>,
        >;
        let types = crate::WasmTypes::from_functions([
            wasmparser::FuncType::new([wasmparser::ValType::I32], []),
            wasmparser::FuncType::new([], [wasmparser::ValType::I32]),
        ]);
        let make = |tags: &[crate::WasmTag]| {
            crate::Engine::lower_wasm_module_with_tags(
                "tag-root-owner",
                std::iter::empty::<Body>(),
                &types,
                &[],
                &[],
                &[],
                &[],
                &[],
                &[],
                tags,
            )
        };
        let exported = make(&[
            crate::WasmTag {
                ty: 0,
                import: None,
            },
            crate::WasmTag {
                ty: 0,
                import: None,
            },
        ])
        .unwrap();
        assert!(
            make(&[crate::WasmTag {
                ty: 1,
                import: None
            }])
            .is_err()
        );
        assert!(
            make(&[crate::WasmTag {
                ty: u32::MAX,
                import: None
            }])
            .is_err()
        );
        let imported = make(&[crate::WasmTag {
            ty: 0,
            import: Some(crate::WasmImportName {
                index: 0,
                module: "owner".into(),
                name: "tag".into(),
            }),
        }])
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        let owner = vm.instantiate_wasm_module(&exported).unwrap();
        let owner_env = vm.root_value(owner.environment).unwrap();
        let tag = vm.wasm_tag(&owner, 0).unwrap();
        assert_ne!(tag, vm.wasm_tag(&owner, 1).unwrap());
        let other_owner = vm.instantiate_wasm_module(&exported).unwrap();
        assert_ne!(tag, vm.wasm_tag(&other_owner, 0).unwrap());
        vm.release_root(other_owner.environment);
        let handle = vm.root(tag);
        let first = vm
            .instantiate_wasm_module_with_imports(&imported, &[handle])
            .unwrap();
        assert_eq!(vm.wasm_tag(&first, 0).unwrap(), tag);
        let second = vm
            .instantiate_wasm_module_with_imports(&imported, &[handle])
            .unwrap();
        vm.release_root(handle);
        vm.release_root(owner.environment);
        vm.release_root(first.environment);
        vm.collect_now(imported.residual());
        assert!(!vm.heap.is_live(owner_env));
        assert_eq!(vm.wasm_tag(&second, 0).unwrap(), tag);
        assert!(matches!(
            vm.heap.get(tag),
            Some(Cell::WasmTag { ty: 0, .. })
        ));
        let roots = vm.heap.root_count_for_test();
        assert!(
            vm.instantiate_wasm_module_with_imports(&imported, &[handle])
                .is_err()
        );
        assert_eq!(vm.heap.root_count_for_test(), roots);
        vm.release_root(second.environment);
        vm.collect_now(imported.residual());
        assert!(!vm.heap.is_live(tag));
    }

    #[test]
    fn memory_import_roots_transfer_bytes_owner_without_retaining_exporter_environment() {
        let make = |import, maximum, shared| {
            crate::Engine::lower_wasm_module_with_state(
                "memory-import-owner",
                [crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![],
                        results: vec![],
                    }),
                    locals: vec![],
                    operators: [Operator::End].into_iter().map(Ok),
                }],
                &crate::WasmTypes::default(),
                &[],
                &[crate::WasmMemory {
                    ty: wasmparser::MemoryType {
                        initial: 1,
                        maximum: maximum,
                        memory64: false,
                        shared,
                        page_size_log2: None,
                    },
                    import,
                }],
                &[],
            )
            .unwrap()
        };
        for shared in [false, true] {
            let exported = make(None, Some(3), shared);
            let imported = make(
                Some(crate::WasmImportName {
                    index: 0,
                    module: "owner".into(),
                    name: "memory".into(),
                }),
                Some(4),
                shared,
            );
            let mut vm = Vm::new(crate::SystemHost);
            let owner = vm.instantiate_wasm_module(&exported).unwrap();
            let owner_env = vm.root_value(owner.environment).unwrap();
            let memory = vm.wasm_memory(&owner, 0).unwrap();
            let handle = vm.root(memory);
            let mismatched = make(
                Some(crate::WasmImportName {
                    index: 0,
                    module: "owner".into(),
                    name: "memory".into(),
                }),
                Some(4),
                !shared,
            );
            let roots = vm.heap.root_count_for_test();
            assert!(
                vm.instantiate_wasm_module_with_imports(&mismatched, &[handle])
                    .err()
                    .unwrap()
                    .wasm_link_error()
                    .is_some()
            );
            assert_eq!(vm.heap.root_count_for_test(), roots);
            let first = vm
                .instantiate_wasm_module_with_imports(&imported, &[handle])
                .unwrap();
            let projected = vm.wasm_memory(&first, 0).unwrap();
            assert_eq!(projected, memory);
            let second_handle = vm.root(projected);
            let second = vm
                .instantiate_wasm_module_with_imports(&imported, &[second_handle])
                .unwrap();
            vm.release_root(handle);
            vm.release_root(second_handle);
            vm.release_root(owner.environment);
            vm.release_root(first.environment);
            vm.collect_now(imported.residual());
            assert!(!vm.heap.is_live(owner_env));
            assert!(matches!(
                vm.heap.get(memory),
                Some(Cell::WasmMemory { header, .. }) if header.ty.maximum == Some(3)
            ));
            vm.wasm_memory_store(
                memory,
                7,
                crate::wasm::memory::MemoryStore::I32Store,
                Value::integer(23),
            )
            .unwrap();
            assert_eq!(
                vm.wasm_memory_bytes(vm.wasm_memory(&second, 0).unwrap())
                    .unwrap()[7],
                23
            );
            let roots = vm.heap.root_count_for_test();
            assert!(
                vm.instantiate_wasm_module_with_imports(&imported, &[handle])
                    .err()
                    .unwrap()
                    .wasm_link_error()
                    .is_some()
            );
            assert_eq!(vm.heap.root_count_for_test(), roots);
            vm.release_root(second.environment);
            vm.collect_now(imported.residual());
            assert!(!vm.heap.is_live(memory));
        }
    }

    #[test]
    fn failed_start_releases_instance_root_and_restores_shared_activation_owner() {
        let function = crate::Engine::lower_wasm_i32_function(
            "start-owner",
            0,
            0,
            false,
            [Operator::Unreachable, Operator::End].into_iter().map(Ok),
        )
        .unwrap();
        assert!(function.module.clone().with_start(1).is_err());
        let module = function.module.with_start(0).unwrap();
        let entry = module.function(0).unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        let script = crate::Engine::specialize("0;", "start-owner.js").unwrap();
        vm.execute(&script).unwrap();
        let host_value = vm.heap.alloc_string("start-host-root".into());
        let host_root = vm.root(host_value);
        let roots = vm.heap.root_count_for_test();
        // A module carrying an instantiation effect cannot bypass that effect
        // through the unbound entry API, including when it has no state slots.
        assert!(
            vm.execute_wasm(&entry, &[])
                .unwrap_err()
                .wasm_trap()
                .is_none()
        );
        for _ in 0..3 {
            assert_eq!(
                vm.instantiate_wasm_module(&module)
                    .err()
                    .unwrap()
                    .wasm_trap(),
                Some(crate::WasmTrap::Unreachable)
            );
            assert_eq!(vm.active_program, ProgramId::MAIN);
            assert!(vm.frames.is_empty());
            assert_eq!(vm.heap.root_count_for_test(), roots);
            vm.collect_now(&script);
            assert_eq!(vm.root_value(host_root), Some(host_value));
            assert!(vm.heap.is_string(host_value));
        }
        vm.release_root(host_root);
    }

    #[test]
    fn wasm_entry_views_preserve_js_roots_and_restore_program_after_traps() {
        let script = crate::Engine::specialize("0;", "shared-root.js").unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.execute(&script).unwrap();
        let value = vm.heap.alloc_string("host-owned".into());
        let root = vm.root(value);
        let module = crate::Engine::lower_wasm_i32_module(
            "shared-entries",
            0,
            [
                (
                    0,
                    0,
                    true,
                    vec![Operator::I32Const { value: 42 }, Operator::End],
                ),
                (0, 0, true, vec![Operator::Unreachable, Operator::End]),
            ]
            .into_iter()
            .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap();
        let trap = module.select_function(1).unwrap();
        assert!(module.select_function(2).is_none());
        assert!(Rc::ptr_eq(&module.module.program, &trap.module.program));
        for _ in 0..3 {
            assert_eq!(vm.execute_wasm_i32(&module, &[]).unwrap(), Some(42));
            assert_eq!(
                vm.execute_wasm_i32(&trap, &[]).unwrap_err().wasm_trap(),
                Some(crate::WasmTrap::Unreachable)
            );
            assert_eq!(vm.active_program, ProgramId::MAIN);
            assert!(vm.frames.is_empty());
            assert_eq!(vm.programs.len(), 2);
            vm.collect_now(&script);
            assert_eq!(vm.root_value(root), Some(value));
            assert!(vm.heap.string(value).is_some_and(|text| text.host_string() == "host-owned"));
        }
        // A deliberate new JS execution retains its existing reset contract.
        vm.execute(&script).unwrap();
        assert!(vm.root_value(root).is_none());
        assert_eq!(vm.programs.len(), 1);
        assert_eq!(vm.execute_wasm_i32(&module, &[]).unwrap(), Some(42));
        assert_eq!(vm.programs.len(), 2);
    }

    #[test]
    fn wasm_program_constants_remain_owned_across_registration_and_collection() {
        let make = |bits| {
            crate::Engine::lower_wasm_module(
                "constant-owner",
                0,
                [crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![],
                        results: vec![crate::WasmType::I64],
                    }),
                    locals: vec![],
                    operators: vec![Operator::I64Const { value: bits }, Operator::End]
                        .into_iter()
                        .map(Ok),
                }],
            )
            .unwrap()
        };
        let first = make(i64::MIN);
        let second = make(i64::MAX);
        let mut vm = Vm::new(crate::SystemHost);
        for _ in 0..3 {
            for (function, expected) in [(&first, i64::MIN), (&second, i64::MAX)] {
                assert_eq!(
                    vm.execute_wasm(function, &[]).unwrap(),
                    Some(crate::WasmValue::I64(expected))
                );
                vm.collect_now(function.residual());
            }
            assert_eq!(vm.programs.len(), 2);
            assert_eq!(vm.active_program, ProgramId::MAIN);
        }
    }

    #[test]
    fn serialized_function_imports_preserve_identity_capture_and_index_domains_through_gc() {
        use crate::{
            Engine, WasmFunctionBody, WasmFunctionImport, WasmGlobal, WasmImportName,
            WasmSignature, WasmType, WasmValue,
        };
        let signature = WasmSignature {
            params: vec![],
            results: vec![WasmType::I64],
        };
        let owner = Engine::lower_wasm_module_definition(
            "function-owner",
            [WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                operators: vec![Operator::GlobalGet { global_index: 0 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            }],
            &crate::WasmTypes::default(),
            &[WasmGlobal {
                initial: WasmValue::I64(i64::MAX).into(),
                mutable: false,
            }],
        )
        .unwrap();
        let imports: Vec<_> = (0..3)
            .map(|index| WasmFunctionImport {
                exact: false,
                ty: crate::WasmCallableType::Embedded(signature.clone()),
                name: WasmImportName {
                    index,
                    module: "owner".into(),
                    name: "get".into(),
                },
            })
            .collect();
        let mut globals = Vec::new();
        for operator in [
            Operator::RefFunc { function_index: 2 },
            Operator::RefFunc { function_index: 3 },
            Operator::GlobalGet { global_index: 0 },
            Operator::GlobalGet { global_index: 1 },
        ] {
            let expression = Engine::lower_wasm_constant_expression(
                "function-initializer-owner",
                WasmType::Reference {
                    kind: crate::WasmReferenceKind::DeclaredFunction {
                        index: 0,
                        exact: true,
                    },
                    nullable: false,
                },
                &globals,
                [operator, Operator::End].into_iter().map(Ok),
            )
            .unwrap();
            globals.push(WasmGlobal {
                mutable: false,
                initial: crate::WasmGlobalInitializer::Expression(expression),
            });
        }
        let importer = Engine::lower_wasm_module_with_imports(
            "function-importer",
            [
                WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![wasmparser::ValType::Ref(
                        wasmparser::RefType::new(
                            false,
                            wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(0)),
                        )
                        .unwrap(),
                    )],
                    operators: vec![
                        Operator::Block {
                            blockty: wasmparser::BlockType::Type(wasmparser::ValType::Ref(
                                wasmparser::RefType::FUNC,
                            )),
                        },
                        Operator::RefFunc { function_index: 2 },
                        Operator::BrOnNonNull { relative_depth: 0 },
                        Operator::Unreachable,
                        Operator::End,
                        Operator::RefAsNonNull,
                        Operator::Drop,
                        Operator::RefFunc { function_index: 2 },
                        Operator::LocalSet { local_index: 0 },
                        Operator::LocalGet { local_index: 0 },
                        Operator::ReturnCallRef { type_index: 0 },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                WasmFunctionBody {
                    ty: crate::WasmCallableType::Declared(1),
                    locals: vec![],
                    operators: vec![Operator::RefFunc { function_index: 2 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
                WasmFunctionBody {
                    ty: crate::WasmCallableType::Declared(2),
                    locals: vec![],
                    operators: vec![
                        Operator::LocalGet { local_index: 0 },
                        Operator::ReturnCallRef { type_index: 0 },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([
                wasmparser::FuncType::new([], [wasmparser::ValType::I64]),
                wasmparser::FuncType::new(
                    [],
                    [wasmparser::ValType::Ref(
                        wasmparser::RefType::new(
                            false,
                            wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(0)),
                        )
                        .unwrap(),
                    )],
                ),
                wasmparser::FuncType::new(
                    [wasmparser::ValType::Ref(
                        wasmparser::RefType::new(
                            false,
                            wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(0)),
                        )
                        .unwrap(),
                    )],
                    [wasmparser::ValType::I64],
                ),
            ]),
            &globals,
            &[],
            &[],
            &[],
            &[],
            &imports,
        )
        .unwrap();
        assert_eq!(importer.program.function_count(), 3);
        assert_eq!(importer.signatures.function_count(), 6);
        let path = std::env::temp_dir().join(format!(
            "quench-wasm-function-imports-{}.qvm",
            std::process::id()
        ));
        importer.residual().write_binary(&path).unwrap();
        let residual = crate::ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let importer = crate::WasmModule {
            program: Rc::new(residual),
            ..importer
        };
        let mut vm = Vm::new(crate::SystemHost);
        let mut corrupt = (*importer.program).clone();
        let reference = corrupt.functions[0]
            .code
            .iter_mut()
            .find(|instruction| instruction.op() == crate::bytecode::Op::WasmRefFunc)
            .unwrap();
        *reference = crate::bytecode::Instr::new(crate::bytecode::Op::WasmRefFunc, 0, 0, 0, 9);
        // Residual validation knows the operand layout; pool attachment owns source indices.
        assert!(corrupt.validate().is_ok());
        let first = vm.instantiate_wasm_module(&owner).unwrap();
        let owner_env = vm.root_value(first.environment).unwrap();
        let mut signature_validation_vm = Vm::new(crate::SystemHost);
        let rejected = signature_validation_vm
            .programs
            .insert_shared(Rc::new(corrupt))
            .unwrap();
        assert!(
            !signature_validation_vm
                .programs
                .attach_wasm_signatures(rejected, &importer.signatures)
        );

        let function = vm.wasm_function(&first, 0).unwrap();
        let handle = vm.root(function);
        let second = vm
            .instantiate_wasm_module_with_imports(&importer, &[handle; 3])
            .unwrap();
        for index in 0..3 {
            assert_eq!(vm.wasm_function(&second, index).unwrap(), function);
        }
        let local_function = vm.wasm_function(&second, 3).unwrap();
        for index in [0, 2] {
            assert_eq!(
                vm.wasm_global(&second, index).unwrap(),
                WasmValue::FuncRef(function)
            );
        }
        for index in [1, 3] {
            assert_eq!(
                vm.wasm_global(&second, index).unwrap(),
                WasmValue::FuncRef(local_function)
            );
        }
        assert!(vm.release_root(first.environment));
        assert!(vm.release_root(handle));
        vm.collect_now(importer.residual());
        assert!(vm.heap.is_live(owner_env));
        assert_eq!(
            vm.invoke_wasm(&second, 0, &[]).unwrap(),
            Some(WasmValue::I64(i64::MAX))
        );
        assert_eq!(
            vm.invoke_wasm(&second, 3, &[]).unwrap(),
            Some(WasmValue::I64(i64::MAX))
        );
        assert_eq!(
            vm.invoke_wasm(&second, 4, &[]).unwrap(),
            Some(WasmValue::FuncRef(function))
        );
        assert_eq!(
            vm.invoke_wasm(&second, 5, &[WasmValue::FuncRef(function)])
                .unwrap(),
            Some(WasmValue::I64(i64::MAX))
        );
        assert!(
            vm.invoke_wasm(&second, 5, &[WasmValue::FuncRef(Value::NULL)])
                .is_err()
        );
        let wrong_signature = vm.wasm_function(&second, 4).unwrap();
        assert!(
            vm.invoke_wasm(&second, 5, &[WasmValue::FuncRef(wrong_signature)])
                .is_err()
        );
        assert_eq!(vm.active_program, ProgramId::MAIN);
        let reference = vm.wasm_function(&second, 2).unwrap();
        let imported_handle = vm.root(reference);
        let third = vm
            .instantiate_wasm_module_with_imports(&importer, &[imported_handle; 3])
            .unwrap();
        assert_eq!(vm.wasm_function(&third, 1).unwrap(), function);
        let third_function = vm.wasm_function(&third, 3).unwrap();
        assert_ne!(third_function, local_function);
        assert_eq!(
            vm.wasm_global(&third, 3).unwrap(),
            WasmValue::FuncRef(third_function)
        );

        assert!(vm.release_root(second.environment));
        vm.collect_now(importer.residual());
        assert_eq!(
            vm.invoke_wasm(&third, 3, &[]).unwrap(),
            Some(WasmValue::I64(i64::MAX))
        );
        let mismatch = Engine::lower_wasm_module_with_imports::<
            std::iter::Empty<Result<Operator<'static>, wasmparser::BinaryReaderError>>,
        >(
            "signature-mismatch",
            [],
            &crate::WasmTypes::default(),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[WasmFunctionImport {
                exact: false,
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![],
                    results: vec![WasmType::I32],
                }),
                name: WasmImportName {
                    index: 0,
                    module: "owner".into(),
                    name: "get".into(),
                },
            }],
        )
        .unwrap();
        let roots = vm.heap.root_count_for_test();
        assert!(
            vm.instantiate_wasm_module_with_imports(&mismatch, &[imported_handle])
                .err()
                .unwrap()
                .wasm_link_error()
                .is_some()
        );
        assert_eq!(vm.heap.root_count_for_test(), roots);
        assert!(vm.release_root(imported_handle));
        assert!(
            vm.instantiate_wasm_module_with_imports(&importer, &[imported_handle; 3])
                .err()
                .unwrap()
                .wasm_link_error()
                .is_some()
        );
        assert!(vm.release_root(third.environment));
        vm.collect_now(importer.residual());
        assert!(!vm.heap.is_live(owner_env));
        assert!(!vm.heap.is_live(function));
    }

    #[test]
    fn global_import_roots_transfer_identity_and_trace_replaced_reference_payloads() {
        type Empty = std::iter::Empty<Result<Operator<'static>, wasmparser::BinaryReaderError>>;
        let mut vm = Vm::new(crate::SystemHost);
        let owner = crate::Engine::lower_wasm_module_definition::<Empty>(
            "global-owner",
            [],
            &crate::WasmTypes::default(),
            &[crate::WasmGlobal {
                initial: crate::WasmValue::ExternRef(Value::NULL).into(),
                mutable: true,
            }],
        )
        .unwrap();
        let importer = crate::Engine::lower_wasm_module_definition::<Empty>(
            "global-importer",
            [],
            &crate::WasmTypes::default(),
            &[crate::WasmGlobal {
                initial: crate::WasmGlobalInitializer::Import {
                    ty: crate::WasmType::EXTERNREF,
                    name: crate::WasmImportName {
                        index: 0,
                        module: "owner".into(),
                        name: "global".into(),
                    },
                },
                mutable: true,
            }],
        )
        .unwrap();
        let first = vm.instantiate_wasm_module(&owner).unwrap();
        let first_env = vm.root_value(first.environment).unwrap();
        let binding = vm.wasm_global_binding(&first, 0).unwrap();
        let payload = vm
            .heap
            .alloc(Cell::Object(Vm::<crate::SystemHost>::empty_object(
                Value::NULL,
            )));
        vm.wasm_global_store(binding, payload).unwrap();
        let handle = vm.root(binding);
        let second = vm
            .instantiate_wasm_module_with_imports(&importer, &[handle])
            .unwrap();
        assert_eq!(vm.wasm_global_binding(&second, 0).unwrap(), binding);
        assert!(vm.release_root(handle));
        assert!(vm.release_root(first.environment));
        vm.collect_now(importer.residual());
        assert!(!vm.heap.is_live(first_env));
        assert!(vm.heap.is_live(payload));
        assert_eq!(
            vm.wasm_global(&second, 0).unwrap(),
            crate::WasmValue::ExternRef(payload)
        );
        let replacement = vm
            .heap
            .alloc(Cell::Object(Vm::<crate::SystemHost>::empty_object(
                Value::NULL,
            )));
        vm.wasm_global_store(binding, replacement).unwrap();
        vm.collect_now(importer.residual());
        assert!(!vm.heap.is_live(payload));
        assert!(vm.heap.is_live(replacement));
        let third_handle = vm.root(vm.wasm_global_binding(&second, 0).unwrap());
        let third = vm
            .instantiate_wasm_module_with_imports(&importer, &[third_handle])
            .unwrap();
        assert_eq!(vm.wasm_global_binding(&third, 0).unwrap(), binding);
        assert!(vm.release_root(third_handle));
        assert!(vm.release_root(second.environment));
        vm.collect_now(importer.residual());
        assert_eq!(
            vm.wasm_global(&third, 0).unwrap(),
            crate::WasmValue::ExternRef(replacement)
        );
        let roots = vm.heap.root_count_for_test();
        let error = vm
            .instantiate_wasm_module_with_imports(&importer, &[third_handle])
            .err()
            .unwrap();
        assert!(error.wasm_link_error().is_some());
        assert_eq!(vm.heap.root_count_for_test(), roots);
        assert!(vm.release_root(third.environment));
        vm.collect_now(importer.residual());
        assert!(!vm.heap.is_live(binding));
        assert!(!vm.heap.is_live(replacement));
    }

    #[test]
    fn wasm_instance_bindings_are_independent_traced_and_runtime_owned() {
        let signature = crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I64],
        };
        let function = crate::Engine::lower_wasm_module_with_globals(
            "binding-owners",
            0,
            [
                vec![
                    Operator::GlobalGet { global_index: 0 },
                    Operator::I64Const { value: 1 },
                    Operator::I64Add,
                    Operator::GlobalSet { global_index: 0 },
                    Operator::GlobalGet { global_index: 0 },
                    Operator::End,
                ],
                vec![Operator::Call { function_index: 0 }, Operator::End],
            ]
            .into_iter()
            .map(|operators| crate::WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                operators: operators.into_iter().map(Ok),
            }),
            &crate::WasmTypes::default(),
            &[crate::WasmGlobal {
                initial: crate::WasmValue::I64(i64::MAX).into(),
                mutable: true,
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        let first = vm.instantiate_wasm(&function).unwrap();
        let second = vm.instantiate_wasm(&function).unwrap();
        assert!(vm.execute_wasm(&function, &[]).is_err());
        assert_eq!(
            vm.invoke_wasm(&first, 1, &[]).unwrap(),
            Some(crate::WasmValue::I64(i64::MIN))
        );
        assert_eq!(
            vm.wasm_global(&second, 0).unwrap(),
            crate::WasmValue::I64(i64::MAX)
        );
        vm.collect_now(function.residual());
        assert_eq!(
            vm.wasm_global(&first, 0).unwrap(),
            crate::WasmValue::I64(i64::MIN)
        );
        assert_eq!(
            vm.wasm_global(&second, 0).unwrap(),
            crate::WasmValue::I64(i64::MAX)
        );
        let mut foreign = Vm::new(crate::SystemHost);
        assert!(foreign.invoke_wasm(&first, 0, &[]).is_err());
        assert!(vm.invoke_wasm(&first, 2, &[]).is_err());
        assert!(vm.wasm_global(&first, 1).is_err());
        assert_eq!(vm.programs.len(), 1);
        assert!(vm.release_root(first.environment));
        assert!(vm.invoke_wasm(&first, 0, &[]).is_err());
        vm.collect_now(function.residual());
        assert_eq!(
            vm.wasm_global(&second, 0).unwrap(),
            crate::WasmValue::I64(i64::MAX)
        );
    }

    #[test]
    fn wasm_module_without_functions_has_bindings_but_no_executable_entry() {
        let module = crate::Engine::lower_wasm_module_definition::<
            std::iter::Empty<Result<Operator<'static>, wasmparser::BinaryReaderError>>,
        >(
            "binding-only",
            [],
            &crate::WasmTypes::default(),
            &[crate::WasmGlobal {
                initial: crate::WasmValue::I64(i64::MAX).into(),
                mutable: false,
            }],
        )
        .unwrap();
        assert!(module.program.functions.is_empty());
        assert!(module.function(0).is_none());
        assert!(module.program.validate().is_ok());
        let mut vm = Vm::new(crate::SystemHost);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        assert_eq!(
            vm.wasm_global(&instance, 0).unwrap(),
            crate::WasmValue::I64(i64::MAX)
        );
        assert!(vm.invoke_wasm(&instance, 0, &[]).is_err());
        assert!(vm.execute(module.residual()).is_err());
        vm.collect_now(module.residual());
        assert_eq!(
            vm.wasm_global(&instance, 0).unwrap(),
            crate::WasmValue::I64(i64::MAX)
        );
        assert_eq!(vm.programs.len(), 1);
        let mut invalid_script = module.residual().clone();
        invalid_script.kind = crate::bytecode::ProgramKind::Script;
        assert!(invalid_script.validate().is_err());
    }

    #[test]
    fn serialized_multi_result_bundles_trace_raw_scalars_and_guard_scalar_entry_effects() {
        const TAG_COLLISION_TEST_BITS: u64 = 0x7ffc_1234_5678_9abc;
        let signature = crate::WasmSignature {
            params: vec![
                crate::WasmType::I64,
                crate::WasmType::F64,
                crate::WasmType::V128,
            ],
            results: vec![
                crate::WasmType::I64,
                crate::WasmType::F64,
                crate::WasmType::V128,
            ],
        };
        let mut function = crate::Engine::lower_wasm_module_with_globals(
            "bundle-owners",
            1,
            [
                vec![
                    Operator::I32Const { value: 1 },
                    Operator::GlobalSet { global_index: 0 },
                    Operator::LocalGet { local_index: 0 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::V128Const {
                        value: (u128::from(TAG_COLLISION_TEST_BITS)
                            | (u128::from(TAG_COLLISION_TEST_BITS) << u64::BITS))
                            .into(),
                    },
                    Operator::End,
                ],
                vec![
                    Operator::LocalGet { local_index: 0 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::LocalGet { local_index: 2 },
                    Operator::Call { function_index: 0 },
                    Operator::End,
                ],
            ]
            .into_iter()
            .map(|operators| crate::WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                operators: operators.into_iter().map(Ok),
            }),
            &crate::WasmTypes::default(),
            &[crate::WasmGlobal {
                initial: crate::WasmValue::I32(0).into(),
                mutable: true,
            }],
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("quench-multi-owner-{}.qbc", std::process::id()));
        function.module.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        function.module.program = Rc::new(decoded);
        let values = [
            crate::WasmValue::I64(TAG_COLLISION_TEST_BITS as i64),
            crate::WasmValue::F64(TAG_COLLISION_TEST_BITS),
            crate::WasmValue::V128(
                u128::from(TAG_COLLISION_TEST_BITS)
                    | (u128::from(TAG_COLLISION_TEST_BITS) << u64::BITS),
            ),
        ];
        let mut vm = Vm::new(crate::SystemHost);
        let instance = vm.instantiate_wasm(&function).unwrap();
        assert!(vm.invoke_wasm(&instance, 1, &values).is_err());
        assert_eq!(
            vm.wasm_global(&instance, 0).unwrap(),
            crate::WasmValue::I32(0)
        );
        let environment = vm.root_value(instance.environment).unwrap();
        let bundle = vm
            .execute_wasm_in_environment(&function, &values, environment)
            .unwrap();
        let root = vm.root(bundle);
        vm.collect_now(function.residual());
        assert_eq!(vm.root_value(root), Some(bundle));
        assert_eq!(vm.decode_wasm_results(&function, bundle).unwrap(), values);
        assert_eq!(
            vm.invoke_wasm_values(&instance, 1, &values).unwrap(),
            values
        );
        assert_eq!(
            vm.wasm_global(&instance, 0).unwrap(),
            crate::WasmValue::I32(1)
        );
        assert!(vm.frames.is_empty());
        assert_eq!(vm.programs.len(), 1);
        assert!(vm.release_root(root));
    }

    #[test]
    fn serialized_memory_bindings_trace_independent_instances_and_raw_bits() {
        const TAG_COLLISION_TEST_BITS: u64 = 0x7ffc_1234_5678_9abc;
        let memarg = wasmparser::MemArg {
            align: 0,
            max_align: 3,
            offset: 0,
            memory: 0,
        };
        let mut module = crate::Engine::lower_wasm_module_with_state(
            "memory-owners",
            [
                (
                    vec![crate::WasmType::I64],
                    vec![crate::WasmType::I64],
                    vec![
                        Operator::I64Const { value: 0 },
                        Operator::LocalGet { local_index: 0 },
                        Operator::I64AtomicStore { memarg },
                        Operator::I64Const { value: 0 },
                        Operator::I64AtomicLoad { memarg },
                        Operator::End,
                    ],
                ),
                (
                    vec![crate::WasmType::I64],
                    vec![crate::WasmType::I64],
                    vec![
                        Operator::LocalGet { local_index: 0 },
                        Operator::MemoryGrow { mem: 0 },
                        Operator::End,
                    ],
                ),
            ]
            .into_iter()
            .map(|(params, results, operators)| crate::WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(crate::WasmSignature { params, results }),
                locals: vec![],
                operators: operators.into_iter().map(Ok),
            }),
            &crate::WasmTypes::default(),
            &[],
            &[crate::WasmMemory {
                import: None,
                ty: wasmparser::MemoryType {
                    initial: 1,
                    maximum: Some(2),
                    memory64: true,
                    shared: false,
                    page_size_log2: None,
                },
            }],
            &[crate::WasmData {
                mode: crate::WasmDataMode::Active {
                    memory: 0,
                    offset: crate::WasmValue::I64(0).into(),
                },
                bytes: Rc::new(TAG_COLLISION_TEST_BITS.to_le_bytes().to_vec()),
            }],
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("quench-memory-owner-{}.qbc", std::process::id()));
        module.program.write_binary(&path).unwrap();
        module.program = Rc::new(ResidualProgram::read_binary(&path).unwrap());
        std::fs::remove_file(path).unwrap();
        let mut invalid = (*module.program).clone();
        let address = invalid.function(0)
            .code
            .iter()
            .map(|packed| {
                if packed.is_wide() {
                    invalid.function(0).wide[packed.wide_index()]
                } else {
                    packed.as_wide()
                }
            })
            .find(|instruction| instruction.op() == Op::WasmMemoryAddress)
            .unwrap();
        invalid.constants[address.constant_index()] = crate::bytecode::Constant::Number(0.0);
        assert!(invalid.validate().is_err());
        let mut vm = Vm::new(crate::SystemHost);
        let first = vm.instantiate_wasm_module(&module).unwrap();
        let second = vm.instantiate_wasm_module(&module).unwrap();
        let memory = |vm: &Vm<crate::SystemHost>, instance: &crate::WasmInstance| {
            vm.heap
                .environment_slot(vm.root_value(instance.environment).unwrap(), 0)
                .unwrap()
        };
        let first_memory = memory(&vm, &first);
        let second_memory = memory(&vm, &second);
        assert_ne!(first_memory, second_memory);
        while !vm.heap.should_collect() {
            vm.heap.alloc(Cell::WasmBits64(TAG_COLLISION_TEST_BITS));
        }
        let third = vm.instantiate_wasm_module(&module).unwrap();
        assert!(
            !vm.heap.should_collect(),
            "instantiation is a collection boundary"
        );
        assert!(vm.heap.is_live(first_memory));
        assert!(vm.heap.is_live(second_memory));
        assert!(vm.release_root(third.environment));
        assert!(
            vm.execute_wasm(&module.function(0).unwrap(), &[crate::WasmValue::I64(0)])
                .is_err()
        );
        assert_eq!(
            vm.invoke_wasm(&first, 0, &[crate::WasmValue::I64(-1)])
                .unwrap(),
            Some(crate::WasmValue::I64(-1))
        );
        vm.collect_now(module.residual());
        assert_eq!(
            vm.wasm_memory_bytes(second_memory).unwrap()[..std::mem::size_of::<u64>()],
            TAG_COLLISION_TEST_BITS.to_le_bytes()
        );
        assert_eq!(
            vm.invoke_wasm(&first, 1, &[crate::WasmValue::I64(1)])
                .unwrap(),
            Some(crate::WasmValue::I64(1))
        );
        vm.collect_now(module.residual());
        assert_eq!(
            vm.wasm_memory_bytes(first_memory).unwrap().len(),
            2 * crate::wasm::memory::WASM_PAGE_BYTES
        );
        assert_eq!(
            vm.wasm_memory_bytes(second_memory).unwrap().len(),
            crate::wasm::memory::WASM_PAGE_BYTES
        );
        let mut foreign = Vm::new(crate::SystemHost);
        assert!(
            foreign
                .invoke_wasm(&first, 1, &[crate::WasmValue::I64(0)])
                .is_err()
        );
        assert!(vm.release_root(first.environment));
        assert!(
            vm.invoke_wasm(&first, 1, &[crate::WasmValue::I64(0)])
                .is_err()
        );
        vm.collect_now(module.residual());
        assert!(!vm.heap.is_live(first_memory));
        assert!(vm.heap.is_live(second_memory));
        assert!(vm.frames.is_empty());
    }

    #[test]
    fn serialized_bulk_bindings_share_immutable_facts_and_drop_only_the_instance_owner() {
        const TAG_COLLISION_TEST_BITS: u64 = 0x7ffc_1234_5678_9abc;
        let mut module = crate::Engine::lower_wasm_module_with_state(
            "segment-owners",
            [
                (
                    vec![crate::WasmType::I32; 3],
                    vec![
                        Operator::LocalGet { local_index: 0 },
                        Operator::LocalGet { local_index: 1 },
                        Operator::LocalGet { local_index: 2 },
                        Operator::MemoryInit {
                            data_index: 0,
                            mem: 0,
                        },
                        Operator::End,
                    ],
                ),
                (
                    vec![],
                    vec![Operator::DataDrop { data_index: 0 }, Operator::End],
                ),
            ]
            .into_iter()
            .map(|(params, operators)| crate::WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                    params,
                    results: vec![],
                }),
                locals: vec![],
                operators: operators.into_iter().map(Ok),
            }),
            &crate::WasmTypes::default(),
            &[],
            &[crate::WasmMemory {
                import: None,
                ty: wasmparser::MemoryType {
                    initial: 1,
                    maximum: Some(1),
                    memory64: false,
                    shared: false,
                    page_size_log2: None,
                },
            }],
            &[crate::WasmData {
                mode: crate::WasmDataMode::Passive,
                bytes: Rc::new(TAG_COLLISION_TEST_BITS.to_le_bytes().to_vec()),
            }],
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("quench-segment-owner-{}.qbc", std::process::id()));
        module.program.write_binary(&path).unwrap();
        module.program = Rc::new(ResidualProgram::read_binary(&path).unwrap());
        std::fs::remove_file(path).unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        let first = vm.instantiate_wasm_module(&module).unwrap();
        let second = vm.instantiate_wasm_module(&module).unwrap();
        let environment = |vm: &Vm<crate::SystemHost>, instance: &crate::WasmInstance| {
            vm.root_value(instance.environment).unwrap()
        };
        let memory = vm
            .heap
            .environment_slot(environment(&vm, &first), 0)
            .unwrap();
        let data = vm
            .heap
            .environment_slot(environment(&vm, &first), 1)
            .unwrap();
        assert_eq!(Rc::strong_count(&module.data[0].bytes), 3);
        assert!(
            vm.wasm_memory_mut(data).is_err(),
            "immutable segment bytes cannot become mutable memory"
        );
        let length = std::mem::size_of::<u64>() as i32;
        vm.invoke_wasm(
            &first,
            0,
            &[
                crate::WasmValue::I32(0),
                crate::WasmValue::I32(0),
                crate::WasmValue::I32(length),
            ],
        )
        .unwrap();
        vm.collect_now(module.residual());
        assert_eq!(
            vm.wasm_memory_bytes(memory).unwrap()[..length as usize],
            TAG_COLLISION_TEST_BITS.to_le_bytes()
        );
        vm.invoke_wasm(&first, 1, &[]).unwrap();
        vm.collect_now(module.residual());
        assert_eq!(Rc::strong_count(&module.data[0].bytes), 2);
        assert_eq!(
            vm.heap.environment_slot(environment(&vm, &first), 1),
            Some(Value::UNDEFINED)
        );
        vm.invoke_wasm(
            &second,
            0,
            &[
                crate::WasmValue::I32(0),
                crate::WasmValue::I32(0),
                crate::WasmValue::I32(length),
            ],
        )
        .unwrap();
        assert!(vm.frames.is_empty());
        assert!(vm.release_root(first.environment));
        assert!(vm.release_root(second.environment));
        vm.collect_now(module.residual());
        assert_eq!(Rc::strong_count(&module.data[0].bytes), 1);
    }

    #[test]
    fn wasm_program_signatures_share_module_authority_and_reject_rebinding() {
        let function = crate::Engine::lower_wasm_i32_function(
            "signature-owner",
            1,
            0,
            true,
            [Operator::LocalGet { local_index: 0 }, Operator::End]
                .into_iter()
                .map(Ok),
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        let script = crate::Engine::specialize("void 0", "signature-host.js").unwrap();
        vm.execute(&script).unwrap();
        assert!(
            vm.programs
                .wasm_function_signature(ProgramId::MAIN, 0)
                .is_none()
        );
        let id = vm.register_wasm_program(&function.module).unwrap();
        assert!(std::ptr::eq(
            vm.programs.wasm_function_signature(id, 0).unwrap(),
            function.signature()
        ));
        assert!(vm.programs.wasm_function_signature(id, 1).is_none());
        assert_eq!(vm.register_wasm_program(&function.module).unwrap(), id);
        let conflicting = Rc::new(
            crate::wasm::WasmSignatures::with_imports(
                "conflicting",
                &[],
                [crate::WasmCallableType::Embedded(crate::WasmSignature {
                    params: vec![],
                    results: vec![],
                })],
                &crate::WasmTypes::default(),
            )
            .unwrap(),
        );
        assert!(!vm.programs.attach_wasm_signatures(id, &conflicting));
        assert!(std::ptr::eq(
            vm.programs.wasm_function_signature(id, 0).unwrap(),
            function.signature()
        ));
        vm.collect_now(function.residual());
        assert_eq!(vm.execute_wasm_i32(&function, &[7]).unwrap(), Some(7));
        assert_eq!(vm.active_program, ProgramId::MAIN);
        assert_eq!(vm.programs.len(), 2);
    }

    #[test]
    fn counted_loop_steps_once_when_its_safepoint_leaves_the_lane() {
        // loop: count += 1; n -= 1 (local.tee) ; br_if loop. The step and test
        // lower to one fused counter-jump, whose safepoint may hand the jump to
        // the general path; the step must still apply exactly once.
        let function = crate::Engine::lower_wasm_i32_function(
            "counted-loop-safepoint",
            1,
            1,
            true,
            [
                Operator::Loop {
                    blockty: wasmparser::BlockType::Empty,
                },
                Operator::LocalGet { local_index: 1 },
                Operator::I32Const { value: 1 },
                Operator::I32Add,
                Operator::LocalSet { local_index: 1 },
                Operator::LocalGet { local_index: 0 },
                Operator::I32Const { value: -1 },
                Operator::I32Add,
                Operator::LocalTee { local_index: 0 },
                Operator::BrIf { relative_depth: 0 },
                Operator::End,
                Operator::LocalGet { local_index: 1 },
                Operator::End,
            ]
            .into_iter()
            .map(Ok),
        )
        .unwrap();
        assert!(
            function
                .residual()
                .disassemble()
                .contains("WasmI32AddImmediateJumpNonZero")
        );
        let mut vm = Vm::new(crate::SystemHost);
        let script = crate::Engine::specialize("void 0", "counted-loop-host.js").unwrap();
        vm.execute(&script).unwrap();
        vm.heap.make_collection_due_for_test();
        assert_eq!(vm.execute_wasm_i32(&function, &[5]).unwrap(), Some(5));
    }

    #[test]
    fn compiled_wasm_loop_backedge_polls_execution_interrupt() {
        struct StartHost(std::sync::mpsc::Sender<()>);
        impl crate::Host for StartHost {
            fn write_line(&mut self, _: &str) {}
            fn clock_millis(&mut self) -> f64 {
                0.0
            }
            fn call_wasm(
                &mut self,
                _: crate::WasmHostFunctionId,
                _: &[crate::WasmHostValue],
            ) -> Result<Vec<crate::WasmHostValue>, String> {
                self.0.send(()).map_err(|error| error.to_string())?;
                Ok(Vec::new())
            }
        }

        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let interrupt = crate::ExecutionInterrupt::default();
        let worker_interrupt = interrupt.clone();
        let worker = std::thread::Builder::new()
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(move || {
                let mut vm =
                    Vm::new_with_execution_interrupt(StartHost(entered_tx), worker_interrupt);
                vm.set_execution_mode(crate::ExecutionMode::Compiled);
                let signature = crate::WasmSignature {
                    params: vec![],
                    results: vec![],
                };
                let module = crate::Engine::lower_wasm_module_with_imports(
                    "compiled-wasm-interrupt-backedge",
                    [crate::WasmFunctionBody {
                        ty: crate::WasmCallableType::Embedded(signature.clone()),
                        locals: vec![],
                        operators: [
                            Operator::Call { function_index: 0 },
                            Operator::Loop {
                                blockty: wasmparser::BlockType::Empty,
                            },
                            Operator::Br { relative_depth: 0 },
                            Operator::End,
                            Operator::End,
                        ]
                        .into_iter()
                        .map(Ok),
                    }],
                    &crate::WasmTypes::default(),
                    &[],
                    &[],
                    &[],
                    &[],
                    &[],
                    &[crate::WasmFunctionImport {
                        exact: false,
                        ty: crate::WasmCallableType::Embedded(signature.clone()),
                        name: crate::WasmImportName {
                            index: 0,
                            module: "interrupt-test".into(),
                            name: "started".into(),
                        },
                    }],
                )
                .unwrap();
                let host_function = vm
                    .create_wasm_host_function(
                        "started",
                        crate::heap::WasmHostTarget::Host(crate::WasmHostFunctionId(0)),
                        signature,
                    )
                    .unwrap();
                let host_root = vm.root(host_function);
                let instance = vm
                    .instantiate_wasm_module_with_imports(&module, &[host_root])
                    .unwrap();
                assert!(
                    vm.invoke_wasm(&instance, 1, &[])
                        .unwrap_err()
                        .is_execution_terminated()
                );
                let program = vm
                    .programs
                    .find_shared(&module.program)
                    .expect("registered interrupt-test module");
                let compiled = vm
                    .programs
                    .compiled(program)
                    .flatten()
                    .expect("compiled loop remains installed");
                assert_eq!(compiled.native_entries(0), Some(1));
                finished_tx.send(()).unwrap();
            })
            .unwrap();

        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("native loop reached its import callout");
        interrupt.request_termination();
        finished_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("compiled loop observed termination at its backedge");
        worker.join().unwrap();
    }

    #[test]
    fn unreachable_trap_unwinds_the_shared_activation() {
        let function = crate::Engine::lower_wasm_i32_function(
            "unreachable-unwind",
            0,
            0,
            true,
            [Operator::Unreachable, Operator::End].into_iter().map(Ok),
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(
            vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        assert!(vm.frames.is_empty());
        vm.collect_now(function.residual());
    }

    #[test]
    fn compiled_wasm_root_selection_leaves_unselected_functions_interpreted() {
        let module = crate::Engine::lower_wasm_i32_module(
            "compiled-wasm-root-closure",
            0,
            [
                (
                    0,
                    0,
                    true,
                    vec![
                        Operator::Call { function_index: 1 },
                        Operator::I32Const { value: 0 },
                        Operator::I32Add,
                        Operator::End,
                    ],
                ),
                (
                    0,
                    0,
                    true,
                    vec![Operator::I32Const { value: 42 }, Operator::End],
                ),
                (
                    0,
                    0,
                    true,
                    vec![Operator::I32Const { value: 7 }, Operator::End],
                ),
            ]
            .into_iter()
            .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap();
        let function = module.select_function(0).unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), Some(42));
        let program = vm
            .programs
            .find_shared(&function.module.program)
            .expect("registered Wasm program");
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        assert!(vm.install_compiled_root_for_test(program, Value::NULL, 0));

        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), Some(42));
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("root closure installed");
        assert_eq!(compiled.native_entries(0), Some(1));
        assert_eq!(compiled.native_entries(1), None);
        assert_eq!(compiled.native_entries(2), None);
    }

    #[test]
    fn compiled_wasm_root_only_calls_unselected_known_callees_in_the_vm() {
        let module = crate::Engine::lower_wasm_i32_module(
            "compiled-wasm-root-only-callout",
            0,
            [
                (
                    0,
                    0,
                    true,
                    vec![
                        Operator::Call { function_index: 1 },
                        Operator::I32Const { value: 2 },
                        Operator::I32Add,
                        Operator::End,
                    ],
                ),
                (
                    0,
                    0,
                    true,
                    vec![Operator::I32Const { value: 40 }, Operator::End],
                ),
                (
                    0,
                    0,
                    true,
                    vec![Operator::I32Const { value: 7 }, Operator::End],
                ),
            ]
            .into_iter()
            .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap();
        let function = module.select_function(0).unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), Some(42));
        let program = vm
            .programs
            .find_shared(&function.module.program)
            .expect("registered Wasm program");
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        assert!(vm.install_compiled_root_only_for_test(program, Value::NULL, 0));

        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), Some(42));
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("root-only artifact installed");
        assert!(compiled.has_function(0));
        assert!(!compiled.has_function(1));
        assert!(!compiled.has_function(2));
        assert_eq!(compiled.native_entries(0), Some(1));
    }

    #[test]
    fn compiled_wasm_root_only_rejects_an_invalid_selected_function() {
        let function = crate::Engine::lower_wasm_i32_function(
            "compiled-wasm-root-only-admission",
            0,
            0,
            true,
            [Operator::I32Const { value: 9 }, Operator::End]
                .into_iter()
                .map(Ok),
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), Some(9));
        let program = vm
            .programs
            .find_shared(&function.module.program)
            .expect("registered Wasm program");
        assert!(!vm.install_compiled_root_only_for_test(program, Value::NULL, 1));
        assert!(vm.programs.compiled(program).is_none());
    }

    fn root_first_hot_fallback_fixture() -> crate::WasmI32Function {
        let root = vec![
            Operator::I32Const { value: 40 },
            Operator::LocalSet { local_index: 0 },
            Operator::I32Const { value: 0 },
            Operator::LocalSet { local_index: 1 },
            Operator::Loop {
                blockty: wasmparser::BlockType::Empty,
            },
            Operator::LocalGet { local_index: 1 },
            Operator::Call { function_index: 1 },
            Operator::I32Add,
            Operator::LocalSet { local_index: 1 },
            Operator::LocalGet { local_index: 0 },
            Operator::I32Const { value: -1 },
            Operator::I32Add,
            Operator::LocalTee { local_index: 0 },
            Operator::BrIf { relative_depth: 0 },
            Operator::End,
            Operator::LocalGet { local_index: 1 },
            Operator::End,
        ];
        crate::Engine::lower_wasm_i32_module(
            "compiled-wasm-root-first-hot-callee",
            0,
            [
                (0, 2, true, root),
                (
                    0,
                    0,
                    true,
                    vec![Operator::Call { function_index: 2 }, Operator::End],
                ),
                (
                    0,
                    0,
                    true,
                    vec![Operator::Call { function_index: 3 }, Operator::End],
                ),
                (
                    0,
                    0,
                    true,
                    vec![Operator::I32Const { value: 42 }, Operator::End],
                ),
            ]
            .into_iter()
            .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap()
    }

    #[test]
    fn compiled_wasm_root_first_promotes_a_hot_fallback_callee() {
        let module = root_first_hot_fallback_fixture();
        let function = module.select_function(0).unwrap();
        let program_code = &module.module.program;
        let root_code = &program_code.function(0);
        let root_instructions = root_code
            .code
            .iter()
            .map(|packed| super::super::dispatch_fast::decoded_instruction(root_code, *packed))
            .collect::<Vec<_>>();
        assert!(
            root_instructions.iter().any(|instruction| {
                instruction.op() == Op::CallKnown && instruction.known_function_index() == 1
            }),
            "root instructions: {root_instructions:?}"
        );
        let expected = 1_680;

        let mut root_first_vm = Vm::new(crate::SystemHost);
        assert_eq!(
            root_first_vm.execute_wasm_i32(&function, &[]).unwrap(),
            Some(expected)
        );
        let program = root_first_vm
            .programs
            .find_shared(program_code)
            .expect("registered Wasm program");
        root_first_vm.set_execution_mode(crate::ExecutionMode::Compiled);

        let result =
            super::super::compiled::with_wasm_jit_mode_for_test(false, true, Some(0), || {
                root_first_vm.execute_wasm_i32(&function, &[])
            })
            .unwrap();
        assert_eq!(result, Some(expected));
        let root_artifact = root_first_vm
            .programs
            .compiled(program)
            .flatten()
            .expect("root-first initial artifact");
        assert!(root_artifact.has_function(0));
        assert!(!root_artifact.has_function(1));
        assert!(!root_artifact.has_function(2));
        assert_eq!(root_artifact.native_entries(0), Some(1));
        assert_eq!(
            root_first_vm.programs.wasm_tier_entry_count(program, 1),
            Some(super::super::program_store::WASM_JIT_PROMOTION_ENTRIES)
        );
        assert_eq!(
            root_first_vm.programs.wasm_tier_attempted(program, 1),
            Some(true)
        );
        let tier = root_first_vm
            .programs
            .wasm_tier(program, 1)
            .expect("hot fallback callee is promoted");
        assert_eq!(tier.native_entries(1), Some(9));

        let mut strict_root_only_vm = Vm::new(crate::SystemHost);
        assert_eq!(
            strict_root_only_vm
                .execute_wasm_i32(&function, &[])
                .unwrap(),
            Some(expected)
        );
        let strict_program = strict_root_only_vm
            .programs
            .find_shared(program_code)
            .expect("registered strict-root-only Wasm program");
        strict_root_only_vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let result =
            super::super::compiled::with_wasm_jit_mode_for_test(true, false, Some(0), || {
                strict_root_only_vm.execute_wasm_i32(&function, &[])
            })
            .unwrap();
        assert_eq!(result, Some(expected));
        assert!(
            strict_root_only_vm
                .programs
                .wasm_tier(strict_program, 1)
                .is_none()
        );
    }

    #[test]
    fn compiled_wasm_root_first_does_not_promote_when_root_admission_fails() {
        let module = root_first_hot_fallback_fixture();
        let function = module.select_function(0).unwrap();
        let program_code = &module.module.program;
        let expected = 1_680;
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), Some(expected));
        let program = vm
            .programs
            .find_shared(program_code)
            .expect("registered Wasm program");
        vm.set_execution_mode(crate::ExecutionMode::Compiled);

        let result = super::super::compiled::with_wasm_jit_mode_for_test(
            false,
            true,
            Some(usize::MAX),
            || vm.execute_wasm_i32(&function, &[]),
        )
        .unwrap();
        assert_eq!(result, Some(expected));
        assert!(matches!(vm.programs.compiled(program), Some(None)));
        assert!(vm.programs.wasm_tier(program, 1).is_none());
        assert!(vm.programs.wasm_tier(program, 2).is_none());
    }

    #[test]
    fn compiled_wasm_selective_tiering_promotes_a_hot_fast_call_known() {
        let root = vec![
            Operator::I32Const { value: 40 },
            Operator::LocalSet { local_index: 0 },
            Operator::I32Const { value: 0 },
            Operator::LocalSet { local_index: 1 },
            Operator::Loop {
                blockty: wasmparser::BlockType::Empty,
            },
            Operator::LocalGet { local_index: 1 },
            Operator::Call { function_index: 1 },
            Operator::I32Add,
            Operator::LocalSet { local_index: 1 },
            Operator::LocalGet { local_index: 0 },
            Operator::I32Const { value: -1 },
            Operator::I32Add,
            Operator::LocalTee { local_index: 0 },
            Operator::BrIf { relative_depth: 0 },
            Operator::End,
            Operator::LocalGet { local_index: 1 },
            Operator::End,
        ];
        let module = crate::Engine::lower_wasm_i32_module(
            "compiled-wasm-selective-fast-call-known",
            0,
            [
                (0, 2, true, root),
                (
                    0,
                    0,
                    true,
                    vec![Operator::Call { function_index: 2 }, Operator::End],
                ),
                (
                    0,
                    0,
                    true,
                    vec![Operator::Call { function_index: 3 }, Operator::End],
                ),
                (
                    0,
                    0,
                    true,
                    vec![Operator::I32Const { value: 42 }, Operator::End],
                ),
            ]
            .into_iter()
            .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap();
        let root_code = &module.module.program.function(0);
        let root_instructions = root_code
            .code
            .iter()
            .map(|packed| super::super::dispatch_fast::decoded_instruction(root_code, *packed))
            .collect::<Vec<_>>();
        assert!(
            root_instructions
                .iter()
                .any(|instruction| instruction.op() == Op::CallKnown
                    && instruction.known_function_index() == 1),
            "root instructions: {root_instructions:?}"
        );
        let function = module.select_function(0).unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), Some(1_680));
        let program = vm
            .programs
            .find_shared(&function.module.program)
            .expect("registered Wasm program");
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        // Selective mode has no eager artifact. Recreate that state directly
        // so this regression test does not depend on process environment.
        vm.programs.set_compiled(program, None);

        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), Some(1_680));
        assert!(matches!(vm.programs.compiled(program), Some(None)));
        assert_eq!(
            vm.programs.wasm_tier_entry_count(program, 1),
            Some(super::super::program_store::WASM_JIT_PROMOTION_ENTRIES)
        );
        assert_eq!(vm.programs.wasm_tier_attempted(program, 1), Some(true));
        let tier = vm
            .programs
            .wasm_tier(program, 1)
            .expect("hot `CallKnown` target was promoted");
        // The threshold call enters native code, and the remaining calls in
        // the same loop are routed back through the general native path.
        assert_eq!(tier.native_entries(1), Some(9));
    }

    #[test]
    fn compiled_wasm_unreachable_raises_the_interpreter_trap() {
        // Rejected from the eager artifact, the function reaches the tier.
        super::super::compiled::with_eager_unreachable_rejected_for_test(|| {
            let module = crate::Engine::lower_wasm_i32_module(
                "compiled-wasm-unreachable-trap",
                0,
                [
                    (0, 0, true, vec![Operator::Unreachable, Operator::End]),
                    (
                        0,
                        0,
                        true,
                        vec![Operator::I32Const { value: 7 }, Operator::End],
                    ),
                ]
                .into_iter()
                .map(|(params, locals, result, ops)| {
                    (params, locals, result, ops.into_iter().map(Ok))
                }),
            )
            .unwrap();
            let function = module.select_function(0).unwrap();
            let mut vm = Vm::new(crate::SystemHost);
            assert_eq!(
                vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
                Some(crate::WasmTrap::Unreachable)
            );
            let program = vm
                .programs
                .find_shared(&function.module.program)
                .expect("registered Wasm program");
            vm.set_execution_mode(crate::ExecutionMode::Compiled);
            assert_eq!(
                vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
                Some(crate::WasmTrap::Unreachable)
            );
            let compiled = vm
                .programs
                .compiled(program)
                .flatten()
                .expect("default Wasm compilation installed");
            assert_eq!(compiled.native_entries(0), None);
            assert_eq!(vm.programs.wasm_tier_entry_count(program, 0), Some(1));

            for _ in 0..(super::super::program_store::WASM_JIT_PROMOTION_ENTRIES as usize - 2) {
                assert_eq!(
                    vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
                    Some(crate::WasmTrap::Unreachable)
                );
            }
            assert_eq!(
                vm.programs.wasm_tier_entry_count(program, 0),
                Some(super::super::program_store::WASM_JIT_PROMOTION_ENTRIES - 1)
            );
            assert_eq!(vm.programs.wasm_tier_attempted(program, 0), Some(false));
            assert!(vm.programs.wasm_tier(program, 0).is_none());
            assert_eq!(
                vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
                Some(crate::WasmTrap::Unreachable)
            );
            let tier = vm
                .programs
                .wasm_tier(program, 0)
                .expect("hot Wasm root is tiered after the entry threshold");
            assert_eq!(tier.native_entries(0), Some(1));
            assert_eq!(vm.programs.wasm_tier_attempted(program, 0), Some(true));

            let mut root_vm = Vm::new(crate::SystemHost);
            assert_eq!(
                root_vm
                    .execute_wasm_i32(&function, &[])
                    .unwrap_err()
                    .wasm_trap(),
                Some(crate::WasmTrap::Unreachable)
            );
            let root_program = root_vm
                .programs
                .find_shared(&function.module.program)
                .expect("registered root Wasm program");
            root_vm.set_execution_mode(crate::ExecutionMode::Compiled);
            assert!(root_vm.install_compiled_root_for_test(root_program, Value::NULL, 0));
            assert_eq!(
                root_vm
                    .execute_wasm_i32(&function, &[])
                    .unwrap_err()
                    .wasm_trap(),
                Some(crate::WasmTrap::Unreachable)
            );
            let root_compiled = root_vm
                .programs
                .compiled(root_program)
                .flatten()
                .expect("experimental root compilation installed");
            assert_eq!(root_compiled.native_entries(0), Some(1));
        });
    }

    #[test]
    fn compiled_wasm_unreachable_lowers_to_a_native_trap() {
        let module = crate::Engine::lower_wasm_i32_module(
            "compiled-wasm-unreachable-trap",
            0,
            [
                (0, 0, true, vec![Operator::Unreachable, Operator::End]),
                (
                    0,
                    0,
                    true,
                    vec![Operator::I32Const { value: 7 }, Operator::End],
                ),
            ]
            .into_iter()
            .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap();
        let function = module.select_function(0).unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(
            vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        let program = vm
            .programs
            .find_shared(&function.module.program)
            .expect("registered Wasm program");
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        assert_eq!(
            vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("default Wasm compilation installed");
        // A small function's `unreachable` lowers to a native trap: the eager artifact holds
        // it, so no tier promotion follows.
        assert_eq!(compiled.native_entries(0), Some(1));
        for _ in 0..usize::from(super::super::program_store::WASM_JIT_PROMOTION_ENTRIES) {
            assert_eq!(
                vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
                Some(crate::WasmTrap::Unreachable)
            );
        }
        assert_eq!(
            compiled.native_entries(0),
            Some(1 + u64::from(super::super::program_store::WASM_JIT_PROMOTION_ENTRIES))
        );
        assert!(vm.programs.wasm_tier(program, 0).is_none());

        let mut root_vm = Vm::new(crate::SystemHost);
        assert_eq!(
            root_vm
                .execute_wasm_i32(&function, &[])
                .unwrap_err()
                .wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        let root_program = root_vm
            .programs
            .find_shared(&function.module.program)
            .expect("registered root Wasm program");
        root_vm.set_execution_mode(crate::ExecutionMode::Compiled);
        assert!(root_vm.install_compiled_root_for_test(root_program, Value::NULL, 0));
        assert_eq!(
            root_vm
                .execute_wasm_i32(&function, &[])
                .unwrap_err()
                .wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        let root_compiled = root_vm
            .programs
            .compiled(root_program)
            .flatten()
            .expect("experimental root compilation installed");
        assert_eq!(root_compiled.native_entries(0), Some(1));
    }

    #[test]
    fn nested_call_trap_unwinds_every_shared_activation() {
        let function = crate::Engine::lower_wasm_i32_module(
            "nested-unwind",
            1,
            [
                (0, 0, true, vec![Operator::Unreachable, Operator::End]),
                (
                    0,
                    0,
                    true,
                    vec![Operator::Call { function_index: 0 }, Operator::End],
                ),
            ]
            .into_iter()
            .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(
            vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        assert!(vm.frames.is_empty());
        vm.collect_now(function.residual());
    }

    #[test]
    fn integer_traps_unwind_the_shared_activation() {
        let operators = [
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32DivS,
            Operator::End,
        ];
        let function = crate::Engine::lower_wasm_i32_function(
            "trap-unwind",
            2,
            0,
            true,
            operators.into_iter().map(Ok),
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        for args in [[1, 0], [i32::MIN, -1]] {
            assert!(
                vm.execute_wasm_i32(&function, &args)
                    .unwrap_err()
                    .wasm_trap()
                    .is_some()
            );
            assert!(vm.frames.is_empty());
            vm.collect_now(function.residual());
        }
        assert_eq!(vm.execute_wasm_i32(&function, &[7, 2]).unwrap(), Some(3));
        assert!(vm.frames.is_empty());
    }
    /// Builds a list of `nodes` words from `LIST_HEAD`, each holding the next one's address
    /// and the last zero, then runs `walk` over it (locals: 0 the node count, 1 the address,
    /// 2 and 3 the walk's) and returns local 2. The test-only floor forces every
    /// backward jump through lane-loop recovery.
    fn walk_list_at_safepoints(name: &str, nodes: i32, walk: Vec<Operator<'static>>) -> Option<crate::WasmValue> {
        const WORD: i32 = 4;
        const LIST_HEAD: i32 = WORD;
        let memarg = wasmparser::MemArg {
            align: 2,
            max_align: 2,
            offset: 0,
            memory: 0,
        };
        let mut operators = vec![
            Operator::I32Const { value: LIST_HEAD },
            Operator::LocalSet { local_index: 1 },
            Operator::Loop { blockty: wasmparser::BlockType::Empty },
            // [address] = address + WORD, up to the last node.
            Operator::LocalGet { local_index: 1 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32Const { value: WORD },
            Operator::I32Add,
            Operator::I32Store { memarg },
            Operator::LocalGet { local_index: 1 },
            Operator::I32Const { value: WORD },
            Operator::I32Add,
            Operator::LocalTee { local_index: 1 },
            Operator::LocalGet { local_index: 0 },
            Operator::I32Const { value: WORD },
            Operator::I32Mul,
            Operator::I32LtU,
            Operator::BrIf { relative_depth: 0 },
            Operator::End,
            // The last node ends the list.
            Operator::LocalGet { local_index: 1 },
            Operator::I32Const { value: 0 },
            Operator::I32Store { memarg },
            Operator::I32Const { value: LIST_HEAD },
            Operator::LocalSet { local_index: 1 },
        ];
        operators.extend(walk);
        operators.extend([Operator::LocalGet { local_index: 2 }, Operator::End]);
        let module = crate::Engine::lower_wasm_module_with_state(
            name,
            [crate::WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                    params: vec![crate::WasmType::I32],
                    results: vec![crate::WasmType::I32],
                }),
                locals: vec![wasmparser::ValType::I32; 3],
                operators: operators.into_iter().map(Ok),
            }],
            &crate::WasmTypes::default(),
            &[],
            &[crate::WasmMemory {
                ty: wasmparser::MemoryType {
                    initial: 1,
                    maximum: Some(1),
                    memory64: false,
                    shared: false,
                    page_size_log2: None,
                },
                import: None,
            }],
            &[],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        super::super::dispatch_fast::with_safepoint_floor(|| {
            vm.invoke_wasm(&instance, 0, &[crate::WasmValue::I32(nodes)]).unwrap()
        })
    }

    const WALK_MEMARG: wasmparser::MemArg = wasmparser::MemArg {
        align: 2,
        max_align: 2,
        offset: 0,
        memory: 0,
    };

    #[test]
    fn lane_safepoint_resumes_a_list_reversal_with_its_carried_register() {
        // CoreMark's `core_list_reverse` shape: the loop's head moves the list into the
        // current node and loads the next one through it, and its latch stores the previous
        // node, moves the current one there and tests the list. Both edges into the head
        // carry the list register in the accumulator, which the head reads instead of the
        // register (`edges_carry`); so must a safepoint that resumes it from the lane loop.
        // Only builds that form sibling calls carry the accumulator (`ACCUMULATOR`), so an
        // optimized test build (`cargo test --release`) is the one that checks it.
        const NODES: i32 = 3_000;
        const WORD: i32 = 4;
        const LIST_HEAD: i32 = WORD;
        // Locals: 1 the list, 2 the reversed list, 3 the current node.
        let reverse = vec![
            Operator::I32Const { value: 0 },
            Operator::LocalSet { local_index: 2 },
            Operator::I32Const { value: LIST_HEAD },
            Operator::LocalSet { local_index: 3 },
            Operator::LocalGet { local_index: 3 },
            Operator::LocalSet { local_index: 1 },
            Operator::Loop { blockty: wasmparser::BlockType::Empty },
            Operator::LocalGet { local_index: 1 },
            Operator::LocalSet { local_index: 3 },
            Operator::LocalGet { local_index: 3 },
            Operator::I32Load { memarg: WALK_MEMARG },
            Operator::LocalSet { local_index: 1 },
            Operator::LocalGet { local_index: 3 },
            Operator::LocalGet { local_index: 2 },
            Operator::I32Store { memarg: WALK_MEMARG },
            Operator::LocalGet { local_index: 3 },
            Operator::LocalSet { local_index: 2 },
            Operator::LocalGet { local_index: 1 },
            Operator::BrIf { relative_depth: 0 },
            Operator::End,
        ];
        // The reversed list starts at the last node.
        assert_eq!(
            walk_list_at_safepoints("list-reversal", NODES, reverse),
            Some(crate::WasmValue::I32(NODES * WORD))
        );
    }


}
