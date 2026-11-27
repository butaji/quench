use super::*;

mod imports;

/// The decoded results of a Wasm call. One result is held inline (`SCALAR_RETURN_ARITY`),
/// so a scalar call returns without a heap allocation.
pub(super) type WasmResults = smallvec::SmallVec<[crate::WasmValue; crate::wasm::SCALAR_RETURN_ARITY]>;

impl Vm {
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
        let args: smallvec::SmallVec<[crate::WasmValue; super::wasm_js::WASM_INLINE_ARGUMENTS]> =
            args.iter().copied().map(crate::WasmValue::I32).collect();
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
        // A stateless function still runs in an instance (its record is what compiled code enters).
        let instance = self.instantiate_wasm_module(&function.module)?;
        let result = match self.root_value(instance.environment) {
            Some(environment) => self.execute_wasm_in_environment(function, args, environment),
            None => Err(JsError::validation("invalid Wasm instance root".into())),
        };
        self.heap.release_root(instance.environment);
        result
    }

    pub(crate) fn instantiate_wasm(
        &mut self,
        function: &crate::WasmFunction,
    ) -> Result<crate::WasmInstance, JsError> {
        self.instantiate_wasm_module(&function.module)
    }

    #[quench_macros::test_visible]
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
        module.program.validate_unless_proven().map_err(JsError::validation)?;
        if imports.len() != module.imports().len() {
            return Err(JsError::wasm_link("Wasm import count mismatch"));
        }
        let tables = self.link_wasm_table_imports(module, imports)?;
        let memories = self.link_wasm_memory_imports(module, imports)?;
        let imported_globals = self.link_wasm_global_imports(module, imports)?;
        let functions = self.link_wasm_function_imports(module, imports)?;
        let tags = self.link_wasm_tag_imports(module, imports)?;
        let id = self.register_wasm_program(module)?;
        // A host-driven sequence of instantiations need not enter dispatch.
        // Collect only before constructing the new, not-yet-rooted bindings.
        if self.heap.should_collect() {
            self.collect_now(module.residual());
        }
        // The rooted environment precedes initializer evaluation so ref.func
        // captures this instance. Undefined slots are uninitialized bindings,
        // never guest globals; each declaration installs its real cell in order.
        // Each slot in the module's one layout (`WasmLayout`).
        let mut values = Vec::new();
        for binding in module.layout().bindings() {
            values.push(match binding {
                crate::WasmBinding::Global(index) => imported_globals[index].unwrap_or(Value::UNDEFINED),
                crate::WasmBinding::Memory(index) => match memories[index] {
                    Some(value) => value,
                    None => self.alloc_wasm_memory(module.memories[index].ty)?,
                },
                // All segments are available before initialization effects. A later
                // trap can leave functions reachable through an imported table.
                crate::WasmBinding::Data(index) => {
                    let segment = &module.data[index];
                    self.heap.alloc(Cell::ArrayBuffer {
                        object: Box::new(Self::empty_object(Value::NULL)),
                        bytes: segment.bytes.clone().into(),
                        shared: false,
                        detached: false,
                        max_byte_length: segment.bytes.len(),
                        resizable: false,
                        immutable: true,
                    })
                }
                crate::WasmBinding::Table(index) => match tables[index] {
                    Some(value) => value,
                    None => self.alloc_wasm_table(module.tables[index].ty, module.signatures.declarations.clone(), Value::NULL)?,
                },
                crate::WasmBinding::Element(_) => Value::UNDEFINED,
                crate::WasmBinding::Tag(index) => tags[index].unwrap_or_else(|| {
                    let object = Box::new(Self::empty_object(self.wasm_js.tag));
                    self.heap.alloc(Cell::WasmTag { object, declarations: module.signatures.declarations.clone(), ty: module.tags[index].ty })
                }),
                // The instance's record (`install_wasm_instance`) takes the slot before the imported
                // function slots, which end the resource layout; ref.func derives their base.
                crate::WasmBinding::Instance => Value::UNDEFINED,
            });
        }
        values.extend(functions);
        let environment = self.heap.alloc_environment(
            Value::NULL,
            0,
            values.into(),
            crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                program: Some(id.raw()),
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: crate::heap::EnvironmentBindings::Owned(vec![]),
                with_objects: Box::default(),
            }),
        ).0;
        let root = self.root(environment);
        let previous = self.replace_active_program(id);
        let missing = || JsError::validation("invalid Wasm instance environment".into());
        let initialized = self
            .install_wasm_instance(environment, id)
            .ok_or_else(missing)
            .and_then(|()| self.initialize_wasm_globals(module, environment))
            .and_then(|()| self.bind_wasm_instance(environment, id).ok_or_else(missing))
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
        let guarded = (!ty.memory64
            && (ty.page_size() as usize).is_multiple_of(crate::wasm::memory::HOST_PAGE))
        .then(|| crate::wasm::memory::LinearBytes::reserved(length))
        .flatten();
        let bytes = match guarded {
            Some(bytes) => bytes,
            None => {
                let bytes = super::array_buffer::zeroed_byte_vec(length)
                    .ok_or_else(|| JsError::validation("Wasm memory allocation failed".into()))?;
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
        let target = self.wasm_element_target(initial);
        let elements =
            crate::heap::WasmTableElements::filled(length, ty.maximum, initial, target)
                .ok_or_else(|| JsError::validation("Wasm table allocation failed".into()))?;
        Ok(self.heap.alloc(Cell::WasmTable {
            object: Box::new(Self::empty_object(self.wasm_js.table)),
            table64: ty.table64,
            elements: Box::new(elements),
            element_type: ty.element_type,
            declarations,
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
        let bits = self.wasm_global_bits(ty, value);
        self.heap.alloc(Cell::WasmGlobal {
            header: Box::new(crate::heap::WasmGlobalHeader {
                object: Self::empty_object(self.wasm_js.global),
                declarations,
            }),
            bits,
            ty,
            mutable,
        })
    }

    /// Fills each table that has a reference initializer with its value, the tables' slots in
    /// `environment` starting at `table_base`.
    fn fill_wasm_reference_tables(&mut self, module: &crate::WasmModule, environment: Value, table_base: usize) -> Result<(), JsError> {
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
            let target = self.wasm_element_target(initial);
            let Some(Cell::WasmTable { elements, .. }) = self.heap.get_mut(table) else {
                unreachable!("allocated table")
            };
            elements.fill(0..elements.len(), initial, target);
        }
        Ok(())
    }

    /// Materializes each element segment's references into an elements cell, the segments' slots in
    /// `environment` starting at `element_base`.
    fn allocate_wasm_element_segments(&mut self, module: &crate::WasmModule, environment: Value, element_base: usize) -> Result<(), JsError> {
        for (index, segment) in module.elements.iter().enumerate() {
            let mut values = Vec::new();
            values
                .try_reserve_exact(segment.items.len())
                .map_err(|_| JsError::validation("Wasm element allocation failed".into()))?;
            for value in segment.items.iter() {
                values.push(self.wasm_reference_initializer(module, &value, environment)?);
            }
            // Materializing references can allocate. The instance root owns each
            // completed segment until it is installed in a table or kept passive.
            let elements = self.heap.alloc(Cell::WasmElements(values));
            *self
                .heap
                .environment_slot_mut(environment, element_base + index)
                .unwrap() = elements;
        }
        Ok(())
    }

    fn initialize_wasm_references(
        &mut self,
        module: &crate::WasmModule,
        environment: Value,
    ) -> Result<(), JsError> {
        let table_base = module.layout().slot(crate::WasmBinding::Table(0));
        let element_base = module.layout().slot(crate::WasmBinding::Element(0));
        self.fill_wasm_reference_tables(module, environment, table_base)?;
        self.allocate_wasm_element_segments(module, environment, element_base)?;
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
        let data_base = module.layout().slot(crate::WasmBinding::Data(0));
        for (index, segment) in module.data.iter().enumerate() {
            let crate::WasmDataMode::Active { memory, offset } = &segment.mode else {
                continue;
            };
            let memory = self
                .heap
                .environment_slot(environment, module.layout().slot(crate::WasmBinding::Memory(*memory as usize)))
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
            .alloc_object(Self::empty_object(Value::NULL));
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
                if self.heap.wasm_gc(value).is_some() {
                    return value;
                }
                // JS host numbers in the signed i31 range have the canonical immediate representation.
                if let Some(int) = value.as_number().and_then(crate::value::exact_int32)
                    && (crate::wasm::i31::SIGNED_MIN..=crate::wasm::i31::SIGNED_MAX).contains(&int)
                    {
                        return Value::integer(int & crate::wasm::i31::I31_MASK);
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
                        let field = |_: &Self, index: usize| fields.as_ref().map_or(Value::UNDEFINED, |fields| fields[index]);
                        let values = fields.as_ref().map(|fields| (fields.len(), &field as &dyn Fn(&Self, usize) -> Value));
                        self.wasm_struct_new(&module.signatures.declarations, index, values, descriptor)
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
                        self.wasm_global_read(binding).map_err(|error| {
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

    #[quench_macros::test_visible]
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
            .environment_slot(environment, instance.module.layout().slot(crate::WasmBinding::Memory(index as usize)))
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

    #[quench_macros::test_visible]
    pub(crate) fn wasm_global(
        &self,
        instance: &crate::WasmInstance,
        index: u32,
    ) -> Result<crate::WasmValue, JsError> {
        self.wasm_global_read(self.wasm_global_binding(instance, index)?)
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

    pub fn wasm_memory_bytes(
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

    /// Runs a staged read rule over the bytes of the linear memory `memory`, on the
    /// interpreter's `ConcreteBytes` stage.
    pub(super) fn wasm_memory_read<R>(
        &self,
        memory: Value,
        rule: impl FnOnce(&mut crate::staged::wasm_machine::ConcreteBytes<'_>) -> R,
    ) -> Result<R, JsError> {
        let Some(Cell::WasmMemory { bytes, .. }) = self.heap.get(memory) else {
            return Err(JsError::validation("invalid Wasm memory binding".into()));
        };
        let guard = bytes.lock();
        Ok(rule(&mut crate::staged::wasm_machine::ConcreteBytes { memory: &guard }))
    }

    /// The log2 of the page size of the Wasm memory `memory`, which `memory.size` counts in;
    /// `None` for a value that is no Wasm memory.
    pub fn wasm_memory_page_bits(&self, memory: Value) -> Option<u32> {
        self.wasm_memory_type(memory).ok().map(|ty| ty.page_size_log2())
    }

    #[inline] pub fn wasm_memory_index_type(&self, memory: Value) -> Result<crate::WasmType, JsError> {
        let Some(Cell::WasmMemory { header, .. }) = self.heap.get(memory) else {
            return Err(JsError::validation("invalid Wasm memory binding".into()));
        };
        crate::WasmType::from_wasm(header.ty.index_type()).ok_or_else(|| JsError::validation("invalid Wasm memory index type".into()))
    }

    pub fn wasm_memory_index(&self, memory: Value, value: Value) -> Result<u64, JsError> {
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

    pub(super) fn wasm_memory_mut(
        &self,
        memory: Value,
    ) -> Result<crate::wasm::memory::MemoryGuard<'_>, JsError> {
        match self.heap.get(memory) {
            Some(Cell::WasmMemory { bytes, .. }) => Ok(bytes.lock()),
            _ => Err(JsError::validation(
                "invalid mutable Wasm memory binding".into(),
            )),
        }
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
                // A poll: byte debt only. Wasm's cell debt is serviced at its op boundary (the general loop's `Continue`).
                self.poll_byte_debt(p);
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

    /// A scalar load of `access` (its width, its type and its sign) at the address of the
    /// instruction: the memory in `b`, the 32-bit address in `c`, the static offset in `imm`.
    /// Out of bounds traps before any value is written.
    #[inline(always)]
    pub(super) fn wasm_direct_load(
        &mut self,
        f: usize,
        i: crate::bytecode::WideInstruction,
        access: crate::staged::wasm_memory::Access,
    ) -> Result<(), JsError> {
        let address = self.wasm_direct_address(f, i)?;
        let memory = self.read(f, i.register_b());
        let value = access
            .read(&self.wasm_memory_bytes(memory)?, address)
            .map_err(JsError::wasm_trap_error)?;
        let value = self.encode_wasm_value(value);
        self.write(f, i.result_register(), value);
        Ok(())
    }

    /// A scalar store of `access` of the value in `a` at the address of the instruction. The
    /// value decodes to the access's type; out of bounds traps before any byte changes.
    #[inline(always)]
    pub(super) fn wasm_direct_store(
        &mut self,
        f: usize,
        i: crate::bytecode::WideInstruction,
        access: crate::staged::wasm_memory::Access,
    ) -> Result<(), JsError> {
        let address = self.wasm_direct_address(f, i)?;
        let memory = self.read(f, i.register_b());
        let value = self.decode_wasm_value(self.read(f, i.register_a()), access.ty)?;
        access
            .write(&mut self.wasm_memory_mut(memory)?, address, value)
            .map_err(JsError::wasm_trap_error)
    }

    pub(super) fn wasm_u32_operand(value: Value) -> Result<u32, JsError> {
        value
            .numeric_key()
            .map(|value| value as u32)
            .ok_or_else(|| JsError::validation("invalid Wasm i32 operand".into()))
    }

    /// Copy memory or immutable data bytes; None is the dropped segment state.
    /// Check both ranges before mutation, including zero-length boundary cases.
    pub fn wasm_copy_bytes(
        &mut self,
        destination: Value,
        source: Option<Value>,
        output: u64,
        input: u64,
        length: u64,
    ) -> Result<(), JsError> {
        if let Some(source) = source
            && let (Some(Cell::WasmMemory { bytes: destination_bytes, .. }), Some(Cell::WasmMemory { bytes: source_bytes, .. })) =
                (self.heap.get(destination), self.heap.get(source))
        {
            let mut stage = crate::staged::wasm_machine::ConcretePair {
                destination: destination_bytes,
                source: source_bytes,
            };
            return crate::staged::wasm_memory::copy_across(&mut stage, output, input, length)
                .map_err(JsError::wasm_trap_error);
        }
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
        let Some(Cell::WasmMemory {
            bytes: destination, ..
        }) = self.heap.get(destination)
        else {
            unreachable!("validated destination binding")
        };
        match self.heap.get(source) {
            Some(Cell::ArrayBuffer { bytes: source, .. }) => {
                if source.is_wasm_view_of(destination) {
                    destination.lock().copy_within(input, output.start);
                } else if let (crate::wasm::memory::MemoryStorage::Shared(destination), Some(source)) = (&**destination, source.shared_backing()) {
                    destination.0.copy_from(&source, input, output);
                } else {
                    destination.lock()[output].copy_from_slice(&source.read()[input]);
                }
            }
            _ => unreachable!("validated source binding"),
        }
        Ok(())
    }

    pub fn wasm_memory_grow(
        &mut self,
        memory: Value,
        delta: u64,
    ) -> Result<Option<u64>, JsError> {
        let _order = crate::shared_backing::memory_order();
        let ty = self.wasm_memory_type(memory)?;
        let (page_size, max_byte_length) = (ty.page_size() as usize, crate::wasm::memory::maximum_bytes(&ty));
        let mut bytes = self.wasm_memory_mut(memory)?;
        let before = bytes.capacity();
        let Some(old_pages) = bytes.grow_pages(delta, page_size, max_byte_length) else {
            return Ok(None);
        };
        let after = bytes.capacity();
        drop(bytes);
        self.heap.adjust_external_bytes(before, after);
        self.refresh_wasm_memory_buffer(memory);
        Ok(Some(old_pages))
    }

    /// The buffer viewing the memory's bytes, created on first request. Shared
    /// buffers preserve their accessible prefix when another agent grows the memory.
    pub(super) fn wasm_memory_buffer(&mut self, memory: Value) -> Option<Value> {
        let Some(Cell::WasmMemory { bytes, buffer, .. }) = self.heap.get(memory) else {
            return None;
        };
        let shared = matches!(&**bytes, crate::wasm::memory::MemoryStorage::Shared(_));
        if !buffer.is_undefined() && (!shared || matches!(self.heap.get(*buffer),
            Some(Cell::ArrayBuffer { bytes: view, .. }) if view.len() == bytes.len())) {
            return Some(*buffer);
        }
        let view = crate::heap::BufferBytes::wasm_view(bytes)?;
        let byte_length = view.len();
        let buffer = self.heap.alloc(Cell::ArrayBuffer {
            object: Box::new(Self::empty_object(if shared { self.shared_array_buffer_proto } else { self.array_buffer_proto })),
            bytes: view,
            shared,
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
        if matches!(self.heap.get(previous), Some(Cell::ArrayBuffer { shared: false, .. })) {
            self.detach_array_buffer(previous);
        }
    }

    fn register_wasm_program(&mut self, module: &crate::WasmModule) -> Result<ProgramId, JsError> {
        let program = &module.program;
        if self.programs.len() == 0 {
            self.initialize_shared(program, super::builtins::RealmScope::Core)?;
        }
        let found = self.programs.find_shared(program);
        let fresh = found.is_none();
        let id = if let Some(id) = found {
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
        // A Wasm module lowers through the one image driver as any program does, planned from its declarations.
        if fresh { crate::vm::engine_types::AheadImages::record_wasm(self, id, program, &module.binding_types()); }
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
        program.validate_unless_proven().map_err(JsError::validation)?;
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
        let args: smallvec::SmallVec<[Value; super::wasm_js::WASM_INLINE_ARGUMENTS]> = args
            .iter()
            .copied()
            .map(|value| self.encode_wasm_value(value))
            .collect();
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
        .map(WasmResults::into_vec)
    }

    /// The decoded results of a call, a single one held inline (`SCALAR_RETURN_ARITY`).
    pub(super) fn decode_wasm_result_types(
        &self,
        types: &[crate::WasmType],
        result: Value,
        declarations: Option<&crate::WasmTypes>,
    ) -> Result<WasmResults, JsError> {
        match types {
            [] => Ok(WasmResults::new()),
            [ty] => self
                .decode_wasm_value_in(result, *ty, declarations)
                .map(|value| smallvec::smallvec![value]),
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
                    .map(|(value, ty)| self.decode_wasm_value_in(value, *ty, declarations))
                    .collect()
            }
        }
    }

    #[inline] pub fn wasm_i64_operand(&self, value: Value) -> Result<i64, JsError> {
        if let Some(bits) = value.bits64() { return Ok(bits as i64); }
        let crate::WasmValue::I64(value) = self.decode_wasm_value_slow(value, crate::WasmType::I64, None)? else { unreachable!("decoded i64 operand") };
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

    /// The register value of a raw i64 operation's result: an i64 as its word, which a raw
    /// register holds; anything else (an i32 flag) as `encode_wasm_value`.
    pub(super) fn encode_raw_wasm_value(&mut self, value: crate::WasmValue) -> Value {
        match value {
            crate::WasmValue::I64(word) => Value::from_bits(word as u64),
            crate::WasmValue::F64(bits) => Value::from_float_bits64(bits),
            value => self.encode_wasm_value(value),
        }
    }

    pub fn encode_wasm_value(&mut self, value: crate::WasmValue) -> Value {
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
    pub fn encode_bits64(&mut self, bits: u64) -> Value {
        Value::from_bits64(bits).unwrap_or_else(|| self.heap.alloc(Cell::WasmBits64(bits)))
    }

    /// The 64-bit Wasm payload of a slot value typed i64 or f64.
    #[inline(always)]
    pub fn decode_bits64(&self, value: Value) -> Option<u64> {
        value.bits64().or_else(|| match self.heap.get(value) {
            Some(Cell::WasmBits64(bits)) => Some(*bits),
            _ => None,
        })
    }

    pub fn decode_wasm_value(
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
                let Some(record) = self.heap.wasm_gc(value) else {
                    return false;
                };
                record.declarations().reference_subtype(
                    wasmparser::RefType::new(
                        false,
                        wasmparser::HeapType::Exact(wasmparser::UnpackedIndex::Module(record.ty())),
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

    pub fn decode_wasm_value_in(
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
        self.decode_wasm_value_slow(value, ty, declarations)
    }

    /// `decode_wasm_value_in` without its fast paths, for a caller that has tested them.
    pub(super) fn decode_wasm_value_slow(
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
        // A vector lives only in its cell, which the scalar probes below would miss first.
        if ty == crate::WasmType::V128 {
            return match self.heap.v128(value) {
                Some(bits) => Ok(crate::WasmValue::V128(u128::from_le_bytes(bits))),
                _ => Err(JsError::validation("invalid Wasm scalar representation".into())),
            };
        }
        let bits = if let Some(bits) = value.numeric_key() {
            Some(crate::wasm::ScalarBits::Bits32(bits as u32))
        } else if let Some(bits) = self.decode_bits64(value) {
            Some(crate::wasm::ScalarBits::Bits64(bits))
        } else { self.heap.v128(value).map(crate::wasm::ScalarBits::Bits128) };
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

        vm.programs.compile_wasm_modules_at_first_entry();
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
        vm.programs.compile_wasm_modules_at_first_entry();
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
    fn compiled_wasm_simd_matches_the_interpreter_bit_for_bit() {
        // Vector accesses, the operators compiled code lowers itself (i64x2 add and shift,
        // xor, shuffle, extending multiply) and ones it runs through their kernels (i32x4
        // multiply, popcount, a lane extracted to a scalar), over the same memory in both
        // execution modes.
        const WORDS: i32 = 16;
        const SOURCE: i32 = 64;
        const TARGET: i32 = 128;
        let memarg = |offset| wasmparser::MemArg {
            align: 0,
            max_align: 4,
            offset,
            memory: 0,
        };
        let word = |offset| wasmparser::MemArg {
            align: 2,
            max_align: 2,
            offset,
            memory: 0,
        };
        let mut lanes = [0u8; 16];
        for (index, lane) in lanes.iter_mut().enumerate() {
            *lane = (index as u8 * 7) % 32;
        }
        // simd(source, target, shift): the combination stored at target, a lane after it.
        let simd = vec![
            Operator::LocalGet { local_index: 1 },
            Operator::LocalGet { local_index: 0 },
            Operator::V128Load { memarg: memarg(0) },
            Operator::LocalGet { local_index: 0 },
            Operator::V128Load { memarg: memarg(16) },
            Operator::I64x2Add,
            Operator::LocalGet { local_index: 2 },
            Operator::I64x2Shl,
            Operator::LocalGet { local_index: 0 },
            Operator::V128Load { memarg: memarg(0) },
            Operator::LocalGet { local_index: 0 },
            Operator::V128Load { memarg: memarg(16) },
            Operator::I8x16Shuffle { lanes },
            Operator::V128Xor,
            Operator::LocalGet { local_index: 0 },
            Operator::V128Load { memarg: memarg(0) },
            Operator::I64x2ExtMulLowI32x4U,
            Operator::LocalGet { local_index: 0 },
            Operator::V128Load { memarg: memarg(16) },
            Operator::I32x4Mul,
            Operator::I8x16Popcnt,
            Operator::V128Store { memarg: memarg(0) },
            Operator::LocalGet { local_index: 1 },
            Operator::LocalGet { local_index: 1 },
            Operator::V128Load { memarg: memarg(0) },
            Operator::I32x4ExtractLane { lane: 2 },
            Operator::I32Store { memarg: word(16) },
            Operator::End,
        ];
        let store = vec![
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32Store { memarg: word(0) },
            Operator::End,
        ];
        let load = vec![
            Operator::LocalGet { local_index: 0 },
            Operator::I32Load { memarg: word(0) },
            Operator::End,
        ];
        let body = |params: usize, result: bool, operators: Vec<Operator<'static>>| crate::WasmFunctionBody {
            ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                params: vec![crate::WasmType::I32; params],
                results: if result { vec![crate::WasmType::I32] } else { vec![] },
            }),
            locals: vec![],
            operators: operators.into_iter().map(Ok),
        };
        let module = crate::Engine::lower_wasm_module_with_state(
            "compiled-simd",
            [body(3, false, simd), body(2, false, store), body(1, true, load)],
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
        let run = |mode: crate::ExecutionMode, shift: i32| {
            let mut vm = Vm::new(crate::SystemHost);
            vm.programs.compile_wasm_modules_at_first_entry();
            vm.set_execution_mode(mode);
            let instance = vm.instantiate_wasm_module(&module).unwrap();
            for index in 0..WORDS {
                let value = (index + 1).wrapping_mul(0x9E37_79B9_u32 as i32);
                vm.invoke_wasm(
                    &instance,
                    1,
                    &[crate::WasmValue::I32(SOURCE + index * 4), crate::WasmValue::I32(value)],
                )
                .unwrap();
            }
            vm.invoke_wasm(
                &instance,
                0,
                &[
                    crate::WasmValue::I32(SOURCE),
                    crate::WasmValue::I32(TARGET),
                    crate::WasmValue::I32(shift),
                ],
            )
            .unwrap();
            let words = (0..5)
                .map(|index| vm.invoke_wasm(&instance, 2, &[crate::WasmValue::I32(TARGET + index * 4)]).unwrap())
                .collect::<Vec<_>>();
            let native = vm
                .programs
                .find_shared(&module.program)
                .and_then(|program| vm.programs.compiled(program).flatten())
                .and_then(|compiled| compiled.native_entries(0));
            (words, native)
        };
        // A shift count past the lane width wraps, as Wasm's does.
        for shift in [0, 5, 63, 64, 70] {
            let (interpreted, _) = run(crate::ExecutionMode::Interpreted, shift);
            let (compiled, native) = run(crate::ExecutionMode::Compiled, shift);
            assert_eq!(compiled, interpreted, "shift {shift}");
            assert!(native.is_some_and(|entries| entries > 0), "the SIMD function ran natively");
        }
        // A vector access past the end traps in compiled code too.
        let mut vm = Vm::new(crate::SystemHost);
        vm.programs.compile_wasm_modules_at_first_entry();
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        let error = vm
            .invoke_wasm(
                &instance,
                0,
                &[
                    crate::WasmValue::I32(65_536 - 24),
                    crate::WasmValue::I32(TARGET),
                    crate::WasmValue::I32(1),
                ],
            )
            .unwrap_err();
        assert_eq!(error.wasm_trap(), Some(crate::WasmTrap::OutOfBoundsMemory));
    }

    #[test]
    fn compiled_wasm_vector_accesses_compile_through_the_prologue_memory_register() {
        // A function whose only accesses are vector ones binds its memory once, in its
        // prologue, and that binding is its memory register (`memory_register`).
        let memarg = |offset| wasmparser::MemArg {
            align: 0,
            max_align: 4,
            offset,
            memory: 0,
        };
        let word = wasmparser::MemArg {
            align: 2,
            max_align: 2,
            offset: 0,
            memory: 0,
        };
        // double(address): the two i64 lanes at address, added to themselves, at address + 16.
        let double = vec![
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 0 },
            Operator::V128Load { memarg: memarg(0) },
            Operator::LocalGet { local_index: 0 },
            Operator::V128Load { memarg: memarg(0) },
            Operator::I64x2Add,
            Operator::V128Store { memarg: memarg(16) },
            Operator::End,
        ];
        let store = vec![
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32Store { memarg: word },
            Operator::End,
        ];
        let load = vec![
            Operator::LocalGet { local_index: 0 },
            Operator::I32Load { memarg: word },
            Operator::End,
        ];
        let body = |params: usize, result: bool, operators: Vec<Operator<'static>>| crate::WasmFunctionBody {
            ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                params: vec![crate::WasmType::I32; params],
                results: if result { vec![crate::WasmType::I32] } else { vec![] },
            }),
            locals: vec![],
            operators: operators.into_iter().map(Ok),
        };
        let module = crate::Engine::lower_wasm_module_with_state(
            "compiled-vector-binding",
            [body(1, false, double), body(2, false, store), body(1, true, load)],
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
        let function = module.residual().function_get(0).expect("double");
        assert_eq!(super::super::dispatch_fast::memory_register(function), Some(0));
        let mut vm = Vm::new(crate::SystemHost);
        vm.programs.compile_wasm_modules_at_first_entry();
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        let i32s = |values: &[i32]| values.iter().map(|value| crate::WasmValue::I32(*value)).collect::<Vec<_>>();
        for (index, value) in [0x7fff_ffff, 1, -1, 3].into_iter().enumerate() {
            vm.invoke_wasm(&instance, 1, &i32s(&[index as i32 * 4, value])).unwrap();
        }
        vm.invoke_wasm(&instance, 0, &i32s(&[0])).unwrap();
        let words = (4..8)
            .map(|index| vm.invoke_wasm(&instance, 2, &i32s(&[index * 4])).unwrap())
            .collect::<Vec<_>>();
        // Lane 0 is 0x00000001_7fffffff, lane 1 is 0x00000003_ffffffff; each doubles.
        assert_eq!(words, i32s(&[-2, 2, -2, 7]).into_iter().map(Some).collect::<Vec<_>>());
        let compiled = vm
            .programs
            .find_shared(&module.program)
            .and_then(|program| vm.programs.compiled(program).flatten())
            .expect("compiled program");
        assert_eq!(compiled.native_entries(0), Some(1));
        // Last full source/destination vectors fit; partial and wrapped accesses trap.
        vm.invoke_wasm(&instance, 0, &i32s(&[65_536 - 32])).unwrap();
        for address in [65_536 - 31, 65_536 - 16, -1] {
            assert_eq!(
                vm.invoke_wasm(&instance, 0, &i32s(&[address])).unwrap_err().wasm_trap(),
                Some(crate::WasmTrap::OutOfBoundsMemory),
                "vector boundary {address}"
            );
        }

    }

    #[test]
    fn memory_first_compile_bounds_instructions_in_flight_and_compiles_every_function() {
        // More functions than workers, together past memory-first's in-flight bound, so the
        // building thread waits for results before sending more.
        const FUNCTIONS: usize = 8;
        const STEPS: usize = 1_500;
        let step = |function: usize, index: usize| (function * STEPS + index) as i32;
        let body = |function: usize| {
            let mut operators = vec![Operator::LocalGet { local_index: 0 }];
            for index in 0..STEPS {
                operators.extend([
                    Operator::I32Const { value: step(function, index) },
                    Operator::I32Add,
                    Operator::I32Const { value: step(function, index).rotate_left(7) },
                    Operator::I32Xor,
                ]);
            }
            operators.push(Operator::End);
            crate::WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                    params: vec![crate::WasmType::I32],
                    results: vec![crate::WasmType::I32],
                }),
                locals: vec![],
                operators: operators.into_iter().map(Ok),
            }
        };
        let expected = |function: usize, argument: i32| {
            (0..STEPS).fold(argument, |value, index| {
                value.wrapping_add(step(function, index)) ^ step(function, index).rotate_left(7)
            })
        };
        let module = crate::Engine::lower_wasm_module_with_state(
            "memory-first-compile",
            (0..FUNCTIONS).map(body),
            &crate::WasmTypes::default(),
            &[],
            &[],
            &[],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_memory_policy(crate::MemoryPolicy::MemoryFirst);
        vm.programs.compile_wasm_modules_at_first_entry();
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        for function in 0..FUNCTIONS {
            let argument = function as i32 * 1_000_003;
            assert_eq!(
                vm.invoke_wasm(&instance, function as u32, &[crate::WasmValue::I32(argument)]).unwrap(),
                Some(crate::WasmValue::I32(expected(function, argument))),
                "function {function}"
            );
        }
        let compiled = vm
            .programs
            .find_shared(&module.program)
            .and_then(|program| vm.programs.compiled(program).flatten())
            .expect("compiled program");
        assert!((0..FUNCTIONS).all(|function| compiled.native_entries(function) == Some(1)));
    }

    #[test]
    fn compiled_wasm_memory_copy_moves_overlapping_ranges_and_traps_out_of_bounds() {
        const PAGE: i32 = 65_536;
        let memarg = wasmparser::MemArg {
            align: 2,
            max_align: 2,
            offset: 0,
            memory: 0,
        };
        // copy(destination, source, length), then the word at 0 and the word at 4.
        let module = crate::Engine::lower_wasm_module_with_state(
            "compiled-memory-copy",
            [
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![crate::WasmType::I32; 3],
                        results: vec![crate::WasmType::I32],
                    }),
                    locals: vec![],
                    operators: vec![
                        Operator::LocalGet { local_index: 0 },
                        Operator::LocalGet { local_index: 1 },
                        Operator::LocalGet { local_index: 2 },
                        Operator::MemoryCopy { dst_mem: 0, src_mem: 0 },
                        Operator::I32Const { value: 0 },
                        Operator::I32Load { memarg },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                crate::WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                        params: vec![crate::WasmType::I32; 2],
                        results: vec![],
                    }),
                    locals: vec![],
                    operators: vec![
                        Operator::LocalGet { local_index: 0 },
                        Operator::LocalGet { local_index: 1 },
                        Operator::I32Store { memarg },
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
        vm.programs.compile_wasm_modules_at_first_entry();
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        let i32s = |values: &[i32]| values.iter().map(|value| crate::WasmValue::I32(*value)).collect::<Vec<_>>();
        let copy = |vm: &mut Vm, args: &[i32]| vm.invoke_wasm(&instance, 0, &i32s(args));
        vm.invoke_wasm(&instance, 1, &i32s(&[0, 0x0403_0201])).unwrap();
        // Forward overlap: bytes 0..3 move up by one.
        assert_eq!(copy(&mut vm, &[1, 0, 3]).unwrap(), Some(crate::WasmValue::I32(0x0302_0101)));
        // Backward overlap: bytes 1..4 move down by one.
        assert_eq!(copy(&mut vm, &[0, 1, 3]).unwrap(), Some(crate::WasmValue::I32(0x0303_0201)));
        // A range past the end traps before any byte moves, from either side.
        for args in [[PAGE - 2, 0, 4], [0, PAGE - 2, 4]] {
            let error = copy(&mut vm, &args).unwrap_err();
            assert_eq!(error.wasm_trap(), Some(crate::WasmTrap::OutOfBoundsMemory), "{args:?}");
        }
        assert_eq!(copy(&mut vm, &[0, 0, 0]).unwrap(), Some(crate::WasmValue::I32(0x0303_0201)));
        // An empty copy at the end is in bounds; one byte further is not.
        assert!(copy(&mut vm, &[PAGE, PAGE, 0]).is_ok());
        assert!(copy(&mut vm, &[PAGE + 1, 0, 0]).is_err());
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("memory.copy compiled");
        assert!(compiled.native_entries(0).is_some_and(|entries| entries > 0));
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

        vm.programs.compile_wasm_modules_at_first_entry();
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

        vm.programs.compile_wasm_modules_at_first_entry();
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
        for indirect in [false, true] {
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
            let vector = wasmparser::MemArg { align: 0, max_align: 4, offset: 0, memory: 0 };
            // The first vector bounds check reads the old length. The second must see
            // growth across the call, before any fill/helper can refresh or clobber it.
            let mut caller = vec![
                Operator::I32Const { value: 0 },
                Operator::V128Load { memarg: vector },
                Operator::Drop,
            ];
            if indirect {
                caller.extend([
                    Operator::I32Const { value: 0 },
                    Operator::CallIndirect { type_index: 0, table_index: 0 },
                ]);
            } else {
                // A native callee may grow via its supported indirect VM callout.
                // A direct call to the unsupported memory.grow body itself is not admitted.
                caller.push(Operator::Call { function_index: 3 });
            }
            caller.extend([
                Operator::Drop,
                Operator::I32Const { value: 65_536 },
                Operator::V128Load { memarg: vector },
                Operator::Drop,
                Operator::I32Const { value: 65_536 },
                Operator::I32Const { value: 0x5a },
                Operator::I32Const { value: 1 },
                Operator::MemoryFill { mem: 0 },
                Operator::I32Const { value: 65_536 },
                Operator::I32Load { memarg },
                Operator::End,
            ]);
            // This regression tests invalidation across actual compiled calls, not
            // inlining of the growth helper into an interpreted caller.
            let kept_before = crate::wasm::COMPILED_CALLS_KEPT_FOR_TEST.with(|kept| kept.replace(true));
            let module = crate::Engine::lower_wasm_module_for(
                crate::ExecutionMode::Compiled,
                "compiled-call-growth",
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
                        operators: caller.into_iter().map(Ok),
                    },
                    crate::WasmFunctionBody {
                        ty: crate::WasmCallableType::Embedded(signature.clone()),
                        locals: vec![],
                        operators: vec![Operator::MemorySize { mem: 0 }, Operator::End]
                            .into_iter()
                            .map(Ok),
                    },
                    crate::WasmFunctionBody {
                        ty: crate::WasmCallableType::Embedded(signature),
                        locals: vec![],
                        operators: vec![
                            Operator::I32Const { value: 0 },
                            Operator::CallIndirect { type_index: 0, table_index: 0 },
                            Operator::End,
                        ].into_iter().map(Ok),
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
                &[],
                &[],
                &[],
            );
            crate::wasm::COMPILED_CALLS_KEPT_FOR_TEST.with(|kept| kept.set(kept_before));
            let module = module.unwrap();
            let mut vm = Vm::new(crate::SystemHost);
            vm.programs.compile_wasm_modules_at_first_entry();
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
                .expect("caller compiled");
            assert_eq!(compiled.native_entries(1), Some(1));
            assert_eq!(compiled.native_entries(2), Some(1));
            // native_entries counts entry-ABI requests. The direct body call to
            // function 3 stays native without crossing that entry ABI.
        }
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
        vm.programs.compile_wasm_modules_at_first_entry();
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
        // The indirect call enters the eligible nine-parameter callee in native code through
        // its table row, spilling its arguments to the call's frame: only the caller is entered
        // from the VM.
        assert_eq!(compiled.native_entries(0), Some(0));
        assert_eq!(compiled.native_entries(1), Some(1));
        assert_eq!(compiled.direct_reference_entries(0), Some(0));
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
        vm.programs.compile_wasm_modules_at_first_entry();
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
        assert_eq!(compiled.native_entries(0), Some(0));
        assert_eq!(compiled.native_entries(1), Some(1));
        assert_eq!(compiled.direct_reference_entries(0), Some(0));
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
        vm.programs.compile_wasm_modules_at_first_entry();
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
        assert_eq!(compiled.native_entries(0), Some(0));
        assert_eq!(compiled.native_entries(1), Some(1));
        assert_eq!(compiled.direct_reference_entries(0), Some(0));
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
        vm.programs.compile_wasm_modules_at_first_entry();
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
        assert_eq!(compiled.direct_reference_entries(0), Some(0));
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
        vm.programs.compile_wasm_modules_at_first_entry();
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let first = vm.instantiate_wasm_module(&module).unwrap();
        let second = vm.instantiate_wasm_module(&module).unwrap();
        let first_env = vm.root_value(first.environment).unwrap();
        let second_env = vm.root_value(second.environment).unwrap();
        assert_ne!(first_env, second_env);
        let first_memory = vm.wasm_memory(&first, 0).unwrap();
        let second_memory = vm.wasm_memory(&second, 0).unwrap();
        vm.wasm_memory_bulk(first_memory, |stage| crate::staged::wasm_memory::fill(stage, 0, 0x11, 4))
            .unwrap();
        vm.wasm_memory_bulk(second_memory, |stage| crate::staged::wasm_memory::fill(stage, 0, 0x22, 4))
            .unwrap();

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
        // The compiled target is called through its row: its site never calls out.
        assert!(compiled.reference_call_site_counts().unwrap().iter().any(
            |(function, _, total, native, fallback)| {
                *function == 2 && *total == 0 && *native == 0 && *fallback == 0
            }
        ));
        assert_eq!(compiled.same_artifact_direct_reference_hits(), Some(0));

        assert_eq!(
            vm.invoke_wasm(&instance, 3, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        assert!(compiled.reference_call_site_counts().unwrap().iter().any(
            |(function, _, total, native, fallback)| {
                *function == 3 && *total == 1 && *native == 0 && *fallback == 1
            }
        ));
        assert_eq!(compiled.same_artifact_direct_reference_hits(), Some(0));
        assert!(compiled.reference_call_target_counts().unwrap().iter().any(
            |(caller, _, target, total, native, same_artifact)| {
                *caller == 3 && *target == 1 && *total == 1 && *native == 0 && *same_artifact == 0
            }
        ));
    }

    /// A function body of the embedded type `ty` with no locals, running `operators`.
    fn embedded_body(
        ty: crate::WasmSignature,
        operators: Vec<Operator<'static>>,
    ) -> crate::WasmFunctionBody<std::vec::IntoIter<Result<Operator<'static>, wasmparser::BinaryReaderError>>> {
        crate::WasmFunctionBody {
            ty: crate::WasmCallableType::Embedded(ty),
            locals: vec![],
            operators: operators.into_iter().map(Ok).collect::<Vec<_>>().into_iter(),
        }
    }

    /// A fixed-size `funcref` table of `size` entries, each initialized to function 0.
    fn funcref_table(size: u64) -> crate::WasmTable {
        crate::WasmTable {
            ty: wasmparser::TableType {
                element_type: wasmparser::RefType::FUNCREF,
                table64: false,
                initial: size,
                maximum: Some(size),
                shared: false,
            },
            initializer: crate::WasmReferenceInitializer::Function(0).into(),
        }
    }

    /// An element segment writing `functions` into table 0 from index 0.
    fn active_function_element(functions: &[u32]) -> crate::WasmElement {
        crate::WasmElement {
            mode: crate::WasmElementMode::Active {
                table: 0,
                offset: crate::WasmValue::I32(0).into(),
            },
            element_type: wasmparser::RefType::FUNCREF,
            items: functions.iter().map(|&function| crate::WasmReferenceInitializer::Function(function)).collect::<Vec<_>>().into(),
        }
    }

    /// The signature of a function with no parameters and one `i32` result.
    fn i32_result_signature() -> crate::WasmSignature {
        crate::WasmSignature {
            params: vec![],
            results: vec![crate::WasmType::I32],
        }
    }

    /// A VM in compiled mode with `module` instantiated.
    fn compiled_instance(module: &crate::WasmModule) -> (Vm, crate::WasmInstance) {
        let mut vm = Vm::new(crate::SystemHost);
        vm.programs.compile_wasm_modules_at_first_entry();
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(module).unwrap();
        (vm, instance)
    }

    #[test]
    fn compiled_wasm_indirect_call_site_calls_each_of_its_targets_through_its_row() {
        // One `call_indirect` site alternates between two table entries: each is called
        // through its own row, whatever the site called before, and the VM enters neither.
        let returns = |value: i32| embedded_body(i32_result_signature(), vec![Operator::I32Const { value }, Operator::End]);
        let caller_type = crate::WasmSignature {
            params: vec![crate::WasmType::I32],
            results: vec![crate::WasmType::I32],
        };
        let caller = embedded_body(
            caller_type,
            vec![Operator::LocalGet { local_index: 0 }, Operator::CallIndirect { type_index: 0, table_index: 0 }, Operator::End],
        );
        let module = crate::Engine::lower_wasm_module_with_elements(
            "compiled-indirect-call-polymorphic-site",
            [returns(21), returns(4), caller],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new([], [wasmparser::ValType::I32])]),
            &[],
            &[],
            &[],
            &[funcref_table(2)],
            &[active_function_element(&[0, 1])],
        )
        .unwrap();
        let (mut vm, instance) = compiled_instance(&module);
        for _ in 0..3 {
            for (index, expected) in [(0, 21), (1, 4)] {
                assert_eq!(vm.invoke_wasm(&instance, 2, &[crate::WasmValue::I32(index)]).unwrap(), Some(crate::WasmValue::I32(expected)));
            }
        }
        let compiled = vm
            .programs
            .find_shared(&module.program)
            .and_then(|program| vm.programs.compiled(program).flatten())
            .expect("compiled program");
        assert_eq!(compiled.native_entries(0), Some(0));
        assert_eq!(compiled.native_entries(1), Some(0));
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
                    ty: crate::WasmCallableType::Embedded(signature),
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
            ;
        // A target the artifact compiled is called through its row, so no call reaches the VM's
        // site profile, which only a target to promote needs.
        assert!(
            compiled
                .profiled_indirect_targets(&vm, program, env, &indirect_callers)
                .is_empty()
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
        assert_eq!(compiled.same_artifact_direct_reference_hits(), Some(0));
        assert!(compiled.reference_call_site_counts().unwrap().iter().any(
            |(function, _, total, native, fallback)| {
                *function == 2 && *total == 0 && *native == 0 && *fallback == 0
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
        assert!(state.borrow().guard().is_none());

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
        vm.programs.compile_wasm_modules_at_first_entry();
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
        vm.programs.compile_wasm_modules_at_first_entry();
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
        root_vm.programs.compile_wasm_modules_at_first_entry();
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
                        maximum,
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
            let stored = vm.decode_wasm_value(Value::integer(23), crate::WasmType::I32).unwrap();
            crate::staged::wasm_memory_bytes::write_memory(
                &mut vm.wasm_memory_mut(memory).unwrap(),
                crate::wasm::memory::MemoryStore::I32Store,
                7,
                stored,
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
                    ty: crate::WasmCallableType::Embedded(signature),
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
        // Its registers stay, so the function's liveness claims still hold.
        *reference = crate::bytecode::Instr::new(crate::bytecode::Op::WasmRefFunc, reference.a(), reference.b(), reference.c(), 9);
        // Residual validation knows the operand layout; pool attachment owns source indices.
        assert_eq!(corrupt.validate(), Ok(()));
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
            .alloc_object(Vm::empty_object(
                Value::NULL,
            ));
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
            .alloc_object(Vm::empty_object(
                Value::NULL,
            ));
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
                invalid.function(0).widened(*packed)
            })
            .find(|instruction| instruction.op().is_wasm_memory_address())
            .unwrap();
        invalid.constants[address.constant_index()] = crate::bytecode::Constant::Number(0.0);
        assert!(invalid.validate().is_err());
        let mut vm = Vm::new(crate::SystemHost);
        let first = vm.instantiate_wasm_module(&module).unwrap();
        let second = vm.instantiate_wasm_module(&module).unwrap();
        let memory = |vm: &Vm, instance: &crate::WasmInstance| {
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
        let environment = |vm: &Vm, instance: &crate::WasmInstance| {
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
            // Its test runs compiled Wasm only (`Host::runs_lane`).
            fn runs_lane(&self) -> bool { false }
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
                vm.programs.compile_wasm_modules_at_first_entry();
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
            .map(|packed| root_code.widened(*packed))
            .collect::<Vec<_>>();
        assert!(
            root_instructions.iter().any(|instruction| {
                instruction.op().is_call_known() && instruction.known_function_index() == 1
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
        assert!(matches!(root_first_vm.programs.wasm_tier_state(program, 1), Some(super::super::program_store::WasmTierState::Attempted | super::super::program_store::WasmTierState::Compiled(_))));
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

    // Loop work is credited at lane safepoints; profiled builds run no lane.
    #[cfg(not(quench_profiled))]
    #[test]
    fn compiled_wasm_selective_tiering_promotes_a_function_by_its_loop_work() {
        // One entry runs this many iterations: below the call threshold in entries, far
        // above it in backward jumps (`TIERS.wasm.backedges_per_entry`).
        const ITERATIONS: i32 = 40_000;
        let root = vec![
            Operator::I32Const { value: ITERATIONS },
            Operator::LocalSet { local_index: 0 },
            Operator::I32Const { value: 0 },
            Operator::LocalSet { local_index: 1 },
            Operator::Loop {
                blockty: wasmparser::BlockType::Empty,
            },
            Operator::LocalGet { local_index: 1 },
            Operator::LocalGet { local_index: 0 },
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
            "compiled-wasm-selective-backedge-promotion",
            0,
            [(0, 2, true, root)]
                .into_iter()
                .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap();
        let expected = Some((1..=ITERATIONS).fold(0i32, |sum, value| sum.wrapping_add(value)));
        let function = module.select_function(0).unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.programs.keep_wasm_modules_selective();
        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), expected);
        let program = vm
            .programs
            .find_shared(&function.module.program)
            .expect("registered Wasm program");
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        // Selective mode: no eager artifact, as in `..._promotes_a_hot_fast_call_known`.
        vm.programs.set_compiled(program, None);

        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), expected);
        assert!(vm.programs.wasm_tier(program, 0).is_none(), "an entry never compiles mid-loop");
        assert!(matches!(vm.programs.wasm_tier_state(program, 0), Some(super::super::program_store::WasmTierState::Counting(n)) if n == crate::vm::tier_policy::TierRows::PROCESS.table().wasm.promotion_entries - 1),
            "the loop's work stops one short of the threshold");

        assert_eq!(vm.execute_wasm_i32(&function, &[]).unwrap(), expected);
        let tier = vm
            .programs
            .wasm_tier(program, 0)
            .expect("the next entry promoted the looping function");
        assert_eq!(tier.native_entries(0), Some(1));
    }

    /// Builds a list of `nodes` words from `LIST_HEAD`, each holding the next one's address
    /// and the last zero, then runs `walk` over it (locals: 0 the node count, 1 the address,
    /// 2 and 3 the walk's) and returns local 2: run in a selective
    /// module, whose lane run raises the floor (`Vm::credits_backedges_in_lane`), so every
    /// backward jump takes the safepoint.
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
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        // Selective mode: no eager artifact.
        vm.programs.set_compiled(program, None);
        vm.invoke_wasm(&instance, 0, &[crate::WasmValue::I32(nodes)]).unwrap()
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

    // Loop work is credited at lane safepoints; profiled builds run no lane.
    #[cfg(not(quench_profiled))]
    #[test]
    fn compiled_wasm_selective_module_compiles_whole_after_its_interpreted_work() {
        // The work at which this test's module compiles whole (never by time).
        const WORK: u32 = 64;
        // One entry loops through that work, plus one entry's worth.
        let iterations = i32::try_from((WORK + 1) * crate::vm::tier_policy::TierRows::PROCESS.table().wasm.backedges_per_entry).unwrap();
        let root = vec![
            Operator::LocalGet { local_index: 0 },
            Operator::LocalSet { local_index: 1 },
            Operator::Loop {
                blockty: wasmparser::BlockType::Empty,
            },
            Operator::LocalGet { local_index: 1 },
            Operator::I32Const { value: -1 },
            Operator::I32Add,
            Operator::LocalTee { local_index: 1 },
            Operator::BrIf { relative_depth: 0 },
            Operator::End,
            Operator::LocalGet { local_index: 0 },
            Operator::End,
        ];
        let module = crate::Engine::lower_wasm_i32_module(
            "compiled-wasm-selective-module-escalation",
            0,
            [(1, 1, true, root)]
                .into_iter()
                .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap();
        let function = module.select_function(0).unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.programs.compile_wasm_modules_after_work(WORK);
        assert_eq!(vm.execute_wasm_i32(&function, &[1]).unwrap(), Some(1));
        let program = vm
            .programs
            .find_shared(&function.module.program)
            .expect("registered Wasm program");
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        // Selective mode: no eager artifact.
        vm.programs.set_compiled(program, None);

        assert_eq!(vm.execute_wasm_i32(&function, &[iterations]).unwrap(), Some(iterations));
        assert!(
            matches!(vm.programs.compiled(program), Some(None)),
            "the module compiles between calls, never inside a loop"
        );

        assert_eq!(vm.execute_wasm_i32(&function, &[1]).unwrap(), Some(1));
        let whole = vm
            .programs
            .compiled(program)
            .flatten()
            .expect("the next entry compiled the whole module");
        assert_eq!(whole.native_entries(0), Some(1));
        assert!(vm.programs.wasm_tier(program, 0).is_none(), "no selective artifact remains");
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
            .map(|packed| root_code.widened(*packed))
            .collect::<Vec<_>>();
        assert!(
            root_instructions
                .iter()
                .any(|instruction| instruction.op().is_call_known()
                    && instruction.known_function_index() == 1),
            "root instructions: {root_instructions:?}"
        );
        let function = module.select_function(0).unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.programs.keep_wasm_modules_selective();
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
        assert!(matches!(vm.programs.wasm_tier_state(program, 1), Some(super::super::program_store::WasmTierState::Attempted | super::super::program_store::WasmTierState::Compiled(_))));
        let tier = vm
            .programs
            .wasm_tier(program, 1)
            .expect("hot `CallKnown` target was promoted");
        // The threshold call enters native code, and the remaining calls in
        // the same loop are routed back through the general native path.
        assert_eq!(tier.native_entries(1), Some(9));
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
        vm.programs.compile_wasm_modules_at_first_entry();
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
        for _ in 0..usize::from(crate::vm::tier_policy::TierRows::PROCESS.table().wasm.promotion_entries) {
            assert_eq!(
                vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
                Some(crate::WasmTrap::Unreachable)
            );
        }
        assert_eq!(
            compiled.native_entries(0),
            Some(1 + u64::from(crate::vm::tier_policy::TierRows::PROCESS.table().wasm.promotion_entries))
        );
        assert!(vm.programs.wasm_tier(program, 0).is_none());

        let mut root_vm = Vm::new(crate::SystemHost);

        root_vm.programs.compile_wasm_modules_at_first_entry();
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


}
