use super::*;
use quench_runtime::{Runtime, SystemHost, WasmTrap};

fn lower(wat: &str) -> WasmI32Function {
    crate::Engine::new()
        .compile_wat(wat)
        .unwrap()
        .lower_shared_i32("f")
        .unwrap()
}

#[test]
fn traps_cross_call_frames_and_runtime_recovers() {
    let function = lower(
        r#"(module
      (func $divide (param i32) (result i32) i32.const 42 local.get 0 i32.div_s)
      (func $middle (param i32) (result i32) local.get 0 call $divide)
      (func (export "f") (param i32) (result i32) local.get 0 call $middle))"#,
    );
    let mut runtime = Runtime::new(SystemHost);
    assert_eq!(
        runtime
            .execute_wasm_i32(&function, &[0])
            .unwrap_err()
            .wasm_trap(),
        Some(WasmTrap::IntegerDivideByZero)
    );
    runtime.collect(function.residual()).unwrap();
    assert_eq!(runtime.execute_wasm_i32(&function, &[2]).unwrap(), Some(21));
}

#[test]
fn ordinary_recursive_call_exhausts_instead_of_becoming_a_tail_call() {
    std::thread::Builder::new()
        .name("wasm-recursion".into())
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(|| {
            let recursive = lower(r#"(module (func $loop (export "f") (result i32) call $loop))"#);
            let recovery = lower(r#"(module (func (export "f") (result i32) i32.const 42))"#);
            let mut runtime = Runtime::new(SystemHost);
            assert_eq!(
                runtime
                    .execute_wasm_i32(&recursive, &[])
                    .unwrap_err()
                    .wasm_trap(),
                Some(WasmTrap::CallStackExhausted)
            );
            runtime.collect(recursive.residual()).unwrap();
            assert_eq!(runtime.execute_wasm_i32(&recovery, &[]).unwrap(), Some(42));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn wide_call_windows_preserve_all_arguments() {
    let count = 300;
    let params = "i32 ".repeat(count);
    let arguments = (0..count)
        .map(|i| format!("i32.const {i} "))
        .collect::<String>();
    let function = lower(&format!(
        r#"(module
      (func $difference (param {params}) (result i32) local.get 0 local.get {last} i32.sub)
      (func (export "f") (result i32) {arguments} call $difference))"#,
        last = count - 1
    ));
    let mut runtime = Runtime::new(SystemHost);
    assert_eq!(
        runtime.execute_wasm_i32(&function, &[]).unwrap(),
        Some(1 - count as i32)
    );
}
