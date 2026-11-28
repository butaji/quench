//! The Wasm float and conversion rules of `staged::wasm_float` on the interpreter's stage
//! against the declarations (`apply`), the order of a truncation's checks on a probe stage, and
//! the opcode table.

use super::wasm_float::{Float, convert, float_binary, is_float_comparison};
use super::wasm_operation::{WasmOperation, wasm_operation_of};
use super::wasm_tests::Probe;
use super::{ConcreteWasm, machine::MachineStage};
use crate::bytecode::Op;
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::float::{
    F32BinaryOperator, F32UnaryOperator, F64BinaryOperator as Binary, F64UnaryOperator as Unary,
};
use crate::wasm::WasmValueScalarExt;
use crate::{WasmTrap, WasmType, WasmValue};
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

const SIGNALING_NAN_PAYLOAD: u32 = 1;

/// Finite values at the edges of every conversion's range and rounding, the infinities, and
/// the NaNs: canonical, negative, signaling and with a payload.
fn f64_operands() -> Vec<u64> {
    let finite = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        1.5,
        2.5,
        -2.5,
        4_294_967_295.5,
        2_147_483_648.0,
        -2_147_483_649.0,
        4_294_967_296.0,
        9_223_372_036_854_775_808.0,
        18_446_744_073_709_551_616.0,
        -9_223_372_036_854_775_808.0,
        f64::MAX,
        f64::MIN_POSITIVE,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];
    let nans = [
        f64::NAN.to_bits(),
        (-f64::NAN).to_bits(),
        f64::INFINITY.to_bits() | u64::from(SIGNALING_NAN_PAYLOAD),
        f64::NAN.to_bits() | u64::from(SIGNALING_NAN_PAYLOAD),
    ];
    finite.map(f64::to_bits).into_iter().chain(nans).collect()
}

fn f32_operands() -> Vec<u32> {
    let finite = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        1.5,
        2.5,
        -2.5,
        2_147_483_648.0,
        -2_147_483_904.0,
        4_294_967_296.0,
        9_223_372_036_854_775_808.0,
        18_446_744_073_709_551_616.0,
        f32::MAX,
        f32::MIN_POSITIVE,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ];
    let nans = [
        f32::NAN.to_bits(),
        (-f32::NAN).to_bits(),
        f32::INFINITY.to_bits() | SIGNALING_NAN_PAYLOAD,
        f32::NAN.to_bits() | SIGNALING_NAN_PAYLOAD,
    ];
    finite.map(f32::to_bits).into_iter().chain(nans).collect()
}

fn pairs<T: Copy>(values: &[T]) -> impl Iterator<Item = (T, T)> {
    values
        .iter()
        .flat_map(move |left| values.iter().map(move |right| (*left, *right)))
}

fn every_binary() -> impl Iterator<Item = Binary> {
    (0..).map_while(Binary::from_tag)
}

fn every_unary() -> impl Iterator<Item = Unary> {
    (0..).map_while(Unary::from_tag)
}

fn every_conversion() -> impl Iterator<Item = Conversion> {
    (0..).map_while(Conversion::from_tag)
}

#[test]
fn the_f64_rules_of_the_stage_are_the_declared_rules() {
    for operator in every_binary() {
        for (left, right) in pairs(&f64_operands()) {
            let (a, b) = (f64::from_bits(left), f64::from_bits(right));
            assert_eq!(
                operator.evaluate(a, b),
                operator.apply(a, b),
                "{operator:?} {left:#x} {right:#x}"
            );
        }
    }
    for operator in every_unary() {
        for value in f64_operands() {
            let a = f64::from_bits(value);
            assert_eq!(
                operator.evaluate(a),
                operator.apply(a),
                "{operator:?} {value:#x}"
            );
        }
    }
}

#[test]
fn the_f32_rules_of_the_stage_are_the_declared_rules() {
    for operator in (0..).map_while(F32BinaryOperator::from_tag) {
        for (left, right) in pairs(&f32_operands()) {
            let (a, b) = (f32::from_bits(left), f32::from_bits(right));
            assert_eq!(
                operator.evaluate(a, b),
                operator.apply(a, b),
                "{operator:?} {left:#x} {right:#x}"
            );
        }
    }
    for operator in (0..).map_while(F32UnaryOperator::from_tag) {
        for value in f32_operands() {
            let a = f32::from_bits(value);
            assert_eq!(
                operator.evaluate(a),
                operator.apply(a),
                "{operator:?} {value:#x}"
            );
        }
    }
}

#[test]
fn a_narrow_float_operator_is_its_wide_namesake() {
    for operator in (0..).map_while(F32BinaryOperator::from_tag) {
        assert_eq!(format!("{operator:?}"), format!("{:?}", operator.widen()));
        assert_eq!(operator as u32, operator.widen() as u32);
    }
    for operator in (0..).map_while(F32UnaryOperator::from_tag) {
        assert_eq!(format!("{operator:?}"), format!("{:?}", operator.widen()));
        assert_eq!(operator as u32, operator.widen() as u32);
    }
}

#[test]
fn a_float_comparison_is_what_the_declaration_says_yields_an_i32() {
    for operator in every_binary() {
        let declared = operator.apply(1.0, 1.0).ty();
        assert_eq!(
            is_float_comparison(operator),
            declared == WasmType::I32,
            "{operator:?}"
        );
    }
}

#[test]
fn the_conversions_of_the_stage_are_the_declared_conversions() {
    for operator in every_conversion() {
        for value in operands(operator.source_type()) {
            assert_eq!(
                operator.evaluate(value),
                operator.apply(value),
                "{operator:?} {value:?}"
            );
        }
    }
}

