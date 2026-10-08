use super::*;

pub(super) fn table_range(
    start: u64,
    length: u64,
    available: usize,
) -> Result<std::ops::Range<usize>, JsError> {
    crate::wasm::memory::checked_range(
        start,
        usize::try_from(length)
            .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsTable))?,
        available,
    )
    .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsTable))
}

impl<H: Host> Vm<H> {
    pub(super) fn wasm_reference_valid(&self, value: Value, ty: wasmparser::RefType) -> bool {
        if value.is_null() {
            return ty.is_nullable();
        }
        // Externrefs can carry any live host value, including an opaque GC object.
        if ty.heap_type() == wasmparser::HeapType::EXTERN {
            return !value.is_heap() || self.heap.get(value).is_some();
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

    pub(super) fn wasm_index_operand(
        &self,
        value: Value,
        ty: crate::WasmType,
    ) -> Result<u64, JsError> {
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

    pub(super) fn wasm_indirect_target(
        &self,
        caller: ProgramId,
        table: Value,
        index: u64,
        expected_index: u32,
    ) -> Result<Value, JsError> {
        let elements = self.wasm_table_elements(table)?;
        let reference = usize::try_from(index)
            .ok()
            .and_then(|index| elements.get(index))
            .copied()
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::UndefinedElement))?;
        if reference.is_null() {
            return Err(JsError::wasm_trap_error(
                crate::WasmTrap::UninitializedElement,
            ));
        }
        let expected = self
            .programs
            .wasm_signatures(caller)
            .filter(|pool| pool.type_signature(expected_index).is_some())
            .ok_or_else(|| JsError::validation("invalid Wasm signature index".into()))?;
        let matches = self
            .wasm_callable_matches(reference, expected, expected_index, false)
            .ok_or_else(|| JsError::validation("invalid Wasm function signature".into()))?;
        if !matches {
            return Err(JsError::wasm_trap_error(
                crate::WasmTrap::IndirectCallTypeMismatch,
            ));
        }
        Ok(reference)
    }

    pub(super) fn wasm_table_get(&self, table: Value, index: u64) -> Result<Value, JsError> {
        self.wasm_table_elements(table)?
            .get(
                usize::try_from(index)
                    .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsTable))?,
            )
            .copied()
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsTable))
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
        let Some(Cell::WasmTable { elements, .. }) = self.heap.get_mut(table) else {
            unreachable!()
        };
        let range = table_range(index, length, elements.len())?;
        elements[range].fill(value);
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
        let Some(Cell::WasmTable {
            elements,
            maximum,
            table64,
            ..
        }) = self.heap.get_mut(table)
        else {
            unreachable!()
        };
        let old_size = elements.len();
        let limit = maximum.unwrap_or(crate::wasm::table::table_index_limit(*table64));
        let Some(length) = usize::try_from(delta)
            .ok()
            .and_then(|delta| old_size.checked_add(delta))
            .filter(|length| (*length as u64) <= limit)
        else {
            return Ok(None);
        };
        let before = elements.capacity() * size_of::<Value>();
        if elements.try_reserve_exact(length - old_size).is_err() {
            return Ok(None);
        }
        elements.resize(length, initial);
        let after = elements.capacity() * size_of::<Value>();
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
        let Some(Cell::WasmTable { elements, .. }) = self.heap.get_mut(table) else {
            unreachable!()
        };
        elements[output].copy_from_slice(&values);
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
            let values = self.wasm_table_elements(source)?[input].to_vec();
            let Some(Cell::WasmTable { elements, .. }) = self.heap.get_mut(destination) else {
                unreachable!()
            };
            elements[output].copy_from_slice(&values);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, WasmFunctionBody, WasmGlobal, WasmSignature, WasmType, WasmValue};
    use wasmparser::Operator;

    #[test]
    fn table_import_root_transfer_keeps_foreign_capture_then_reclaims_owner_cycle() {
        let signature = WasmSignature {
            params: vec![],
            results: vec![WasmType::I32],
        };
        let exporter = Engine::lower_wasm_module_with_tables(
            "export-owner",
            [WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                operators: [Operator::GlobalGet { global_index: 0 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            }],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new(
                [],
                [wasmparser::ValType::I32],
            )]),
            &[WasmGlobal {
                initial: WasmValue::I32(17).into(),
                mutable: true,
            }],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::new(
                        true,
                        wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(0)),
                    )
                    .unwrap(),
                    table64: false,
                    initial: 1,
                    maximum: Some(3),
                    shared: false,
                },
                initializer: crate::WasmReferenceInitializer::Function(0).into(),
            }],
        )
        .unwrap();
        let importer = Engine::lower_wasm_module_with_tables(
            "import-owner",
            [WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(signature),
                locals: vec![],
                operators: [
                    Operator::I32Const { value: 0 },
                    Operator::CallIndirect {
                        type_index: 1,
                        table_index: 0,
                    },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            }],
            &crate::WasmTypes::from_functions([
                wasmparser::FuncType::new([], []),
                wasmparser::FuncType::new([], [wasmparser::ValType::I32]),
            ]),
            &[],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::new(
                        true,
                        wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(1)),
                    )
                    .unwrap(),
                    table64: false,
                    initial: 1,
                    maximum: Some(3),
                    shared: false,
                },
                initializer: crate::WasmTableInitializer::Import(crate::WasmImportName {
                    index: 0,
                    module: "owner".into(),
                    name: "table".into(),
                }),
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        let source = vm.instantiate_wasm_module(&exporter).unwrap();
        let environment = vm.root_value(source.environment).unwrap();
        let table = vm.wasm_table(&source, 0).unwrap();
        let function = vm.wasm_table_get(table, 0).unwrap();
        let handle = vm.root(table);
        let first = vm
            .instantiate_wasm_module_with_imports(&importer, &[handle])
            .unwrap();
        let second_handle = vm.root(vm.wasm_table(&first, 0).unwrap());
        let second = vm
            .instantiate_wasm_module_with_imports(&importer, &[second_handle])
            .unwrap();
        assert_eq!(vm.wasm_table(&first, 0).unwrap(), table);
        assert_eq!(vm.wasm_table(&second, 0).unwrap(), table);
        vm.release_root(source.environment);
        vm.release_root(first.environment);
        vm.release_root(handle);
        vm.release_root(second_handle);
        vm.collect_now(importer.residual());
        assert!(vm.heap.get(environment).is_some());
        assert_eq!(
            vm.invoke_wasm(&second, 0, &[]).unwrap(),
            Some(WasmValue::I32(17))
        );
        let binding = vm.heap.environment_slot(environment, 0).unwrap();
        vm.wasm_global_store(binding, Value::integer(23)).unwrap();
        assert_eq!(
            vm.invoke_wasm(&second, 0, &[]).unwrap(),
            Some(WasmValue::I32(23))
        );
        let roots = vm.heap.root_count_for_test();
        let error = vm
            .instantiate_wasm_module_with_imports(&importer, &[handle])
            .err()
            .unwrap();
        assert!(error.wasm_link_error().is_some());
        assert!(error.wasm_trap().is_none());
        assert_eq!(vm.heap.root_count_for_test(), roots);
        vm.release_root(second.environment);
        vm.collect_now(importer.residual());
        for value in [environment, table, function] {
            assert!(vm.heap.get(value).is_none());
        }
    }

    #[test]
    fn table_function_identity_and_cross_instance_edges_survive_collection_then_release() {
        let module = Engine::lower_wasm_module_with_tables(
            "table-owner",
            [
                WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(WasmSignature {
                        params: vec![],
                        results: vec![WasmType::FUNCREF],
                    }),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::RefFunc { function_index: 1 },
                        Operator::TableSet { table: 0 },
                        Operator::I32Const { value: 0 },
                        Operator::TableGet { table: 0 },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(WasmSignature {
                        params: vec![],
                        results: vec![WasmType::I32],
                    }),
                    locals: vec![],
                    operators: vec![Operator::GlobalGet { global_index: 0 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
            ],
            &crate::WasmTypes::default(),
            &[WasmGlobal {
                initial: WasmValue::I32(17).into(),
                mutable: true,
            }],
            &[],
            &[],
            &[wasmparser::TableType {
                element_type: wasmparser::RefType::FUNCREF,
                table64: false,
                initial: 1,
                maximum: Some(3),
                shared: false,
            }
            .into()],
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "quench-wasm-table-owner-{}.qvm",
            std::process::id()
        ));
        module.residual().write_binary(&path).unwrap();
        let program = crate::ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        program.validate().unwrap();
        let module = crate::WasmModule {
            program: Rc::new(program),
            ..module
        };
        let mut vm = Vm::new(crate::SystemHost);
        let first = vm.instantiate_wasm_module(&module).unwrap();
        let second = vm.instantiate_wasm_module(&module).unwrap();
        let WasmValue::FuncRef(function) = vm.invoke_wasm(&first, 0, &[]).unwrap().unwrap() else {
            panic!()
        };
        assert_eq!(
            vm.invoke_wasm(&first, 0, &[]).unwrap(),
            Some(WasmValue::FuncRef(function))
        );
        assert_ne!(
            vm.invoke_wasm(&second, 0, &[]).unwrap(),
            Some(WasmValue::FuncRef(function))
        );
        let first_env = vm.root_value(first.environment).unwrap();
        let second_env = vm.root_value(second.environment).unwrap();
        let table = vm.heap.environment_slot(second_env, 1).unwrap();
        vm.wasm_table_fill(table, 0, function, 1).unwrap();
        assert!(vm.release_root(first.environment));
        vm.collect_now(module.residual());
        assert!(vm.heap.get(first_env).is_some());
        assert!(
            matches!(vm.heap.get(function), Some(Cell::Function { env, .. }) if *env == first_env)
        );
        assert_eq!(vm.wasm_table_get(table, 0).unwrap(), function);
        assert!(vm.release_root(second.environment));
        vm.collect_now(module.residual());
        assert!(vm.heap.get(first_env).is_none());
        assert!(vm.heap.get(second_env).is_none());
        assert!(vm.heap.get(function).is_none());
        assert!(vm.heap.get(table).is_none());
    }
}

