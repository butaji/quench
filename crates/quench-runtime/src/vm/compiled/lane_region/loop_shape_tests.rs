//! The shape of a counted typed-array sum loop region (`Coverage::Loop`): one latch carrying
//! only what the loop writes, no join inside it, and a hot path of at most a few blocks.

use super::element_tests::SUM;
use super::*;
use cranelift_codegen::{dominator_tree::DominatorTree, flowgraph::ControlFlowGraph, loop_analysis::LoopAnalysis};

/// The blocks a loop region's hot path may span: the bounds test, the element and the sum, the
/// fault poll and the loop's own test.
const HOT_LOOP_BLOCKS: usize = 4;

/// The region of `SUM`'s loop over a Float64Array, entered at its header as the loop tier enters it
/// (every register but the array holding the int32 it had), and how many registers the loop writes.
fn sum_loop() -> (ir::Function, usize) {
    use crate::heap::TypedArrayKind as Kind;
    let program = crate::Engine::specialize(SUM, "loop-shape.js").unwrap();
    let unit = super::super::lane_unit::unit(&program, 1, program.function(1), &|_, _| Vec::new(), &|_| None).unwrap();
    let (header, backedge) = unit.instructions.iter().enumerate().find_map(|(pc, instruction)|
        (instruction.op().control_flow_layout().jumps() && instruction.jump_target() as usize <= pc)
            .then(|| (instruction.jump_target() as usize, pc))).expect("the function has a loop");
    let written: BTreeSet<Register> = unit.instructions[header..=backedge].iter().flat_map(|instruction| writes(*instruction)).collect();
    let entering: Vec<_> = (1..unit.registers).map(|register| (register, IntRange::FULL)).collect();
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let addresses = LaneHelper::addresses();
    let region = lower(&program, 1, &unit, LaneHelpers::Process(addresses), &[], &Default::default(), VmFacts::STATIC,
        &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], (&entering, &[], &[]), &[], &[],
        Coverage::Loop { header, backedge, poll: true }, Completion::Body, ElementViews::Typed(Some(Kind::Float64)), true)
        .expect("the loop lowers");
    (region.function, written.len())
}

/// A loop's header, its blocks, and the edges entering the header from inside it.
struct Shape { header: ir::Block, blocks: Vec<ir::Block>, latches: usize }

impl Shape {
    fn of(function: &ir::Function) -> Self {
        let cfg = ControlFlowGraph::with_function(function);
        let dominators = DominatorTree::with_function(function, &cfg);
        let mut loops = LoopAnalysis::new();
        loops.compute(function, &cfg, &dominators);
        assert_eq!(loops.loops().count(), 1, "one native loop: {function}");
        let header = loops.loop_header(loops.loops().next().unwrap());
        let blocks: Vec<_> = function.layout.blocks().filter(|&block| loops.innermost_loop(block).is_some()).collect();
        let latches = cfg.pred_iter(header).filter(|edge| blocks.contains(&edge.block)).count();
        Self { header, blocks, latches }
    }
}

#[test]
fn a_counted_sum_loop_has_one_latch_carrying_only_what_it_writes() {
    if !super::super::FAULT_POLL_SUPPORTED { return; }
    let (function, written) = sum_loop();
    let shape = Shape::of(&function);
    assert_eq!(shape.latches, 1, "a single latch: {function}");
    // The trip count rides beside the registers the loop writes.
    let carried = function.dfg.block_params(shape.header).len();
    assert!(carried <= written + 1, "the header carries {carried} values for {written} written registers: {function}");
    for &block in shape.blocks.iter().filter(|&&block| block != shape.header) {
        assert!(function.dfg.block_params(block).is_empty(), "no join inside the loop: {block}: {function}");
    }
    let hot = shape.blocks.iter().filter(|&&block| !function.layout.is_cold(block)
        && function.layout.block_insts(block).any(|inst| !function.dfg.insts[inst].opcode().is_terminator())).count();
    assert!(hot <= HOT_LOOP_BLOCKS, "{hot} hot blocks in the loop: {function}");
}

/// Every value some instruction of `function` reads.
fn read_values(function: &ir::Function) -> BTreeSet<ir::Value> {
    function.layout.blocks().flat_map(|block| function.layout.block_insts(block))
        .flat_map(|inst| function.dfg.inst_values(inst)).map(|value| function.dfg.resolve_aliases(value)).collect()
}

#[test]
fn a_counted_sum_loop_stores_no_counter_and_polls_by_fault() {
    if !super::super::FAULT_POLL_SUPPORTED { return; }
    let (function, _) = sum_loop();
    let shape = Shape::of(&function);
    let body: Vec<ir::Inst> = shape.blocks.iter().flat_map(|&block| function.layout.block_insts(block)).collect();
    assert!(body.iter().all(|&inst| !function.dfg.insts[inst].opcode().can_store()),
        "the trip count is a register, flushed where the run leaves: {function}");
    let read = read_values(&function);
    let polls: Vec<ir::Inst> = body.iter().copied().filter(|&inst| function.dfg.insts[inst].opcode() == ir::Opcode::AtomicLoad).collect();
    assert!(!polls.is_empty(), "the loop polls: {function}");
    for poll in polls {
        let byte = function.dfg.resolve_aliases(function.dfg.first_result(poll));
        assert!(!read.contains(&byte), "a poll by fault reads no flag byte: {poll}: {function}");
    }
}
