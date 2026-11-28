//! The static Ir predictor (S1.12, TOD-2153): each staged Number row's u-jit projection, compiled through
//! `Emit` as a one-row Cranelift function on the row's common operand facts, and the machine instructions
//! of its fast path counted, so the 3C+6 bound (S1.17) and the tier order can be judged for a row before
//! any benchmark op measures it. The rows file `crates/quench-tier-corpus/ir_predict.txt` is generated
//! here (`QUENCH_IR_PREDICT_WRITE=1`) and held to the code on the host ISA it names; quench-gate's
//! `ir-predict` step joins it with the C twins, and the ir-budget step's c-bound section reads it for a
//! row whose kernel this run did not measure.
//!
//! A row's operands arrive as Cranelift values of their register homes' types (an int32 an `i32`, a
//! binary64 an `f64`: `tier_homomorphism_tests::EmitNumberFrame`), its defined word is stored through a
//! result pointer, and its miss edge traps. The fast path is the fewest instructions from the entry to the
//! return, decoded as x86-64: a path that reaches the trap leaves. The same function defining its first
//! operand unchanged is the harness (prologue, store, return), whose path is subtracted. A call on the
//! path counts as one instruction: the callee is not followed.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::tier_homomorphism_tests::EmitNumberFrame;
use iced_x86::{Decoder, DecoderOptions, FlowControl, Instruction};
use quench_runtime::bytecode::{Operand, Register};
use quench_runtime::staged::{self as staged, Domain, NegativeZero, NumberOperation, Overflow};
use quench_runtime::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation, NumberUnary};
use std::collections::{BTreeSet, VecDeque};

/// The generated predictions, from this crate's manifest directory.
const PREDICTIONS: &str = "../quench-tier-corpus/ir_predict.txt";
/// Every staged row and its C twin (S1.17): each `NumberOperation::` row must have a prediction.
const ROWS: &str = "../quench-tier-corpus/c_twin_rows.txt";
/// `1` rewrites `PREDICTIONS` from the code instead of holding it.
const WRITE: &str = "QUENCH_IR_PREDICT_WRITE";
/// The rows' prefix in the rows file.
const ROW_PREFIX: &str = "NumberOperation::";
/// The header line naming the ISA the counts were taken on.
const ISA_PREFIX: &str = "# isa: ";
const HEADER: &str = "# Static u-jit instruction prediction per staged Number row (S1.12, TOD-2153): `row insns facts`, insns the\n\
# machine instructions of the row's fast path (no leave) on its common operand facts, the harness subtracted, or\n\
# `row - reason`. Generated from the code: QUENCH_IR_PREDICT_WRITE=1 cargo test -p quench-runtime-engine --lib\n\
# ir_predict. Read by quench-gate (step ir-predict; ir-budget section c-bound). Never edit by hand.\n";
/// The register a row defines; its operands are registers 0 and 1.
const DESTINATION: Register = 2;
/// The trap the miss edge ends in: a decoded path reaching it leaves.
const LEAVE_TRAP: u8 = 1;

/// One row as predicted: its operation on its common facts, and the facts' operand representations.
struct Case {
    /// The row as the rows file names it (`NumberOperation::` and the variant; the test holds it to the file).
    row: &'static str,
    operation: NumberOperation,
    kinds: [Domain; 2],
    facts: &'static str,
}

/// The two-operand rows on their common facts (the facts of the row's C twin, `c_twin_rows.txt`).
fn binary_cases() -> Vec<Case> {
    use Domain::{Float64, Int32};
    let (left, right, destination) = (Operand::register(0), Operand::register(1), DESTINATION);
    vec![
        Case { kinds: [Int32; 2], facts: "Add int32, overflow and -0 leave", row: "NumberOperation::Arithmetic", operation: NumberOperation::Arithmetic { destination,
            left, right, operation: NumberArithmetic::Add, domain: Int32, overflow: Overflow::Exit, zero: NegativeZero::Exit } },
        Case { kinds: [Int32; 2], facts: "LessThan int32", row: "NumberOperation::Relation", operation: NumberOperation::Relation { left, right,
            relation: NumberRelation::LessThan, domain: Int32 } },
        Case { kinds: [Float64; 2], facts: "LessThan binary64, Boolean value", row: "NumberOperation::RelationValue", operation: NumberOperation::RelationValue {
            destination, left, right, relation: NumberRelation::LessThan, domain: Float64 } },
        Case { kinds: [Int32; 2], facts: "And int32", row: "NumberOperation::Bitwise", operation: NumberOperation::Bitwise { destination, left, right,
            operation: NumberBitwise::And } },
        Case { kinds: [Int32; 2], facts: "int32, divisor unproven, -0 leaves", row: "NumberOperation::Remainder", operation: NumberOperation::Remainder {
            destination, left, right, domain: Int32, zero: NegativeZero::Exit, divisor_nonzero: false } },
        Case { kinds: [Float64; 2], facts: "max binary64", row: "NumberOperation::Extreme", operation: NumberOperation::Extreme { destination, left, right,
            maximum: true } },
        Case { kinds: [Int32; 2], facts: "int32", row: "NumberOperation::Imul", operation: NumberOperation::Imul { destination, left, right } },
    ]
}

