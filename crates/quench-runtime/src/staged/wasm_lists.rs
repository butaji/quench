//! The operator lists the runtime and the b-jit both expand: the SIMD operators in tag order (each
//! with its family, the column the b-jit's row families are built from), the memory loads in
//! `MemoryLoad` tag order (each with its family), and the scalar accesses in the b-jit's
//! `ACCESSES` order (each with the load it is, its value type, its bytes and whether it sign-extends).
//!
//! The I64 and float operator lists and the scalar conversions (`i64_binary_list!` and the rest) are
//! the selector lists of `wasm/integer.rs`, `wasm/float.rs` and `wasm/conversion.rs`, which expand
//! them; `wasm_families.rs` counts them and derives the comparison bounds from their entries.
//!
//! This file is pure `macro_rules!`: no `use`, no items. The runtime includes it once, at the crate
//! root (`lib.rs`, `#[macro_use]`), and the Wasm rules and SIMD lanes expand it from there. It is
//! included with `#[path]` by `quench-bjit`'s `stencils/wasm_kinds.rs`, `wasm_families.rs`,
//! `wasm_layout.rs` and `wasm_runtime_codes.rs`, and by the stencil builds of `wasm/integer.rs`,
//! `wasm/float.rs`, `wasm/conversion.rs` and `staged/wasm_simd_lanes.rs` (`cfg(stencil_build)`), each
//! context including the files it reads. A callback that needs only names (the b-jit's codes) ignores
//! each row's body, which names runtime items that only the runtime resolves. A row's position is its
//! tag, so reordering a list renumbers its tags; the runtime asserts each tag it names
//! (`vm/compiled/wasm_codes_check.rs`).

#![allow(unused_macros, unused_imports)]

/// Every SIMD operator, in tag order: the name, the lane immediate marker (`[lane]` for an operator
/// that takes one), the lane width, the left and right operand types (`None` for a unary operator),
/// the result type, the family (the column `wasm_families.rs` groups rows by), and the body over `a`
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

