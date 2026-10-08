use super::{WasmType, WasmValue};

const OPERATORS: &[&str] = &[
    "V128Load",
    "V128Load8x8S",
    "V128Load8x8U",
    "V128Load16x4S",
    "V128Load16x4U",
    "V128Load32x2S",
    "V128Load32x2U",
    "V128Load8Splat",
    "V128Load16Splat",
    "V128Load32Splat",
    "V128Load64Splat",
    "V128Load32Zero",
    "V128Load64Zero",
    "V128Store",
    "V128Load8Lane",
    "V128Load16Lane",
    "V128Load32Lane",
    "V128Load64Lane",
    "V128Store8Lane",
    "V128Store16Lane",
    "V128Store32Lane",
    "V128Store64Lane",
    "V128Const",
    "V128Not",
    "V128And",
    "V128AndNot",
    "V128Or",
    "V128Xor",
    "V128Bitselect",
    "V128AnyTrue",
    "I8x16Shuffle",
    "I8x16Swizzle",
    "I8x16Splat",
    "I8x16ExtractLaneS",
    "I8x16ExtractLaneU",
    "I8x16ReplaceLane",
    "I8x16Eq",
    "I8x16Ne",
    "I8x16LtS",
    "I8x16LtU",
    "I8x16GtS",
    "I8x16GtU",
    "I8x16LeS",
    "I8x16LeU",
    "I8x16GeS",
    "I8x16GeU",
    "I8x16Abs",
    "I8x16Neg",
    "I8x16Popcnt",
    "I8x16AllTrue",
    "I8x16Bitmask",
    "I8x16Shl",
    "I8x16ShrS",
    "I8x16ShrU",
    "I8x16Add",
    "I8x16AddSatS",
    "I8x16AddSatU",
    "I8x16Sub",
    "I8x16SubSatS",
    "I8x16SubSatU",
    "I8x16MinS",
    "I8x16MinU",
    "I8x16MaxS",
    "I8x16MaxU",
    "I8x16AvgrU",
    "I8x16NarrowI16x8S",
    "I8x16NarrowI16x8U",
    "I16x8Splat",
    "I16x8ExtractLaneS",
    "I16x8ExtractLaneU",
    "I16x8ReplaceLane",
    "I16x8Eq",
    "I16x8Ne",
    "I16x8LtS",
    "I16x8LtU",
    "I16x8GtS",
    "I16x8GtU",
    "I16x8LeS",
    "I16x8LeU",
    "I16x8GeS",
    "I16x8GeU",
    "I16x8Abs",
    "I16x8Neg",
    "I16x8AllTrue",
    "I16x8Bitmask",
    "I16x8Shl",
    "I16x8ShrS",
    "I16x8ShrU",
    "I16x8Add",
    "I16x8AddSatS",
    "I16x8AddSatU",
    "I16x8Sub",
    "I16x8SubSatS",
    "I16x8SubSatU",
    "I16x8Mul",
    "I16x8MinS",
    "I16x8MinU",
    "I16x8MaxS",
    "I16x8MaxU",
    "I16x8AvgrU",
    "I16x8NarrowI32x4S",
    "I16x8NarrowI32x4U",
    "I16x8ExtAddPairwiseI8x16S",
    "I16x8ExtAddPairwiseI8x16U",
    "I16x8Q15MulrSatS",
    "I16x8ExtMulLowI8x16S",
    "I16x8ExtMulHighI8x16S",
    "I16x8ExtMulLowI8x16U",
    "I16x8ExtMulHighI8x16U",
    "I16x8ExtendLowI8x16S",
    "I16x8ExtendHighI8x16S",
    "I16x8ExtendLowI8x16U",
    "I16x8ExtendHighI8x16U",
    "I32x4Splat",
    "I32x4ExtractLane",
    "I32x4ReplaceLane",
    "I32x4Eq",
    "I32x4Ne",
    "I32x4LtS",
    "I32x4LtU",
    "I32x4GtS",
    "I32x4GtU",
    "I32x4LeS",
    "I32x4LeU",
    "I32x4GeS",
    "I32x4GeU",
    "I32x4Abs",
    "I32x4Neg",
    "I32x4AllTrue",
    "I32x4Bitmask",
    "I32x4Shl",
    "I32x4ShrS",
    "I32x4ShrU",
    "I32x4Add",
    "I32x4Sub",
    "I32x4Mul",
    "I32x4MinS",
    "I32x4MinU",
    "I32x4MaxS",
    "I32x4MaxU",
    "I32x4ExtendLowI16x8S",
    "I32x4ExtendHighI16x8S",
    "I32x4ExtendLowI16x8U",
    "I32x4ExtendHighI16x8U",
    "I32x4ExtAddPairwiseI16x8S",
    "I32x4ExtAddPairwiseI16x8U",
    "I32x4DotI16x8S",
    "I32x4ExtMulLowI16x8S",
    "I32x4ExtMulHighI16x8S",
    "I32x4ExtMulLowI16x8U",
    "I32x4ExtMulHighI16x8U",
    "I64x2Splat",
    "I64x2ExtractLane",
    "I64x2ReplaceLane",
    "I64x2Eq",
    "I64x2Ne",
    "I64x2LtS",
    "I64x2GtS",
    "I64x2LeS",
    "I64x2GeS",
    "I64x2Abs",
    "I64x2Neg",
    "I64x2AllTrue",
    "I64x2Bitmask",
    "I64x2Shl",
    "I64x2ShrS",
    "I64x2ShrU",
    "I64x2Add",
    "I64x2Sub",
    "I64x2Mul",
    "I64x2ExtendLowI32x4S",
    "I64x2ExtendHighI32x4S",
    "I64x2ExtendLowI32x4U",
    "I64x2ExtendHighI32x4U",
    "I64x2ExtMulLowI32x4S",
    "I64x2ExtMulHighI32x4S",
    "I64x2ExtMulLowI32x4U",
    "I64x2ExtMulHighI32x4U",
    "F32x4Splat",
    "F32x4ExtractLane",
    "F32x4ReplaceLane",
    "F32x4Eq",
    "F32x4Ne",
    "F32x4Lt",
    "F32x4Gt",
    "F32x4Le",
    "F32x4Ge",
    "F32x4Ceil",
    "F32x4Floor",
    "F32x4Trunc",
    "F32x4Nearest",
    "F32x4Abs",
    "F32x4Neg",
    "F32x4Sqrt",
    "F32x4Add",
    "F32x4Sub",
    "F32x4Mul",
    "F32x4Div",
    "F32x4Min",
    "F32x4Max",
    "F32x4PMin",
    "F32x4PMax",
    "F64x2Splat",
    "F64x2ExtractLane",
    "F64x2ReplaceLane",
    "F64x2Eq",
    "F64x2Ne",
    "F64x2Lt",
    "F64x2Gt",
    "F64x2Le",
    "F64x2Ge",
    "F64x2Ceil",
    "F64x2Floor",
    "F64x2Trunc",
    "F64x2Nearest",
    "F64x2Abs",
    "F64x2Neg",
    "F64x2Sqrt",
    "F64x2Add",
    "F64x2Sub",
    "F64x2Mul",
    "F64x2Div",
    "F64x2Min",
    "F64x2Max",
    "F64x2PMin",
    "F64x2PMax",
    "F32x4ConvertI32x4S",
    "F32x4ConvertI32x4U",
    "I32x4TruncSatF32x4S",
    "I32x4TruncSatF32x4U",
    "I32x4TruncSatF64x2SZero",
    "I32x4TruncSatF64x2UZero",
    "F64x2ConvertLowI32x4S",
    "F64x2ConvertLowI32x4U",
    "F32x4DemoteF64x2Zero",
    "F64x2PromoteLowF32x4",
    "I16x8RelaxedQ15mulrS",
    "I8x16RelaxedSwizzle",
    "I16x8RelaxedDotI8x16I7x16S",
    "I32x4RelaxedDotI8x16I7x16AddS",
    "I8x16RelaxedLaneselect",
    "I16x8RelaxedLaneselect",
    "I32x4RelaxedLaneselect",
    "I64x2RelaxedLaneselect",
    "F32x4RelaxedMadd",
    "F32x4RelaxedNmadd",
    "F64x2RelaxedMadd",
    "F64x2RelaxedNmadd",
    "F32x4RelaxedMin",
    "F32x4RelaxedMax",
    "F64x2RelaxedMin",
    "F64x2RelaxedMax",
];

