//! The predicted tier of every function (design decision 4.9): a deterministic fixed point over the call graph. A function's
//! cost in a tier is its own run and compile over its hosted activations, plus, for each call edge it makes, either the
//! callee's work inlined into its body (where the tier inlines the callee: the one rule `tier_policy::inlines`) or the
//! crossing into the callee's tier. Pure: no count and no clock.

use super::{
    charges_growth, inlines, PredictedTier, BJIT_COMPILE_IR_PER_OP, BJIT_IR_PER_OP, CROSSING_IR_INTO_BJIT,
    ENTRY_IR_BJIT_BASE, ENTRY_IR_BJIT_PER_REGISTER, ENTRY_IR_INTERPRETER, ENTRY_IR_UJIT_BASE, ENTRY_IR_UJIT_PER_REGISTER,
    INTERPRETER_IR_PER_OP, UJIT_COMPILE_IR_PER_OP, UJIT_IR_PER_OP,
};
use crate::bytecode::ResidualProgram;
use crate::compile::call_graph::{CallGraph, LOOP_WEIGHT};

/// One call edge of the program: its frequency (the caller's frequency times the loop weight of the site) and whether a
/// compiled caller inlines the callee there. An edge inside a call cycle is never inlined: the lowering's depth bound cuts
/// a recursive inline, so the edge stays a call.
struct Edge {
    caller: usize,
    callee: usize,
    frequency: u64,
    inlined: bool,
}

/// The call graph and program, with each edge and each function's edges by index.
struct Model<'a> {
    graph: &'a CallGraph,
    program: &'a ResidualProgram,
    edges: Vec<Edge>,
    /// Per function, the indexes of the edges into it.
    incoming: Vec<Vec<usize>>,
    /// Per function, the indexes of the edges it makes.
    outgoing: Vec<Vec<usize>>,
}

fn compiled(tier: PredictedTier) -> bool {
    tier != PredictedTier::Interpreter
}

/// The tier of `function` in `tiers`; a function past the table runs in the interpreter.
fn tier_at(tiers: &[PredictedTier], function: usize) -> PredictedTier {
    tiers.get(function).copied().unwrap_or(PredictedTier::Interpreter)
}

/// IR per bytecode of one run of a function in `tier`.
fn per_op(tier: PredictedTier) -> u64 {
    match tier {
        PredictedTier::Interpreter => INTERPRETER_IR_PER_OP,
        PredictedTier::Base => BJIT_IR_PER_OP,
        PredictedTier::Ultra => UJIT_IR_PER_OP,
    }
}

/// IR per bytecode of a one-time compile in `tier`; none for the interpreter.
fn compile_per_op(tier: PredictedTier) -> u64 {
    match tier {
        PredictedTier::Interpreter => 0,
        PredictedTier::Base => BJIT_COMPILE_IR_PER_OP,
        PredictedTier::Ultra => UJIT_COMPILE_IR_PER_OP,
    }
}

/// IR per call of a function of `registers` registers entered in `tier` (`ENTRY_IR_*`, the derivations there).
fn entry(tier: PredictedTier, registers: u64) -> u64 {
    match tier {
        PredictedTier::Interpreter => ENTRY_IR_INTERPRETER,
        PredictedTier::Base => ENTRY_IR_BJIT_BASE.saturating_add(ENTRY_IR_BJIT_PER_REGISTER.saturating_mul(registers)),
        PredictedTier::Ultra => ENTRY_IR_UJIT_BASE.saturating_add(ENTRY_IR_UJIT_PER_REGISTER.saturating_mul(registers)),
    }
}

/// IR per call of a caller in `from` into a callee in `to` that is not inlined: the crossing. Only a u-jit caller entering
/// a b-jit body pays one (the body ABI's thunk over the row hop); every other pair pays the callee's entry alone, since a
/// u-jit body's entry already passes the shim (`ENTRY_IR_UJIT_BASE`).
fn crossing(from: PredictedTier, to: PredictedTier) -> u64 {
    match (from, to) {
        (PredictedTier::Ultra, PredictedTier::Base) => CROSSING_IR_INTO_BJIT,
        _ => 0,
    }
}

impl<'a> Model<'a> {
    fn new(graph: &'a CallGraph, program: &'a ResidualProgram) -> Self {
        let count = program.function_count();
        let mut edges = Vec::new();
        let mut incoming = vec![Vec::new(); count];
        let mut outgoing = vec![Vec::new(); count];
        for caller in 0..count {
            let weight = graph.frequency(caller as u32);
            for &(callee, depth) in graph.call_edges(caller as u32) {
                let inlined = inlines(graph, program, callee) && !graph.cycle_of(caller as u32).contains(&callee);
                let frequency = weight.saturating_mul(LOOP_WEIGHT.saturating_pow(depth));
                let index = edges.len();
                edges.push(Edge { caller, callee: callee as usize, frequency, inlined });
                incoming[callee as usize].push(index);
                outgoing[caller].push(index);
            }
        }
        Self { graph, program, edges, incoming, outgoing }
    }

