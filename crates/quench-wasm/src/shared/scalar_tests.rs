use super::*;
use rqj::{Runtime, SystemHost, WasmValue};

fn lower(wat: &str) -> WasmFunction {
    crate::Engine::new()
        .compile_wat(wat)
        .unwrap()
        .lower_shared("f")
        .unwrap()
}

#[test]
fn scalar_constants_preserve_integer_boundaries_and_float_payloads() {
    let mut runtime = Runtime::new(SystemHost);
    for (ty, text, expected) in [
        ("i64", "9223372036854775807", WasmValue::I64(i64::MAX)),
        ("i64", "-9223372036854775808", WasmValue::I64(i64::MIN)),
        (
            "i64",
            "9007199254740993",
            WasmValue::I64(9_007_199_254_740_993),
        ),
        ("f32", "-0", WasmValue::F32((-0.0f32).to_bits())),
        ("f32", "nan:0x123456", WasmValue::F32(0x7f92_3456)),
        ("f64", "-0", WasmValue::F64((-0.0f64).to_bits())),
        (
            "f64",
            "nan:0x123456789abcd",
            WasmValue::F64(0x7ff1_2345_6789_abcd),
        ),
        (
            "f64",
            "-nan:0x9000000000001",
            WasmValue::F64(0xfff9_0000_0000_0001),
        ),
    ] {
        let function = lower(&format!(
            r#"(module (func (export "f") (result {ty}) {ty}.const {text}))"#
        ));
        assert_eq!(
            runtime.execute_wasm(&function, &[]).unwrap(),
            Some(expected)
        );
        runtime.collect(function.residual()).unwrap();
    }
}

#[test]
fn scalar_arguments_locals_and_calls_keep_exact_bits() {
    let mut runtime = Runtime::new(SystemHost);
    for (ty, value) in [
        ("i64", WasmValue::I64(i64::MIN + 17)),
        ("f32", WasmValue::F32(0xffd2_3456)),
        ("f64", WasmValue::F64(0x7ffc_1234_5678_9abc)),
    ] {
        let function = lower(&format!(
            r#"(module
          (func $copy (param {ty}) (result {ty}) (local {ty}) local.get 0 local.tee 1)
          (func (export "f") (param {ty}) (result {ty}) local.get 0 call $copy))"#
        ));
        assert_eq!(
            runtime.execute_wasm(&function, &[value]).unwrap(),
            Some(value)
        );
        runtime.collect(function.residual()).unwrap();
        assert_eq!(
            runtime.execute_wasm(&function, &[value]).unwrap(),
            Some(value)
        );
    }
}

#[test]
fn every_scalar_local_uses_its_typed_zero() {
    let mut runtime = Runtime::new(SystemHost);
    for (ty, expected) in [
        ("i32", WasmValue::I32(0)),
        ("i64", WasmValue::I64(0)),
        ("f32", WasmValue::F32(0)),
        ("f64", WasmValue::F64(0)),
    ] {
        let function = lower(&format!(
            r#"(module (func (export "f") (result {ty}) (local {ty}) local.get 0))"#
        ));
        assert_eq!(
            runtime.execute_wasm(&function, &[]).unwrap(),
            Some(expected)
        );
    }
}

#[test]
fn typed_branches_and_select_preserve_nan_payloads() {
    let function = lower(
        r#"(module (func (export "f") (param i32 f64 f64) (result f64)
      block (result f64)
        local.get 1 local.get 2 local.get 0 select
        local.get 0 br_if 0
      end))"#,
    );
    let mut runtime = Runtime::new(SystemHost);
    let left = WasmValue::F64(0xfffe_0000_0000_0001);
    let right = WasmValue::F64(0x7ffd_0000_0000_0001);
    for (condition, expected) in [(0, right), (1, left), (-1, left)] {
        assert_eq!(
            runtime
                .execute_wasm(&function, &[WasmValue::I32(condition), left, right])
                .unwrap(),
            Some(expected)
        );
    }
}

#[test]
fn host_boundary_rejects_type_mismatches_and_i32_convenience_misuse() {
    let function = lower(r#"(module (func (export "f") (param i64) (result i64) local.get 0))"#);
    let mut runtime = Runtime::new(SystemHost);
    assert!(
        runtime
            .execute_wasm(&function, &[WasmValue::F64(0)])
            .is_err()
    );
    assert!(runtime.execute_wasm(&function, &[]).is_err());
    assert!(runtime.execute_wasm_i32(&function, &[0]).is_err());
    assert_eq!(
        runtime
            .execute_wasm(&function, &[WasmValue::I64(-1)])
            .unwrap(),
        Some(WasmValue::I64(-1))
    );
}

#[test]
fn unsupported_scalar_arithmetic_and_reference_types_fail_explicitly() {
    for wat in [
        r#"(module (func (export "f") (result f64) f64.const 1 f64.const 2 f64.add))"#,
        r#"(module (func (export "f") (result externref) ref.null extern))"#,
    ] {
        assert!(matches!(
            crate::Engine::new()
                .compile_wat(wat)
                .unwrap()
                .lower_shared("f"),
            Err(Error::Unsupported(_))
        ));
    }
}