pub(crate) fn tag(name: &str) -> Option<u16> {
    OPERATORS
        .iter()
        .position(|candidate| *candidate == name)
        .and_then(|index| u16::try_from(index).ok())
}

pub(crate) fn name(tag: u16) -> Option<&'static str> {
    OPERATORS.get(usize::from(tag)).copied()
}

pub(crate) fn arity(name: &str) -> usize {
    if name == "V128Const" {
        0
    } else if matches!(name, "V128Not" | "V128AnyTrue")
        || name.ends_with("Abs")
        || name.ends_with("Neg")
        || name.ends_with("Popcnt")
        || name.ends_with("AllTrue")
        || name.ends_with("Bitmask")
        || name.ends_with("Ceil")
        || name.ends_with("Floor")
        || name.ends_with("Trunc")
        || name.ends_with("Nearest")
        || name.ends_with("Sqrt")
        || name.ends_with("Splat")
        || name.contains("ExtractLane")
        || name.contains("ConvertI32x4")
        || name.contains("ConvertLowI32x4")
        || name.contains("TruncSat")
        || name.contains("Demote")
        || name.contains("Promote")
        || name.contains("ExtendLow")
        || name.contains("ExtendHigh")
        || name.contains("ExtAddPairwise")
    {
        1
    } else if name == "V128Bitselect"
        || name.contains("RelaxedMadd")
        || name.contains("RelaxedNmadd")
        || name.contains("RelaxedLaneselect")
        || name == "I32x4RelaxedDotI8x16I7x16AddS"
    {
        3
    } else if name.contains("ReplaceLane") {
        2
    } else {
        2
    }
}