#[cfg(test)]
mod reference_boundary_tests {
    use super::*;
    use crate::{Engine, WasmFunctionBody, WasmSignature, WasmType, WasmValue};
    use wasmparser::Operator;

    #[test]
    fn external_heap_reference_is_owned_by_table_and_function_boundary_checks_reference_kind() {
        let module = Engine::lower_wasm_module_with_tables(
            "external-table-owner",
            [WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![WasmType::EXTERNREF],
                    results: vec![WasmType::EXTERNREF],
                }),
                locals: vec![],
                operators: [
                    Operator::I32Const { value: 0 },
                    Operator::LocalGet { local_index: 0 },
                    Operator::TableSet { table: 0 },
                    Operator::I32Const { value: 0 },
                    Operator::TableGet { table: 0 },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            }
            .into()],
            &crate::WasmTypes::default(),
            &[],
            &[],
            &[],
            &[wasmparser::TableType {
                element_type: wasmparser::RefType::EXTERNREF,
                table64: false,
                initial: 1,
                maximum: Some(3),
                shared: false,
            }
            .into()],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        let host_value = vm
            .heap
            .alloc(Cell::String("host-owned external identity".into()));
        let reference = WasmValue::ExternRef(host_value);
        assert_eq!(
            vm.invoke_wasm(&instance, 0, &[reference]).unwrap(),
            Some(reference)
        );
        assert!(vm.decode_wasm_value(host_value, WasmType::FUNCREF).is_err());
        assert!(
            vm.invoke_wasm(&instance, 0, &[WasmValue::FuncRef(host_value)])
                .is_err()
        );
        vm.collect_now(module.residual());
        assert!(matches!(vm.heap.get(host_value), Some(Cell::String(_))));
        let env = vm.root_value(instance.environment).unwrap();
        let table = vm.heap.environment_slot(env, 0).unwrap();
        assert_eq!(vm.wasm_table_get(table, 0).unwrap(), host_value);
        // An imported initializer must transfer its reference into owned table
        // and segment cells, even after the import and instance roots disappear.
        type Empty = std::iter::Empty<Result<Operator<'static>, wasmparser::BinaryReaderError>>;
        let globals = [crate::WasmGlobal {
            mutable: false,
            initial: crate::WasmGlobalInitializer::Import {
                ty: WasmType::EXTERNREF,
                name: crate::WasmImportName {
                    index: 0,
                    module: "host".into(),
                    name: "reference".into(),
                },
            },
        }];
        let initializer = crate::WasmReferenceInitializer::Expression(
            Engine::lower_wasm_constant_expression(
                "imported-reference-owner",
                WasmType::EXTERNREF,
                &globals,
                [Operator::GlobalGet { global_index: 0 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            )
            .unwrap(),
        );
        let imported = Engine::lower_wasm_module_with_elements::<Empty>(
            "imported-reference-owner",
            [],
            &crate::WasmTypes::default(),
            &globals,
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: wasmparser::RefType::EXTERNREF,
                    table64: false,
                    initial: 2,
                    maximum: None,
                    shared: false,
                },
                initializer: initializer.clone().into(),
            }],
            &[crate::WasmElement {
                mode: crate::WasmElementMode::Active {
                    table: 0,
                    offset: 1.into(),
                },
                element_type: wasmparser::RefType::EXTERNREF,
                items: vec![initializer].into(),
            }],
        )
        .unwrap();
        let binding = vm.heap.alloc(Cell::WasmGlobal {
            value: host_value,
            ty: WasmType::EXTERNREF,
            declarations: crate::WasmTypes::default(),
            mutable: false,
        });
        let binding_root = vm.root(binding);
        let imported_instance = vm
            .instantiate_wasm_module_with_imports(&imported, &[binding_root])
            .unwrap();
        let imported_table = vm.wasm_table(&imported_instance, 0).unwrap();
        let table_root = vm.root(imported_table);
        assert_eq!(vm.wasm_table_get(imported_table, 0).unwrap(), host_value);
        assert_eq!(vm.wasm_table_get(imported_table, 1).unwrap(), host_value);
        assert!(vm.release_root(binding_root));
        assert!(vm.release_root(imported_instance.environment));
        assert!(vm.release_root(instance.environment));
        vm.collect_now(module.residual());
        assert!(vm.heap.get(table).is_none());
        assert!(vm.heap.get(binding).is_none());
        assert!(vm.heap.get(host_value).is_some());
        assert!(vm.release_root(table_root));
        vm.collect_now(module.residual());
        assert!(vm.heap.get(imported_table).is_none());
        assert!(vm.heap.get(host_value).is_none());
    }
}

