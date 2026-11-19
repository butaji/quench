//! The operator lists the runtime and the b-jit both expand: the SIMD operators in tag order (each
//! with its family, the column the b-jit's row families are built from), the memory loads in
//! `MemoryLoad` tag order (each with its family), and the scalar accesses in the b-jit's
//! `ACCESSES` order (each with the load it is, its value type, its bytes and whether it sign-extends).
//!
//! This file is pure `macro_rules!`: no `use`, no items. It is included with `#[path]` by
//! `staged/wasm_simd_lanes.rs` and `wasm/memory.rs` (the runtime), and by `quench-bjit`'s
//! `stencils/wasm_codes.rs`, which the stencil crate, the lib and the build script all compile. A
//! callback that needs only names (the b-jit's codes) ignores each row's body, which names runtime
//! items that only the runtime resolves. A row's position is its tag, so reordering a list renumbers
//! its tags; the runtime asserts each tag it names (`vm/compiled/wasm_codes_check.rs`).

#![allow(unused_macros, unused_imports)]

/// Every SIMD operator, in tag order: the name, the lane immediate marker (`[lane]` for an operator
/// that takes one), the lane width, the left and right operand types (`None` for a unary operator),
/// the result type, the family (the column `wasm_codes.rs` groups rows by), and the body over `a`
/// (left), `b` (right) and `l` (lane). The body's value is the result's bits: a vector whole, a
/// scalar in the low bits.
macro_rules! simd_operator_list {
    ($callback:ident) => {
        $callback! {
            V128Not: 128, V128, None, V128, Unary => |a, b, l| !a;
            V128And: 128, V128, Some(WasmType::V128), V128, Binary => |a, b, l| a & b;
            V128AndNot: 128, V128, Some(WasmType::V128), V128, Binary => |a, b, l| a & !b;
            V128Or: 128, V128, Some(WasmType::V128), V128, Binary => |a, b, l| a | b;
            V128Xor: 128, V128, Some(WasmType::V128), V128, Binary => |a, b, l| a ^ b;
            V128AnyTrue: 128, V128, None, I32, MoreTest => |a, b, l| u128::from(a != 0);
            I8x16Swizzle: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| swizzle(a, b);
            I8x16Splat: 8, I32, None, V128, Splat => |a, b, l| splat(a as u64, 8);
            I16x8Splat: 16, I32, None, V128, Splat => |a, b, l| splat(a as u64, 16);
            I32x4Splat: 32, I32, None, V128, Splat => |a, b, l| splat(a as u64, 32);
            I64x2Splat: 64, I64, None, V128, Splat => |a, b, l| splat(a as u64, 64);
            F32x4Splat: 32, F32, None, V128, Splat => |a, b, l| splat(a as u64, 32);
            F64x2Splat: 64, F64, None, V128, Splat => |a, b, l| splat(a as u64, 64);
            I8x16ExtractLaneS [lane]: 8, V128, None, I32, Extract => |a, b, l| u128::from(signed(lane(a, 8, l), 8) as u32);
            I8x16ExtractLaneU [lane]: 8, V128, None, I32, Extract => |a, b, l| u128::from(lane(a, 8, l) as u32);
            I16x8ExtractLaneS [lane]: 16, V128, None, I32, Extract => |a, b, l| u128::from(signed(lane(a, 16, l), 16) as u32);
            I16x8ExtractLaneU [lane]: 16, V128, None, I32, Extract => |a, b, l| u128::from(lane(a, 16, l) as u32);
            I32x4ExtractLane [lane]: 32, V128, None, I32, Extract => |a, b, l| u128::from(lane(a, 32, l) as u32);
            I64x2ExtractLane [lane]: 64, V128, None, I64, Extract => |a, b, l| u128::from(lane(a, 64, l));
            F32x4ExtractLane [lane]: 32, V128, None, F32, Extract => |a, b, l| u128::from(lane(a, 32, l));
            F64x2ExtractLane [lane]: 64, V128, None, F64, Extract => |a, b, l| u128::from(lane(a, 64, l));
            I8x16ReplaceLane [lane]: 8, V128, Some(WasmType::I32), V128, Replace => |a, b, l| replace(a, 8, l, b as u64);
            I16x8ReplaceLane [lane]: 16, V128, Some(WasmType::I32), V128, Replace => |a, b, l| replace(a, 16, l, b as u64);
            I32x4ReplaceLane [lane]: 32, V128, Some(WasmType::I32), V128, Replace => |a, b, l| replace(a, 32, l, b as u64);
            I64x2ReplaceLane [lane]: 64, V128, Some(WasmType::I64), V128, Replace => |a, b, l| replace(a, 64, l, b as u64);
            F32x4ReplaceLane [lane]: 32, V128, Some(WasmType::F32), V128, Replace => |a, b, l| replace(a, 32, l, b as u64);
            F64x2ReplaceLane [lane]: 64, V128, Some(WasmType::F64), V128, Replace => |a, b, l| replace(a, 64, l, b as u64);
            F32x4Add: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Add);
            F32x4Sub: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Subtract);
            F32x4Mul: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Multiply);
            F32x4Div: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Divide);
            F32x4Min: 32, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Minimum);
            F32x4Max: 32, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Maximum);
            F32x4Eq: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::Equal);
            F32x4Ne: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::NotEqual);
            F32x4Lt: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::Less);
            F32x4Gt: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::Greater);
            F32x4Le: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::LessEqual);
            F32x4Ge: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::GreaterEqual);
            F32x4PMin: 32, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_pseudo(a, b, Float::F32, FloatBinary::Greater);
            F32x4PMax: 32, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_pseudo(a, b, Float::F32, FloatBinary::Less);
            F32x4Abs: 32, V128, None, V128, Unary => |a, b, l| float_unary(a, Float::F32, FloatUnary::Absolute);
            F32x4Neg: 32, V128, None, V128, Unary => |a, b, l| float_unary(a, Float::F32, FloatUnary::Negate);
            F32x4Sqrt: 32, V128, None, V128, MoreUnary => |a, b, l| float_unary(a, Float::F32, FloatUnary::SquareRoot);
            F32x4Ceil: 32, V128, None, V128, MoreUnary => |a, b, l| float_unary(a, Float::F32, FloatUnary::Ceiling);
            F32x4Floor: 32, V128, None, V128, MoreUnary => |a, b, l| float_unary(a, Float::F32, FloatUnary::Floor);
            F32x4Trunc: 32, V128, None, V128, MoreUnary => |a, b, l| float_unary(a, Float::F32, FloatUnary::Truncate);
            F32x4Nearest: 32, V128, None, V128, MoreUnary => |a, b, l| float_unary(a, Float::F32, FloatUnary::Nearest);
            F64x2Add: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Add);
            F64x2Sub: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Subtract);
            F64x2Mul: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Multiply);
            F64x2Div: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Divide);
            F64x2Min: 64, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Minimum);
            F64x2Max: 64, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Maximum);
            F64x2Eq: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::Equal);
            F64x2Ne: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::NotEqual);
            F64x2Lt: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::Less);
            F64x2Gt: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::Greater);
            F64x2Le: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::LessEqual);
            F64x2Ge: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::GreaterEqual);
            F64x2PMin: 64, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_pseudo(a, b, Float::F64, FloatBinary::Greater);
            F64x2PMax: 64, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_pseudo(a, b, Float::F64, FloatBinary::Less);
            F64x2Abs: 64, V128, None, V128, Unary => |a, b, l| float_unary(a, Float::F64, FloatUnary::Absolute);
            F64x2Neg: 64, V128, None, V128, Unary => |a, b, l| float_unary(a, Float::F64, FloatUnary::Negate);
            F64x2Sqrt: 64, V128, None, V128, MoreUnary => |a, b, l| float_unary(a, Float::F64, FloatUnary::SquareRoot);
            F64x2Ceil: 64, V128, None, V128, MoreUnary => |a, b, l| float_unary(a, Float::F64, FloatUnary::Ceiling);
            F64x2Floor: 64, V128, None, V128, MoreUnary => |a, b, l| float_unary(a, Float::F64, FloatUnary::Floor);
            F64x2Trunc: 64, V128, None, V128, MoreUnary => |a, b, l| float_unary(a, Float::F64, FloatUnary::Truncate);
            F64x2Nearest: 64, V128, None, V128, MoreUnary => |a, b, l| float_unary(a, Float::F64, FloatUnary::Nearest);
            I8x16Eq: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 8, |x, y| x == y);
            I8x16Ne: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 8, |x, y| x != y);
            I8x16LtS: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 8, |x, y| signed(x, 8) < signed(y, 8));
            I8x16GtS: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 8, |x, y| signed(x, 8) > signed(y, 8));
            I8x16LeS: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 8, |x, y| signed(x, 8) <= signed(y, 8));
            I8x16GeS: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 8, |x, y| signed(x, 8) >= signed(y, 8));
            I8x16LtU: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 8, |x, y| x < y);
            I8x16GtU: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 8, |x, y| x > y);
            I8x16LeU: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 8, |x, y| x <= y);
            I8x16GeU: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 8, |x, y| x >= y);
            I8x16Add: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| zip(a, b, 8, u64::wrapping_add);
            I8x16Sub: 8, V128, Some(WasmType::V128), V128, Binary => |a, b, l| zip(a, b, 8, u64::wrapping_sub);
            I8x16Abs: 8, V128, None, V128, Unary => |a, b, l| absolute(a, 8);
            I8x16Neg: 8, V128, None, V128, Unary => |a, b, l| negate(a, 8);
            I8x16AllTrue: 8, V128, None, I32, MoreTest => |a, b, l| all_true(a, 8);
            I8x16Bitmask: 8, V128, None, I32, MoreTest => |a, b, l| bitmask(a, 8);
            I8x16Shl: 8, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_left(a, b, 8);
            I8x16ShrS: 8, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_right_signed(a, b, 8);
            I8x16ShrU: 8, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_right_unsigned(a, b, 8);
            I8x16MinS: 8, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 8, true, false);
            I8x16MinU: 8, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 8, false, false);
            I8x16MaxS: 8, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 8, true, true);
            I8x16MaxU: 8, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 8, false, true);
            I8x16AddSatS: 8, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| saturating(a, b, 8, true, false);
            I8x16AddSatU: 8, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| saturating(a, b, 8, false, false);
            I8x16SubSatS: 8, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| saturating(a, b, 8, true, true);
            I8x16SubSatU: 8, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| saturating(a, b, 8, false, true);
            I8x16AvgrU: 8, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| average(a, b, 8);
            I8x16NarrowI16x8S: 8, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| narrow(a, b, 8, true);
            I8x16NarrowI16x8U: 8, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| narrow(a, b, 8, false);
            I16x8Eq: 16, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 16, |x, y| x == y);
            I16x8Ne: 16, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 16, |x, y| x != y);
            I16x8LtS: 16, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 16, |x, y| signed(x, 16) < signed(y, 16));
            I16x8GtS: 16, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 16, |x, y| signed(x, 16) > signed(y, 16));
            I16x8LeS: 16, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 16, |x, y| signed(x, 16) <= signed(y, 16));
            I16x8GeS: 16, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 16, |x, y| signed(x, 16) >= signed(y, 16));
            I16x8LtU: 16, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 16, |x, y| x < y);
            I16x8GtU: 16, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 16, |x, y| x > y);
            I16x8LeU: 16, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 16, |x, y| x <= y);
            I16x8GeU: 16, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 16, |x, y| x >= y);
            I16x8Add: 16, V128, Some(WasmType::V128), V128, Binary => |a, b, l| zip(a, b, 16, u64::wrapping_add);
            I16x8Sub: 16, V128, Some(WasmType::V128), V128, Binary => |a, b, l| zip(a, b, 16, u64::wrapping_sub);
            I16x8Mul: 16, V128, Some(WasmType::V128), V128, Binary => |a, b, l| zip(a, b, 16, u64::wrapping_mul);
            I16x8Abs: 16, V128, None, V128, MoreUnary => |a, b, l| absolute(a, 16);
            I16x8Neg: 16, V128, None, V128, MoreUnary => |a, b, l| negate(a, 16);
            I16x8AllTrue: 16, V128, None, I32, MoreTest => |a, b, l| all_true(a, 16);
            I16x8Bitmask: 16, V128, None, I32, MoreTest => |a, b, l| bitmask(a, 16);
            I16x8Shl: 16, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_left(a, b, 16);
            I16x8ShrS: 16, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_right_signed(a, b, 16);
            I16x8ShrU: 16, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_right_unsigned(a, b, 16);
            I16x8MinS: 16, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 16, true, false);
            I16x8MinU: 16, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 16, false, false);
            I16x8MaxS: 16, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 16, true, true);
            I16x8MaxU: 16, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 16, false, true);
            I16x8AddSatS: 16, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| saturating(a, b, 16, true, false);
            I16x8AddSatU: 16, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| saturating(a, b, 16, false, false);
            I16x8SubSatS: 16, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| saturating(a, b, 16, true, true);
            I16x8SubSatU: 16, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| saturating(a, b, 16, false, true);
            I16x8AvgrU: 16, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| average(a, b, 16);
            I16x8ExtendLowI8x16S: 16, V128, None, V128, MoreWiden => |a, b, l| extend(a, 16, true, false);
            I16x8ExtMulLowI8x16S: 16, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 16, true, false);
            I16x8ExtendLowI8x16U: 16, V128, None, V128, MoreWiden => |a, b, l| extend(a, 16, false, false);
            I16x8ExtMulLowI8x16U: 16, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 16, false, false);
            I16x8ExtendHighI8x16S: 16, V128, None, V128, MoreWiden => |a, b, l| extend(a, 16, true, true);
            I16x8ExtMulHighI8x16S: 16, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 16, true, true);
            I16x8ExtendHighI8x16U: 16, V128, None, V128, MoreWiden => |a, b, l| extend(a, 16, false, true);
            I16x8ExtMulHighI8x16U: 16, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 16, false, true);
            I16x8ExtAddPairwiseI8x16S: 16, V128, None, V128, MoreWiden => |a, b, l| pairwise_add(a, 16, true);
            I16x8ExtAddPairwiseI8x16U: 16, V128, None, V128, MoreWiden => |a, b, l| pairwise_add(a, 16, false);
            I16x8NarrowI32x4S: 16, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| narrow(a, b, 16, true);
            I16x8NarrowI32x4U: 16, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| narrow(a, b, 16, false);
            I32x4Eq: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 32, |x, y| x == y);
            I32x4Ne: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| compare(a, b, 32, |x, y| x != y);
            I32x4LtS: 32, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 32, |x, y| signed(x, 32) < signed(y, 32));
            I32x4GtS: 32, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 32, |x, y| signed(x, 32) > signed(y, 32));
            I32x4LeS: 32, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 32, |x, y| signed(x, 32) <= signed(y, 32));
            I32x4GeS: 32, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 32, |x, y| signed(x, 32) >= signed(y, 32));
            I32x4LtU: 32, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 32, |x, y| x < y);
            I32x4GtU: 32, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 32, |x, y| x > y);
            I32x4LeU: 32, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 32, |x, y| x <= y);
            I32x4GeU: 32, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 32, |x, y| x >= y);
            I32x4Add: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| zip(a, b, 32, u64::wrapping_add);
            I32x4Sub: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| zip(a, b, 32, u64::wrapping_sub);
            I32x4Mul: 32, V128, Some(WasmType::V128), V128, Binary => |a, b, l| zip(a, b, 32, u64::wrapping_mul);
            I32x4Abs: 32, V128, None, V128, MoreUnary => |a, b, l| absolute(a, 32);
            I32x4Neg: 32, V128, None, V128, MoreUnary => |a, b, l| negate(a, 32);
            I32x4AllTrue: 32, V128, None, I32, MoreTest => |a, b, l| all_true(a, 32);
            I32x4Bitmask: 32, V128, None, I32, MoreTest => |a, b, l| bitmask(a, 32);
            I32x4Shl: 32, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_left(a, b, 32);
            I32x4ShrS: 32, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_right_signed(a, b, 32);
            I32x4ShrU: 32, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_right_unsigned(a, b, 32);
            I32x4MinS: 32, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 32, true, false);
            I32x4MinU: 32, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 32, false, false);
            I32x4MaxS: 32, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 32, true, true);
            I32x4MaxU: 32, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| extrema(a, b, 32, false, true);
            I32x4ExtendLowI16x8S: 32, V128, None, V128, MoreWiden => |a, b, l| extend(a, 32, true, false);
            I32x4ExtMulLowI16x8S: 32, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 32, true, false);
            I32x4ExtendLowI16x8U: 32, V128, None, V128, MoreWiden => |a, b, l| extend(a, 32, false, false);
            I32x4ExtMulLowI16x8U: 32, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 32, false, false);
            I32x4ExtendHighI16x8S: 32, V128, None, V128, MoreWiden => |a, b, l| extend(a, 32, true, true);
            I32x4ExtMulHighI16x8S: 32, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 32, true, true);
            I32x4ExtendHighI16x8U: 32, V128, None, V128, MoreWiden => |a, b, l| extend(a, 32, false, true);
            I32x4ExtMulHighI16x8U: 32, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 32, false, true);
            I32x4ExtAddPairwiseI16x8S: 32, V128, None, V128, MoreWiden => |a, b, l| pairwise_add(a, 32, true);
            I32x4ExtAddPairwiseI16x8U: 32, V128, None, V128, MoreWiden => |a, b, l| pairwise_add(a, 32, false);
            I64x2Eq: 64, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 64, |x, y| x == y);
            I64x2Ne: 64, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 64, |x, y| x != y);
            I64x2LtS: 64, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 64, |x, y| signed(x, 64) < signed(y, 64));
            I64x2GtS: 64, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 64, |x, y| signed(x, 64) > signed(y, 64));
            I64x2LeS: 64, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 64, |x, y| signed(x, 64) <= signed(y, 64));
            I64x2GeS: 64, V128, Some(WasmType::V128), V128, MoreCompare => |a, b, l| compare(a, b, 64, |x, y| signed(x, 64) >= signed(y, 64));
            I64x2Add: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| zip(a, b, 64, u64::wrapping_add);
            I64x2Sub: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| zip(a, b, 64, u64::wrapping_sub);
            I64x2Mul: 64, V128, Some(WasmType::V128), V128, Binary => |a, b, l| zip(a, b, 64, u64::wrapping_mul);
            I64x2Abs: 64, V128, None, V128, MoreUnary => |a, b, l| absolute(a, 64);
            I64x2Neg: 64, V128, None, V128, MoreUnary => |a, b, l| negate(a, 64);
            I64x2AllTrue: 64, V128, None, I32, MoreTest => |a, b, l| all_true(a, 64);
            I64x2Bitmask: 64, V128, None, I32, MoreTest => |a, b, l| bitmask(a, 64);
            I64x2Shl: 64, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_left(a, b, 64);
            I64x2ShrS: 64, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_right_signed(a, b, 64);
            I64x2ShrU: 64, V128, Some(WasmType::I32), V128, MoreShift => |a, b, l| shift_right_unsigned(a, b, 64);
            I64x2ExtendLowI32x4S: 64, V128, None, V128, MoreWiden => |a, b, l| extend(a, 64, true, false);
            I64x2ExtMulLowI32x4S: 64, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 64, true, false);
            I64x2ExtendLowI32x4U: 64, V128, None, V128, MoreWiden => |a, b, l| extend(a, 64, false, false);
            I64x2ExtMulLowI32x4U: 64, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 64, false, false);
            I64x2ExtendHighI32x4S: 64, V128, None, V128, MoreWiden => |a, b, l| extend(a, 64, true, true);
            I64x2ExtMulHighI32x4S: 64, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 64, true, true);
            I64x2ExtendHighI32x4U: 64, V128, None, V128, MoreWiden => |a, b, l| extend(a, 64, false, true);
            I64x2ExtMulHighI32x4U: 64, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| extend_multiply(a, b, 64, false, true);
            I8x16Popcnt: 8, V128, None, V128, MoreUnary => |a, b, l| population_count(a);
            I16x8Q15MulrSatS: 16, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| q15(a, b);
            I32x4DotI16x8S: 32, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| dot(a, b);
            I32x4TruncSatF32x4S: 32, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::SaturateF32SignedI32, 32, 32);
            I32x4TruncSatF32x4U: 32, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::SaturateF32UnsignedI32, 32, 32);
            F32x4ConvertI32x4S: 32, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::ConvertI32SignedF32, 32, 32);
            F32x4ConvertI32x4U: 32, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::ConvertI32UnsignedF32, 32, 32);
            I32x4TruncSatF64x2SZero: 64, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::SaturateF64SignedI32, 64, 32);
            I32x4TruncSatF64x2UZero: 64, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::SaturateF64UnsignedI32, 64, 32);
            F64x2ConvertLowI32x4S: 32, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::ConvertI32SignedF64, 32, 64);
            F64x2ConvertLowI32x4U: 32, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::ConvertI32UnsignedF64, 32, 64);
            F32x4DemoteF64x2Zero: 64, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::DemoteF64, 64, 32);
            F64x2PromoteLowF32x4: 32, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::PromoteF32, 32, 64);
            I8x16RelaxedSwizzle: 8, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| swizzle(a, b);
            I32x4RelaxedTruncF32x4S: 32, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::SaturateF32SignedI32, 32, 32);
            I32x4RelaxedTruncF32x4U: 32, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::SaturateF32UnsignedI32, 32, 32);
            I32x4RelaxedTruncF64x2SZero: 64, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::SaturateF64SignedI32, 64, 32);
            I32x4RelaxedTruncF64x2UZero: 64, V128, None, V128, MoreConvert => |a, b, l| convert_lanes(a, Conversion::SaturateF64UnsignedI32, 64, 32);
            F32x4RelaxedMin: 32, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Minimum);
            F32x4RelaxedMax: 32, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Maximum);
            F64x2RelaxedMin: 64, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Minimum);
            F64x2RelaxedMax: 64, V128, Some(WasmType::V128), V128, MoreFloat => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Maximum);
            I16x8RelaxedQ15mulrS: 16, V128, Some(WasmType::V128), V128, MoreBinary => |a, b, l| q15(a, b);
            I16x8RelaxedDotI8x16I7x16S: 16, V128, Some(WasmType::V128), V128, MoreWide => |a, b, l| relaxed_dot(a, b);
        }
    };
}

