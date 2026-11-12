//! Leave blocks store only the registers their frame slots lack (`exits.rs`): the cold code of
//! a loop is bounded by what the exits need, and no box is built that no store or result reads.

use super::*;
use super::home_tests::{lower_loop, stores};

/// The cold blocks that store registers, and the stores they hold.
fn exit_stores(function: &ir::Function) -> (usize, usize) {
    let blocks = function.layout.blocks().filter(|block| function.layout.is_cold(*block)
        && function.layout.block_insts(*block).any(|inst| matches!(function.dfg.insts[inst], ir::InstructionData::Store { .. }))).count();
    (blocks, stores(function, true).len())
}

/// The boxes of an int32 into tagged bits: each widens the int before tagging it.
fn boxes(function: &ir::Function) -> usize {
    function.layout.blocks().flat_map(|block| function.layout.block_insts(block))
        .filter(|inst| function.dfg.insts[*inst].opcode() == ir::Opcode::Uextend).count()
}

#[test]
fn a_numeric_loop_boxes_only_what_a_store_or_the_result_reads() {
    let function = lower_loop("function run(n){let t=0,i=0;while(i<n){t=(t+i)|0;i=(i+1)|0;}return t;}", "exit-numeric.js");
    let (_, cold) = exit_stores(&function);
    let hot = stores(&function, false).len();
    assert!(boxes(&function) <= cold + hot + 1, "a box no store reads: {function}");
}

/// The registers the call loop writes: `t`, `i` and the call's result.
const CALL_LOOP_WRITES: usize = 3;

#[test]
fn a_call_loop_exits_store_fewer_registers_than_it_writes() {
    let function = lower_loop("function run(g,n){let t=0,i=0;while(i<n){g(i);g(i);t=(t+i)|0;i=(i+1)|0;}return t;}", "exit-calls.js");
    let (blocks, cold) = exit_stores(&function);
    assert!(blocks > 0 && cold < blocks * CALL_LOOP_WRITES, "every exit stored every written register: {function}");
}