pub(crate) fn result_type(name: &str) -> WasmType {
    if matches!(name, "V128AnyTrue")
        || name.ends_with("AllTrue")
        || name.ends_with("Bitmask")
        || name.contains("I8x16ExtractLane")
        || name.contains("I16x8ExtractLane")
        || name.contains("I32x4ExtractLane")
    {
        WasmType::I32
    } else if name.contains("I64x2ExtractLane") {
        WasmType::I64
    } else if name.contains("F32x4ExtractLane") {
        WasmType::F32
    } else if name.contains("F64x2ExtractLane") {
        WasmType::F64
    } else {
        WasmType::V128
    }
}

pub(crate) fn apply(
    name: &str,
    lane: Option<u8>,
    shuffle: Option<[u8; 16]>,
    a: u128,
    b: u128,
    c: u128,
    scalar: Option<WasmValue>,
) -> Option<WasmValue> {
    match name {
        "I16x8RelaxedQ15mulrS" => {
            return apply("I16x8Q15MulrSatS", lane, shuffle, a, b, c, scalar);
        }
        "I8x16RelaxedSwizzle" => {
            return apply("I8x16Swizzle", lane, shuffle, a, b, c, scalar);
        }
        "I8x16RelaxedLaneselect"
        | "I16x8RelaxedLaneselect"
        | "I32x4RelaxedLaneselect"
        | "I64x2RelaxedLaneselect" => {
            return Some(WasmValue::V128((a & c) | (b & !c)));
        }
        "F32x4RelaxedMin" => return apply_float("F32x4Min", 32, 4, a, b),
        "F32x4RelaxedMax" => return apply_float("F32x4Max", 32, 4, a, b),
        "F64x2RelaxedMin" => return apply_float("F64x2Min", 64, 2, a, b),
        "F64x2RelaxedMax" => return apply_float("F64x2Max", 64, 2, a, b),
        "F32x4RelaxedMadd" | "F32x4RelaxedNmadd" => {
            return apply_relaxed_madd_f32(name, a, b, c);
        }
        "F64x2RelaxedMadd" | "F64x2RelaxedNmadd" => {
            return apply_relaxed_madd_f64(name, a, b, c);
        }
        "I16x8RelaxedDotI8x16I7x16S" => {
            let mut result = 0u128;
            for index in 0..8 {
                let lane = index * 2;
                let left0 = sign_extend(lane_get(a, 8, lane)?, 8) as i32;
                let left1 = sign_extend(lane_get(a, 8, lane + 1)?, 8) as i32;
                let right0 = sign_extend(lane_get(b, 8, lane)?, 8) as i32;
                let right1 = sign_extend(lane_get(b, 8, lane + 1)?, 8) as i32;
                result = lane_set(
                    result,
                    16,
                    index,
                    (left0 * right0 + left1 * right1) as u16 as u64,
                )?;
            }
            return Some(WasmValue::V128(result));
        }
        "I32x4RelaxedDotI8x16I7x16AddS" => {
            let mut result = 0u128;
            for index in 0..4 {
                let mut sum = 0i32;
                for offset in 0..4 {
                    let lane = index * 4 + offset;
                    let left = sign_extend(lane_get(a, 8, lane)?, 8) as i32;
                    let right = sign_extend(lane_get(b, 8, lane)?, 8) as i32;
                    sum = sum.wrapping_add(left.wrapping_mul(right));
                }
                sum = sum.wrapping_add(lane_get(c, 32, index)? as u32 as i32);
                result = lane_set(result, 32, index, sum as u32 as u64)?;
            }
            return Some(WasmValue::V128(result));
        }
        _ => {}
    }
    if name == "V128Const" {
        return Some(WasmValue::V128(0));
    }
    if name.contains("NarrowI16x8") || name.contains("NarrowI32x4") {
        let (source_width, signed, output_width) = if name.starts_with("I8x16") {
            (16, name.ends_with('S'), 8)
        } else {
            (32, name.ends_with('S'), 16)
        };
        let mut result = 0u128;
        for index in 0..(128 / output_width) {
            let source = if index < 128 / source_width {
                lane_get(a, source_width, index)?
            } else {
                lane_get(b, source_width, index - 128 / source_width)?
            };
            let narrowed = if signed {
                let value = sign_extend(source, source_width);
                let min = -(1i64 << (output_width - 1));
                let max = (1i64 << (output_width - 1)) - 1;
                value.clamp(min, max) as u64
            } else {
                sign_extend(source, source_width)
                    .max(0)
                    .min(((1u64 << output_width) - 1) as i64) as u64
            };
            result = lane_set(result, output_width, index, narrowed)?;
        }
        return Some(WasmValue::V128(result));
    }
    if name.contains("ExtendLow") || name.contains("ExtendHigh") {
        let output_width = lane_width(name)?;
        let source_width = output_width / 2;
        let source_count = 128 / source_width;
        let low_or_high = usize::from(name.contains("ExtendHigh")) * (source_count / 2);
        let signed = name.ends_with('S');
        let mut result = 0u128;
        for index in 0..128 / output_width {
            let source = lane_get(a, source_width, low_or_high + index)?;
            let widened = if signed {
                sign_extend(source, source_width) as u64
            } else {
                source
            };
            result = lane_set(result, output_width, index, widened)?;
        }
        return Some(WasmValue::V128(result));
    }
    if name.contains("ExtAddPairwise") {
        let output_width = lane_width(name)?;
        let input_width = output_width / 2;
        let signed = name.ends_with('S');
        let mut result = 0u128;
        for index in 0..128 / output_width {
            let left = lane_get(a, input_width, index * 2)?;
            let right = lane_get(a, input_width, index * 2 + 1)?;
            let sum = if signed {
                sign_extend(left, input_width) + sign_extend(right, input_width)
            } else {
                (left + right) as i64
            } as u64;
            result = lane_set(result, output_width, index, sum)?;
        }
        return Some(WasmValue::V128(result));
    }
    if name.contains("ExtMulLow") || name.contains("ExtMulHigh") {
        let output_width = lane_width(name)?;
        let input_width = output_width / 2;
        let input_count = 128 / input_width;
        let start = usize::from(name.contains("ExtMulHigh")) * (input_count / 2);
        let signed = name.ends_with('S');
        let mut result = 0u128;
        for index in 0..128 / output_width {
            let left = lane_get(a, input_width, start + index)?;
            let right = lane_get(b, input_width, start + index)?;
            let product = if signed {
                (sign_extend(left, input_width) as i128)
                    .wrapping_mul(sign_extend(right, input_width) as i128) as u64
            } else {
                left.wrapping_mul(right)
            };
            result = lane_set(result, output_width, index, product)?;
        }
        return Some(WasmValue::V128(result));
    }
    if name == "I16x8Q15MulrSatS" {
        let mut result = 0u128;
        for index in 0..8 {
            let left = sign_extend(lane_get(a, 16, index)?, 16) as i32;
            let right = sign_extend(lane_get(b, 16, index)?, 16) as i32;
            let value = ((left * right + 0x4000) >> 15).clamp(i16::MIN as i32, i16::MAX as i32);
            result = lane_set(result, 16, index, value as u32 as u64)?;
        }
        return Some(WasmValue::V128(result));
    }
    if name == "I32x4DotI16x8S" {
        let mut result = 0u128;
        for index in 0..4 {
            let lane = index * 2;
            let left0 = sign_extend(lane_get(a, 16, lane)?, 16) as i32;
            let left1 = sign_extend(lane_get(a, 16, lane + 1)?, 16) as i32;
            let right0 = sign_extend(lane_get(b, 16, lane)?, 16) as i32;
            let right1 = sign_extend(lane_get(b, 16, lane + 1)?, 16) as i32;
            let value = left0 * right0 + left1 * right1;
            result = lane_set(result, 32, index, value as u32 as u64)?;
        }
        return Some(WasmValue::V128(result));
    }
    if name == "V128Not" {
        return Some(WasmValue::V128(!a));
    }
    if name == "V128And" {
        return Some(WasmValue::V128(a & b));
    }
    if name == "V128AndNot" {
        return Some(WasmValue::V128(a & !b));
    }
    if name == "V128Or" {
        return Some(WasmValue::V128(a | b));
    }
    if name == "V128Xor" {
        return Some(WasmValue::V128(a ^ b));
    }
    if name == "V128Bitselect" {
        return Some(WasmValue::V128((a & c) | (b & !c)));
    }
    if name == "V128AnyTrue" {
        return Some(WasmValue::I32(i32::from(a != 0)));
    }
    if name == "I8x16Shuffle" || name == "I8x16Swizzle" {
        let mut result = 0u128;
        for index in 0..16 {
            let source_index = if name == "I8x16Shuffle" {
                usize::from(*shuffle?.get(index)?)
            } else {
                usize::from((b >> (index * 8)) as u8)
            };
            let value = if source_index < 16 {
                (a >> (source_index * 8)) as u8
            } else if name == "I8x16Shuffle" && source_index < 32 {
                (b >> ((source_index - 16) * 8)) as u8
            } else {
                0
            };
            result |= u128::from(value) << (index * 8);
        }
        return Some(WasmValue::V128(result));
    }
    if name == "F32x4ConvertI32x4S" || name == "F32x4ConvertI32x4U" {
        let mut result = 0u128;
        for index in 0..4 {
            let bits = lane_get(a, 32, index)? as u32;
            let value = if name.ends_with('S') {
                (bits as i32) as f32
            } else {
                bits as f32
            };
            result = lane_set(result, 32, index, u64::from(value.to_bits()))?;
        }
        return Some(WasmValue::V128(result));
    }
    if name.starts_with("I32x4TruncSatF32x4") || name.starts_with("I32x4TruncSatF64x2") {
        let is_f32 = name.contains("F32x4");
        let is_signed = name.ends_with('S') || name.ends_with("SZero");
        let lanes = if is_f32 { 4 } else { 2 };
        let mut result = 0u128;
        for index in 0..lanes {
            let value = if is_f32 {
                f64::from(f32::from_bits(lane_get(a, 32, index)? as u32))
            } else {
                f64::from_bits(lane_get(a, 64, index)?)
            };
            result = lane_set(result, 32, index, trunc_sat(value, is_signed, 32))?;
        }
        return Some(WasmValue::V128(result));
    }
    if name == "F64x2ConvertLowI32x4S" || name == "F64x2ConvertLowI32x4U" {
        let mut result = 0u128;
        for index in 0..2 {
            let bits = lane_get(a, 32, index)? as u32;
            let value = if name.ends_with('S') {
                (bits as i32) as f64
            } else {
                bits as f64
            };
            result = lane_set(result, 64, index, value.to_bits())?;
        }
        return Some(WasmValue::V128(result));
    }
    if name == "F32x4DemoteF64x2Zero" {
        let mut result = 0u128;
        for index in 0..2 {
            let value = f64::from_bits(lane_get(a, 64, index)?);
            result = lane_set(result, 32, index, u64::from((value as f32).to_bits()))?;
        }
        return Some(WasmValue::V128(result));
    }
    if name == "F64x2PromoteLowF32x4" {
        let mut result = 0u128;
        for index in 0..2 {
            let value = f32::from_bits(lane_get(a, 32, index)? as u32) as f64;
            result = lane_set(result, 64, index, value.to_bits())?;
        }
        return Some(WasmValue::V128(result));
    }
    if name.ends_with("Splat") {
        let width = lane_width(name)?;
        let scalar = scalar?;
        let bits = match (width, scalar) {
            (8 | 16 | 32, WasmValue::I32(value)) => value as u32 as u64,
            (64, WasmValue::I64(value)) => value as u64,
            (32, WasmValue::F32(bits)) => u64::from(bits),
            (64, WasmValue::F64(bits)) => bits,
            _ => return None,
        };
        return Some(WasmValue::V128(splat(bits, width)));
    }
    if name.contains("ExtractLane") {
        let width = lane_width(name)?;
        let index = usize::from(lane?);
        let bits = lane_get(a, width, index)?;
        return Some(match name {
            "I8x16ExtractLaneS" => WasmValue::I32((bits as u8 as i8) as i32),
            "I8x16ExtractLaneU" => WasmValue::I32(bits as u8 as i32),
            "I16x8ExtractLaneS" => WasmValue::I32((bits as u16 as i16) as i32),
            "I16x8ExtractLaneU" => WasmValue::I32(bits as u16 as i32),
            "I32x4ExtractLane" => WasmValue::I32(bits as u32 as i32),
            "I64x2ExtractLane" => WasmValue::I64(bits as i64),
            "F32x4ExtractLane" => WasmValue::F32(bits as u32),
            "F64x2ExtractLane" => WasmValue::F64(bits),
            _ => return None,
        });
    }
    if name.contains("ReplaceLane") {
        let width = lane_width(name)?;
        let index = usize::from(lane?);
        let bits = scalar_bits(scalar?, width)?;
        return Some(WasmValue::V128(lane_set(a, width, index, bits)?));
    }

    let width = lane_width(name)?;
    let count = 128 / width;
    if name.ends_with("AllTrue") {
        return Some(WasmValue::I32(i32::from(
            (0..count).all(|index| lane_get(a, width, index).unwrap_or(0) != 0),
        )));
    }
    if name.ends_with("Bitmask") {
        let mut mask = 0u32;
        for index in 0..count {
            if lane_get(a, width, index)? & (1u64 << (width - 1)) != 0 {
                mask |= 1 << index;
            }
        }
        return Some(WasmValue::I32(mask as i32));
    }

    let float_lanes = name.starts_with("F32") || name.starts_with("F64");
    if float_lanes {
        return apply_float(name, width, count, a, b);
    }
    apply_integer(name, width, count, a, b, scalar)
}

