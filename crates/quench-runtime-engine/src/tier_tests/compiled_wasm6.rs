//! Compiled WebAssembly against the interpreter: what the tier compiles, and that its runs end as the interpreter's do (moved from the core's `vm/wasm.rs` tests, whose binary does not link the engine).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use wasmparser::Operator;

#[test]
fn compiled_wasm_loop_backedge_polls_execution_interrupt() {
    struct StartHost(std::sync::mpsc::Sender<()>);
    impl quench_runtime::Host for StartHost {
        // Its test runs compiled Wasm only (`Host::runs_lane`).
        fn runs_lane(&self) -> bool { false }
        fn write_line(&mut self, _: &str) {}
        fn clock_millis(&mut self) -> f64 {
            0.0
        }
        fn call_wasm(
            &mut self,
            _: quench_runtime::WasmHostFunctionId,
            _: &[quench_runtime::WasmHostValue],
        ) -> Result<Vec<quench_runtime::WasmHostValue>, String> {
            self.0.send(()).map_err(|error| error.to_string())?;
            Ok(Vec::new())
        }
    }

    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let interrupt = quench_runtime::ExecutionInterrupt::default();
    let worker_interrupt = interrupt.clone();
    let worker = std::thread::Builder::new()
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(move || {
            let mut vm =
                Vm::new_with_execution_interrupt(StartHost(entered_tx), worker_interrupt);
            vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
            let signature = quench_runtime::WasmSignature {
                params: vec![],
                results: vec![],
            };
            let module = quench_runtime::Engine::lower_wasm_module_with_imports(
                "compiled-wasm-interrupt-backedge",
                [quench_runtime::WasmFunctionBody {
                    ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
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
                &quench_runtime::WasmTypes::default(),
                &[],
                &[],
                &[],
                &[],
                &[],
                &[quench_runtime::WasmFunctionImport {
                    exact: false,
                    ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                    name: quench_runtime::WasmImportName {
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
                    quench_runtime::heap::WasmHostTarget::Host(quench_runtime::WasmHostFunctionId(0)),
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
                .wasm_tier(program, 0)
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
fn compiled_wasm_root_selection_leaves_unselected_functions_interpreted() {
    let module = quench_runtime::Engine::lower_wasm_i32_module(
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
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&function.module).unwrap();
    let program = vm
        .programs
        .find_shared(&function.module.program)
        .expect("registered Wasm program");
    assert!(vm.install_compiled_root_for_test(program, Value::NULL, 0));

    assert_eq!(
        vm.invoke_wasm(&instance, 0, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(42))
    );
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("root closure installed");
    assert_eq!(compiled.native_entries(0), Some(1));
    assert_eq!(compiled.native_entries(1), None);
    assert_eq!(compiled.native_entries(2), None);
}

#[test]
fn compiled_wasm_unreachable_lowers_to_a_native_trap() {
    let module = quench_runtime::Engine::lower_wasm_i32_module(
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
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&function.module).unwrap();
    let program = vm
        .programs
        .find_shared(&function.module.program)
        .expect("registered Wasm program");
    assert!(vm.install_compiled_root_for_test(program, Value::NULL, 0));
    assert_eq!(
        vm.invoke_wasm(&instance, 0, &[]).unwrap_err().wasm_trap(),
        Some(quench_runtime::WasmTrap::Unreachable)
    );
    // A small function's `unreachable` lowers to a native trap in its root artifact.
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("installed Wasm root");
    assert_eq!(compiled.native_entries(0), Some(1));

    let mut interpreted = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    assert_eq!(
        interpreted.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
        Some(quench_runtime::WasmTrap::Unreachable)
    );
}

/// A function body of the embedded type `ty` with no locals, running `operators`.
pub(super) fn embedded_body(
    ty: quench_runtime::WasmSignature,
    operators: Vec<Operator<'static>>,
) -> quench_runtime::WasmFunctionBody<std::vec::IntoIter<Result<Operator<'static>, wasmparser::BinaryReaderError>>> {
    quench_runtime::WasmFunctionBody {
        ty: quench_runtime::WasmCallableType::Embedded(ty),
        locals: vec![],
        operators: operators.into_iter().map(Ok).collect::<Vec<_>>().into_iter(),
    }
}

/// A fixed-size `funcref` table of `size` entries, each initialized to function 0.
pub(super) fn funcref_table(size: u64) -> quench_runtime::WasmTable {
    quench_runtime::WasmTable {
        ty: wasmparser::TableType {
            element_type: wasmparser::RefType::FUNCREF,
            table64: false,
            initial: size,
            maximum: Some(size),
            shared: false,
        },
        initializer: quench_runtime::WasmReferenceInitializer::Function(0).into(),
    }
}

/// An element segment writing `functions` into table 0 from index 0.
pub(super) fn active_function_element(functions: &[u32]) -> quench_runtime::WasmElement {
    quench_runtime::WasmElement {
        mode: quench_runtime::WasmElementMode::Active {
            table: 0,
            offset: quench_runtime::WasmValue::I32(0).into(),
        },
        element_type: wasmparser::RefType::FUNCREF,
        items: functions.iter().map(|&function| quench_runtime::WasmReferenceInitializer::Function(function)).collect::<Vec<_>>().into(),
    }
}

/// The signature of a function with no parameters and one `i32` result.
pub(super) fn i32_result_signature() -> quench_runtime::WasmSignature {
    quench_runtime::WasmSignature {
        params: vec![],
        results: vec![quench_runtime::WasmType::I32],
    }
}
