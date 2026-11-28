//! Compile-time checks that the b-jit's Wasm codes (`quench-bjit/stencils/wasm_*.rs`) equal the
//! runtime's values. The stencil build compiles `quench-bjit` alone and cannot see the runtime, so
//! a count or tag that drifts from its operator family fails here, at build time, instead of
//! silently building a row no operator selects. The SIMD operator tags are checked where they are
//! used (`wasm_stitch_simd.rs`, `wasm_stitch_simd_more.rs`); the layouts of the context, the table
//! and the frame are checked beside their stitcher (`wasm_stitch_indirect.rs`, `wasm_stitch.rs`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::wasm::conversion::ScalarConversionOperator;
use quench_runtime::wasm::float::{F64BinaryOperator, F64UnaryOperator};
use quench_runtime::wasm::integer::{I32BinaryOperator, I32UnaryOperator, I64BinaryOperator, I64UnaryOperator};
use quench_runtime::wasm::memory::WASM_PAGE_BITS;
use quench_bjit::wasm as codes;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

// The integer families: the i32 operators are the first `SHARED_*` tags of the i64 families, which
// add the wide operators after them.
const _: () = assert!(I32BinaryOperator::ALL.len() == codes::SHARED_BINARY as usize);
const _: () = assert!(I64BinaryOperator::ALL.len() == codes::BINARY_COUNT as usize);
const _: () = assert!(I32UnaryOperator::ALL.len() == codes::SHARED_UNARY as usize);
const _: () = assert!(I64UnaryOperator::ALL.len() == codes::UNARY_COUNT as usize);
// The comparisons are the operators from `Equal` through `GreaterEqualUnsigned`.
const _: () = assert!(I64BinaryOperator::Equal as u32 == codes::FIRST_COMPARISON);
const _: () = assert!(I64BinaryOperator::GreaterEqualUnsigned as u32 == codes::LAST_COMPARISON);

// The float families and the scalar conversions.
const _: () = assert!(F64BinaryOperator::ALL.len() == codes::FLOAT_BINARY_COUNT as usize);
const _: () = assert!(F64UnaryOperator::ALL.len() == codes::FLOAT_UNARY_COUNT as usize);
const _: () = assert!(ScalarConversionOperator::ALL.len() == codes::CONVERT_COUNT as usize);

// The page size the bulk rows shift by.
const _: () = assert!(WASM_PAGE_BITS == codes::WASM_PAGE_BITS);

// The SIMD operators: the list the b-jit's codes derive from has one tag per runtime operator.
const _: () = assert!(codes::SIMD_OPERATOR_COUNT as usize == quench_runtime::staged::wasm_simd_lanes::SimdOperator::ALL.len());
