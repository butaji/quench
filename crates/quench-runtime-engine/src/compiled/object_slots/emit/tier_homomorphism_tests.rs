//! Tier homomorphism, u-jit projection (TOD-2177, S1.14 step 2): every staged Number row of the
//! Concrete test (`quench-runtime/tests/tier_homomorphism.rs`, which checks Concrete against an
//! ECMAScript reference) is compiled through `Emit` as a one-row Cranelift function and run on the
//! same operand states, and each run must define the same word in the same representation as
//! Concrete, or leave where Concrete leaves. Rows, states and the Concrete frame are that test's
//! (`tier_homomorphism/shared.rs`), mounted here because `Emit` is crate-private.
//!
//! A row is compiled once per pair of operand representations its states hold (an int32 register
//! is an `i32` value, a binary64 register an `f64` one, as the u-jit's register homes are), then run
//! on every state of that pair. The b-jit stencil run is not compared: it needs the stencil build
//! (`QUENCH_NO_STENCILS` unset) and an in-process stencil loader.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::{Operand, Register};
use quench_runtime::staged::{self as staged, Domain, Edge, Float64, Int32, IntFact, NumberFact, NumberOperation};

mod shared {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../quench-runtime/tests/tier_homomorphism/shared.rs"));
}

use shared::{NumberWord, Outcome};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// What the compiled row returns: it left at its miss edge, or it defined an int32 or a binary64
/// word, which it stored through its result pointer.
const LEFT: u64 = 0;
const DEFINED_INT32: u64 = 1;
const DEFINED_FLOAT64: u64 = 2;

/// A register frame over Cranelift values: registers 0 and 1 hold the operands in the
/// representation the state names, and the row's write of register 2 is kept with its fact. Its
/// reads are `shared::NumberFrame`'s, emitted: an int32 as itself, the binary64 of an int32
/// (`i32_to_f64`), and the int32 of a binary64 as Rust's `as` (saturating, NaN zero), which no
/// generated state reaches.
pub(super) struct EmitNumberFrame {
    pub(super) values: [Option<(ir::Value, Domain)>; 3],
    pub(super) miss: ir::Block,
}

impl EmitNumberFrame {
    fn operand(&self, operand: Operand) -> (ir::Value, Domain) {
        self.values[usize::from(operand.register_index().expect("a register operand"))].expect("an operand register")
    }
}

impl staged::Frame<Emit<'_, '_>> for EmitNumberFrame {
    type Fact = Domain;
    type Control = Edge;

    fn fact(&self, register: Register) -> Domain { self.values[usize::from(register)].map_or(Domain::Int32, |(_, fact)| fact) }
    fn read(&mut self, _stage: &mut Emit<'_, '_>, register: Register) -> ir::Value {
        self.values[usize::from(register)].expect("a defined register").0
    }
    fn write(&mut self, _stage: &mut Emit<'_, '_>, register: Register, word: ir::Value, fact: Domain) {
        self.values[usize::from(register)] = Some((word, fact));
    }
    fn this(&mut self, _stage: &mut Emit<'_, '_>) -> ir::Value { unreachable!("Number operations read no this") }
    fn define_this(&mut self, _stage: &mut Emit<'_, '_>, _destination: Register, _this: ir::Value) {
        unreachable!("Number operations read no this")
    }
    fn constant(&mut self, _stage: &mut Emit<'_, '_>, _constant: staged::Constant) -> (ir::Value, Domain) {
        unreachable!("the rows read registers only")
    }
    fn truthiness(&mut self, _stage: &mut Emit<'_, '_>, _register: Register) -> Option<ir::Value> {
        unreachable!("a Number operation decides no truthiness")
    }
    fn branch(&mut self, _stage: &mut Emit<'_, '_>, _condition: ir::Value) -> Edge { unreachable!("a Number operation branches nowhere") }
    fn jump(&mut self, _stage: &mut Emit<'_, '_>) -> Edge { unreachable!("a Number operation jumps nowhere") }
    /// The tagged Boolean the u-jit's register frame makes (`lane_region::boolean_word`), read by the
    /// static predictor's `RelationValue` row (`ir_predict_tests.rs`); the agreement test runs no such row.
    fn boolean(&mut self, stage: &mut Emit<'_, '_>, condition: ir::Value) -> (ir::Value, Domain) {
        (crate::compiled::lane_region::boolean_word(stage.builder, condition), Domain::Int32)
    }
}

impl staged::GlobalFrame<Emit<'_, '_>> for EmitNumberFrame {}
impl staged::CallFrame<Emit<'_, '_>> for EmitNumberFrame {}
impl staged::PropertyFrame<Emit<'_, '_>> for EmitNumberFrame {
    type Plans = staged::NoPlans;
    fn plans(&self) -> staged::NoPlans { staged::NoPlans }
}
impl staged::ElementFrame<Emit<'_, '_>> for EmitNumberFrame {}