fn operands(ty: WasmType) -> Vec<WasmValue> {
    match ty {
        WasmType::I32 => [0, 1, -1, i32::MAX, i32::MIN].map(WasmValue::I32).to_vec(),
        WasmType::I64 => [0, 1, -1, i64::MAX, i64::MIN].map(WasmValue::I64).to_vec(),
        WasmType::F32 => f32_operands().into_iter().map(WasmValue::F32).collect(),
        WasmType::F64 => f64_operands().into_iter().map(WasmValue::F64).collect(),
        _ => unreachable!("a scalar conversion reads a number"),
    }
}

#[test]
fn only_a_truncation_to_integer_traps() {
    for operator in every_conversion() {
        let traps = operands(operator.source_type())
            .into_iter()
            .any(|value| operator.apply(value).is_err());
        assert_eq!(operator.truncation().is_some(), traps, "{operator:?}");
    }
}

#[test]
fn a_truncation_traps_on_a_nan_then_outside_the_range() {
    let nan = WasmValue::F64(f64::NAN.to_bits());
    assert_eq!(
        Conversion::TruncateF64SignedI32.evaluate(nan),
        Err(WasmTrap::InvalidConversionToInteger)
    );
    let past = WasmValue::F32(2_147_483_648.0_f32.to_bits());
    assert_eq!(
        Conversion::TruncateF32SignedI32.evaluate(past),
        Err(WasmTrap::IntegerOverflow)
    );
    let almost = WasmValue::F64((-1.0_f64 + f64::EPSILON).to_bits());
    assert_eq!(
        Conversion::TruncateF64UnsignedI32.evaluate(almost),
        Ok(WasmValue::I32(0))
    );
}

fn probed_binary(operator: Binary, float: Float) -> Vec<String> {
    let mut probe = Probe::default();
    float_binary(&mut probe, operator, float, 0, 0);
    probe.log
}

#[test]
fn an_extremum_replaces_the_machine_result_where_an_operand_is_a_nan() {
    for (float, operator) in [Float::F32, Float::F64]
        .into_iter()
        .flat_map(|float| [Binary::Minimum, Binary::Maximum].map(move |operator| (float, operator)))
    {
        assert_eq!(
            probed_binary(operator, float),
            [
                format!("{operator:?} {float:?}"),
                format!("Unordered {float:?}"),
                format!("constant {float:?} {:#x}", float.canonical_nan()),
                "select".to_owned(),
            ]
        );
    }
}

#[test]
fn the_other_float_operators_ask_for_nothing_else() {
    for operator in
        every_binary().filter(|operator| !matches!(operator, Binary::Minimum | Binary::Maximum))
    {
        assert_eq!(
            probed_binary(operator, Float::F64),
            [format!("{operator:?} F64")]
        );
    }
}

fn probed_conversion(operator: Conversion) -> Vec<String> {
    let mut probe = Probe::default();
    convert(&mut probe, operator, 0).expect("the probe never traps");
    probe.log
}

#[test]
fn a_truncation_checks_nan_then_range_then_converts() {
    let truncation = Conversion::TruncateF32UnsignedI64
        .truncation()
        .expect("a truncation");
    assert_eq!(
        probed_conversion(Conversion::TruncateF32UnsignedI64),
        [
            "Unordered F32".to_owned(),
            "trap InvalidConversionToInteger".into(),
            format!("overflows {truncation:?}"),
            "trap IntegerOverflow".into(),
            "TruncateF32UnsignedI64".into(),
        ]
    );
}

#[test]
fn the_other_conversions_ask_for_nothing_else() {
    for operator in every_conversion().filter(|operator| operator.truncation().is_none()) {
        assert_eq!(probed_conversion(operator), [format!("{operator:?}")]);
    }
}

#[test]
fn the_table_names_every_float_and_conversion_operator() {
    for operator in (0..).map_while(F32BinaryOperator::from_tag) {
        assert_eq!(
            wasm_operation_of(Op::WasmF32Binary, operator as u32),
            Some(WasmOperation::FloatBinary {
                operator: operator.widen(),
                float: Float::F32
            })
        );
    }
    for operator in every_binary() {
        let operation =
            wasm_operation_of(Op::WasmF64Binary, operator as u32).expect("a float operator");
        assert_eq!(
            operation,
            WasmOperation::FloatBinary {
                operator,
                float: Float::F64
            }
        );
        assert_eq!(operation.result_type(), operator.apply(0.0, 0.0).ty());
        assert!(operation.writes() && !operation.jumps());
    }
    for operator in every_unary() {
        let operation =
            wasm_operation_of(Op::WasmF64Unary, operator as u32).expect("a float operator");
        assert_eq!(operation.result_type(), operator.apply(0.0).ty());
    }
    for operator in every_conversion() {
        let operation =
            wasm_operation_of(Op::WasmScalarConvert, operator as u32).expect("a conversion");
        assert_eq!(operation, WasmOperation::Convert { operator });
        assert_eq!(operation.operand_type(), operator.source_type());
        assert_eq!(operation.result_type(), operator.result_type());
    }
    assert_eq!(wasm_operation_of(Op::WasmF32Binary, u32::MAX), None);
    assert_eq!(wasm_operation_of(Op::WasmF64Unary, u32::MAX), None);
    assert_eq!(wasm_operation_of(Op::WasmScalarConvert, u32::MAX), None);
}

#[test]
fn the_concrete_words_of_a_float_are_its_ieee_bits() {
    let one = 1.0_f32.to_bits();
    let word = ConcreteWasm.operate_float(Binary::Add, Float::F32, u64::from(one), u64::from(one));
    assert_eq!(word, u64::from(2.0_f32.to_bits()));
}
