//! The register dataflow of one function (`dataflow::solve` over its basic blocks): each
//! register's fact at each point, and what the function does that the closed-world facts read.

use super::blocks::Blocks;
use super::effects::{Ctx, Events, step};
use super::scope::Scope;
use super::value::{Fact, Key};
use crate::bytecode::{ControlEdge, Function, Successors, Traits, WideInstruction, used_registers};
use crate::dataflow::{Dataflow, solve};
use crate::typed_ir::Representation;

/// A plain call or a `new` a function makes, with its callee's fact.
pub(super) struct Call {
    pub(super) pc: u32,
    pub(super) callee: Fact,
    pub(super) construct: bool,
}

/// What one function contributes to the closed-world facts.
pub(super) struct FunctionFlow {
    pub(super) calls: Vec<Call>,
    /// Every binding write, with the fact of the value written.
    pub(super) writes: Vec<(Key, Fact)>,
    /// Whether a write the analysis cannot follow reaches the function's own slots.
    pub(super) wild_owner: bool,
    /// Whether a write the analysis cannot follow reaches the globals.
    pub(super) global_wild: bool,
    /// Whether a store through a key the analysis cannot name may write a named property.
    pub(super) member_wild: bool,
    /// Per register, the Number representation it holds at every read and write.
    pub(super) numbers: Box<[Option<Representation>]>,
}

/// The dataflow problem of one function: its registers' facts over its basic blocks.
struct Solver<'a> {
    ctx: &'a Ctx<'a>,
    function: &'a Function,
    blocks: &'a Blocks,
}

/// The facts of function `id`, whose record is `function`.
pub(super) fn analyze_function(id: u32, function: &Function, scope: &Scope,
    resolved: &std::collections::BTreeMap<Key, u32>) -> FunctionFlow {
    let successors = Successors::of_code(function);
    let blocks = Blocks::of(&successors, function.code.len());
    let ctx = Ctx {
        id,
        parameter_end: function.parameter_end_pc as usize,
        is_root: function.parent.is_none(),
        scope,
        resolved,
        registers: usize::from(function.registers),
    };
    let mut solver = Solver { ctx: &ctx, function, blocks: &blocks };
    let inputs = solve(&mut solver);
    let mut record = Record::new(ctx.registers);
    for (block, input) in inputs.into_iter().enumerate() {
        if let Some(state) = input {
            solver.replay(block, state, &mut record);
        }
    }
    record.finish()
}

impl Dataflow for Solver<'_> {
    type State = Vec<Fact>;

    fn blocks(&self) -> usize {
        self.blocks.ranges.len()
    }

    fn edges(&self, block: usize) -> &[usize] {
        &self.blocks.targets[block]
    }

    fn entry_state(&self, block: usize) -> Option<Vec<Fact>> {
        (block == 0).then(|| vec![Fact::UNKNOWN; self.ctx.registers])
    }

    fn join(&self, into: &mut Vec<Fact>, incoming: &Vec<Fact>) -> bool {
        let mut changed = false;
        for (slot, &fact) in into.iter_mut().zip(incoming) {
            let joined = slot.join(fact);
            changed |= joined != *slot;
            *slot = joined;
        }
        changed
    }

    fn transfer(&mut self, block: usize, state: &Vec<Fact>) -> Vec<Fact> {
        let mut out = state.clone();
        for pc in self.blocks.ranges[block].clone() {
            step(self.ctx, pc, self.function.instruction_at(pc), &mut out);
        }
        out
    }

    /// An exception edge may leave the block before any of its writes, so it carries no fact.
    fn transfer_edge_at(&self, block: usize, index: usize, out: &Vec<Fact>) -> Vec<Fact> {
        match self.blocks.kinds[block][index] {
            ControlEdge::Handler => vec![Fact::UNKNOWN; out.len()],
            ControlEdge::Normal => out.clone(),
        }
    }
}

impl Solver<'_> {
    /// Replays `block` from its input `state`, recording what its instructions do.
    fn replay(&self, block: usize, mut state: Vec<Fact>, record: &mut Record) {
        for pc in self.blocks.ranges[block].clone() {
            let instruction = self.function.instruction_at(pc);
            record.observe(instruction, pc, &state);
            let events = step(self.ctx, pc, instruction, &mut state);
            record.commit(instruction, &state, events);
        }
    }
}

/// What a replay gathers over a function's reachable code.
struct Record {
    calls: Vec<Call>,
    writes: Vec<(Key, Fact)>,
    wild_owner: bool,
    wild_global: bool,
    wild_member: bool,
    /// Per register, the join of the facts it is defined with.
    defs: Vec<Option<Fact>>,
    /// Per register, whether a read saw a value that is not a Number.
    unsafe_reads: Vec<bool>,
}

impl Record {
    fn new(registers: usize) -> Self {
        Self {
            calls: Vec::new(),
            writes: Vec::new(),
            wild_owner: false,
            wild_global: false,
            wild_member: false,
            defs: vec![None; registers],
            unsafe_reads: vec![false; registers],
        }
    }

    /// Notes the reads and the calls `instruction` makes, from the facts before it.
    fn observe(&mut self, instruction: WideInstruction, pc: usize, state: &[Fact]) {
        for register in used_registers(instruction) {
            if !state[usize::from(register)].is_number() {
                self.unsafe_reads[usize::from(register)] = true;
            }
        }
        let traits = instruction.op().traits();
        let construct = traits.has(Traits::CONSTRUCTION);
        if construct || traits.has(Traits::PLAIN_CALL) {
            let callee = state[usize::from(instruction.register_b())];
            self.calls.push(Call { pc: pc as u32, callee, construct });
        }
    }

    /// Notes the writes `instruction` made and the facts it defined, from the facts after it.
    fn commit(&mut self, instruction: WideInstruction, state: &[Fact], events: Events) {
        self.wild_owner |= events.wild_owner;
        self.wild_global |= events.wild_global;
        self.wild_member |= events.wild_member;
        self.writes.extend(events.writes);
        for register in instruction.written_registers() {
            let index = usize::from(register);
            self.defs[index] = Some(self.defs[index].map_or(state[index], |def| def.join(state[index])));
        }
    }

    fn finish(self) -> FunctionFlow {
        let numbers: Box<[Option<Representation>]> = self
            .defs
            .iter()
            .zip(&self.unsafe_reads)
            .map(|(def, &unsafe_read)| if unsafe_read { None } else { def.and_then(Fact::representation) })
            .collect();
        FunctionFlow {
            calls: self.calls,
            writes: self.writes,
            wild_owner: self.wild_owner,
            global_wild: self.wild_global,
            member_wild: self.wild_member,
            numbers,
        }
    }
}
