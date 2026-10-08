use quench_runtime::WasmValue;

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