/// The I64 binary operators, one list every family expands (`wasm/integer.rs`, `wasm_families.rs`): each
/// entry is its name, its i32 and i64 wasm operators, in brackets the machine row it is
/// (`staged::machine::MachineOperator`, which `I64BinaryOperator::machine` returns) and its semantics over `left` and `right`. The shared
/// entries come first (the i32 family is their prefix); the wide ones follow `@i64`.
macro_rules! i64_binary_list {
    ($callback:ident) => {
        $callback! { (Signed, Unsigned), (left, right);
            Add, I32Add, I64Add [Binary(Add)] => Ok(NumericResult::Value(left.wrapping_add(right)));
            Subtract, I32Sub, I64Sub [Binary(Sub)] => Ok(NumericResult::Value(left.wrapping_sub(right)));
            Multiply, I32Mul, I64Mul [Binary(Mul)] => Ok(NumericResult::Value(left.wrapping_mul(right)));
            DivideSigned, I32DivS, I64DivS [Binary(DivideSigned)] => {
                if right == 0 { Err(WasmTrap::IntegerDivideByZero) }
                else { left.checked_div(right).map(NumericResult::Value).ok_or(WasmTrap::IntegerOverflow) }
            };
            DivideUnsigned, I32DivU, I64DivU [Binary(DivideUnsigned)] => {
                (left as Unsigned).checked_div(right as Unsigned).map(|v| NumericResult::Value(v as Signed))
                    .ok_or(WasmTrap::IntegerDivideByZero)
            };
            RemainderSigned, I32RemS, I64RemS [SignedRemainder] => {
                if right == 0 { Err(WasmTrap::IntegerDivideByZero) }
                else { Ok(NumericResult::Value(left.wrapping_rem(right))) }
            };
            RemainderUnsigned, I32RemU, I64RemU [Binary(RemainderUnsigned)] => {
                (left as Unsigned).checked_rem(right as Unsigned).map(|v| NumericResult::Value(v as Signed))
                    .ok_or(WasmTrap::IntegerDivideByZero)
            };
            And, I32And, I64And [Binary(And)] => Ok(NumericResult::Value(left & right));
            Or, I32Or, I64Or [Binary(Or)] => Ok(NumericResult::Value(left | right));
            Xor, I32Xor, I64Xor [Binary(Xor)] => Ok(NumericResult::Value(left ^ right));
            ShiftLeft, I32Shl, I64Shl [Binary(ShiftLeft)] => Ok(NumericResult::Value(left.wrapping_shl(right as u32)));
            ShiftRightSigned, I32ShrS, I64ShrS [Binary(ShiftRightSigned)] => Ok(NumericResult::Value(left.wrapping_shr(right as u32)));
            ShiftRightUnsigned, I32ShrU, I64ShrU [Binary(ShiftRightUnsigned)] => Ok(NumericResult::Value((left as Unsigned).wrapping_shr(right as u32) as Signed));
            RotateLeft, I32Rotl, I64Rotl [Binary(RotateLeft)] => Ok(NumericResult::Value(left.rotate_left(right as u32)));
            RotateRight, I32Rotr, I64Rotr [Binary(RotateRight)] => Ok(NumericResult::Value(left.rotate_right(right as u32)));
            Equal, I32Eq, I64Eq [Relation(Equal)] => Ok(NumericResult::Comparison(left == right));
            NotEqual, I32Ne, I64Ne [Relation(NotEqual)] => Ok(NumericResult::Comparison(left != right));
            LessSigned, I32LtS, I64LtS [Relation(SignedLess)] => Ok(NumericResult::Comparison(left < right));
            LessUnsigned, I32LtU, I64LtU [Relation(UnsignedLess)] => Ok(NumericResult::Comparison((left as Unsigned) < (right as Unsigned)));
            GreaterSigned, I32GtS, I64GtS [Relation(SignedGreater)] => Ok(NumericResult::Comparison(left > right));
            GreaterUnsigned, I32GtU, I64GtU [Relation(UnsignedGreater)] => Ok(NumericResult::Comparison((left as Unsigned) > (right as Unsigned)));
            LessEqualSigned, I32LeS, I64LeS [Relation(SignedLessEqual)] => Ok(NumericResult::Comparison(left <= right));
            LessEqualUnsigned, I32LeU, I64LeU [Relation(UnsignedLessEqual)] => Ok(NumericResult::Comparison((left as Unsigned) <= (right as Unsigned)));
            GreaterEqualSigned, I32GeS, I64GeS [Relation(SignedGreaterEqual)] => Ok(NumericResult::Comparison(left >= right));
            GreaterEqualUnsigned, I32GeU, I64GeU [Relation(UnsignedGreaterEqual)] => Ok(NumericResult::Comparison((left as Unsigned) >= (right as Unsigned)));
            @i64
            MultiplyHighSigned, I64MulWideS [Binary(MulHighSigned)] => Ok(NumericResult::Value(((i128::from(left) * i128::from(right)) >> i64::BITS) as i64));
            MultiplyHighUnsigned, I64MulWideU [Binary(MulHighUnsigned)] => Ok(NumericResult::Value(((u128::from(left as u64) * u128::from(right as u64)) >> u64::BITS) as i64));
        }
    };
}

/// The I64 unary operators, one list every family expands: the shared entries, then the i64-only ones
/// after `@i64`, each with its i32 and i64 wasm operators and its semantics over `value`.
macro_rules! i64_unary_list {
    ($callback:ident) => {
        $callback! { Signed, value;
            EqualZero, I32Eqz, I64Eqz => NumericResult::Comparison(value == 0);
            LeadingZeros, I32Clz, I64Clz => NumericResult::Value(value.leading_zeros() as Signed);
            TrailingZeros, I32Ctz, I64Ctz => NumericResult::Value(value.trailing_zeros() as Signed);
            PopulationCount, I32Popcnt, I64Popcnt => NumericResult::Value(value.count_ones() as Signed);
            ExtendSigned8, I32Extend8S, I64Extend8S => NumericResult::Value(value as i8 as Signed);
            ExtendSigned16, I32Extend16S, I64Extend16S => NumericResult::Value(value as i16 as Signed);
            @i64 ExtendSigned32, I64Extend32S => NumericResult::Value(value as i32 as Signed);
        }
    };
}