/// Every memory load, in `MemoryLoad` tag order: the name and its family (`Scalar`: a scalar access,
/// `Plain`: the whole vector, `Vector`: a vector load that splats, widens or zeroes).
macro_rules! memory_load_list {
    ($callback:ident) => {
        $callback! {
            I32Load Scalar,
            I64Load Scalar,
            F32Load Scalar,
            F64Load Scalar,
            I32Load8S Scalar,
            I32Load8U Scalar,
            I32Load16S Scalar,
            I32Load16U Scalar,
            I64Load8S Scalar,
            I64Load8U Scalar,
            I64Load16S Scalar,
            I64Load16U Scalar,
            I64Load32S Scalar,
            I64Load32U Scalar,
            V128Load Plain,
            V128Load8x8S Vector,
            V128Load8x8U Vector,
            V128Load16x4S Vector,
            V128Load16x4U Vector,
            V128Load32x2S Vector,
            V128Load32x2U Vector,
            V128Load8Splat Vector,
            V128Load16Splat Vector,
            V128Load32Splat Vector,
            V128Load64Splat Vector,
            V128Load32Zero Vector,
            V128Load64Zero Vector
        }
    };
}

/// Every scalar access, in the b-jit's `ACCESSES` order (the operator codes of its `LOAD` and `STORE`
/// rows index it): the `MemoryLoad` it is, its value type, its bytes and whether it sign-extends.
macro_rules! scalar_access_list {
    ($callback:ident) => {
        $callback! {
            I32Load: I32, 4, false;
            I32Load8S: I32, 1, true;
            I32Load8U: I32, 1, false;
            I32Load16S: I32, 2, true;
            I32Load16U: I32, 2, false;
            I64Load: I64, 8, false;
            I64Load8S: I64, 1, true;
            I64Load8U: I64, 1, false;
            I64Load16S: I64, 2, true;
            I64Load16U: I64, 2, false;
            I64Load32S: I64, 4, true;
            I64Load32U: I64, 4, false;
            F32Load: F32, 4, false;
            F64Load: F64, 8, false;
        }
    };
}

pub(crate) use memory_load_list;
pub(crate) use scalar_access_list;
pub(crate) use simd_operator_list;
