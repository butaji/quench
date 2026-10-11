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
                    maximum,
                    table64,
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
                        *maximum,
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
                let Some(Cell::WasmMemory { bytes, ty }) = self.heap.get(value) else {
                    return Err(JsError::wasm_link(
                        "Wasm memory import has wrong external kind",
                    ));
                };
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
                        declarations,
                        mutable,
                        ..
                    }) if *mutable == global.mutable
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
            let length = memory.byte_length().ok_or_else(|| {
                JsError::validation("Wasm memory allocation exceeds host address space".into())
            })?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(length)
                .map_err(|_| JsError::validation("Wasm memory allocation failed".into()))?;
            bytes.resize(length, 0);
            values.push(self.heap.alloc(Cell::WasmMemory {
                bytes: std::sync::Arc::new(crate::wasm::memory::MemoryStorage::new(bytes)),
                ty: Box::new(memory.ty),
            }));
        }
        for segment in module.data.iter() {
            // All segments are available before initialization effects. A later
            // trap can leave functions reachable through an imported table.
            values.push(self.heap.alloc(Cell::ArrayBuffer {
                object: Box::new(Self::empty_object(Value::NULL)),
                bytes: segment.bytes.clone(),
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
            let length = usize::try_from(table.ty.initial).map_err(|_| {
                JsError::validation("Wasm table size exceeds host address space".into())
            })?;
            let mut elements = Vec::new();
            elements
                .try_reserve_exact(length)
                .map_err(|_| JsError::validation("Wasm table allocation failed".into()))?;
            elements.resize(length, Value::NULL);
            values.push(self.heap.alloc(Cell::WasmTable {
                table64: table.ty.table64,
                elements: Box::new(elements),
                element_type: table.ty.element_type,
                declarations: Box::new(module.signatures.declarations.clone()),
                maximum: table.ty.maximum,
            }));
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
            slots: values.into_boxed_slice().into(),
            scope: Box::new(crate::heap::EnvironmentScope {
                program: Some(id.raw()),
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: crate::heap::EnvironmentBindings::Owned(vec![]),
                with_objects: Box::default(),
            }),
        });
        let root = self.root(environment);
        let previous = std::mem::replace(&mut self.active_program, id);
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
        self.active_program = previous;
        if let Err(error) = initialized {
            self.release_root(root);
            return Err(error);
        }
        Ok(crate::WasmInstance {
            module: module.clone(),
            environment: root,
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
            let binding = self.heap.alloc(Cell::WasmGlobal {
                value,
                ty: expression.value_type(),
                declarations: module.signatures.declarations.clone(),
                mutable: global.mutable,
            });
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
                            value,
                            ty,
                            declarations,
                            ..
                        }) = self.heap.get(binding)
                        else {
                            return Err(crate::Diagnostic::unsupported(
                                name,
                                "invalid Wasm initializer global binding",
                            ));
                        };
                        self.decode_wasm_value_in(*value, *ty, Some(declarations))
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
        let previous = std::mem::replace(&mut self.active_program, id);
        let result = self.wasm_function_reference(instance.module.residual(), index, environment);
        self.active_program = previous;
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
            value,
            ty,
            declarations,
            ..
        }) = self.heap.get(binding)
        else {
            return Err(JsError::validation("invalid Wasm global binding".into()));
        };
        self.decode_wasm_value_in(*value, *ty, Some(declarations))
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
            }) => Ok(crate::wasm::memory::MemoryRead::Segment(bytes)),
            _ => Err(JsError::validation("invalid Wasm memory binding".into())),
        }
    }

    pub(super) fn wasm_memory_type(
        &self,
        memory: Value,
    ) -> Result<wasmparser::MemoryType, JsError> {
        match self.heap.get(memory) {
            Some(Cell::WasmMemory { ty, .. }) => Ok(**ty),
            _ => Err(JsError::validation("invalid Wasm memory binding".into())),
        }
    }

    pub(super) fn wasm_memory_pages(&self, memory: Value) -> Result<u64, JsError> {
        let Some(Cell::WasmMemory { bytes, ty }) = self.heap.get(memory) else {
            return Err(JsError::validation("invalid Wasm memory binding".into()));
        };
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
            Some(Cell::WasmMemory { bytes, ty }) => {
                Ok((bytes.lock(), crate::wasm::memory::maximum_bytes(ty)))
            }
            _ => Err(JsError::validation(
                "invalid mutable Wasm memory binding".into(),
            )),
        }
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
                    destination.lock()[output].copy_from_slice(&source[input])
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
        let extra = length - bytes.len();
        if bytes.try_reserve_exact(extra).is_err() {
            return Ok(None);
        }
        bytes.resize(length, 0);
        let after = bytes.capacity();
        drop(bytes);
        self.heap.adjust_external_bytes(before, after);
        Ok(Some(old_pages as u64))
    }

    fn register_wasm_program(&mut self, module: &crate::WasmModule) -> Result<ProgramId, JsError> {
        let program = &module.program;
        if self.programs.len() == 0 {
            self.initialize_shared(program)?;
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
            let previous = std::mem::replace(&mut self.active_program, id);
            let result = self.wasm_function_reference(program, function.entry, environment);
            self.active_program = previous;
            Some(result?)
        } else {
            None
        };
        let args = args
            .iter()
            .copied()
            .map(|value| self.encode_wasm_value(value))
            .collect::<Vec<_>>();
        let previous = std::mem::replace(&mut self.active_program, id);
        let result = match (defined, imported) {
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
        self.active_program = previous;
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
                let Some(cell @ Cell::Array { .. }) = self.heap.get(result) else {
                    return Err(JsError::validation("invalid Wasm result bundle".into()));
                };
                let elements = cell.array_elements();
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
            crate::wasm::ScalarBits::Bits64(bits) => self.heap.alloc(Cell::WasmBits64(bits)),
            crate::wasm::ScalarBits::Bits128(bits) => self.heap.alloc(Cell::WasmV128(bits)),
        }
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
                match self.heap.get(value) {
                    Some(Cell::Function {
                        kind: FunctionKind::User(program, function),
                        ..
                    }) => self.programs.wasm_signatures(*program).is_some_and(|pool| {
                        pool.defined_type_index(*function).is_some_and(|actual| {
                            pool.matches_declaration(actual, declarations, index, exact)
                        })
                    }),
                    Some(Cell::Function {
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
        let bits = if let Some(bits) = value.as_int() {
            Some(crate::wasm::ScalarBits::Bits32(bits as u32))
        } else if let Some(Cell::WasmBits64(bits)) = self.heap.get(value) {
            Some(crate::wasm::ScalarBits::Bits64(*bits))
        } else if let Some(Cell::WasmV128(bits)) = self.heap.get(value) {
            Some(crate::wasm::ScalarBits::Bits128(*bits))
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
        assert!(vm.heap.get(owner_env).is_none());
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
        assert!(vm.heap.get(tag).is_none());
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
            assert!(vm.heap.get(owner_env).is_none());
            assert!(matches!(
                vm.heap.get(memory),
                Some(Cell::WasmMemory { ty, .. }) if ty.maximum == Some(3)
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
            assert!(vm.heap.get(memory).is_none());
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
        let host_value = vm.heap.alloc(Cell::String("start-host-root".into()));
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
            assert!(matches!(vm.heap.get(host_value), Some(Cell::String(_))));
        }
        vm.release_root(host_root);
    }

    #[test]
    fn wasm_entry_views_preserve_js_roots_and_restore_program_after_traps() {
        let script = crate::Engine::specialize("0;", "shared-root.js").unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.execute(&script).unwrap();
        let value = vm.heap.alloc(Cell::String("host-owned".into()));
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
            assert!(matches!(vm.heap.get(value), Some(Cell::String(text))
                if text.host_string() == "host-owned"));
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
        assert!(vm.heap.get(owner_env).is_some());
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
        assert!(vm.heap.get(owner_env).is_none());
        assert!(vm.heap.get(function).is_none());
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
        assert!(vm.heap.get(first_env).is_none());
        assert!(vm.heap.get(payload).is_some());
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
        assert!(vm.heap.get(payload).is_none());
        assert!(vm.heap.get(replacement).is_some());
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
        assert!(vm.heap.get(binding).is_none());
        assert!(vm.heap.get(replacement).is_none());
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
        let address = invalid.functions[0]
            .code
            .iter()
            .map(|packed| {
                if packed.is_wide() {
                    invalid.functions[0].wide[packed.wide_index()]
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
        assert!(vm.heap.get(first_memory).is_some());
        assert!(vm.heap.get(second_memory).is_some());
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
        assert!(vm.heap.get(first_memory).is_none());
        assert!(vm.heap.get(second_memory).is_some());
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
}