    /// The activations of `function` its own body runs: its frequency less the activations a compiled caller inlines,
    /// which that caller's body runs (`tiers` are the callers' tiers).
    fn hosted(&self, function: usize, tiers: &[PredictedTier]) -> u64 {
        let inlined = self.incoming[function]
            .iter()
            .map(|&index| &self.edges[index])
            .filter(|edge| edge.inlined && compiled(tier_at(tiers, edge.caller)))
            .fold(0u64, |sum, edge| sum.saturating_add(edge.frequency));
        self.graph.frequency(function as u32).saturating_sub(inlined)
    }

    /// The IR `function` costs in `tier`, given the tiers `tiers` of the program: its own run, entry and compile over its
    /// hosted activations, and for each call edge it makes, the callee's work and a copy's compile where `tier` inlines the
    /// callee, else the crossing into the callee's tier.
    fn cost(&self, function: usize, tier: PredictedTier, tiers: &[PredictedTier]) -> u64 {
        let body = self.program.function_get(function);
        let bytecodes = body.map_or(0, |body| body.instructions().count() as u64);
        let registers = body.map_or(0, |body| u64::from(body.registers));
        let hosted = self.hosted(function, tiers);
        let function_work = self.graph.loop_work(function as u32);
        let run = hosted.saturating_mul(entry(tier, registers).saturating_add(function_work.saturating_mul(per_op(tier))));
        let mut total = run.saturating_add(bytecodes.saturating_mul(compile_per_op(tier)));
        for &index in &self.outgoing[function] {
            let edge = &self.edges[index];
            let per_call = if compiled(tier) && edge.inlined {
                let callee = edge.callee as u32;
                let callee_bytecodes = self.program.function_get(edge.callee).map_or(0, |body| body.instructions().count() as u64);
                // A callee with one call site is moved into its caller, not copied: only copies compile again.
                let copy = if charges_growth(self.graph, callee) { callee_bytecodes.saturating_mul(compile_per_op(tier)) } else { 0 };
                self.graph.loop_work(callee).saturating_mul(per_op(tier)).saturating_add(copy)
            } else {
                let callee_tier = if edge.callee == function { tier } else { tier_at(tiers, edge.callee) };
                crossing(tier, callee_tier)
            };
            total = total.saturating_add(edge.frequency.saturating_mul(per_call));
        }
        total
    }

    /// The argmin over the three tiers of `function`'s cost (`cost`). Ties go to the tier that compiles less: the
    /// interpreter, then the b-jit.
    fn pick(&self, function: usize, tiers: &[PredictedTier]) -> PredictedTier {
        let interpreter = self.cost(function, PredictedTier::Interpreter, tiers);
        let base = self.cost(function, PredictedTier::Base, tiers);
        let ultra = self.cost(function, PredictedTier::Ultra, tiers);
        if interpreter <= base && interpreter <= ultra {
            PredictedTier::Interpreter
        } else if base <= ultra {
            PredictedTier::Base
        } else {
            PredictedTier::Ultra
        }
    }
}

/// The predicted tier of every function of `program`: the fixed point of each function's argmin (`Model::pick`), passes
/// over the functions callee-first (`CallGraph::callee_first`). A function's callees are already priced in a pass, and a
/// caller's tier reaches its callees' costs in the next. The passes stop at the first that changes nothing, and are bounded
/// by the call graph's chain depth (`CallGraph::chain_depth`, one pass per component on the longest chain) plus one
/// confirming pass, so a cycle of components settles or stops at the bound, deterministically.
pub fn predicted_tiers(graph: &CallGraph, program: &ResidualProgram) -> Vec<PredictedTier> {
    let model = Model::new(graph, program);
    let mut tiers = vec![PredictedTier::Interpreter; program.function_count()];
    for _ in 0..=graph.chain_depth() {
        let mut changed = false;
        for &function in graph.callee_first() {
            let function = function as usize;
            let pick = model.pick(function, &tiers);
            if pick != tiers[function] {
                tiers[function] = pick;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    tiers
}

/// The IR a tier assignment costs over `program`, by the model `predicted_tiers` minimizes: the sum of each function's cost
/// in its tier under the assignment. The AOT estimate is this of the all-Ultra assignment, an image compiling every function.
pub fn predicted_cost(graph: &CallGraph, program: &ResidualProgram, tiers: &[PredictedTier]) -> u64 {
    let model = Model::new(graph, program);
    (0..program.function_count()).fold(0u64, |sum, function| {
        let tier = tiers.get(function).copied().unwrap_or(PredictedTier::Interpreter);
        sum.saturating_add(model.cost(function, tier, tiers))
    })
}