impl staged::NumericFrame<Emit<'_, '_>> for EmitNumberFrame {
    fn int_operand(&mut self, stage: &mut Emit<'_, '_>, operand: Operand, _miss: ir::Block) -> Option<Int32<ir::Value>> {
        let word = match self.operand(operand) {
            (word, Domain::Int32) => word,
            (word, Domain::Float64) => stage.raw_unary(Unary::ToSignedSaturating(Lane::I32), word),
        };
        Some(Int32::by_fact(word, IntFact::int32_home()))
    }
    fn number_operand(&mut self, stage: &mut Emit<'_, '_>, operand: Operand, _miss: ir::Block) -> Option<Float64<ir::Value>> {
        let word = match self.operand(operand) {
            (word, Domain::Int32) => stage.i32_to_f64(word),
            (word, Domain::Float64) => word,
        };
        Some(Float64::by_fact(word, NumberFact::float64_home()))
    }
    fn prove_number(&mut self, _stage: &mut Emit<'_, '_>, _register: Register, _miss: ir::Block) -> Option<()> { Some(()) }
    /// An int32 register is a proven int32 (only `IntCase`, a row of the static predictor, asks).
    fn held_int(&mut self, _stage: &mut Emit<'_, '_>, operand: Operand) -> Option<Int32<ir::Value>> {
        let (word, domain) = self.operand(operand);
        (domain == Domain::Int32).then(|| Int32::by_fact(word, IntFact::int32_home()))
    }
    fn miss(&self) -> ir::Block { self.miss }
    fn int_fact(&self) -> Domain { Domain::Int32 }
    fn number_fact(&self) -> Domain { Domain::Float64 }
}

/// `value` as the 64-bit word the result pointer receives: an integer zero-extended, a binary64's bits.
fn result_bits(builder: &mut FunctionBuilder<'_>, value: ir::Value) -> ir::Value {
    match builder.func.dfg.value_type(value) {
        types::I64 => value,
        types::F64 => builder.ins().bitcast(types::I64, MemFlagsData::new(), value),
        _ => builder.ins().uextend(types::I64, value),
    }
}

/// Native code running `operation` on operands of the representations `kinds`, passed as words
/// (`fn(left, right, result) -> status`): `LEFT` where it leaves, else the defined word's
/// representation with the word stored at `result`. A relation defines its predicate as an int32.
fn compiled_row(isa: &dyn cranelift_codegen::isa::TargetIsa, operation: NumberOperation, kinds: [Domain; 2]) -> CodeMemory {
    let mut signature = Signature::new(isa.default_call_conv());
    signature.params.extend([AbiParam::new(types::I64); 3]);
    signature.returns.push(AbiParam::new(types::I64));
    let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let [left, right, result] = builder.block_params(entry).try_into().unwrap();
    let miss = builder.create_block();
    let mut stage = Emit { builder: &mut builder, property_values: None, gate: None, payload: None, facts: Default::default(), fresh: false };
    let [left, right] = [(left, kinds[0]), (right, kinds[1])].map(|(word, kind)| match kind {
        Domain::Int32 => (stage.narrow(word), kind),
        Domain::Float64 => (stage.f64_from_bits(word), kind),
    });
    let mut frame = EmitNumberFrame { values: [Some(left), Some(right), None], miss };
    let step = staged::step_of::<{ staged::op_kind::NUMBER }, _, _>(&mut stage, &mut frame,
        staged::Operation::Number(operation));
    let defined = match step {
        staged::Step::Value(predicate) if matches!(operation, NumberOperation::Relation { .. }) => Some((predicate, Domain::Int32)),
        staged::Step::Value(_) => Some(frame.values[2].expect("a row defines its destination")),
        staged::Step::Unresolved => None,
        staged::Step::Control(_) => panic!("a Number row transfers no control"),
    };
    match defined {
        Some((word, fact)) => {
            let bits = result_bits(&mut builder, word);
            builder.ins().store(MemFlagsData::trusted(), bits, result, 0);
            let status = if fact == Domain::Int32 { DEFINED_INT32 } else { DEFINED_FLOAT64 };
            let status = builder.ins().iconst(types::I64, status as i64);
            builder.ins().return_(&[status]);
        }
        None => { builder.ins().jump(miss, &[]); }
    }
    builder.switch_to_block(miss);
    let left = builder.ins().iconst(types::I64, LEFT as i64);
    builder.ins().return_(&[left]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    compile_native_function(native, isa, usize::MAX, CodeMemory::new).unwrap()
}

/// The outcome of the compiled row `memory` on `left` and `right`, read as `shared::run` reads
/// Concrete's: a relation's predicate is the int32 `0` or `1`.
fn run_compiled(memory: &CodeMemory, relation: bool, left: NumberWord, right: NumberWord) -> Outcome {
    // SAFETY: the function reads its word arguments and writes one word through `result`, a live local.
    let row = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64, *mut u64) -> u64>(memory.pointer()) };
    let mut result = 0_u64;
    match unsafe { row(left.bits(), right.bits(), &mut result) } {
        LEFT => None,
        DEFINED_INT32 if relation => Some(NumberWord::Int(i32::from(result as u32 != 0))),
        DEFINED_INT32 => Some(NumberWord::Int(result as u32 as i32)),
        DEFINED_FLOAT64 => Some(NumberWord::Float(f64::from_bits(result))),
        status => panic!("a compiled row returned status {status}"),
    }
}

