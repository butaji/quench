//! Typed embedding calls through ordinary native functions and VM root leases.

use super::*;
use crate::{WasmHostFunctionId, WasmHostValue, WasmSignature, WasmValue};

impl<H: Host> Vm<H> {
    pub(crate) fn create_wasm_host_function(
        &mut self,
        name: &str,
        id: WasmHostFunctionId,
        signature: WasmSignature,
    ) -> Result<Value, JsError> {
        if signature
            .params
            .iter()
            .chain(&signature.results)
            .any(|ty| ty.requires_declarations())
        {
            return Err(JsError::validation(
                "embedded Wasm signature requires a declaration owner".into(),
            ));
        }
        if self.programs.len() == 0 {
            type Empty = std::iter::Empty<
                Result<wasmparser::Operator<'static>, wasmparser::BinaryReaderError>,
            >;
            let bootstrap = crate::Engine::lower_wasm_module_definition::<Empty>(
                "wasm-host-bootstrap",
                [],
                &crate::WasmTypes::default(),
                &[],
            )
            .map_err(|error| JsError::validation(error.to_string()))?;
            self.initialize_shared(&bootstrap.program)?;
        }
        let length = signature.params.len();
        let environment = self.heap.alloc(Cell::WasmHostFunction {
            id,
            signature: Rc::new(signature),
        });
        let function = self.native_with_env(Native::WasmHost, environment);
        self.set_builtin_function_name(function, name)?;
        let atom = self.intern_atom("length");
        self.set_property(function, atom, Value::number(length as f64))?;
        self.set_property_attributes(
            function,
            PropertyKey::string(atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(function)
    }

    pub(crate) fn invoke_wasm_host_function(
        &mut self,
        root: crate::RootId,
        args: &[WasmValue],
    ) -> Result<Vec<WasmValue>, JsError> {
        let function = self
            .root_value(root)
            .ok_or_else(|| JsError::wasm_link("invalid Wasm host function root"))?;
        let Some(Cell::Function {
            kind: FunctionKind::Native(Native::WasmHost),
            env,
            ..
        }) = self.heap.get(function)
        else {
            return Err(JsError::wasm_link("not a typed Wasm host function"));
        };
        let Some(Cell::WasmHostFunction { signature, .. }) = self.heap.get(*env) else {
            return Err(JsError::validation("invalid Wasm host binding".into()));
        };
        let signature = signature.clone();
        if args.len() != signature.params.len() {
            return Err(JsError::validation("Wasm argument count mismatch".into()));
        }
        let mut arguments = Vec::with_capacity(args.len());
        for (&value, &ty) in args.iter().zip(&signature.params) {
            if !value.fits_type(ty) {
                return Err(JsError::validation("Wasm argument type mismatch".into()));
            }
            let value = self.encode_wasm_value(value);
            self.decode_wasm_value(value, ty)?;
            arguments.push(value);
        }
        let program = self
            .programs
            .get(self.active_program)
            .ok_or_else(|| JsError::validation("missing host caller program".into()))?;
        let result = self.call_value(&program, function, Value::UNDEFINED, &arguments)?;
        self.decode_wasm_result_types(&signature.results, result, None)
    }

    pub(super) fn wasm_callable_signature(&self, function: Value) -> Option<&WasmSignature> {
        match self.heap.get(function)? {
            Cell::Function {
                kind: FunctionKind::User(program, index),
                ..
            } => self.programs.wasm_function_signature(*program, *index),
            Cell::Function {
                kind: FunctionKind::Native(Native::WasmHost),
                env,
                ..
            } => {
                let Cell::WasmHostFunction { signature, .. } = self.heap.get(*env)? else {
                    return None;
                };
                Some(signature)
            }
            _ => None,
        }
    }

    pub(super) fn wasm_callable_matches(
        &self,
        function: Value,
        expected: &crate::wasm::WasmSignatures,
        expected_index: u32,
        exact: bool,
    ) -> Option<bool> {
        match self.heap.get(function)? {
            Cell::Function {
                kind: FunctionKind::User(program, index),
                ..
            } => {
                let actual = self.programs.wasm_signatures(*program)?;
                Some(expected.accepts(
                    expected_index,
                    actual,
                    actual.defined_type_index(*index)?,
                    exact,
                ))
            }
            Cell::Function {
                kind: FunctionKind::Native(Native::WasmHost),
                ..
            } => Some(
                expected.accepts_embedded(expected_index, self.wasm_callable_signature(function)?),
            ),
            _ => None,
        }
    }

    pub(super) fn call_wasm_host(&mut self, args: &[Value]) -> Result<Value, JsError> {
        let environment = self
            .active_native_env()
            .ok_or_else(|| JsError::validation("missing Wasm host binding".into()))?;
        let Some(Cell::WasmHostFunction { id, signature }) = self.heap.get(environment) else {
            return Err(JsError::validation("invalid Wasm host binding".into()));
        };
        let id = *id;
        let signature = signature.clone();
        if args.len() != signature.params.len() {
            return Err(JsError::validation(
                "Wasm host argument count mismatch".into(),
            ));
        }
        let mut roots = Vec::new();
        let result = (|| {
            let mut arguments = Vec::with_capacity(args.len());
            for (&value, &ty) in args.iter().zip(&signature.params) {
                let value = self.decode_wasm_value(value, ty)?;
                arguments.push(match value {
                    WasmValue::I32(value) => WasmHostValue::I32(value),
                    WasmValue::I64(value) => WasmHostValue::I64(value),
                    WasmValue::F32(value) => WasmHostValue::F32(value),
                    WasmValue::F64(value) => WasmHostValue::F64(value),
                    WasmValue::V128(value) => WasmHostValue::V128(value),
                    WasmValue::FuncRef(value)
                    | WasmValue::ExternRef(value)
                    | WasmValue::GcRef(value) => {
                        let root = (!value.is_null()).then(|| self.root(value));
                        roots.extend(root);
                        if matches!(
                            ty,
                            crate::WasmType::Reference {
                                kind: crate::WasmReferenceKind::Internal(_)
                                    | crate::WasmReferenceKind::DeclaredGc { .. },
                                ..
                            }
                        ) {
                            WasmHostValue::GcRef(root)
                        } else if matches!(
                            ty,
                            crate::WasmType::Reference {
                                kind: crate::WasmReferenceKind::Function
                                    | crate::WasmReferenceKind::BottomFunction
                                    | crate::WasmReferenceKind::DeclaredFunction { .. },
                                ..
                            }
                        ) {
                            WasmHostValue::FuncRef(root)
                        } else {
                            WasmHostValue::ExternRef(root)
                        }
                    }
                });
            }
            let results = self
                .host
                .call_wasm(id, &arguments)
                .map_err(|error| JsError(error.into()))?;
            if results.len() != signature.results.len() {
                return Err(JsError::validation(
                    "Wasm host result count mismatch".into(),
                ));
            }
            let mut values = Vec::with_capacity(results.len());
            for (value, &ty) in results.into_iter().zip(&signature.results) {
                let value = match value {
                    WasmHostValue::I32(value) => WasmValue::I32(value),
                    WasmHostValue::I64(value) => WasmValue::I64(value),
                    WasmHostValue::F32(value) => WasmValue::F32(value),
                    WasmHostValue::F64(value) => WasmValue::F64(value),
                    WasmHostValue::V128(value) => WasmValue::V128(value),
                    WasmHostValue::FuncRef(root)
                    | WasmHostValue::ExternRef(root)
                    | WasmHostValue::GcRef(root) => {
                        let reference = match root {
                            Some(root) => self.root_value(root).ok_or_else(|| {
                                JsError::validation("invalid Wasm host result root".into())
                            })?,
                            None => Value::NULL,
                        };
                        if matches!(value, WasmHostValue::FuncRef(_)) {
                            WasmValue::FuncRef(reference)
                        } else if matches!(value, WasmHostValue::GcRef(_)) {
                            WasmValue::GcRef(reference)
                        } else {
                            WasmValue::ExternRef(reference)
                        }
                    }
                };
                if !value.fits_type(ty) {
                    return Err(JsError::validation("Wasm host result type mismatch".into()));
                }
                let value = self.encode_wasm_value(value);
                self.decode_wasm_value(value, ty)?;
                values.push(value);
            }
            Ok(match values.as_slice() {
                [] => Value::UNDEFINED,
                [value] => *value,
                _ => self.heap.alloc(Cell::Array {
                    object: Self::empty_object(Value::NULL),
                    elements: values.into(),
                }),
            })
        })();
        for root in roots {
            self.release_root(root);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, RootId, WasmFunctionBody, WasmFunctionImport, WasmImportName, WasmType};
    use wasmparser::Operator;

    #[derive(Default)]
    enum Reply {
        #[default]
        Echo,
        StaleReference,
        WrongType,
        NullReference,
        WrongArity,
        Error,
    }
    #[derive(Default)]
    struct EchoHost {
        reply: Reply,
        expired: Option<RootId>,
        expired_gc: Option<RootId>,
    }
    impl Host for EchoHost {
        fn write_line(&mut self, _: &str) {}
        fn clock_millis(&mut self) -> f64 {
            0.0
        }
        fn call_wasm(
            &mut self,
            _: WasmHostFunctionId,
            args: &[WasmHostValue],
        ) -> Result<Vec<WasmHostValue>, String> {
            let mut values = args.to_vec();
            match self.reply {
                Reply::Echo => {
                    let WasmHostValue::ExternRef(root) = args[0] else {
                        panic!()
                    };
                    self.expired = root;
                    let WasmHostValue::GcRef(root) = args[args.len() - 1] else {
                        panic!()
                    };
                    self.expired_gc = root;
                }
                Reply::StaleReference => values[0] = WasmHostValue::ExternRef(self.expired),
                Reply::WrongType => values[0] = WasmHostValue::FuncRef(None),
                Reply::NullReference => values[0] = WasmHostValue::ExternRef(None),
                Reply::WrongArity => {
                    values.pop();
                }
                Reply::Error => return Err("embedding rejected operation".into()),
            }
            Ok(values)
        }
    }

    #[test]
    fn typed_host_imports_lease_roots_restore_native_guards_and_release_signature_owner() {
        let signature = WasmSignature {
            params: vec![
                WasmType::EXTERN,
                WasmType::I64,
                WasmType::F64,
                WasmType::FUNC,
                WasmType::V128,
                WasmType::Reference {
                    kind: crate::WasmReferenceKind::Internal(wasmparser::AbstractHeapType::Any),
                    nullable: true,
                },
            ],
            results: vec![
                WasmType::EXTERN,
                WasmType::I64,
                WasmType::F64,
                WasmType::FUNC,
                WasmType::V128,
                WasmType::Reference {
                    kind: crate::WasmReferenceKind::Internal(wasmparser::AbstractHeapType::Any),
                    nullable: true,
                },
            ],
        };
        let module = Engine::lower_wasm_module_with_imports(
            "host-lease-owner",
            [
                Operator::Call { function_index: 0 },
                Operator::ReturnCall { function_index: 0 },
            ]
            .map(|call| WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                operators: (0..signature.params.len() as u32)
                    .map(|local_index| Operator::LocalGet { local_index })
                    .chain([call, Operator::End])
                    .map(Ok),
            }),
            &crate::WasmTypes::default(),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[WasmFunctionImport {
                exact: false,
                ty: crate::WasmCallableType::Embedded(signature.clone()),
                name: WasmImportName {
                    index: 0,
                    module: "host".into(),
                    name: "echo".into(),
                },
            }],
        )
        .unwrap();
        let mut vm = Vm::new(EchoHost::default());
        // Rust embedding inputs are not representable by Wast's opaque host tokens.
        for (number, immediate) in [
            (f64::from(crate::wasm::i31::SIGNED_MIN), true),
            (f64::from(crate::wasm::i31::SIGNED_MAX), true),
            (f64::from(crate::wasm::i31::SIGNED_MIN) - 1.0, false),
            (f64::from(crate::wasm::i31::SIGNED_MAX) + 1.0, false),
            (-1.0, true),
            (0.0, true),
            (-0.0, false),
            (1.5, false),
            (f64::INFINITY, false),
            (f64::NAN, false),
        ] {
            let external = Value::number(number);
            let internal = vm.wasm_external_conversion(
                crate::wasm::reference::ExternalConversion::Internalize,
                external,
            );
            assert_eq!(crate::wasm::i31::bits(internal).is_some(), immediate);
            assert_eq!(vm.wasm_external_value(internal), external);
        }

        let function = vm
            .create_wasm_host_function("echo", WasmHostFunctionId(0), signature)
            .unwrap();
        let Some(Cell::Function { env, .. }) = vm.heap.get(function) else {
            panic!()
        };
        let descriptor = *env;
        let function_root = vm.root(function);
        let instance = vm
            .instantiate_wasm_module_with_imports(&module, &[function_root])
            .unwrap();
        assert_eq!(vm.wasm_function(&instance, 0).unwrap(), function);
        assert!(vm.release_root(function_root));
        vm.collect_now(module.residual());
        assert!(vm.heap.get(descriptor).is_some());
        let payload = vm
            .heap
            .alloc(Cell::Object(Vm::<EchoHost>::empty_object(Value::NULL)));
        let payload_root = vm.root(payload);
        let args = [
            WasmValue::ExternRef(payload),
            WasmValue::I64(i64::MIN),
            WasmValue::F64((-0.0_f64).to_bits()),
            WasmValue::FuncRef(function),
            WasmValue::V128(u128::from_le_bytes([0xff; crate::wasm::V128_BYTES])),
            WasmValue::GcRef(Value::integer(crate::wasm::i31::I31_MASK)),
        ];
        let count = vm.heap.root_count_for_test();
        for function_index in (0..module.signatures.function_count() as u32)
            .filter(|&index| module.signatures.defined_index(index).is_some())
        {
            for (index, value) in args.iter().enumerate() {
                let null = match value {
                    WasmValue::ExternRef(_) => WasmValue::ExternRef(Value::NULL),
                    WasmValue::FuncRef(_) => WasmValue::FuncRef(Value::NULL),
                    _ => continue,
                };
                let mut invalid = args;
                invalid[index] = null;
                assert!(
                    vm.invoke_wasm_values(&instance, function_index, &invalid)
                        .is_err()
                );
                assert_eq!(vm.heap.root_count_for_test(), count);
            }
            assert_eq!(
                vm.invoke_wasm_values(&instance, function_index, &args)
                    .unwrap(),
                args
            );
            let mut wrong_family = args;
            wrong_family[args.len() - 1] = WasmValue::ExternRef(Value::NULL);
            assert!(
                vm.invoke_wasm_values(&instance, function_index, &wrong_family)
                    .is_err()
            );
            assert_eq!(vm.heap.root_count_for_test(), count);
            let expired = vm.host.expired.unwrap();
            assert!(vm.root_value(expired).is_none());
            assert!(vm.root_value(vm.host.expired_gc.unwrap()).is_none());
            assert_eq!(vm.heap.root_count_for_test(), count);
            for reply in [
                Reply::StaleReference,
                Reply::WrongType,
                Reply::NullReference,
                Reply::WrongArity,
                Reply::Error,
            ] {
                vm.host.reply = reply;
                assert!(
                    vm.invoke_wasm_values(&instance, function_index, &args)
                        .is_err()
                );
                assert_eq!(vm.heap.root_count_for_test(), count);
                assert!(vm.realm.promise.active_native.is_empty());
                assert!(vm.active_call_roots.is_empty());
                assert_eq!(vm.active_program, ProgramId::MAIN);
            }
            vm.host.reply = Reply::Echo;
            assert_eq!(
                vm.invoke_wasm_values(&instance, function_index, &args)
                    .unwrap(),
                args
            );
        }
        assert!(vm.release_root(payload_root));
        assert!(vm.release_root(instance.environment));
        vm.collect_now(module.residual());
        for value in [payload, function, descriptor] {
            assert!(vm.heap.get(value).is_none());
        }
    }
}
