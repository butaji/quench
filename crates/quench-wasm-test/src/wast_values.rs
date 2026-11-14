//! A wast constant as a Wasm value, and a returned value against a wast result pattern.

use quench_runtime::WasmValue;
use wast::core::{WastArgCore, WastRetCore};
use wast::WastArg;

fn scalar_arg(arg: &WastArg<'_>) -> Option<WasmValue> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Some(WasmValue::I32(*v)),
        WastArg::Core(WastArgCore::I64(v)) => Some(WasmValue::I64(*v)),
        WastArg::Core(WastArgCore::F32(v)) => Some(WasmValue::F32(v.bits)),
        WastArg::Core(WastArgCore::F64(v)) => Some(WasmValue::F64(v.bits)),
        _ => None,
    }
}

pub(crate) fn shared_arg(arg: &WastArg<'_>) -> Option<WasmValue> {
    use wast::core::{AbstractHeapType, HeapType};
    scalar_arg(arg).or_else(|| match arg {
        WastArg::Core(WastArgCore::V128(value)) => Some(WasmValue::V128(v128_bits(value))),
        WastArg::Core(WastArgCore::RefNull(HeapType::Abstract {
            shared: false,
            ty: AbstractHeapType::Func | AbstractHeapType::NoFunc,
        })) => Some(WasmValue::FuncRef(quench_runtime::Value::NULL)),
        WastArg::Core(WastArgCore::RefNull(HeapType::Abstract {
            shared: false,
            ty: AbstractHeapType::Extern | AbstractHeapType::NoExtern,
        })) => Some(WasmValue::ExternRef(quench_runtime::Value::NULL)),
        WastArg::Core(WastArgCore::RefNull(HeapType::Abstract {
            shared: false,
            ty:
                AbstractHeapType::Any
                | AbstractHeapType::Eq
                | AbstractHeapType::Struct
                | AbstractHeapType::Array
                | AbstractHeapType::I31
                | AbstractHeapType::None,
        })) => Some(WasmValue::GcRef(quench_runtime::Value::NULL)),
        _ => None,
    })
}

fn v128_bits(value: &wast::core::V128Const) -> u128 {
    u128::from_le_bytes(value.to_le_bytes())
}

fn v128_matches(pattern: &wast::core::V128Pattern, bits: u128) -> bool {
    let b = bits.to_le_bytes();
    match pattern {
        wast::core::V128Pattern::I8x16(vals) => {
            vals.iter().enumerate().all(|(i, lane)| *lane == b[i] as i8)
        }
        wast::core::V128Pattern::I16x8(vals) => vals
            .iter()
            .enumerate()
            .all(|(i, lane)| *lane == i16::from_le_bytes([b[i * 2], b[i * 2 + 1]])),
        wast::core::V128Pattern::I32x4(vals) => vals
            .iter()
            .enumerate()
            .all(|(i, lane)| *lane == i32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())),
        wast::core::V128Pattern::I64x2(vals) => vals
            .iter()
            .enumerate()
            .all(|(i, lane)| *lane == i64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap())),
        wast::core::V128Pattern::F32x4(vals) => vals.iter().enumerate().all(|(i, lane)| {
            let got = u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
            nan_f32(lane, got)
        }),
        wast::core::V128Pattern::F64x2(vals) => vals.iter().enumerate().all(|(i, lane)| {
            let got = u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
            nan_f64(lane, got)
        }),
    }
}

pub(crate) fn scalar_matches(want: &WastRetCore<'_>, got: WasmValue) -> bool {
    match (want, got) {
        (WastRetCore::Either(options), got) => options.iter().any(|v| scalar_matches(v, got)),
        (WastRetCore::V128(want), WasmValue::V128(got)) => v128_matches(want, got),
        (WastRetCore::I32(want), WasmValue::I32(got)) => *want == got,
        (WastRetCore::I64(want), WasmValue::I64(got)) => *want == got,
        (WastRetCore::F32(want), WasmValue::F32(got)) => nan_f32(want, got),
        (WastRetCore::F64(want), WasmValue::F64(got)) => nan_f64(want, got),
        (WastRetCore::RefNull(_) | WastRetCore::RefExtern(_) | WastRetCore::RefFunc(_), got) => reference_matches(want, got),
        _ => false,
    }
}

/// A returned reference against a null, extern or func reference pattern.
fn reference_matches(want: &WastRetCore<'_>, got: WasmValue) -> bool {
    match (want, got) {
        (WastRetCore::RefNull(_), WasmValue::FuncRef(value) | WasmValue::ExternRef(value) | WasmValue::GcRef(value)) => value.is_null(),
        (WastRetCore::RefExtern(None), WasmValue::ExternRef(value)) => !value.is_null(),
        (WastRetCore::RefExtern(Some(id)), WasmValue::ExternRef(value)) => value.as_number() == Some(f64::from(*id)),
        (WastRetCore::RefFunc(None), WasmValue::FuncRef(value)) => !value.is_null(),
        _ => false,
    }
}

fn nan_f32(pattern: &wast::core::NanPattern<wast::token::F32>, bits: u32) -> bool {
    match pattern {
        wast::core::NanPattern::CanonicalNan => WasmValue::F32(bits).is_canonical_nan(),
        wast::core::NanPattern::ArithmeticNan => WasmValue::F32(bits).is_arithmetic_nan(),
        wast::core::NanPattern::Value(v) => v.bits == bits,
    }
}

fn nan_f64(pattern: &wast::core::NanPattern<wast::token::F64>, bits: u64) -> bool {
    match pattern {
        wast::core::NanPattern::CanonicalNan => WasmValue::F64(bits).is_canonical_nan(),
        wast::core::NanPattern::ArithmeticNan => WasmValue::F64(bits).is_arithmetic_nan(),
        wast::core::NanPattern::Value(v) => v.bits == bits,
    }
}
