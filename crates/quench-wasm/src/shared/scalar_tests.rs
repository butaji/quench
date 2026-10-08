use super::*;
use quench_runtime_next::{Runtime, SystemHost, WasmValue};

fn lower(wat: &str) -> WasmFunction {
    crate::Engine::new()
        .compile_wat(wat)
        .unwrap()
        .lower_shared("f")
        .unwrap()
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
fn host_boundary_rejects_type_mismatches_and_i32_convenience_misuse() {
    let function = lower(r#"(module (func (export "f") (param i64) (result i64) local.get 0))"#);
    let mut runtime = Runtime::new(SystemHost);
    assert!(runtime
        .execute_wasm(&function, &[WasmValue::F64(0)])
        .is_err());
    assert!(runtime.execute_wasm(&function, &[]).is_err());
    assert!(runtime.execute_wasm_i32(&function, &[0]).is_err());
    assert_eq!(
        runtime
            .execute_wasm(&function, &[WasmValue::I64(-1)])
            .unwrap(),
        Some(WasmValue::I64(-1))
    );
}
