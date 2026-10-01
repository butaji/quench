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
fn nearest_uses_even_ties_and_preserves_negative_zero() {
    let mut runtime = Runtime::new(SystemHost);
    for ty in ["f32", "f64"] {
        let function = lower(&format!(
            r#"(module (func (export "f") (param {ty}) (result {ty}) local.get 0 {ty}.nearest))"#
        ));
        for (input, expected) in [
            (0.5, 0.0),
            (1.5, 2.0),
            (2.5, 2.0),
            (-0.5, -0.0),
            (-1.5, -2.0),
            (-2.5, -2.0),
        ] {
            let value = |number: f64| {
                if ty == "f32" {
                    WasmValue::F32((number as f32).to_bits())
                } else {
                    WasmValue::F64(number.to_bits())
                }
            };
            assert_eq!(
                runtime.execute_wasm(&function, &[value(input)]).unwrap(),
                Some(value(expected))
            );
        }
    }
}

#[test]
fn min_max_order_zero_signs_and_propagate_nan() {
    let mut runtime = Runtime::new(SystemHost);
    for ty in ["f32", "f64"] {
        let (positive, negative, nan) = if ty == "f32" {
            (
                WasmValue::F32(0.0f32.to_bits()),
                WasmValue::F32((-0.0f32).to_bits()),
                WasmValue::F32(f32::NAN.to_bits()),
            )
        } else {
            (
                WasmValue::F64(0.0f64.to_bits()),
                WasmValue::F64((-0.0f64).to_bits()),
                WasmValue::F64(f64::NAN.to_bits()),
            )
        };
        for (op, zero) in [("min", negative), ("max", positive)] {
            let function = lower(&format!(
                r#"(module (func (export "f") (param {ty} {ty}) (result {ty}) local.get 0 local.get 1 {ty}.{op}))"#
            ));
            for args in [[positive, negative], [negative, positive]] {
                assert_eq!(runtime.execute_wasm(&function, &args).unwrap(), Some(zero));
            }
            for args in [[nan, positive], [positive, nan]] {
                assert!(
                    runtime
                        .execute_wasm(&function, &args)
                        .unwrap()
                        .unwrap()
                        .is_canonical_nan()
                );
            }
        }
    }
}

#[test]
fn f32_call_results_round_at_each_operation() {
    let function = lower(
        r#"(module
      (func $add (param f32 f32) (result f32) local.get 0 local.get 1 f32.add)
      (func (export "f") (result f32)
        f32.const 16777216 f32.const 1 call $add f32.const -16777216 call $add))"#,
    );
    let mut runtime = Runtime::new(SystemHost);
    assert_eq!(
        runtime.execute_wasm(&function, &[]).unwrap(),
        Some(WasmValue::F32(0.0f32.to_bits()))
    );
    runtime.collect(function.residual()).unwrap();
}

#[test]
fn nan_predicates_require_float_types_and_quiet_payloads() {
    for (value, canonical, arithmetic) in [
        (WasmValue::F32(0x7fc0_0000), true, true),
        (WasmValue::F32(0xffc0_0000), true, true),
        (WasmValue::F32(0x7fc0_0001), false, true),
        (WasmValue::F32(0x7f80_0001), false, false),
        (WasmValue::F32(f32::INFINITY.to_bits()), false, false),
        (WasmValue::F64(0x7ff8_0000_0000_0000), true, true),
        (WasmValue::F64(0xfff8_0000_0000_0001), false, true),
        (WasmValue::F64(0x7ff0_0000_0000_0001), false, false),
        (WasmValue::I64(0x7ff8_0000_0000_0000), false, false),
    ] {
        assert_eq!(value.is_canonical_nan(), canonical);
        assert_eq!(value.is_arithmetic_nan(), arithmetic);
    }
}