/// The float binary operators, one list every family expands (`wasm/float.rs`, `wasm_families.rs`): each
/// entry is its name, its f32 and f64 wasm operators and its semantics over `left` and `right`.
macro_rules! float_binary_list {
    ($callback:ident) => {
        $callback! { (Float, Bits, SIGN_MASK, CANONICAL_NAN), (left, right);
            Add, F32Add, F64Add => NumericResult::Value(left + right);
            Subtract, F32Sub, F64Sub => NumericResult::Value(left - right);
            Multiply, F32Mul, F64Mul => NumericResult::Value(left * right);
            Divide, F32Div, F64Div => NumericResult::Value(left / right);
            Minimum, F32Min, F64Min => NumericResult::Value({
                if left.is_nan() || right.is_nan() { CANONICAL_NAN }
                else if left == right { Float::from_bits(left.to_bits() | right.to_bits()) }
                else { left.min(right) }
            });
            Maximum, F32Max, F64Max => NumericResult::Value({
                if left.is_nan() || right.is_nan() { CANONICAL_NAN }
                else if left == right { Float::from_bits(left.to_bits() & right.to_bits()) }
                else { left.max(right) }
            });
            CopySign, F32Copysign, F64Copysign => NumericResult::Value(Float::from_bits(
                (left.to_bits() & !SIGN_MASK) | (right.to_bits() & SIGN_MASK)));
            Equal, F32Eq, F64Eq => NumericResult::Comparison(left == right);
            NotEqual, F32Ne, F64Ne => NumericResult::Comparison(left != right);
            Less, F32Lt, F64Lt => NumericResult::Comparison(left < right);
            Greater, F32Gt, F64Gt => NumericResult::Comparison(left > right);
            LessEqual, F32Le, F64Le => NumericResult::Comparison(left <= right);
            GreaterEqual, F32Ge, F64Ge => NumericResult::Comparison(left >= right);
        }
    };
}

/// The float unary operators, one list every family expands: each entry is its name, its f32 and f64
/// wasm operators and its semantics over `value`.
macro_rules! float_unary_list {
    ($callback:ident) => {
        $callback! { (Float, Bits, SIGN_MASK), value;
            Absolute, F32Abs, F64Abs => Float::from_bits(value.to_bits() & !SIGN_MASK);
            Negate, F32Neg, F64Neg => Float::from_bits(value.to_bits() ^ SIGN_MASK);
            Ceiling, F32Ceil, F64Ceil => value.ceil();
            Floor, F32Floor, F64Floor => value.floor();
            Truncate, F32Trunc, F64Trunc => value.trunc();
            Nearest, F32Nearest, F64Nearest => value.round_ties_even();
            SquareRoot, F32Sqrt, F64Sqrt => value.sqrt();
        }
    };
}