fn apply_relaxed_madd_f32(name: &str, a: u128, b: u128, c: u128) -> Option<WasmValue> {
    let negate_product = name.ends_with("Nmadd");
    let mut result = 0u128;
    for index in 0..4 {
        let left = f32::from_bits(lane_get(a, 32, index)? as u32);
        let right = f32::from_bits(lane_get(b, 32, index)? as u32);
        let addend = f32::from_bits(lane_get(c, 32, index)? as u32);
        let product = left * right;
        let value = if negate_product {
            -product + addend
        } else {
            product + addend
        };
        result = lane_set(result, 32, index, u64::from(value.to_bits()))?;
    }
    Some(WasmValue::V128(result))
}

fn apply_relaxed_madd_f64(name: &str, a: u128, b: u128, c: u128) -> Option<WasmValue> {
    let negate_product = name.ends_with("Nmadd");
    let mut result = 0u128;
    for index in 0..2 {
        let left = f64::from_bits(lane_get(a, 64, index)?);
        let right = f64::from_bits(lane_get(b, 64, index)?);
        let addend = f64::from_bits(lane_get(c, 64, index)?);
        let product = left * right;
        let value = if negate_product {
            -product + addend
        } else {
            product + addend
        };
        result = lane_set(result, 64, index, value.to_bits())?;
    }
    Some(WasmValue::V128(result))
}

