//! The length relations of a function's points, solved after the signatures over its settled words (`lengths::advance`): each
//! block replays from the words its settled solve entered it with, and its relations join over the edges until they settle.
//! A call resizes an array only where the settled effects say its class may be written (`FunctionFacts::may_write_elements_of`).

use super::blocks::Blocks;
use super::effects::{Ctx, step};
use super::flow::{FunctionFlow, conditional_target, whole_literals};
use super::lengths::{Lengths, Relation, advance, proves_below, refine};
use super::scope::Scope;
use super::types::Types;
use super::value::{Key, Word};
use super::FunctionFacts;
use crate::bytecode::{ControlEdge, Function, Register, Successors};
use crate::staged::DecodeContext;
use std::collections::BTreeMap;
use std::ops::Range;

/// The relations of every point of a function, by the pc each holds them before (`flow::Record`): a point that holds none
/// is absent.
#[derive(Default)]
pub(crate) struct Settled {
    points: Vec<(u32, Box<[Relation]>)>,
}

impl Settled {
    /// Records the relations the instruction at `pc` finds before it, where any hold.
    pub(super) fn push(&mut self, pc: u32, relations: &[Relation]) {
        if !relations.is_empty() {
            self.points.push((pc, relations.into()));
        }
    }

    /// Orders the points by pc, so the lookups binary-search them.
    pub(super) fn sort(&mut self) {
        self.points.sort_by_key(|(pc, _)| *pc);
    }

    /// Whether `index + offset < length(array)` holds before the instruction at `pc`.
    pub(crate) fn proves_below(&self, pc: u32, index: Register, array: Register, offset: i64) -> bool {
        self.points.binary_search_by_key(&pc, |(at, _)| *at)
            .is_ok_and(|found| proves_below(&self.points[found].1, index, array, offset))
    }
}


/// The length relations of every point of `function`, solved over the words its settled solve entered each block with
/// (`entries`, `FunctionFlow::entries`): a block replays from them, and its relations join over the edges until they settle
/// (`Lengths::meet` only shrinks them, so the loop ends). A call resizes an array only where `facts` say its class may be written.
pub(super) fn settle(ctx: &Ctx<'_>, function: &Function, entries: &[Option<Vec<Word>>], facts: &FunctionFacts<'_>) -> Settled {
    let successors = Successors::of_code(function);
    let blocks = Blocks::of(&successors, function.code.len());
    let mut inputs: Vec<Option<Lengths>> = vec![None; blocks.ranges.len()];
    if let Some(entry) = inputs.first_mut() {
        *entry = Some(Lengths::default());
    }
    let mut changed = true;
    while changed {
        changed = false;
        for block in 0..blocks.ranges.len() {
            let (Some(input), Some(words)) = (inputs[block].clone(), entries[block].clone()) else { continue };
            let out = replay(ctx, function, facts, blocks.ranges[block].clone(), words, input, None);
            for index in 0..blocks.targets[block].len() {
                let target = blocks.targets[block][index];
                let incoming = edge(ctx, function, &blocks, block, index, &out);
                match &mut inputs[target] {
                    Some(known) => changed |= known.meet(&incoming),
                    slot => {
                        *slot = Some(incoming);
                        changed = true;
                    }
                }
            }
        }
    }
    let mut settled = Settled::default();
    for block in 0..blocks.ranges.len() {
        if let (Some(input), Some(words)) = (inputs[block].clone(), entries[block].clone()) {
            replay(ctx, function, facts, blocks.ranges[block].clone(), words, input, Some(&mut settled));
        }
    }
    settled.sort();
    settled
}

/// The length relations of function `id` (`settle`), from its settled solve `flow` and the settled facts of the program.
pub(crate) fn settle_function(id: u32, function: &Function, flow: &FunctionFlow, (scope, decode, records): (&Scope, DecodeContext<'_>, &[Register]),
    resolved: &BTreeMap<Key, u32>, types: &Types, facts: &FunctionFacts<'_>) -> Settled {
    let whole = whole_literals(function);
    let ctx = Ctx {
        id,
        parameter_end: function.parameter_end_pc as usize,
        is_root: function.parent.is_none(),
        scope,
        resolved,
        registers: usize::from(function.registers),
        whole_literals: &whole[..],
        types: Some(types),
        decode,
        records,
    };
    settle(&ctx, function, &flow.entries, facts)
}

/// Replays the instructions of `range` from `words` and `lengths`, and returns the state after them. `record` keeps the relations
/// each pc holds before its instruction.
fn replay(ctx: &Ctx<'_>, function: &Function, facts: &FunctionFacts<'_>, range: Range<usize>, mut words: Vec<Word>, mut lengths: Lengths,
    mut record: Option<&mut Settled>) -> (Vec<Word>, Lengths) {
    for pc in range {
        let instruction = function.instruction_at(pc);
        if let Some(settled) = record.as_mut() {
            settled.push(pc as u32, lengths.relations());
        }
        advance(ctx, facts, pc, instruction, &mut lengths, &mut words, |words| step(ctx, pc, instruction, words));
    }
    (words, lengths)
}

/// The lengths a successor edge `index` of `block` carries, from the block's output `out` (its words and lengths): those of the
/// output, and the relations the block's conditional jump proves on that edge (`refine`). An exception edge carries none.
fn edge(ctx: &Ctx<'_>, function: &Function, blocks: &Blocks, block: usize, index: usize, out: &(Vec<Word>, Lengths)) -> Lengths {
    if matches!(blocks.kinds[block][index], ControlEdge::Handler) {
        return Lengths::default();
    }
    let target = blocks.targets[block][index];
    let last = blocks.ranges[block].end - 1;
    let instruction = function.instruction_at(last);
    let fallthrough = last + 1;
    let at = blocks.ranges[target].start;
    match conditional_target(instruction) {
        Some(jump) if jump != fallthrough && (at == jump || at == fallthrough) => refine(ctx, instruction, at == jump, &out.0, &out.1),
        _ => out.1.clone(),
    }
}
