//! Residual counts of a compiled hot loop, the measure of execution law 4 (`quench-bench
//! --residuals`, kernels and specs in `quench-tier-corpus`).
//!
//! Under `QUENCH_TRACE=residuals` each loop the tiers compile reports one line on stderr:
//! `QUENCH_RESIDUALS tier=<tier> header=<pc> boxes=.. guards=.. spills=.. calls=.. instructions=..`
//! (b-jit adds `bytes=`). u-jit counts the CLIF of the loop's innermost blocks; b-jit counts the
//! rows of the stitched loop (`base_region/residuals.rs`).

use crate::trace::Channel;
use cranelift_codegen::{
    dominator_tree::DominatorTree, flowgraph::ControlFlowGraph, ir, loop_analysis::LoopAnalysis,
};

/// The categories of a count, in field order.
pub(super) const NAMES: [&str; 5] = ["boxes", "guards", "spills", "calls", "instructions"];

/// The index of the registers parameter of the lane body signature (`lane_body_signature`):
/// the activation's register file, where a loop keeps nothing it can hold in a register.
const REGISTERS_PARAM: usize = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Residual {
    /// Stores of a value word to the register file.
    pub(super) boxes: u32,
    /// Exits other than the loop's own test.
    pub(super) guards: u32,
    /// Loop-variable traffic through the register file or the stack.
    pub(super) spills: u32,
    pub(super) calls: u32,
    pub(super) instructions: u32,
}

impl Residual {
    pub(super) fn counts(self) -> [u32; NAMES.len()] {
        [
            self.boxes,
            self.guards,
            self.spills,
            self.calls,
            self.instructions,
        ]
    }
}

/// Prints `residual` as one report line when the channel is on.
pub(super) fn report(tier: &str, header: u32, residual: Residual, bytes: Option<usize>) {
    if !Channel::Residuals.on() {
        return;
    }
    let counts: Vec<String> = NAMES
        .iter()
        .zip(residual.counts())
        .map(|(name, count)| format!("{name}={count}"))
        .collect();
    let bytes = bytes
        .map(|bytes| format!(" bytes={bytes}"))
        .unwrap_or_default();
    eprintln!(
        "QUENCH_RESIDUALS tier={tier} header={header} {}{bytes}",
        counts.join(" ")
    );
}

/// Reports the u-jit loop `function` lowered at `header`.
pub(super) fn report_clif(header: u32, function: &ir::Function) {
    if Channel::Residuals.on() {
        report("u-jit", header, of_clif(function), None);
    }
}

/// The counts of the deepest loops of `function`: of the blocks whose innermost loop is at the
/// greatest nesting level.
pub(super) fn of_clif(function: &ir::Function) -> Residual {
    let cfg = ControlFlowGraph::with_function(function);
    let dominators = DominatorTree::with_function(function, &cfg);
    let mut loops = LoopAnalysis::new();
    loops.compute(function, &cfg, &dominators);
    let deepest = function
        .layout
        .blocks()
        .map(|block| loops.loop_level(block).level())
        .max()
        .unwrap_or_default();
    let registers = function.layout.entry_block().and_then(|entry| {
        function
            .dfg
            .block_params(entry)
            .get(REGISTERS_PARAM)
            .copied()
    });
    let mut residual = Residual::default();
    let mut exits = 0;
    for block in function
        .layout
        .blocks()
        .filter(|block| deepest > 0 && loops.loop_level(*block).level() == deepest)
    {
        let own = loops.innermost_loop(block);
        exits += own
            .is_some_and(|own| leaves(function, block, |target| !loops.is_in_loop(target, own)))
            as u32;
        for inst in function.layout.block_insts(block) {
            residual.instructions += 1;
            count_memory(function, inst, registers, &mut residual);
            residual.calls += function.dfg.insts[inst].opcode().is_call() as u32;
        }
    }
    residual.guards = exits.saturating_sub(1);
    residual
}

/// Whether the block's terminator is a conditional branch to a block `outside` says is out.
fn leaves(function: &ir::Function, block: ir::Block, outside: impl Fn(ir::Block) -> bool) -> bool {
    let Some(last) = function.layout.last_inst(block) else {
        return false;
    };
    let data = &function.dfg.insts[last];
    data.opcode() == ir::Opcode::Brif
        && data
            .branch_destination(&function.dfg.jump_tables, &function.dfg.exception_tables)
            .iter()
            .any(|target| outside(target.block(&function.dfg.value_lists)))
}

/// Adds the register-file and stack traffic of `inst`: a store of a word to the file is a box, a
/// load from it or any stack access a spill.
fn count_memory(
    function: &ir::Function,
    inst: ir::Inst,
    registers: Option<ir::Value>,
    residual: &mut Residual,
) {
    let opcode = function.dfg.insts[inst].opcode();
    let args = function.dfg.inst_args(inst);
    let in_file = |address: Option<&ir::Value>| {
        address
            .zip(registers)
            .is_some_and(|(address, file)| function.dfg.resolve_aliases(*address) == file)
    };
    match opcode {
        ir::Opcode::StackLoad | ir::Opcode::StackStore => residual.spills += 1,
        _ if opcode.can_store() && in_file(args.get(1)) => residual.boxes += 1,
        _ if opcode.can_load() && in_file(args.first()) => residual.spills += 1,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn category_names_are_the_corpus_names() {
        assert_eq!(super::NAMES, quench_tier_corpus::residuals::CATEGORIES);
    }

    #[cfg(feature = "aot")]
    #[test]
    fn scalar_kernels_stay_within_their_residual_ceiling() {
        use super::super::{lower_static_typed_regions, LaneHelpers, VmFacts};
        use quench_tier_corpus::residuals::{Counts, Ratchet, Tier, KERNELS};

        // The kernels whose loop is Number work alone, which a static typed region lowers
        // without a running VM.
        const SCALAR: [&str; 3] = ["int-sum", "bitwise-mix", "early-exit"];
        let ratchet = Ratchet::parse(include_str!(
            "../../../../../tests/fixtures/residuals/ratchet.txt"
        ))
        .unwrap();
        let isa = super::super::super::jit_backend::target_isa().unwrap();
        for kernel in KERNELS
            .iter()
            .filter(|kernel| SCALAR.contains(&kernel.name))
        {
            let program = crate::Engine::specialize(kernel.source, kernel.name).unwrap();
            let id = program
                .functions
                .iter()
                .position(|body| {
                    body.header()
                        .name
                        .is_some_and(|name| &program.atoms[name as usize] == "kernel")
                })
                .unwrap();
            let region = lower_static_typed_regions(
                &program,
                id as u32,
                &[],
                &*isa,
                LaneHelpers::linked_for(0, &program),
                VmFacts::STATIC,
                &Default::default(),
            )
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{}: no typed loop region", kernel.name));
            let counts = Counts(super::of_clif(&region.function).counts());
            let over: Vec<_> = counts
                .exceeding(ratchet.ceiling(kernel, Tier::UJit))
                .collect();
            assert!(
                over.is_empty(),
                "{}: {over:?} over its ceiling, counts {counts:?}\n{}",
                kernel.name,
                region.function
            );
        }
    }
}