pub(crate) fn memory_width(name: &str) -> Option<usize> {
    Some(match name {
        "V128Load" | "V128Store" => 16,
        "V128Load8x8S" | "V128Load8x8U" | "V128Load16x4S" | "V128Load16x4U" | "V128Load32x2S"
        | "V128Load32x2U" => 8,
        "V128Load8Splat" | "V128Load8Lane" | "V128Store8Lane" => 1,
        "V128Load16Splat" | "V128Load16Lane" | "V128Store16Lane" => 2,
        "V128Load32Splat" | "V128Load32Zero" | "V128Load32Lane" | "V128Store32Lane" => 4,
        "V128Load64Splat" | "V128Load64Zero" | "V128Load64Lane" | "V128Store64Lane" => 8,
        _ => return None,
    })
}

pub(crate) fn memory_load(
    name: &str,
    bytes: &[u8],
    original: u128,
    lane: Option<u8>,
) -> Option<u128> {
    let raw = bytes.iter().enumerate().fold(0u64, |raw, (index, byte)| {
        raw | (u64::from(*byte) << (index * 8))
    });
    Some(match name {
        "V128Load" => u128::from_le_bytes(bytes.try_into().ok()?),
        "V128Load8x8S" | "V128Load8x8U" | "V128Load16x4S" | "V128Load16x4U" | "V128Load32x2S"
        | "V128Load32x2U" => {
            let output_width = if name.starts_with("V128Load8x8") {
                16
            } else if name.starts_with("V128Load16x4") {
                32
            } else {
                64
            };
            let input_width = output_width / 2;
            let signed = name.ends_with('S');
            let mut result = 0u128;
            for index in 0..128 / output_width {
                let source = (raw >> (index * input_width))
                    & if input_width == 64 {
                        u64::MAX
                    } else {
                        (1u64 << input_width) - 1
                    };
                let widened = if signed {
                    sign_extend(source, input_width) as u64
                } else {
                    source
                };
                result = lane_set(result, output_width, index, widened)?;
            }
            result
        }
        "V128Load8Splat" | "V128Load16Splat" | "V128Load32Splat" | "V128Load64Splat" => {
            splat(raw, bytes.len() * 8)
        }
        "V128Load32Zero" | "V128Load64Zero" => u128::from(raw),
        "V128Load8Lane" | "V128Load16Lane" | "V128Load32Lane" | "V128Load64Lane" => {
            let width = match name {
                "V128Load8Lane" => 8,
                "V128Load16Lane" => 16,
                "V128Load32Lane" => 32,
                "V128Load64Lane" => 64,
                _ => return None,
            };
            lane_set(original, width, usize::from(lane?), raw)?
        }
        _ => return None,
    })
}