/// The index of a pair of operand representations among the four a row may be compiled for.
fn kinds_index([left, right]: [Domain; 2]) -> usize { usize::from(left == Domain::Float64) * 2 + usize::from(right == Domain::Float64) }

/// The rows (`shared::Row::label`) where Emit may define, as a binary64, the Number Concrete defines
/// as an int32. A representation split is a real disagreement: downstream facts depend on int32-ness.
/// Under `Overflow::Widen`, `staged/numeric.rs` `int_arithmetic_result` writes the int32 only where
/// `stage.known(widen)` decides it (Concrete); Emit, which cannot, writes the binary64 of every
/// result. The fix belongs in `int_arithmetic_result`: Emit should branch on the overflow flag and
/// write the int32 when it fits, as Concrete does (TOD-2177 finding, owner Engine B). Shrink-only:
/// a listed row that no longer splits fails the test until it is removed here.
const WIDEN_ALLOWANCE: [&str; 8] = [
    "Add Int32 overflow Widen zero Exit",
    "Add Int32 overflow Widen zero Ignore",
    "Subtract Int32 overflow Widen zero Exit",
    "Subtract Int32 overflow Widen zero Ignore",
    "Multiply Int32 overflow Widen zero Exit",
    "Multiply Int32 overflow Widen zero Ignore",
    "IncDec decrement false overflow Widen",
    "IncDec decrement true overflow Widen",
];

/// The split `WIDEN_ALLOWANCE` admits: Concrete's int32 and Emit's binary64 of the same Number.
fn widen_split(expected: Outcome, actual: Outcome) -> bool {
    matches!((expected, actual), (Some(NumberWord::Int(int)), Some(NumberWord::Float(float)))
        if f64::from(int).to_bits() == float.to_bits())
}

/// Every row, every state: the Emit run of the row agrees with Concrete's (`shared::same`: the same
/// int32, the same binary64 bits with one NaN, or a leave on both), except the Widen split of the
/// rows in `WIDEN_ALLOWANCE`, each of which must still show it. Disagreements are all collected.
#[test]
fn emitted_number_rows_agree_with_concrete() {
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let cases = shared::cases();
    let (mut rows, mut states, mut compiled, mut leaves, mut allowed) = (0, 0, 0, 0, 0);
    let (mut mismatches, mut stale) = (Vec::new(), Vec::new());
    let mut split_rows = [false; WIDEN_ALLOWANCE.len()];
    for row in shared::rows(&mut shared::Rng(shared::seed()), cases) {
        rows += 1;
        let relation = matches!(row.operation, NumberOperation::Relation { .. });
        let allowance = WIDEN_ALLOWANCE.iter().position(|label| *label == row.label);
        let mut code: [Option<CodeMemory>; 4] = Default::default();
        for &(left, right) in &row.states {
            let kinds = [left.domain(), right.domain()];
            let memory = code[kinds_index(kinds)].get_or_insert_with(|| {
                compiled += 1;
                compiled_row(&*isa, row.operation, kinds)
            });
            let expected = shared::run(left, right, row.operation);
            let actual = run_compiled(memory, relation, left, right);
            states += 1;
            leaves += usize::from(expected.is_none());
            if shared::same(expected, actual) {
                continue;
            }
            if let Some(index) = allowance.filter(|_| widen_split(expected, actual)) {
                split_rows[index] = true;
                allowed += 1;
                continue;
            }
            mismatches.push(format!("{}: left {left:?} right {right:?}: Concrete {expected:?}, Emit {actual:?}", row.label));
        }
    }
    for (label, split) in WIDEN_ALLOWANCE.iter().zip(split_rows) {
        if !split {
            stale.push(format!("remove {label} from the Widen allowance"));
        }
    }
    assert_eq!(rows, shared::expected_row_count(), "the enumerated row count changed");
    eprintln!("tier_homomorphism (Emit): {rows} rows compiled as {compiled} functions, {states} states (seed {:#x}, {cases} random \
        per row), {leaves} of them Concrete leaves, {allowed} allowed Widen splits", shared::seed());
    assert!(stale.is_empty(), "{stale:#?}");
    assert!(mismatches.is_empty(), "{} disagreements, first {:#?}", mismatches.len(), &mismatches[..mismatches.len().min(12)]);
}