/// The one-operand rows on their common facts.
fn unary_cases() -> Vec<Case> {
    use Domain::{Float64, Int32};
    let (source, destination) = (Operand::register(0), DESTINATION);
    vec![
        Case { kinds: [Int32; 2], facts: "increment int32, overflow leaves", row: "NumberOperation::IncDec", operation: NumberOperation::IncDec { destination,
            source: 0, decrement: false, overflow: Overflow::Exit } },
        Case { kinds: [Int32; 2], facts: "Negate int32", row: "NumberOperation::Unary", operation: NumberOperation::Unary { destination, source,
            operation: NumberUnary::Negate, domain: Int32 } },
        Case { kinds: [Int32; 2], facts: "int32", row: "NumberOperation::BitwiseNot", operation: NumberOperation::BitwiseNot { destination, source } },
        Case { kinds: [Int32; 2], facts: "int32", row: "NumberOperation::ToNumeric", operation: NumberOperation::ToNumeric { destination, source: 0 } },
        Case { kinds: [Float64; 2], facts: "Sqrt binary64", row: "NumberOperation::Math", operation: NumberOperation::Math { destination, source,
            operation: staged::math::FloatUnary::Sqrt } },
        Case { kinds: [Int32; 2], facts: "int32, labels 0..=7", row: "NumberOperation::IntCase", operation: NumberOperation::IntCase { destination, source,
            minimum: 0, maximum: 7, count: 8 } },
    ]
}

/// The word a row defined, or why it defined none on its facts.
fn defined(stage: &mut Emit<'_, '_>, frame: &mut EmitNumberFrame, operation: NumberOperation) -> Result<ir::Value, String> {
    match staged::step_of::<{ staged::op_kind::NUMBER }, _, _>(stage, frame, staged::Operation::Number(operation)) {
        staged::Step::Value(predicate) if matches!(operation, NumberOperation::Relation { .. }) => Ok(predicate),
        staged::Step::Value(_) => frame.values[usize::from(DESTINATION)].map(|(word, _)| word)
            .ok_or_else(|| "defines no destination".to_owned()),
        staged::Step::Unresolved => Err("unresolved on its facts".to_owned()),
        staged::Step::Control(_) => Err("transfers control".to_owned()),
    }
}

/// The code of `operation` (`None`: the harness, defining its first operand) on operands of `kinds`.
fn compiled(isa: &dyn cranelift_codegen::isa::TargetIsa, operation: Option<NumberOperation>, kinds: [Domain; 2]) -> Result<Vec<u8>, String> {
    let home = |kind: Domain| if kind == Domain::Int32 { types::I32 } else { types::F64 };
    let mut signature = Signature::new(isa.default_call_conv());
    signature.params.extend([AbiParam::new(home(kinds[0])), AbiParam::new(home(kinds[1])), AbiParam::new(types::I64)]);
    let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let [left, right, result] = builder.block_params(entry).try_into().unwrap();
    let miss = builder.create_block();
    let mut stage = Emit { builder: &mut builder, property_values: None, gate: None, payload: None };
    let mut frame = EmitNumberFrame { values: [Some((left, kinds[0])), Some((right, kinds[1])), None], miss };
    let word = match operation {
        Some(operation) => defined(&mut stage, &mut frame, operation)?,
        None => left,
    };
    builder.ins().store(MemFlagsData::trusted(), word, result, 0);
    builder.ins().return_(&[]);
    builder.switch_to_block(miss);
    builder.ins().trap(ir::TrapCode::unwrap_user(LEAVE_TRAP));
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    quench_runtime::vm::jit_backend::compile_function(native, isa, None, |compiled, _| Ok(compiled.code_buffer().to_vec()))
        .map_err(|rejection| format!("{rejection:?}"))
}