pub(crate) fn memory_store(name: &str, value: u128, lane: Option<u8>) -> Option<(usize, u128)> {
    Some(if name == "V128Store" {
        (16, value)
    } else {
        let width = match name {
            "V128Store8Lane" => 8,
            "V128Store16Lane" => 16,
            "V128Store32Lane" => 32,
            "V128Store64Lane" => 64,
            _ => return None,
        };
        let bits = lane_get(value, width, usize::from(lane?))?;
        (width / 8, u128::from(bits))
    })
}

fn apply_float(name: &str, width: usize, count: usize, a: u128, b: u128) -> Option<WasmValue> {
    let mut result = 0u128;
    for index in 0..count {
        let left = lane_get(a, width, index)?;
        let right = lane_get(b, width, index)?;
        let (raw, comparison) = if width == 32 {
            let x = f32::from_bits(left as u32);
            let y = f32::from_bits(right as u32);
            if is_float_compare(name) {
                (
                    if compare_float(name, x as f64, y as f64)? {
                        u64::from(u32::MAX)
                    } else {
                        0
                    },
                    true,
                )
            } else {
                let value = match name {
                    "F32x4Ceil" => x.ceil(),
                    "F32x4Floor" => x.floor(),
                    "F32x4Trunc" => x.trunc(),
                    "F32x4Nearest" => x.round_ties_even(),
                    "F32x4Abs" => f32::from_bits((left as u32) & 0x7fff_ffff),
                    "F32x4Neg" => f32::from_bits((left as u32) ^ 0x8000_0000),
                    "F32x4Sqrt" => x.sqrt(),
                    "F32x4Add" => x + y,
                    "F32x4Sub" => x - y,
                    "F32x4Mul" => x * y,
                    "F32x4Div" => x / y,
                    "F32x4Min" => {
                        if x.is_nan() || y.is_nan() {
                            f32::NAN
                        } else if x == y && x == 0.0 {
                            f32::from_bits((left as u32) | (right as u32))
                        } else {
                            x.min(y)
                        }
                    }
                    "F32x4Max" => {
                        if x.is_nan() || y.is_nan() {
                            f32::NAN
                        } else if x == y && x == 0.0 {
                            f32::from_bits((left as u32) & (right as u32))
                        } else {
                            x.max(y)
                        }
                    }
                    "F32x4PMin" => {
                        if x.is_nan() || y.is_nan() {
                            x
                        } else if x <= y {
                            x
                        } else {
                            y
                        }
                    }
                    "F32x4PMax" => {
                        if x.is_nan() || y.is_nan() {
                            x
                        } else if x >= y {
                            x
                        } else {
                            y
                        }
                    }
                    _ => return None,
                };
                let is_sign_operation =
                    matches!(name, "F32x4Abs" | "F32x4Neg" | "F32x4PMin" | "F32x4PMax");
                let bits = if value.is_nan() && !is_sign_operation {
                    f32::NAN.to_bits()
                } else {
                    value.to_bits()
                };
                (u64::from(bits), false)
            }
        } else {
            let x = f64::from_bits(left);
            let y = f64::from_bits(right);
            if is_float_compare(name) {
                (
                    if compare_float(name, x, y)? {
                        u64::MAX
                    } else {
                        0
                    },
                    true,
                )
            } else {
                let value = match name {
                    "F64x2Ceil" => x.ceil(),
                    "F64x2Floor" => x.floor(),
                    "F64x2Trunc" => x.trunc(),
                    "F64x2Nearest" => x.round_ties_even(),
                    "F64x2Abs" => f64::from_bits(left & 0x7fff_ffff_ffff_ffff),
                    "F64x2Neg" => f64::from_bits(left ^ 0x8000_0000_0000_0000),
                    "F64x2Sqrt" => x.sqrt(),
                    "F64x2Add" => x + y,
                    "F64x2Sub" => x - y,
                    "F64x2Mul" => x * y,
                    "F64x2Div" => x / y,
                    "F64x2Min" => {
                        if x.is_nan() || y.is_nan() {
                            f64::NAN
                        } else if x == y && x == 0.0 {
                            f64::from_bits(left | right)
                        } else {
                            x.min(y)
                        }
                    }
                    "F64x2Max" => {
                        if x.is_nan() || y.is_nan() {
                            f64::NAN
                        } else if x == y && x == 0.0 {
                            f64::from_bits(left & right)
                        } else {
                            x.max(y)
                        }
                    }
                    "F64x2PMin" => {
                        if x.is_nan() || y.is_nan() {
                            x
                        } else if x <= y {
                            x
                        } else {
                            y
                        }
                    }
                    "F64x2PMax" => {
                        if x.is_nan() || y.is_nan() {
                            x
                        } else if x >= y {
                            x
                        } else {
                            y
                        }
                    }
                    _ => return None,
                };
                let is_sign_operation =
                    matches!(name, "F64x2Abs" | "F64x2Neg" | "F64x2PMin" | "F64x2PMax");
                let bits = if value.is_nan() && !is_sign_operation {
                    f64::NAN.to_bits()
                } else {
                    value.to_bits()
                };
                (bits, false)
            }
        };
        let _ = comparison;
        result = lane_set(result, width, index, raw)?;
    }
    Some(WasmValue::V128(result))
}

