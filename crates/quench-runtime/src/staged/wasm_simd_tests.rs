//! The SIMD table of `staged::wasm_simd`: every native row against the operator's kernel on the
//! bit-level stage, the choice between a row and the kernel, the opcode table and the address
//! of a vector access.

use super::wasm_machine::ConcreteInstance;
use super::wasm_memory::effective_address;
use super::wasm_simd::{Lanes, Native, NativeOp, Simd, SimdWasmSimdExt, WasmSimd};
use super::wasm_simd_rules::{NATIVE, native, simd};
use super::wasm_simd_concrete::{ConcreteSimd, execute};
use super::wasm_step::Step;
use super::wasm_tests::Probe;
use crate::bytecode::{InstructionField, Op, WideInstruction};
use crate::wasm::V128_BYTES;
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use crate::wasm::simd::{SimdKernel, SimdOperator, bits_of, shuffle, value_of};

const VECTORS: [u128; 7] = [
    0,
    1,
    u128::MAX,
    0x0123_4567_89ab_cdef_fedc_ba98_7654_3210,
    0x8000_0001_8000_0001_8000_0001_8000_0001,
    0x7f7f_7f7f_ff00_ff00_0080_0080_ffff_0000,
    0x0000_0000_ffff_ffff_0000_0000_8000_0000,
];

const COUNTS: [u128; 12] = [0, 1, 7, 8, 15, 16, 31, 32, 63, 64, 65, 127];

const fn is_shift(op: NativeOp) -> bool {
    matches!(
        op,
        NativeOp::ShiftLeft | NativeOp::ShiftRightUnsigned | NativeOp::ShiftRightSigned
    )
}

#[test]
fn every_native_row_is_its_operators_kernel() {
    for (operator, row) in NATIVE {
        let (kernel, lane) = SimdKernel::of(operator.selector(0)).expect("an operator");
        let rights: &[u128] = if is_shift(row.op) { &COUNTS } else { &VECTORS };
        let unary = kernel.right.is_none();
        for (left, right) in VECTORS
            .iter()
            .flat_map(|l| rights.iter().map(move |r| (*l, *r)))
        {
            let right = (!unary).then_some(right);
            assert_eq!(
                ConcreteSimd.vector_native(row, left, right),
                kernel.apply_bits(left, right.unwrap_or(0), lane),
                "{operator:?} {left:#x} {right:?}"
            );
        }
    }
}

#[test]
fn a_row_names_the_lanes_its_operator_computes() {
    for (operator, row) in NATIVE {
        assert_eq!(native(operator), Some(row));
        let bitwise = matches!(
            row.op,
            NativeOp::Not | NativeOp::And | NativeOp::Or | NativeOp::Xor | NativeOp::AndNot
        );
        assert_eq!(row.lanes == Lanes::Bits, bitwise, "{operator:?}");
    }
    assert_eq!(native(SimdOperator::I8x16Swizzle), None);
    assert_eq!(native(SimdOperator::V128AnyTrue), None);
}

/// Records which way `simd` computes.
#[derive(Default)]
struct Route(Vec<String>);

impl WasmSimd for Route {
    type Value = ();
    type Scalar = ();

    fn constant_vector(&mut self, _: u128) {
        self.0.push("constant".into());
    }

    fn vector_native(&mut self, native: Native, _: (), _: Option<()>) {
        self.0.push(format!("native {native:?}"));
    }

    fn vector_kernel(&mut self, selector: u32, _: (), _: Option<()>) {
        self.0.push(format!("kernel {selector}"));
    }

    fn scalar_kernel(&mut self, selector: u32, _: (), _: Option<()>) {
        self.0.push(format!("scalar {selector}"));
    }

    fn vector_shuffle(&mut self, _: [u8; V128_BYTES], _: (), _: ()) {
        self.0.push("shuffle".into());
    }
}

#[test]
fn an_operator_with_a_row_is_native_and_any_other_runs_its_kernel() {
    let mut route = Route::default();
    let and = SimdOperator::V128And.selector(0);
    let swizzle = SimdOperator::I8x16Swizzle.selector(0);
    simd(&mut route, and, (), Some(()));
    simd(&mut route, swizzle, (), Some(()));
    assert_eq!(
        route.0,
        [
            "native Native { op: And, lanes: Bits }".to_owned(),
            format!("kernel {swizzle}")
        ]
    );
}

