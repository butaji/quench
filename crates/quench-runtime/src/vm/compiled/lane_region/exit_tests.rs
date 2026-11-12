//! Leave blocks store only the registers their frame slots lack (`exits.rs`): the cold code of
//! a loop is bounded by what the exits need, no box is built that no store or result reads, and
//! a register whose tagged view is unavailable is boxed by the leave that stores it, in its cold
//! block, or by the first tagged read after (`frame::register_tagged`).

use super::*;
use super::home_tests::{lower_loop, stores};

/// The cold blocks that store registers, and the stores they hold.
fn exit_stores(function: &ir::Function) -> (usize, usize) {
    let blocks = function.layout.blocks().filter(|block| function.layout.is_cold(*block)
        && function.layout.block_insts(*block).any(|inst| matches!(function.dfg.insts[inst], ir::InstructionData::Store { .. }))).count();
    (blocks, stores(function, true).len())
}

/// The boxes of an int32 into tagged bits: each widens the int before tagging it.
fn is_box(function: &ir::Function, inst: ir::Inst) -> bool { function.dfg.insts[inst].opcode() == ir::Opcode::Uextend }

/// The boxes in the hot blocks, or in the cold ones.
fn boxes(function: &ir::Function, cold: bool) -> usize {
    function.layout.blocks().filter(|block| function.layout.is_cold(*block) == cold)
        .flat_map(|block| function.layout.block_insts(block)).filter(|inst| is_box(function, *inst)).count()
}

/// The boxes in the blocks of the loop, which run once per iteration.
fn loop_boxes(function: &ir::Function) -> usize {
    let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(function);
    let mut dominators = cranelift_codegen::dominator_tree::DominatorTree::new();
    dominators.compute(function, &cfg);
    let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
    loops.compute(function, &cfg, &dominators);
    function.layout.blocks().filter(|block| loops.innermost_loop(*block).is_some())
        .flat_map(|block| function.layout.block_insts(block)).filter(|inst| is_box(function, *inst)).count()
}

#[test]
fn a_numeric_loop_boxes_only_what_a_store_or_the_result_reads() {
    let function = lower_loop("function run(n){let t=0,i=0;while(i<n){t=(t+i)|0;i=(i+1)|0;}return t;}", "exit-numeric.js");
    let (_, cold) = exit_stores(&function);
    let hot = stores(&function, false).len();
    assert!(boxes(&function, true) + boxes(&function, false) <= cold + hot + 1, "a box no store reads: {function}");
}

/// The registers the call loop writes: `t`, `i` and the call's result.
const CALL_LOOP_WRITES: usize = 3;

#[test]
fn a_call_loop_exits_store_fewer_registers_than_it_writes() {
    let function = lower_loop("function run(g,n){let t=0,i=0;while(i<n){g(i);g(i);t=(t+i)|0;i=(i+1)|0;}return t;}", "exit-calls.js");
    let (blocks, cold) = exit_stores(&function);
    assert!(blocks > 0 && cold < blocks * CALL_LOOP_WRITES, "every exit stored every written register: {function}");
}

/// An int loop that calls nothing: `t` and `i` stay int32 through the loop header and are read tagged
/// only by what leaves, so no iteration boxes.
const NUMERIC_LOOP: &str = "function run(n){let t=0,i=0;while(i<n){t=(t+i)|0;i=(i+1)|0;}return t;}";

#[test]
fn a_loop_carried_int_is_boxed_by_its_leaves_not_by_the_loop() {
    let function = lower_loop(NUMERIC_LOOP, "exit-carried.js");
    assert_eq!(loop_boxes(&function), 0, "an iteration boxed a register no tagged read needs: {function}");
    assert!(boxes(&function, true) > 0, "no leave boxed the int it stores from its unboxed view: {function}");
}

#[test]
fn a_tagged_read_after_the_loop_header_boxes_the_int_where_it_reads() {
    let function = lower_loop("function run(g,n){let i=0;while(i<n){g(i);i=(i+1)|0;}return i;}", "exit-rematerialized.js");
    assert!(loop_boxes(&function) > 0, "the call read `i` tagged without a box: {function}");
}
