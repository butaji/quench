//! The element rows over a loop's hoisted array, read in the IR the lane lowers them to.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::Class;
use cranelift_codegen::{dominator_tree::DominatorTree, flowgraph::ControlFlowGraph, loop_analysis::LoopAnalysis};

pub(crate) const SUM: &str = "function sum(a, n) { var s = 0; for (var i = 0; i < n; i++) { s += a[i]; } return s; }";
const FILL: &str = "function fill(a, n) { for (var i = 0; i < n; i++) { a[i] = i; } }";

/// The loop region of the function `source` declares, over dense views; `left` records that its element stores left before (`generic`).
fn region(source: &str, left: bool) -> ir::Function {
    region_over(source, left, ElementViews::Dense)
}

/// `region` over the element `views`.
fn region_over(source: &str, left: bool, views: ElementViews) -> ir::Function {
    let program = quench_runtime::Engine::specialize(source, "element-rows.js").unwrap();
    let unit = super::super::lane_unit::unit(&program, None, 1, program.function(1), &|_, _| Vec::new(), &|_, _| None, &|_| None).unwrap();
    let header = quench_runtime::bytecode::back_edges_of(unit.instructions.iter().copied()).map(|(_, header)| header).min().unwrap();
    let at = |class: Class| -> Vec<usize> { unit.instructions.iter().enumerate()
        .filter(|(_, instruction)| instruction.op().class().eq(&class)).map(|(pc, _)| pc).collect() };
    let generic = if left { at(Class::SetIndex) } else { Vec::new() };
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let addresses = LaneHelper::addresses();
    lower(&program, 1, &unit, LaneHelpers::Process(addresses), &[], &Default::default(), VmFacts::STATIC,
        &generic, &BTreeMap::new(), &[], &*isa, &[], Coverage::PrefixFrom(header), Completion::Body, views, true).expect("a loop region").function
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

/// The calls to the element store's host helper in `body`.
fn index_set_calls(function: &ir::Function, body: &[ir::Inst]) -> usize {
    let helper = LaneHelper::addresses()[LaneHelper::IndexSet as usize] as usize;
    body.iter().filter(|&&inst| function.dfg.insts[inst].opcode() == ir::Opcode::CallIndirect
        && is_constant(function, function.dfg.inst_args(inst)[0], helper as u64)).count()
}

#[test]
fn a_hoisted_store_calls_no_helper_in_its_loop_until_its_site_has_missed() {
    let unobserved = region(FILL, false);
    assert_eq!(index_set_calls(&unobserved, &loop_body(&unobserved)), 0, "a never-missed store exits cold: {unobserved}");
    let missed = region(FILL, true);
    assert!(index_set_calls(&missed, &loop_body(&missed)) > 0, "a recorded miss plans the helper arm: {missed}");
}

/// Whether `body` loads a record table base as a read-only, movable word.
fn hoistable_loads(function: &ir::Function, body: &[ir::Inst]) -> usize {
    body.iter().filter(|&&inst| matches!(function.dfg.insts[inst], ir::InstructionData::Load { .. })
        && function.dfg.insts[inst].memflags_data(&function.dfg).is_some_and(|flags| flags.readonly() && flags.can_move())).count()
}

#[test]
fn a_numeric_loop_over_an_array_loads_the_record_base_read_only_and_movable() {
    let numeric = region("function sum(a) { var s = 0; for (var i = 0; i < 100; i++) { s += a[i]; } return s; }", false);
    assert!(hoistable_loads(&numeric, &loop_body(&numeric)) > 0, "the record base is hoisted out of the loop: {numeric}");
    let storing = region(FILL, false);
    assert_eq!(hoistable_loads(&storing, &loop_body(&storing)), 0, "a loop that stores moves arrays: {storing}");
}

/// The comparisons in `body` with the constant `bits`, by `icmp` or `icmp_imm`.
fn compares_with(function: &ir::Function, body: &[ir::Inst], bits: u64) -> usize {
    body.iter().filter(|&&inst| match function.dfg.insts[inst] {
        ir::InstructionData::IntCompare { args, .. } => args.iter().any(|&arg| is_constant(function, arg, bits)),
        _ => false,
    }).count()
}

#[test]
fn a_float64_sum_loop_has_no_kind_dispatch_in_its_body_and_checks_the_kind_once_before_it() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    // Kinds whose discriminants no other comparison of a sum loop names.
    let kinds = [Kind::Int32, Kind::Float32, Kind::Float64].map(|kind| kind as u64);
    let dispatch = |function: &ir::Function| -> usize {
        let body = loop_body(function);
        kinds.iter().map(|&kind| compares_with(function, &body, kind)).sum()
    };
    let folded = region_over(SUM, false, ElementViews::Typed(Some(Kind::Float64)));
    assert_eq!(dispatch(&folded), 0, "a site kind the proof compared is a constant of the loop: {folded}");
    let body = loop_body(&folded);
    let before: Vec<ir::Inst> = folded.layout.blocks().flat_map(|block| folded.layout.block_insts(block))
        .filter(|inst| !body.contains(inst)).collect();
    assert!(compares_with(&folded, &before, Kind::Float64 as u64) >= 1, "the kind is checked before the loop: {folded}");
    let dynamic = region_over(SUM, false, ElementViews::Typed(None));
    assert!(dispatch(&dynamic) >= kinds.len(), "a view of no site kind dispatches on the record's in the loop: {dynamic}");
}

/// The conversions between a Number and an integer in `body`.
fn conversions(function: &ir::Function, body: &[ir::Inst]) -> usize {
    body.iter().filter(|&&inst| matches!(function.dfg.insts[inst].opcode(),
        ir::Opcode::FcvtFromSint | ir::Opcode::FcvtToSint | ir::Opcode::FcvtToSintSat
            | ir::Opcode::FcvtFromUint | ir::Opcode::FcvtToUint | ir::Opcode::FcvtToUintSat)).count()
}

#[test]
fn a_uint8_load_is_an_int32_word_that_no_conversion_touches() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    let crc = "function crc(d) { var c = 0; for (var i = 0; i < 100; i++) { c = (c ^ d[i]) & 255; } return c; }";
    let bytes = region_over(crc, false, ElementViews::Typed(Some(Kind::Uint8)));
    assert_eq!(conversions(&bytes, &loop_body(&bytes)), 0, "a byte is read as the int32 it is: {bytes}");
}

const MATMUL: &str = "function mm(a, b, c, n) { for (var i = 0; i < n; i++) for (var j = 0; j < n; j++) { var s = 0; \
    for (var k = 0; k < n; k++) s += a[i * n + k] * b[k * n + j]; c[i * n + j] = s; } }";

#[test]
fn a_loop_nest_proves_the_views_of_its_arrays_once_where_their_bases_are_invariant() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    let function = region_over(MATMUL, false, ElementViews::Typed(Some(Kind::Float64)));
    let kind = Kind::Float64 as u64;
    let everything: Vec<ir::Inst> = function.layout.blocks().flat_map(|block| function.layout.block_insts(block)).collect();
    assert!(compares_with(&function, &everything, kind) >= 3, "each of `a`, `b` and `c` is proved: {function}");
    assert_eq!(compares_with(&function, &loop_body(&function), kind), 0,
        "no loop of the nest proves a view again per iteration of the one around it: {function}");
}