/// The scalar conversions, one list every family expands (`wasm/conversion.rs`, `wasm_families.rs`): each
/// entry is its name, its wasm operator, its source and result types and its semantics over `value`.
macro_rules! scalar_conversion_list {
    ($callback:ident) => {
        $callback! { value;
            WrapI64, I32WrapI64, I64 => I32, WasmValue::I32(value as i32);
            ExtendI32Signed, I64ExtendI32S, I32 => I64, WasmValue::I64(i64::from(value));
            ExtendI32Unsigned, I64ExtendI32U, I32 => I64, WasmValue::I64(i64::from(value as u32));

            TruncateF32SignedI32, I32TruncF32S, F32 => I32, WasmValue::I32(trunc_to_integer(f64::from(f32::from_bits(value)), i32::BITS, true, false)? as i32);
            TruncateF32UnsignedI32, I32TruncF32U, F32 => I32, WasmValue::I32(trunc_to_integer(f64::from(f32::from_bits(value)), i32::BITS, false, false)? as i32);
            TruncateF64SignedI32, I32TruncF64S, F64 => I32, WasmValue::I32(trunc_to_integer(f64::from_bits(value), i32::BITS, true, false)? as i32);
            TruncateF64UnsignedI32, I32TruncF64U, F64 => I32, WasmValue::I32(trunc_to_integer(f64::from_bits(value), i32::BITS, false, false)? as i32);
            TruncateF32SignedI64, I64TruncF32S, F32 => I64, WasmValue::I64(trunc_to_integer(f64::from(f32::from_bits(value)), i64::BITS, true, false)?);
            TruncateF32UnsignedI64, I64TruncF32U, F32 => I64, WasmValue::I64(trunc_to_integer(f64::from(f32::from_bits(value)), i64::BITS, false, false)?);
            TruncateF64SignedI64, I64TruncF64S, F64 => I64, WasmValue::I64(trunc_to_integer(f64::from_bits(value), i64::BITS, true, false)?);
            TruncateF64UnsignedI64, I64TruncF64U, F64 => I64, WasmValue::I64(trunc_to_integer(f64::from_bits(value), i64::BITS, false, false)?);

            SaturateF32SignedI32, I32TruncSatF32S, F32 => I32, WasmValue::I32(trunc_to_integer(f64::from(f32::from_bits(value)), i32::BITS, true, true)? as i32);
            SaturateF32UnsignedI32, I32TruncSatF32U, F32 => I32, WasmValue::I32(trunc_to_integer(f64::from(f32::from_bits(value)), i32::BITS, false, true)? as i32);
            SaturateF64SignedI32, I32TruncSatF64S, F64 => I32, WasmValue::I32(trunc_to_integer(f64::from_bits(value), i32::BITS, true, true)? as i32);
            SaturateF64UnsignedI32, I32TruncSatF64U, F64 => I32, WasmValue::I32(trunc_to_integer(f64::from_bits(value), i32::BITS, false, true)? as i32);
            SaturateF32SignedI64, I64TruncSatF32S, F32 => I64, WasmValue::I64(trunc_to_integer(f64::from(f32::from_bits(value)), i64::BITS, true, true)?);
            SaturateF32UnsignedI64, I64TruncSatF32U, F32 => I64, WasmValue::I64(trunc_to_integer(f64::from(f32::from_bits(value)), i64::BITS, false, true)?);
            SaturateF64SignedI64, I64TruncSatF64S, F64 => I64, WasmValue::I64(trunc_to_integer(f64::from_bits(value), i64::BITS, true, true)?);
            SaturateF64UnsignedI64, I64TruncSatF64U, F64 => I64, WasmValue::I64(trunc_to_integer(f64::from_bits(value), i64::BITS, false, true)?);

            ConvertI32SignedF32, F32ConvertI32S, I32 => F32, WasmValue::F32((value as f32).to_bits());
            ConvertI32UnsignedF32, F32ConvertI32U, I32 => F32, WasmValue::F32((value as u32 as f32).to_bits());
            ConvertI64SignedF32, F32ConvertI64S, I64 => F32, WasmValue::F32((value as f32).to_bits());
            ConvertI64UnsignedF32, F32ConvertI64U, I64 => F32, WasmValue::F32((value as u64 as f32).to_bits());
            ConvertI32SignedF64, F64ConvertI32S, I32 => F64, WasmValue::F64((value as f64).to_bits());
            ConvertI32UnsignedF64, F64ConvertI32U, I32 => F64, WasmValue::F64((value as u32 as f64).to_bits());
            ConvertI64SignedF64, F64ConvertI64S, I64 => F64, WasmValue::F64((value as f64).to_bits());
            ConvertI64UnsignedF64, F64ConvertI64U, I64 => F64, WasmValue::F64((value as u64 as f64).to_bits());
            PromoteF32, F64PromoteF32, F32 => F64, WasmValue::F64(f64::from(f32::from_bits(value)).to_bits());
            DemoteF64, F32DemoteF64, F64 => F32, WasmValue::F32((f64::from_bits(value) as f32).to_bits());

            ReinterpretF32I32, I32ReinterpretF32, F32 => I32, WasmValue::I32(value as i32);
            ReinterpretF64I64, I64ReinterpretF64, F64 => I64, WasmValue::I64(value as i64);
            ReinterpretI32F32, F32ReinterpretI32, I32 => F32, WasmValue::F32(value as u32);
            ReinterpretI64F64, F64ReinterpretI64, I64 => F64, WasmValue::F64(value as u64);
        }
    };
}

/// The log2 of a Wasm page, which the runtime and the b-jit both name. `wasm_layout.rs` expands it;
/// the runtime asserts its own `WASM_PAGE_BITS` equal (`vm/compiled/wasm_codes_check.rs`).
macro_rules! wasm_page_bits {
    ($callback:ident) => {
        $callback! { 16 }
    };
}

pub(crate) use memory_load_list;
pub(crate) use scalar_access_list;
pub(crate) use simd_operator_list;
