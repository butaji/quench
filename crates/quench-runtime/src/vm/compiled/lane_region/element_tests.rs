//! The element rows over a loop's hoisted array, read in the IR the lane lowers them to.

use super::*;
use crate::bytecode::Class;
use cranelift_codegen::{dominator_tree::DominatorTree, flowgraph::ControlFlowGraph, loop_analysis::LoopAnalysis};

const SUM: &str = "function sum(a, n) { var s = 0; for (var i = 0; i < n; i++) { s += a[i]; } return s; }";
const FILL: &str = "function fill(a, n) { for (var i = 0; i < n; i++) { a[i] = i; } }";

/// The loop region of the function `source` declares, over dense views; `observed` types its element
/// reads Numbers (`ObservedRead`), `left` records that its element stores left before (`generic`).
fn region(source: &str, observed: bool, left: bool) -> ir::Function {
    let program = crate::Engine::specialize(source, "element-rows.js").unwrap();
    let unit = super::super::lane_unit::unit(&program, 1, program.function(1), &|_, _| Vec::new(), &|_| None).unwrap();
    let header = unit.instructions.iter().enumerate().filter_map(|(pc, instruction)|
        instruction.op().control_flow_layout().jumps().then(|| instruction.jump_target() as usize).filter(|target| *target <= pc))
        .min().unwrap();
    let at = |class: Class| -> Vec<usize> { unit.instructions.iter().enumerate()
        .filter(|(_, instruction)| instruction.op().class().eq(class)).map(|(pc, _)| pc).collect() };
    let reads: Vec<_> = at(Class::GetIndex).into_iter().filter(|_| observed)
        .map(|pc| super::super::facts::ObservedRead { pc, ty: Type::NUMBER }).collect();
    let generic = if left { at(Class::SetIndex) } else { Vec::new() };
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let addresses = LaneHelper::addresses::<crate::test_host::TestHost>();
    lower(&program, 1, &unit, LaneHelpers::Process(addresses), &BTreeMap::new(), &Default::default(), VmFacts::STATIC,
        &generic, &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &reads, &[], &[], &[],
        Coverage::PrefixFrom(header), Completion::StatusAndPc, ElementViews::Dense).expect("a loop region").function
}

/// The instructions of `function`'s loop blocks.
fn loop_body(function: &ir::Function) -> Vec<ir::Inst> {
    let cfg = ControlFlowGraph::with_function(function);
    let dom = DominatorTree::with_function(function, &cfg);
    let mut loops = LoopAnalysis::new();
    loops.compute(function, &cfg, &dom);
    function.layout.blocks().filter(|&block| loops.innermost_loop(block).is_some())
        .flat_map(|block| function.layout.block_insts(block)).collect()
}

/// Whether `value` is the integer constant `bits`.
fn is_constant(function: &ir::Function, value: ir::Value, bits: u64) -> bool {
    let ir::ValueDef::Result(def, _) = function.dfg.value_def(function.dfg.resolve_aliases(value)) else { return false };
    matches!(function.dfg.insts[def], ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } if imm.bits() as u64 == bits)
}

/// The comparisons in `body` against the hole (`Value::DELETED`).
fn hole_tests(function: &ir::Function, body: &[ir::Inst]) -> usize {
    body.iter().filter(|&&inst| match function.dfg.insts[inst] {
        ir::InstructionData::IntCompare { args, .. } => args.iter().any(|&arg| is_constant(function, arg, Value::DELETED.to_bits())),
        _ => false,
    }).count()
}

#[test]
fn a_hoisted_number_read_has_no_hole_test_and_any_other_read_keeps_it() {
    let observed = region(SUM, true, false);
    assert_eq!(hole_tests(&observed, &loop_body(&observed)), 0, "a packed view's read has no hole test: {observed}");
    let unobserved = region(SUM, false, false);
    assert!(hole_tests(&unobserved, &loop_body(&unobserved)) > 0, "a read no Number proof follows tests the hole: {unobserved}");
}

/// The calls to the element store's host helper in `body`.
fn index_set_calls(function: &ir::Function, body: &[ir::Inst]) -> usize {
    let helper = LaneHelper::addresses::<crate::test_host::TestHost>()[LaneHelper::IndexSet as usize] as usize;
    body.iter().filter(|&&inst| function.dfg.insts[inst].opcode() == ir::Opcode::CallIndirect
        && is_constant(function, function.dfg.inst_args(inst)[0], helper as u64)).count()
}

#[test]
fn a_hoisted_store_calls_no_helper_in_its_loop_until_its_site_has_missed() {
    let unobserved = region(FILL, false, false);
    assert_eq!(index_set_calls(&unobserved, &loop_body(&unobserved)), 0, "a never-missed store exits cold: {unobserved}");
    let missed = region(FILL, false, true);
    assert!(index_set_calls(&missed, &loop_body(&missed)) > 0, "a recorded miss plans the helper arm: {missed}");
}