#[test]
fn the_shuffle_is_the_interpreters() {
    let indices: [u8; V128_BYTES] = std::array::from_fn(|byte| (byte as u8 * 7) % 32);
    assert_eq!(
        ConcreteSimd.vector_shuffle(indices, VECTORS[3], VECTORS[5]),
        shuffle(VECTORS[3], VECTORS[5], indices)
    );
}

#[test]
fn the_table_names_each_vector_opcode() {
    let selector = SimdOperator::V128And.selector(0);
    assert_eq!(
        Simd::of(Op::WasmSimd, selector),
        Some(Simd::Operator { selector })
    );
    assert_eq!(Simd::of(Op::WasmSimd, u32::MAX), None);
    assert_eq!(Simd::of(Op::WasmSimdShuffle, 0), Some(Simd::Shuffle));
    assert_eq!(Simd::of(Op::WasmMemoryAddress, 0), Some(Simd::Address));
    let (load, store) = (MemoryLoad::V128Load as u32, MemoryStore::V128Store as u32);
    assert_eq!(Simd::of(Op::WasmMemoryLoad, load), Some(Simd::Load));
    assert_eq!(Simd::of(Op::WasmMemoryStore, store), Some(Simd::Store));
    assert_eq!(
        Simd::of(Op::WasmMemoryLoad, MemoryLoad::I32Load as u32),
        None
    );
    assert_eq!(
        Simd::of(Op::WasmMemoryStore, MemoryStore::I32Store as u32),
        None
    );
    assert_eq!(Simd::of(Op::WasmI32Add, 0), None);
    assert_eq!(
        Step::of(Op::WasmSimdShuffle, 0),
        Some(Step::Simd(Simd::Shuffle))
    );
}

#[test]
fn a_vector_access_reads_its_memory_and_address_where_the_table_says() {
    let instruction = WideInstruction::new(Op::WasmMemoryLoad, 1, 2, 3, 0);
    assert_eq!(Simd::Load.memory(instruction), Some(2));
    assert_eq!(Simd::Store.memory(instruction), Some(2));
    assert_eq!(Simd::Address.memory(instruction), Some(3));
    assert_eq!(Simd::Shuffle.memory(instruction), None);
    assert_eq!(Simd::Load.operand(instruction), 3);
    assert_eq!(Simd::Address.operand(instruction), 2);
    assert!(Step::Simd(Simd::Load).reads_memory());
    assert!(!Step::Simd(Simd::Shuffle).reads_memory());
}

#[test]
fn only_a_one_operand_operator_leaves_its_second_operand_unread() {
    let unary = Simd::Operator {
        selector: SimdOperator::V128Not.selector(0),
    };
    let binary = Simd::Operator {
        selector: SimdOperator::V128And.selector(0),
    };
    assert!(unary.leaves_unread(InstructionField::C));
    assert!(!unary.leaves_unread(InstructionField::B));
    assert!(!binary.leaves_unread(InstructionField::C));
    assert!(!Simd::Shuffle.leaves_unread(InstructionField::C));
}

#[test]
fn a_vector_access_address_is_its_index_plus_its_offset_without_wrapping() {
    let mut probe = Probe::default();
    effective_address(&mut probe, 0, 8);
    assert_eq!(probe.log, ["index", "constant I64 8", "Add I64"]);
    let mut stage = ConcreteInstance {
        globals: &mut [],
        memory: &mut [],
    };
    let largest = u64::from(u32::MAX);
    assert_eq!(effective_address(&mut stage, largest, largest), largest * 2);
}

#[test]
fn an_executed_operator_is_its_kernel_typed_by_its_table_entry() {
    let kernels = [
        SimdOperator::V128AnyTrue,
        SimdOperator::I8x16Swizzle,
        SimdOperator::I8x16Splat,
        SimdOperator::I32x4ExtractLane,
    ];
    let operators = NATIVE.map(|(operator, _)| operator).into_iter().chain(kernels);
    for operator in operators {
        let selector = operator.selector(0);
        let (kernel, lane) = SimdKernel::of(selector).expect("an operator");
        for (left, right) in VECTORS.iter().zip(VECTORS.iter().rev()) {
            let left = value_of(kernel.left, *left);
            let right = kernel.right.map(|ty| value_of(ty, *right));
            assert_eq!(
                execute(kernel, selector, left, right),
                value_of(kernel.result_type(lane), kernel.apply_bits(bits_of(left), right.map_or(0, bits_of), lane)),
                "{operator:?} {left:?} {right:?}"
            );
        }
    }
}