/// The fewest instructions from the entry of `code` to a return, a trap ending a leaving path; `None`
/// where no return is reached, or an indirect branch's targets are unknown.
fn fast_path(code: &[u8]) -> Option<usize> {
    let instructions: Vec<Instruction> = Decoder::with_ip(u64::BITS, code, 0, DecoderOptions::NONE).into_iter().collect();
    let mut executed = vec![0_usize; instructions.len()];
    executed[0] = 1;
    let mut queue = VecDeque::from([0]);
    while let Some(index) = queue.pop_front() {
        let instruction = &instructions[index];
        let target = || instructions.binary_search_by_key(&instruction.near_branch_target(), Instruction::ip).ok();
        let successors = match instruction.flow_control() {
            FlowControl::Return => return Some(executed[index]),
            FlowControl::Next | FlowControl::Call | FlowControl::IndirectCall => [Some(index + 1), None],
            FlowControl::UnconditionalBranch => [target(), None],
            FlowControl::ConditionalBranch => [Some(index + 1), target()],
            FlowControl::Exception | FlowControl::Interrupt => [None, None],
            FlowControl::IndirectBranch | FlowControl::XbeginXabortXend => return None,
        };
        for next in successors.into_iter().flatten().filter(|&next| next < instructions.len()) {
            if executed[next] == 0 {
                executed[next] = executed[index] + 1;
                queue.push_back(next);
            }
        }
    }
    None
}

/// The predicted fast-path instructions of `case`, the harness on the same operands subtracted.
fn predicted(isa: &dyn cranelift_codegen::isa::TargetIsa, case: &Case) -> Result<usize, String> {
    let path = |operation| fast_path(&compiled(isa, operation, case.kinds)?).ok_or_else(|| "no decodable path to its return".to_owned());
    Ok(path(Some(case.operation))?.saturating_sub(path(None)?))
}

/// The predictions file's text on `isa`.
fn render(isa: &dyn cranelift_codegen::isa::TargetIsa) -> String {
    let flags: Vec<&str> = isa.isa_flags().iter().filter(|flag| flag.as_bool() == Some(true)).map(|flag| flag.name).collect();
    let mut text = format!("{HEADER}{ISA_PREFIX}{} {}\n", isa.triple(), flags.join(","));
    for case in binary_cases().iter().chain(&unary_cases()) {
        let row = case.row;
        match predicted(isa, case) {
            Ok(count) => text.push_str(&format!("{row} {count} {}\n", case.facts)),
            Err(reason) => text.push_str(&format!("{row} - {reason}\n")),
        }
    }
    text
}

/// The rows a file names, `#` lines its header.
fn named_rows(text: &str) -> BTreeSet<&str> {
    text.lines().filter(|line| !line.starts_with('#')).filter_map(|line| line.split(' ').next())
        .filter(|row| row.starts_with(ROW_PREFIX)).collect()
}

/// Every Number row of the rows file is predicted, and the held file is the code's on the ISA it names (a
/// host of other CPU features prints the difference and keeps the file: the counts are that ISA's).
#[test]
fn ir_predict_number_rows_are_predicted_from_their_emit_projection() {
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let text = render(&*isa);
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let rows = std::fs::read_to_string(directory.join(ROWS)).unwrap();
    assert_eq!(named_rows(&text), named_rows(&rows), "every {ROW_PREFIX} row of {ROWS} has a case here, and no other");
    eprintln!("{text}");
    let path = directory.join(PREDICTIONS);
    if std::env::var(WRITE).as_deref() == Ok("1") {
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let held = std::fs::read_to_string(&path).unwrap_or_default();
    let isa_line = |text: &str| text.lines().find(|line| line.starts_with(ISA_PREFIX)).map(str::to_owned);
    if isa_line(&held) != isa_line(&text) {
        eprintln!("ir_predict: {PREDICTIONS} holds counts of {:?}, this host is {:?}: not compared", isa_line(&held), isa_line(&text));
        return;
    }
    assert_eq!(held, text, "{PREDICTIONS} is stale: rerun with {WRITE}=1");
}
