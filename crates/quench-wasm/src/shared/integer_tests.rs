use super::*;
use quench_runtime::{Runtime, SystemHost, WasmTrap, WasmValue};

fn lower(wat: &str) -> WasmFunction {
    crate::Engine::new()
        .compile_wat(wat)
        .unwrap()
        .lower_shared("f")
        .unwrap()
}

#[test]
fn integer_width_conversions_preserve_low_bits_and_signedness() {
    let mut runtime = Runtime::new(SystemHost);
    let wrap =
        lower(r#"(module (func (export "f") (param i64) (result i32) local.get 0 i32.wrap_i64))"#);
    for (value, expected) in [
        (i64::MAX, -1),
        (i64::MIN, 0),
        (0x1234_5678_8000_0001, i32::MIN + 1),
    ] {
        assert_eq!(
            runtime
                .execute_wasm(&wrap, &[WasmValue::I64(value)])
                .unwrap(),
            Some(WasmValue::I32(expected))
        );
    }
    for (op, unsigned) in [("extend_i32_s", false), ("extend_i32_u", true)] {
        let function = lower(&format!(
            r#"(module (func (export "f") (param i32) (result i64) local.get 0 i64.{op}))"#
        ));
        for input in [i32::MIN, -1, 0, 1, i32::MAX] {
            let expected = if unsigned {
                i64::from(input as u32)
            } else {
                i64::from(input)
            };
            assert_eq!(
                runtime
                    .execute_wasm(&function, &[WasmValue::I32(input)])
                    .unwrap(),
                Some(WasmValue::I64(expected))
            );
        }
    }
}

#[test]
fn allocating_i64_call_loop_preserves_live_locals_and_operands() {
    const DELTA: i64 = 0x0001_0000_0000_0001;
    let function = lower(&format!(
        r#"(module
      (func $next (param i64) (result i64) local.get 0 i64.const {DELTA} i64.add)
      (func (export "f") (param i32) (result i64) (local i64)
        block loop
          local.get 0 i32.eqz br_if 1
          local.get 1 call $next local.set 1
          local.get 0 i32.const 1 i32.sub local.set 0
          br 0
        end end local.get 1))"#
    ));
    let mut runtime = Runtime::new(SystemHost);
    for count in [0, 1, 6000] {
        assert_eq!(
            runtime
                .execute_wasm(&function, &[WasmValue::I32(count)])
                .unwrap(),
            Some(WasmValue::I64(DELTA.wrapping_mul(i64::from(count))))
        );
        runtime.collect(function.residual()).unwrap();
    }
}

#[test]
fn i64_integer_traps_cross_frames_and_recover() {
    let function = lower(
        r#"(module
      (func $divide (param i64 i64) (result i64) local.get 0 local.get 1 i64.div_s)
      (func (export "f") (param i64 i64) (result i64) local.get 0 local.get 1 call $divide))"#,
    );
    let mut runtime = Runtime::new(SystemHost);
    for (left, right, trap) in [
        (1, 0, WasmTrap::IntegerDivideByZero),
        (i64::MIN, -1, WasmTrap::IntegerOverflow),
    ] {
        assert_eq!(
            runtime
                .execute_wasm(&function, &[WasmValue::I64(left), WasmValue::I64(right)])
                .unwrap_err()
                .wasm_trap(),
            Some(trap)
        );
        runtime.collect(function.residual()).unwrap();
    }
    assert_eq!(
        runtime
            .execute_wasm(&function, &[WasmValue::I64(i64::MAX), WasmValue::I64(1)])
            .unwrap(),
        Some(WasmValue::I64(i64::MAX))
    );
}
