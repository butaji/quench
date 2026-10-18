//! Typed embedding calls through ordinary native functions and VM root leases.

use super::*;
use crate::{WasmHostValue, WasmSignature, WasmValue};

#[derive(Clone, Copy)]
enum WasmHostInvocation {
    Host(crate::WasmHostFunctionId),
    Js {
        function: Value,
        memcpy_capture: Option<(Value, usize)>,
    },
}

impl<H: Host> Vm<H> {
    pub(crate) fn create_wasm_host_function(
        &mut self,
        name: &str,
        target: crate::heap::WasmHostTarget,
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
            self.initialize_shared(&bootstrap.program, super::builtins::RealmScope::Core)?;
        }
        let length = signature.params.len();
        let environment = self.heap.alloc(Cell::WasmHostFunction {
            target,
            signature: Rc::new(signature),
        });
        let function = self.native_with_env(Native::WasmHost, environment);
        self.set_builtin_function_name(function, name)?;
        let atom = self.intern_atom("length");
        self.set_property_with_attributes(
            function,
            atom,
            Value::number(length as f64),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        )?;
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
        let (target, signature) = match self.heap.get(environment) {
            Some(Cell::WasmHostFunction { target, signature }) => {
                let target = match target {
                    crate::heap::WasmHostTarget::Host(id) => WasmHostInvocation::Host(*id),
                    crate::heap::WasmHostTarget::Js(function) => WasmHostInvocation::Js {
                        function: *function,
                        memcpy_capture: None,
                    },
                    crate::heap::WasmHostTarget::JsEmscriptenMemcpy(target) => {
                        WasmHostInvocation::Js {
                            function: target.function,
                            memcpy_capture: Some((target.view_environment, target.view_slot)),
                        }
                    }
                };
                (target, signature.clone())
            }
            _ => return Err(JsError::validation("invalid Wasm host binding".into())),
        };
        if args.len() != signature.params.len() {
            return Err(JsError::validation(
                "Wasm host argument count mismatch".into(),
            ));
        }
        let id = match target {
            WasmHostInvocation::Host(id) => id,
            WasmHostInvocation::Js {
                function,
                memcpy_capture,
            } => {
                return self.call_wasm_js_import(
                    function,
                    memcpy_capture,
                    &signature,
                    args,
                );
            }
        };
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
                    object: crate::heap::ArrayObject::new(
                        Self::empty_object(Value::NULL),
                        values.into(),
                    ),
                }),
            })
        })();
        for root in roots {
            self.release_root(root);
        }
        result
    }

    /// Calls a JS-backed Wasm import from compiled code without dispatching
    /// through its `Native::WasmHost` wrapper. Keep the wrapper's VM boundary
    /// observable state while sharing the ordinary JS import conversion path.
    pub(super) fn call_compiled_js_wasm_import(
        &mut self,
        program: ProgramId,
        p: &ResidualProgram,
        wrapper: Value,
        target: Value,
        memcpy_capture: Option<(Value, usize)>,
        signature: &WasmSignature,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let call_roots = self.active_call_roots.len();
        self.active_call_roots
            .extend([wrapper, receiver].into_iter().chain(args.iter().copied()));
        self.profile.call_target(0, args.len());

        let realm = match self.heap.get(wrapper) {
            Some(Cell::Function { object, .. })
                if object.realm == self.realm.globals
                    || !self.shared_natives.contains(&wrapper) =>
            {
                object.realm
            }
            _ => self.realm.globals,
        };
        let previous_program = self.replace_active_program(program);
        let previous_global = self.switch_realm_global(realm);
        let native_depth = self.realm.promise.active_native.len();
        self.realm
            .promise
            .active_native
            .push(super::activation::NativeActivation {
                callable: wrapper,
                frame_depth: self.frames.len(),
                boundary: super::activation::NativeCallBoundary::Opaque,
            });

        // The compiled-callout boundary catches Rust panics. Catch here long
        // enough to restore the state this shortcut installs before resuming
        // the panic to that boundary.
        let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _stack = self.enter_stack()?;
            self.check_execution_interrupt()?;
            let receiver = self.string_method_receiver(p, Native::WasmHost, receiver)?;
            self.with_call_roots([receiver], |vm| {
                vm.call_wasm_js_import(target, memcpy_capture, signature, args)
            })
        }));

        self.realm.promise.active_native.truncate(native_depth);
        self.switch_realm_global(previous_global);
        self.set_active_program(previous_program);
        self.active_call_roots.truncate(call_roots);
        match called {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, RootId, WasmFunctionBody, WasmHostFunctionId, WasmFunctionImport, WasmImportName, WasmType};
    use wasmparser::Operator;

    fn compiled_js_import_module(name: &str, signature: WasmSignature) -> crate::WasmModule {
        Engine::lower_wasm_module_with_imports(
            name,
            [WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![],
                    results: vec![WasmType::I32],
                }),
                locals: vec![],
                operators: [
                    Operator::I32Const { value: 7 },
                    Operator::I32Const { value: 11 },
                    Operator::I32Const { value: 13 },
                    Operator::Call { function_index: 0 },
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
            &[WasmFunctionImport {
                exact: false,
                ty: crate::WasmCallableType::Embedded(signature),
                name: WasmImportName {
                    index: 0,
                    module: "env".into(),
                    name: "jsImport".into(),
                },
            }],
        )
        .unwrap()
    }

    #[test]
    fn compiled_emscripten_memcpy_import_reads_current_view_and_honors_method_replacement() {
        let signature = WasmSignature {
            params: vec![WasmType::I32, WasmType::I32, WasmType::I32],
            results: vec![],
        };
        let module = Engine::lower_wasm_module_with_imports(
            "compiled-emscripten-memcpy-import",
            [WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![],
                    results: vec![],
                }),
                locals: vec![],
                operators: [
                    Operator::I32Const { value: 7 },
                    Operator::I32Const { value: 11 },
                    Operator::I32Const { value: 13 },
                    Operator::Call { function_index: 0 },
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
            &[WasmFunctionImport {
                exact: false,
                ty: crate::WasmCallableType::Embedded(signature.clone()),
                name: WasmImportName {
                    index: 0,
                    module: "env".into(),
                    name: "_emscripten_memcpy_js".into(),
                },
            }],
        )
        .unwrap();

        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let owner = Engine::specialize("void 0", "compiled-emscripten-memcpy-owner.js").unwrap();
        vm.execute(&owner).unwrap();
        let script = vm
            .compile_embedding_host_script(
                "(() => { var HEAPU8 = new Uint8Array(Array.from({length: 32}, (_, i) => i)); const replacement = new Uint8Array(Array.from({length: 32}, (_, i) => 100 + i)); return { helper: (dest, src, num) => HEAPU8.copyWithin(dest, src, src + num), replacement }; })()",
                "compiled-emscripten-memcpy-helper.js",
                (0, 0),
            )
            .unwrap();
        let exports = vm.evaluate_embedding_compiled_script(&script).unwrap();
        let helper_atom = vm.intern_atom("helper");
        let helper = vm.own_property(exports, helper_atom).unwrap();
        let replacement_atom = vm.intern_atom("replacement");
        let replacement = vm.own_property(exports, replacement_atom).unwrap();
        let Cell::Function {
            kind: FunctionKind::User(program_id, function_id),
            env,
            ..
        } = vm.heap.get(helper).unwrap()
        else {
            panic!("helper should be a user function")
        };
        let helper_program = vm.programs.get(*program_id).unwrap();
        let helper_metadata = &helper_program.functions[*function_id as usize];
        let helper_source = helper_metadata.source_span.map(|span| {
            &helper_program.source.as_ref().unwrap().text[span.start as usize..span.end as usize]
        });
        assert_eq!(
            helper_source,
            Some("(dest, src, num) => HEAPU8.copyWithin(dest, src, src + num)"),
            "the source span used for exact-body recognition"
        );
        let view_atom = helper_program
            .atoms
            .iter()
            .position(|atom| atom == "HEAPU8")
            .unwrap() as u32;
        assert!(
            vm.outer_environment_binding(*env, view_atom).is_some(),
            "the helper closure should expose its HEAPU8 lexical binding"
        );
        let (view_environment, view_slot) = vm
            .emscripten_memcpy_view_capture(helper, &signature)
            .expect("the exact helper and its lexical view should be recognized");
        let target = crate::heap::WasmHostTarget::JsEmscriptenMemcpy(Box::new(
            crate::heap::WasmHostMemcpyTarget {
            function: helper,
            view_environment,
            view_slot,
            },
        ));
        let imported = vm
            .create_wasm_host_function("_emscripten_memcpy_js", target, signature.clone())
            .unwrap();
        let imported_root = vm.root(imported);
        let instance = vm
            .instantiate_wasm_module_with_imports(&module, &[imported_root])
            .unwrap();

        vm.invoke_wasm(&instance, 1, &[]).unwrap();
        let original = vm
            .heap
            .environment_slot(view_environment, view_slot)
            .unwrap();
        assert_eq!(
            (7..20)
                .map(|index| vm.typed_array_get(original, index).unwrap().as_number().unwrap() as u8)
                .collect::<Vec<_>>(),
            (11..24).collect::<Vec<_>>()
        );

        *vm.heap
            .environment_slot_mut(view_environment, view_slot)
            .expect("captured HEAPU8 slot") = replacement;
        vm.invoke_wasm(&instance, 1, &[]).unwrap();
        assert_eq!(
            (7..20)
                .map(|index| vm.typed_array_get(replacement, index).unwrap().as_number().unwrap() as u8)
                .collect::<Vec<_>>(),
            (111..124).map(|value| value as u8).collect::<Vec<_>>()
        );

        let custom_method = vm
            .compile_embedding_host_script(
                "(function replacementCopyWithin() { this[0] = 99; return this; })",
                "compiled-emscripten-memcpy-custom-method.js",
                (0, 0),
            )
            .unwrap();
        let custom_method = vm
            .evaluate_embedding_compiled_script(&custom_method)
            .unwrap();
        vm.set_builtin_value_named(replacement, "copyWithin", custom_method)
            .unwrap();
        vm.invoke_wasm(&instance, 1, &[]).unwrap();
        assert_eq!(vm.typed_array_get(replacement, 0).unwrap().as_number(), Some(99.0));

        let shadowed = vm
            .compile_embedding_host_script(
                "(() => { let HEAPU8 = new Uint8Array(Array.from({length: 32}, (_, i) => i)); const alternate = new Uint8Array(Array.from({length: 32}, (_, i) => 100 + i)); let helper; with ({ HEAPU8: alternate }) { helper = (dest, src, num) => HEAPU8.copyWithin(dest, src, src + num); } return { helper, HEAPU8, alternate }; })()",
                "compiled-emscripten-memcpy-with-shadow.js",
                (0, 0),
            )
            .unwrap();
        let shadowed = vm.evaluate_embedding_compiled_script(&shadowed).unwrap();
        let shadowed_helper = vm.own_property(shadowed, helper_atom).unwrap();
        let lexical_view_atom = vm.intern_atom("HEAPU8");
        let lexical_view = vm.own_property(shadowed, lexical_view_atom).unwrap();
        let alternate_atom = vm.intern_atom("alternate");
        let alternate_view = vm
            .own_property(shadowed, alternate_atom)
            .unwrap();
        assert_eq!(
            vm.emscripten_memcpy_view_capture(shadowed_helper, &signature),
            None,
            "a with-shadowed free name must not specialize to an outer lexical slot"
        );
        let shadowed_import = vm
            .create_wasm_host_function(
                "_emscripten_memcpy_js",
                crate::heap::WasmHostTarget::Js(shadowed_helper),
                signature.clone(),
            )
            .unwrap();
        let shadowed_root = vm.root(shadowed_import);
        let shadowed_instance = vm
            .instantiate_wasm_module_with_imports(&module, &[shadowed_root])
            .unwrap();
        vm.invoke_wasm(&shadowed_instance, 1, &[]).unwrap();
        assert_eq!(
            (7..20)
                .map(|index| vm.typed_array_get(alternate_view, index).unwrap().as_number().unwrap() as u8)
                .collect::<Vec<_>>(),
            (111..124).map(|value| value as u8).collect::<Vec<_>>()
        );
        assert_eq!(
            (7..20)
                .map(|index| vm.typed_array_get(lexical_view, index).unwrap().as_number().unwrap() as u8)
                .collect::<Vec<_>>(),
            (7..20).collect::<Vec<_>>(),
            "the with object shadows the outer lexical HEAPU8"
        );
        assert!(vm.realm.promise.active_native.is_empty());
        assert!(vm.active_call_roots.is_empty());
    }

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
            .create_wasm_host_function(
                "echo",
                crate::heap::WasmHostTarget::Host(WasmHostFunctionId(0)),
                signature,
            )
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
        assert!(vm.heap.is_live(descriptor));
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
            assert!(!vm.heap.is_live(value));
        }
    }

    #[test]
    fn compiled_js_import_converts_inline_arguments_and_result() {
        let signature = WasmSignature {
            params: vec![WasmType::I32, WasmType::I32, WasmType::I32],
            results: vec![WasmType::I32],
        };
        let body_signature = WasmSignature {
            params: vec![],
            results: vec![WasmType::I32],
        };
        let module = Engine::lower_wasm_module_with_imports(
            "compiled-js-import-inline-arguments",
            [WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(body_signature),
                locals: vec![],
                operators: [
                    Operator::I32Const { value: 7 },
                    Operator::I32Const { value: 11 },
                    Operator::I32Const { value: 13 },
                    Operator::Call { function_index: 0 },
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
            &[WasmFunctionImport {
                exact: false,
                ty: crate::WasmCallableType::Embedded(signature.clone()),
                name: WasmImportName {
                    index: 0,
                    module: "env".into(),
                    name: "_emscripten_memcpy_js".into(),
                },
            }],
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let owner = Engine::specialize("void 0", "compiled-js-import-owner.js").unwrap();
        vm.execute(&owner).unwrap();
        let helper = vm
            .compile_embedding_host_script(
                "(dest,src,num)=>dest*10000+src*100+num",
                "compiled-js-import-helper.js",
                (0, 0),
            )
            .unwrap();
        let target = vm.evaluate_embedding_compiled_script(&helper).unwrap();
        let imported = vm
            .create_wasm_host_function(
                "_emscripten_memcpy_js",
                crate::heap::WasmHostTarget::Js(target),
                signature,
            )
            .unwrap();
        let imported_root = vm.root(imported);
        let instance = vm
            .instantiate_wasm_module_with_imports(&module, &[imported_root])
            .unwrap();

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(WasmValue::I32(71_113))
        );
        assert!(vm.realm.promise.active_native.is_empty());
        assert!(vm.active_call_roots.is_empty());
        assert_eq!(vm.active_program, crate::vm::ProgramId::MAIN);
    }

    #[test]
    fn compiled_js_import_keeps_opaque_caller_boundary() {
        let signature = WasmSignature {
            params: vec![WasmType::I32, WasmType::I32, WasmType::I32],
            results: vec![WasmType::I32],
        };
        let module = compiled_js_import_module(
            "compiled-js-import-opaque-caller",
            signature.clone(),
        );
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let owner = Engine::specialize("void 0", "compiled-js-import-caller-owner.js").unwrap();
        vm.execute(&owner).unwrap();
        let helper = vm
            .compile_embedding_host_script(
                "(function imported(){ return imported.caller === null ? 1 : 0; })",
                "compiled-js-import-caller.js",
                (0, 0),
            )
            .unwrap();
        let target = vm.evaluate_embedding_compiled_script(&helper).unwrap();
        let imported = vm
            .create_wasm_host_function(
                "jsImport",
                crate::heap::WasmHostTarget::Js(target),
                signature,
            )
            .unwrap();
        let imported_root = vm.root(imported);
        let instance = vm
            .instantiate_wasm_module_with_imports(&module, &[imported_root])
            .unwrap();

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(WasmValue::I32(1))
        );
        assert!(vm.realm.promise.active_native.is_empty());
        assert!(vm.active_call_roots.is_empty());
        assert_eq!(vm.active_program, crate::vm::ProgramId::MAIN);
    }

    #[test]
    fn compiled_js_import_preserves_strict_undefined_receiver() {
        let signature = WasmSignature {
            params: vec![WasmType::I32, WasmType::I32, WasmType::I32],
            results: vec![WasmType::I32],
        };
        let module = compiled_js_import_module(
            "compiled-js-import-strict-receiver",
            signature.clone(),
        );
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let owner = Engine::specialize("void 0", "compiled-js-import-strict-owner.js").unwrap();
        vm.execute(&owner).unwrap();
        let helper = vm
            .compile_embedding_host_script(
                "(function imported(){ 'use strict'; return this === undefined ? 1 : 0; })",
                "compiled-js-import-strict.js",
                (0, 0),
            )
            .unwrap();
        let target = vm.evaluate_embedding_compiled_script(&helper).unwrap();
        let imported = vm
            .create_wasm_host_function(
                "jsImport",
                crate::heap::WasmHostTarget::Js(target),
                signature,
            )
            .unwrap();
        let imported_root = vm.root(imported);
        let instance = vm
            .instantiate_wasm_module_with_imports(&module, &[imported_root])
            .unwrap();

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(WasmValue::I32(1))
        );
        assert!(vm.realm.promise.active_native.is_empty());
        assert!(vm.active_call_roots.is_empty());
        assert_eq!(vm.active_program, crate::vm::ProgramId::MAIN);
    }

    #[test]
    fn compiled_js_import_throw_after_collection_restores_roots_and_vm_state() {
        let signature = WasmSignature {
            params: vec![WasmType::I32, WasmType::I32, WasmType::I32],
            results: vec![WasmType::I32],
        };
        let module = compiled_js_import_module("compiled-js-import-throw-gc", signature.clone());
        let mut vm = Vm::new(crate::SystemHost);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        let owner = Engine::specialize("void 0", "compiled-js-import-throw-owner.js").unwrap();
        vm.execute(&owner).unwrap();
        let gc = vm.native_value(Native::CollectGarbage);
        vm.set_builtin_value_named(vm.realm.globals, "gc", gc)
            .unwrap();
        let helper = vm
            .compile_embedding_host_script(
                "(() => { const marker = {}; const imported = function imported(){ gc(); throw marker; }; imported.marker = marker; return imported; })()",
                "compiled-js-import-throw.js",
                (0, 0),
            )
            .unwrap();
        let target = vm.evaluate_embedding_compiled_script(&helper).unwrap();
        let marker_atom = vm.intern_atom("marker");
        let marker = vm
            .own_property(target, marker_atom)
            .expect("the imported target retains its thrown marker");
        let imported = vm
            .create_wasm_host_function(
                "jsImport",
                crate::heap::WasmHostTarget::Js(target),
                signature,
            )
            .unwrap();
        let imported_root = vm.root(imported);
        let instance = vm
            .instantiate_wasm_module_with_imports(&module, &[imported_root])
            .unwrap();
        let roots_before = vm.heap.root_count_for_test();
        let realm_before = vm.realm.globals;

        let error = vm.invoke_wasm(&instance, 1, &[]).unwrap_err();

        assert_eq!(error.thrown_value(), Some(marker));
        assert_eq!(vm.heap.root_count_for_test(), roots_before);
        assert!(vm.realm.promise.active_native.is_empty());
        assert!(vm.active_call_roots.is_empty());
        assert_eq!(vm.active_program, crate::vm::ProgramId::MAIN);
        assert_eq!(vm.realm.globals, realm_before);
        assert!(vm.heap.is_live(target));
        assert!(vm.heap.is_live(marker));
    }
}
