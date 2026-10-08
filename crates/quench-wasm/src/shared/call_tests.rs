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
fn forward_calls_preserve_argument_order_and_operand_prefix() {
    let function = lower(
        r#"(module
      (func (export "f") (result i32)
        i32.const 100 i32.const 9 i32.const 4 call $sub i32.add)
      (func $sub (param i32 i32) (result i32)
        local.get 0 local.get 1 i32.sub))"#,
    );
    let mut runtime = Runtime::new(SystemHost);
    assert_eq!(runtime.execute_wasm_i32(&function, &[]).unwrap(), Some(105));
}

#[test]
fn nested_void_calls_do_not_push_an_operand() {
    let function = lower(
        r#"(module
      (func $sink (param i32) (local i32) local.get 1 drop)
      (func $middle (param i32) local.get 0 call $sink)
      (func (export "f") (result i32)
        i32.const 31 i32.const 77 call $middle i32.const 11 i32.add))"#,
    );
    let void = lower(r#"(module (func $empty) (func (export "f") call $empty))"#);
    let mut runtime = Runtime::new(SystemHost);
    assert_eq!(runtime.execute_wasm_i32(&function, &[]).unwrap(), Some(42));
    assert_eq!(runtime.execute_wasm_i32(&void, &[]).unwrap(), None);
}

#[test]
fn recursion_keeps_caller_operands_and_distinct_locals() {
    let function = lower(
        r#"(module
      (func $factorial (param i32) (result i32)
        local.get 0 i32.eqz if (result i32) i32.const 1 else
          local.get 0 local.get 0 i32.const 1 i32.sub call $factorial i32.mul
        end)
      (func (export "f") (param i32) (result i32) local.get 0 call $factorial))"#,
    );
    let mut runtime = Runtime::new(SystemHost);
    for (input, expected) in [(0, 1), (1, 1), (5, 120), (10, 3628800)] {
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[input]).unwrap(),
            Some(expected)
        );
    }
    runtime.collect(function.residual()).unwrap();
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