fn apply_integer(
    name: &str,
    width: usize,
    count: usize,
    a: u128,
    b: u128,
    scalar: Option<WasmValue>,
) -> Option<WasmValue> {
    let lane_mask = if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    let shift_mask = width - 1;
    let mut result = 0u128;
    for index in 0..count {
        let x = lane_get(a, width, index)?;
        let y = if name.ends_with("Shl") || name.ends_with("ShrS") || name.ends_with("ShrU") {
            match scalar? {
                WasmValue::I32(value) => value as u32 as u64,
                _ => return None,
            }
        } else {
            lane_get(b, width, index)?
        };
        let signed_x = sign_extend(x, width);
        let signed_y = sign_extend(y, width);
        let value = if is_integer_compare(name) {
            if integer_compare(name, x, y, signed_x, signed_y)? {
                lane_mask
            } else {
                0
            }
        } else {
            match name {
                "I8x16Abs" | "I16x8Abs" | "I32x4Abs" | "I64x2Abs" => {
                    signed_x.unsigned_abs() & lane_mask
                }
                "I8x16Neg" | "I16x8Neg" | "I32x4Neg" | "I64x2Neg" => {
                    0u64.wrapping_sub(x) & lane_mask
                }
                "I8x16Popcnt" => u64::from((x as u8).count_ones()),
                "I8x16Shl" | "I16x8Shl" | "I32x4Shl" | "I64x2Shl" => {
                    x.wrapping_shl((y as usize & shift_mask) as u32) & lane_mask
                }
                "I8x16ShrS" | "I16x8ShrS" | "I32x4ShrS" | "I64x2ShrS" => {
                    (signed_x >> (y as usize & shift_mask)) as u64 & lane_mask
                }
                "I8x16ShrU" | "I16x8ShrU" | "I32x4ShrU" | "I64x2ShrU" => {
                    x >> (y as usize & shift_mask)
                }
                "I8x16Add" | "I16x8Add" | "I32x4Add" | "I64x2Add" => x.wrapping_add(y) & lane_mask,
                "I8x16AddSatS" | "I16x8AddSatS" => {
                    saturating_add_signed(signed_x, signed_y, width) as u64 & lane_mask
                }
                "I8x16AddSatU" | "I16x8AddSatU" => x.saturating_add(y).min(lane_mask),
                "I8x16Sub" | "I16x8Sub" | "I32x4Sub" | "I64x2Sub" => x.wrapping_sub(y) & lane_mask,
                "I8x16SubSatS" | "I16x8SubSatS" => {
                    saturating_sub_signed(signed_x, signed_y, width) as u64 & lane_mask
                }
                "I8x16SubSatU" | "I16x8SubSatU" => x.saturating_sub(y),
                "I16x8Mul" | "I32x4Mul" | "I64x2Mul" => x.wrapping_mul(y) & lane_mask,
                "I8x16MinS" | "I16x8MinS" | "I32x4MinS" => {
                    signed_x.min(signed_y) as u64 & lane_mask
                }
                "I8x16MinU" | "I16x8MinU" | "I32x4MinU" => x.min(y),
                "I8x16MaxS" | "I16x8MaxS" | "I32x4MaxS" => {
                    signed_x.max(signed_y) as u64 & lane_mask
                }
                "I8x16MaxU" | "I16x8MaxU" | "I32x4MaxU" => x.max(y),
                "I8x16AvgrU" | "I16x8AvgrU" => (x + y + 1) >> 1,
                _ => return None,
            }
        };
        result = lane_set(result, width, index, value & lane_mask)?;
    }
    Some(WasmValue::V128(result))
}