#[cfg(test)]
mod element_owner_tests {
    use super::*;
    use crate::{
        Engine, WasmElement, WasmElementMode, WasmFunctionBody, WasmGlobal,
        WasmReferenceInitializer, WasmSignature, WasmType, WasmValue,
    };
    use wasmparser::{Operator, RefType};

    #[test]
    fn serialized_elements_preserve_foreign_capture_ownership_and_reject_unregistered_type_facts() {
        let signature = WasmSignature {
            params: vec![],
            results: vec![WasmType::I32],
        };
        let module = Engine::lower_wasm_module_with_elements(
            "element-owner",
            [
                WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![Operator::GlobalGet { global_index: 0 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
                WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(WasmSignature {
                        params: vec![WasmType::I64],
                        results: vec![WasmType::I32],
                    }),
                    locals: vec![],
                    operators: vec![
                        Operator::LocalGet { local_index: 0 },
                        Operator::TableGet { table: 0 },
                        Operator::RefCastNonNull {
                            hty: wasmparser::HeapType::Exact(wasmparser::UnpackedIndex::Module(0)),
                        },
                        Operator::CallRef { type_index: 0 },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(WasmSignature {
                        params: vec![],
                        results: vec![],
                    }),
                    locals: vec![],
                    operators: vec![Operator::ElemDrop { elem_index: 1 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
            ],
            &crate::WasmTypes::from_functions([wasmparser::FuncType::new(
                [],
                [wasmparser::ValType::I32],
            )]),
            &[WasmGlobal {
                initial: WasmValue::I32(17).into(),
                mutable: true,
            }],
            &[],
            &[],
            &[crate::WasmTable {
                ty: wasmparser::TableType {
                    element_type: RefType::FUNCREF,
                    table64: true,
                    initial: 2,
                    maximum: Some(3),
                    shared: false,
                },
                initializer: WasmReferenceInitializer::Function(0).into(),
            }],
            &[
                WasmElement {
                    mode: WasmElementMode::Active {
                        table: 0,
                        offset: WasmValue::I64(0).into(),
                    },
                    element_type: RefType::FUNCREF,
                    items: vec![WasmReferenceInitializer::Function(0)].into(),
                },
                WasmElement {
                    mode: WasmElementMode::Passive,
                    element_type: RefType::FUNCREF,
                    items: vec![WasmReferenceInitializer::Function(0)].into(),
                },
                WasmElement {
                    mode: WasmElementMode::Declared,
                    element_type: RefType::FUNCREF,
                    items: vec![WasmReferenceInitializer::Function(0)].into(),
                },
            ],
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "quench-wasm-elements-owner-{}.qvm",
            std::process::id()
        ));
        module.residual().write_binary(&path).unwrap();
        let program = ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let module = crate::WasmModule {
            program: Rc::new(program),
            ..module
        };
        let mut vm = Vm::new(crate::SystemHost);
        let first = vm.instantiate_wasm_module(&module).unwrap();
        let second = vm.instantiate_wasm_module(&module).unwrap();
        let first_env = vm.root_value(first.environment).unwrap();
        let second_env = vm.root_value(second.environment).unwrap();
        let first_table = vm.heap.environment_slot(first_env, 1).unwrap();
        let second_table = vm.heap.environment_slot(second_env, 1).unwrap();
        let passive = vm.heap.environment_slot(first_env, 3).unwrap();
        assert_eq!(
            vm.heap.environment_slot(first_env, 2),
            Some(Value::UNDEFINED)
        );
        assert_eq!(
            vm.heap.environment_slot(first_env, 4),
            Some(Value::UNDEFINED)
        );
        let function = vm.wasm_table_get(first_table, 0).unwrap();
        assert_eq!(vm.wasm_table_get(first_table, 1).unwrap(), function);
        assert!(
            matches!(vm.heap.get(passive), Some(Cell::WasmElements(values)) if values.as_slice() == [function])
        );
        vm.wasm_table_init(second_table, passive, 1, 0, 1).unwrap();
        let binding = vm.heap.environment_slot(first_env, 0).unwrap();
        vm.wasm_global_store(binding, Value::integer(23)).unwrap();
        assert_eq!(
            vm.invoke_wasm(&second, 1, &[WasmValue::I64(0)]).unwrap(),
            Some(WasmValue::I32(17))
        );
        assert_eq!(
            vm.invoke_wasm(&second, 1, &[WasmValue::I64(1)]).unwrap(),
            Some(WasmValue::I32(23))
        );
        assert!(vm.release_root(first.environment));
        vm.collect_now(module.residual());
        assert!(vm.heap.get(first_env).is_some());
        assert_eq!(
            vm.invoke_wasm(&second, 1, &[WasmValue::I64(1)]).unwrap(),
            Some(WasmValue::I32(23))
        );
        vm.invoke_wasm(&second, 2, &[]).unwrap();
        assert_eq!(
            vm.heap.environment_slot(second_env, 3),
            Some(Value::UNDEFINED)
        );
        vm.wasm_table_fill(second_table, 1, Value::NULL, 1).unwrap();
        vm.collect_now(module.residual());
        assert!(vm.heap.get(first_env).is_none());
        assert!(vm.heap.get(passive).is_none());
        assert!(vm.heap.get(function).is_none());
        // A different program interns these types in a different order. A
        // foreign closure keeps its own environment, and types compare by shape.
        let foreign = Engine::lower_wasm_module_definition(
            "foreign-type-order",
            [
                WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(WasmSignature {
                        params: vec![],
                        results: vec![WasmType::F64],
                    }),
                    locals: vec![],
                    operators: vec![
                        Operator::F64Const {
                            value: wasmparser::Ieee64::from(0.0),
                        },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(WasmSignature {
                        params: vec![],
                        results: vec![WasmType::I32],
                    }),
                    locals: vec![],
                    operators: vec![Operator::I32Const { value: 44 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
                WasmFunctionBody {
                    ty: crate::WasmCallableType::Embedded(WasmSignature {
                        params: vec![],
                        results: vec![WasmType::FUNCREF],
                    }),
                    locals: vec![],
                    operators: vec![Operator::RefFunc { function_index: 1 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
            ],
            &crate::WasmTypes::default(),
            &[],
        )
        .unwrap();
        let foreign_instance = vm.instantiate_wasm_module(&foreign).unwrap();
        let foreign_env = vm.root_value(foreign_instance.environment).unwrap();
        let WasmValue::FuncRef(foreign_function) =
            vm.invoke_wasm(&foreign_instance, 2, &[]).unwrap().unwrap()
        else {
            panic!()
        };
        vm.wasm_table_fill(second_table, 1, foreign_function, 1)
            .unwrap();
        assert_eq!(
            vm.invoke_wasm(&second, 1, &[WasmValue::I64(1)]).unwrap(),
            Some(WasmValue::I32(44))
        );
        assert!(vm.release_root(foreign_instance.environment));
        vm.collect_now(module.residual());
        assert!(vm.heap.get(foreign_env).is_some());
        vm.wasm_table_fill(second_table, 1, Value::NULL, 1).unwrap();
        vm.collect_now(module.residual());
        assert!(vm.heap.get(foreign_env).is_none());
        assert!(vm.heap.get(foreign_function).is_none());
        let mut invalid = (*module.program).clone();
        let function = &mut invalid.functions[1];
        let target = function
            .code
            .iter()
            .position(|instruction| instruction.op() == Op::WasmRefCast)
            .unwrap();
        let original = function.code[target].as_wide();
        function.code[target] = Instr::wide(function.wide.len()).unwrap();
        function.wide.push(crate::bytecode::WideInstruction::new(
            Op::WasmRefCast,
            original.result_register(),
            original.register_b(),
            0,
            crate::wasm::reference::ReferenceTarget::from_type(
                wasmparser::RefType::new(
                    false,
                    wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(
                        module
                            .signatures
                            .declarations
                            .groups()
                            .iter()
                            .map(Vec::len)
                            .sum::<usize>() as u32,
                    )),
                )
                .unwrap(),
            )
            .unwrap()
            .tag(),
        ));
        let invalid = crate::WasmModule {
            program: Rc::new(invalid),
            ..module.clone()
        };
        assert!(vm.instantiate_wasm_module(&invalid).is_err());
        for op in [Op::WasmDescriptorTest, Op::WasmDescriptorCast] {
            let mut invalid = (*module.program).clone();
            let function = &mut invalid.functions[1];
            function.code[target] = Instr::wide(function.wide.len()).unwrap();
            function.wide.push(crate::bytecode::WideInstruction::new(
                op,
                original.result_register(),
                original.register_b(),
                original.register_b(),
                crate::wasm::reference::ReferenceTarget::from_type(wasmparser::RefType::STRUCTREF)
                    .unwrap()
                    .tag(),
            ));
            // Encoding and operands are valid; module admission must still reject
            // descriptor operations without a declared descriptor relationship.
            assert!(invalid.validate().is_ok());
            let invalid = crate::WasmModule {
                program: Rc::new(invalid),
                ..module.clone()
            };
            assert!(vm.instantiate_wasm_module(&invalid).is_err());
        }
        assert!(vm.release_root(second.environment));
        vm.collect_now(module.residual());
        assert!(vm.heap.get(second_env).is_none());
    }
}