fn lane_width(name: &str) -> Option<usize> {
    if name.starts_with("I8x16") {
        Some(8)
    } else if name.starts_with("I16x8") {
        Some(16)
    } else if name.starts_with("I32x4") || name.starts_with("F32x4") {
        Some(32)
    } else if name.starts_with("I64x2") || name.starts_with("F64x2") {
        Some(64)
    } else {
        None
    }
}

fn lane_get(vector: u128, width: usize, index: usize) -> Option<u64> {
    (index < 128 / width).then(|| {
        ((vector >> (index * width)) as u64)
            & if width == 64 {
                u64::MAX
            } else {
                (1u64 << width) - 1
            }
    })
}

fn lane_set(vector: u128, width: usize, index: usize, value: u64) -> Option<u128> {
    if index >= 128 / width {
        return None;
    }
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    let shift = index * width;
    let lane_mask = u128::from(mask) << shift;
    Some((vector & !lane_mask) | (u128::from(value & mask) << shift))
}

fn splat(value: u64, width: usize) -> u128 {
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    let mut result = 0u128;
    for index in 0..128 / width {
        result |= u128::from(value & mask) << (index * width);
    }
    result
}

fn scalar_bits(value: WasmValue, width: usize) -> Option<u64> {
    match (value, width) {
        (WasmValue::I32(value), 8 | 16 | 32) => Some(value as u32 as u64),
        (WasmValue::I64(value), 64) => Some(value as u64),
        (WasmValue::F32(bits), 32) => Some(u64::from(bits)),
        (WasmValue::F64(bits), 64) => Some(bits),
        _ => None,
    }
}

fn sign_extend(value: u64, width: usize) -> i64 {
    if width == 64 {
        value as i64
    } else {
        ((value << (64 - width)) as i64) >> (64 - width)
    }
}

fn is_float_compare(name: &str) -> bool {
    name.ends_with("Eq")
        || name.ends_with("Ne")
        || name.ends_with("Lt")
        || name.ends_with("Gt")
        || name.ends_with("Le")
        || name.ends_with("Ge")
}

fn compare_float(name: &str, x: f64, y: f64) -> Option<bool> {
    Some(match name.get(5..)? {
        "Eq" => x == y,
        "Ne" => x != y,
        "Lt" => x < y,
        "Gt" => x > y,
        "Le" => x <= y,
        "Ge" => x >= y,
        _ => return None,
    })
}

fn is_integer_compare(name: &str) -> bool {
    [
        "Eq", "Ne", "LtS", "LtU", "GtS", "GtU", "LeS", "LeU", "GeS", "GeU",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
}

fn integer_compare(name: &str, x: u64, y: u64, sx: i64, sy: i64) -> Option<bool> {
    Some(match () {
        _ if name.ends_with("Eq") => x == y,
        _ if name.ends_with("Ne") => x != y,
        _ if name.ends_with("LtS") => sx < sy,
        _ if name.ends_with("LtU") => x < y,
        _ if name.ends_with("GtS") => sx > sy,
        _ if name.ends_with("GtU") => x > y,
        _ if name.ends_with("LeS") => sx <= sy,
        _ if name.ends_with("LeU") => x <= y,
        _ if name.ends_with("GeS") => sx >= sy,
        _ if name.ends_with("GeU") => x >= y,
        _ => return None,
    })
}

fn saturating_add_signed(left: i64, right: i64, width: usize) -> i64 {
    let min = -(1i64 << (width - 1));
    let max = (1i64 << (width - 1)) - 1;
    left.saturating_add(right).clamp(min, max)
}

fn saturating_sub_signed(left: i64, right: i64, width: usize) -> i64 {
    let min = -(1i64 << (width - 1));
    let max = (1i64 << (width - 1)) - 1;
    left.saturating_sub(right).clamp(min, max)
}

fn trunc_sat(value: f64, signed: bool, width: usize) -> u64 {
    if value.is_nan() {
        return 0;
    }
    if signed {
        let minimum = -(2f64).powi((width - 1) as i32);
        let maximum = (2f64).powi((width - 1) as i32) - 1.0;
        value.trunc().clamp(minimum, maximum) as i64 as u64
    } else {
        let maximum = (2f64).powi(width as i32) - 1.0;
        value.trunc().clamp(0.0, maximum) as u64
    }
}
