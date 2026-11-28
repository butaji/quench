//! The length relations of a function's points, solved after the signatures over its settled words (`lengths::advance`): each
//! block replays from the words its settled solve entered it with, and its relations join over the edges until they settle.
//! A call resizes an array only where the settled effects say its class may be written (`FunctionFacts::may_write_elements_of`).

use super::blocks::Blocks;
use super::gate::World;
use super::class_ids::ClassIds;
use super::effects::{Ctx, step};
use super::flow::{FunctionFlow, conditional_target, loop_headers, whole_literals};
use super::lengths::{IndexRange, Interval, Lengths, Relation, advance, index_ranges, loops_of, parameter_registers, proves_below, refine, store_span};
use super::scope::Scope;
use super::types::Types;
use super::value::{Key, Origin, Word};
use super::FunctionFacts;
use crate::bytecode::{Class, ControlEdge, FieldLookup, Function, Register, Successors, WideInstruction, used_registers};
use crate::staged::{DecodeContext, Operation};
use crate::dataflow::BitSet;
use crate::typed_ir::ElementClass;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// What one round's length relations prove, which the next round's solve reads (`Ctx`): the allocations filled, the element
/// stores that write no hole, the element reads in bounds, and the spans the element stores cover, each by (function, pc). Round 1
/// solves with none; a round's relations depend on the bytecode and on the words the solve settled, and a fact here only refines
/// the next solve.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Feedback {
    pub(super) filled: BTreeSet<(u32, u32)>,
    pub(super) hole_free_stores: BTreeSet<(u32, u32)>,
    pub(super) in_bounds_reads: BTreeSet<(u32, u32)>,
    /// The element stores that cover the parameters `start .. start + len` (`store_span`), by (function, pc): (start, len).
    pub(super) spans: BTreeMap<(u32, u32), (u16, u16)>,
}

impl Feedback {
    /// The feedback the settled relations of every function prove (`Settled`).
    pub(super) fn of(settled: &BTreeMap<u32, Settled>) -> Self {
        let mut feedback = Self::default();
        for (&function, settled) in settled {
            feedback.filled.extend(settled.filled.iter().map(|&pc| (function, pc)));
            feedback.hole_free_stores.extend(settled.hole_free_stores.iter().map(|&pc| (function, pc)));
            feedback.in_bounds_reads.extend(settled.in_bounds_reads.iter().map(|&pc| (function, pc)));
            feedback.spans.extend(settled.spans.iter().map(|(&pc, &span)| ((function, pc), span)));
        }
        feedback
    }
}

/// The relations of every point of a function, by the pc each holds them before (`flow::Record`): a point that holds none
/// is absent. It also names the allocations the function's settled points prove filled, the element stores that write no hole
/// and the element reads proven in bounds (`Settled::record`), and the (pc, class) pairs whose step may resize an array of that
/// class (`lengths::advance`), which a loop's length check must account for.
#[derive(Default)]
pub(crate) struct Settled {
    points: Vec<(u32, Box<[Relation]>)>,
    filled: BTreeSet<u32>,
    hole_free_stores: BTreeSet<u32>,
    in_bounds_reads: BTreeSet<u32>,
    /// The allocations (pcs) the function's settled points use, and those a use of which needs the array complete.
    allocations: BTreeSet<u32>,
    unfilled: BTreeSet<u32>,
    resizing: BTreeSet<(u32, ElementClass)>,
    /// The element stores (pcs) that cover a span of parameters, as (start, len) (`store_span`).
    spans: BTreeMap<u32, (u16, u16)>,
    /// The interval of the index each element read takes, by pc, before the read (`Interval`).
    index_intervals: BTreeMap<u32, Interval>,
    /// The index ranges of the element reads whose forms are invariant in a loop (`ceilings::index_ranges`), by pc.
    index_ranges: BTreeMap<u32, IndexRange>,
}

impl Settled {
    /// Records the relations and the facts the instruction at `pc` finds before it, from the words before it. An allocation
    /// is filled when every use of it other than a hole-free store, a length read or a compare finds the array complete
    /// (`Relation::Complete`): the array is then read, escaped, stored and returned only once its elements are all written.
    fn record(&mut self, ctx: &Ctx<'_>, pc: u32, instruction: WideInstruction, words: &[Word], lengths: &Lengths) {
        let relations = lengths.relations();
        if !relations.is_empty() {
            self.points.push((pc, relations.into()));
        }
        let store = match Operation::<u64, ()>::of(instruction) {
            Some(Operation::ElementWrite { base, key, .. }) => Some((base, key)),
            _ => None,
        };
        if let Some(span) = store.and_then(|(_, key)| store_span(relations, key)) {
            self.spans.insert(pc, span);
        }
        let fill = store.filter(|&(base, key)| relations.contains(&Relation::Filled { array: base, index: key }));
        if fill.is_some() {
            self.hole_free_stores.insert(pc);
        }
        // An element read the relations prove within its array's length (`GetIndex` decodes as one).
        let read = match Operation::<u64, ()>::of(instruction) {
            Some(Operation::ElementRead { base, key, .. }) => key.register_index().map(|key| (base, key)),
            _ => None,
        };
        if read.is_some_and(|(base, key)| proves_below(relations, key, base, 0)) {
            self.in_bounds_reads.insert(pc);
        }
        if let Some((_, key)) = read && words.get(usize::from(key)).is_some_and(|word| word.is_number()) {
            self.index_intervals.insert(pc, lengths.typed_interval(words, key));
        }
        let length_read = matches!(instruction.op().class(), Class::GetField)
            && matches!(instruction.field_lookup(), Some(FieldLookup::Atom { atom, .. }) if ctx.scope.length == Some(atom));
        let compare = matches!(Operation::<u64, ()>::of(instruction), Some(Operation::CompareJump { .. }));
        for register in used_registers(instruction) {
            let Some(Origin::Alloc { function, pc: allocated }) = words.get(usize::from(register)).and_then(|word| word.origin) else { continue };
            if function != ctx.id || allocated == pc {
                continue;
            }
            self.allocations.insert(allocated);
            let allowed = fill.is_some_and(|(base, _)| base == register) || length_read || compare;
            if !allowed && !relations.contains(&Relation::Complete { array: register }) {
                self.unfilled.insert(allocated);
            }
        }
    }

    /// Settles the allocations: filled where no use of them found its array incomplete.
    fn finish(&mut self) {
        self.filled = self.allocations.difference(&self.unfilled).copied().collect();
    }

    /// Records that the step at `pc` may resize an array of `class`.
    pub(super) fn resize(&mut self, pc: u32, class: ElementClass) {
        self.resizing.insert((pc, class));
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

    /// The register `index` is below before the instruction at `pc` (`index < bound` by a comparison that held), if any.
    pub(crate) fn bound_below(&self, pc: u32, index: Register) -> Option<Register> {
        let found = self.points.binary_search_by_key(&pc, |(at, _)| *at).ok()?;
        self.points[found].1.iter().find_map(|relation| match *relation {
            Relation::Less { index: held, bound } if held == index => Some(bound),
            _ => None,
        })
    }

    /// The index range of the element read at `pc`, where its forms are invariant in a loop (`ceilings::index_ranges`).
    pub(crate) fn index_range(&self, pc: u32) -> Option<IndexRange> {
        self.index_ranges.get(&pc).copied()
    }

    /// Whether the step at `pc` may resize an array of `class`, as the settled solve reads it.
    pub(crate) fn may_resize(&self, pc: u32, class: ElementClass) -> bool {
        self.resizing.contains(&(pc, class))
    }

    /// Whether the step at `pc` may resize an array of any class.
    pub(crate) fn may_resize_any(&self, pc: u32) -> bool {
        self.resizing.iter().any(|&(at, _)| at == pc)
    }
}


/// The length relations of every point of `function`, solved over the words its settled solve entered each block with
/// (`entries`, `FunctionFlow::entries`): a block replays from them, and its relations join over the edges until they settle
/// (`Lengths::meet` only shrinks them, so the loop ends). A call resizes an array only where `facts` say its class may be written.
pub(super) fn settle(ctx: &Ctx<'_>, function: &Function, entries: &[Option<Vec<Word>>], facts: &FunctionFacts<'_>) -> Settled {
    let successors = Successors::of_code(function);
    let blocks = Blocks::of(&successors, function.code.len());
    let mut inputs: Vec<Option<Lengths>> = vec![None; blocks.ranges.len()];
    let headers = loop_headers(&blocks);
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
                let incoming = edge(ctx, function, &blocks, block, index, &out, &headers);
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
    settled.finish();
    // A parameter is a `Base::Parameter` only where nothing can write it but this function's own instructions: not through a
    // closure's environment slot (a closure captures one of this function's slots), not through code added at run time (an open
    // world, or a function with direct eval or a `with` scope), which reaches its parameters by name.
    let captured = facts.facts.captures.values().any(|slots| slots.iter().any(|&(owner, _)| owner == ctx.id));
    let open = facts.facts.world == World::Open || function.has_dynamic_scope_access();
    let parameters = if captured || open { Vec::new() } else { parameter_registers(function) };
    settled.index_ranges = index_ranges(&settled.index_intervals, &loops_of(function, &blocks), &parameters);
    settled
}

/// The length relations of function `id` (`settle`), from its settled solve `flow` and the settled facts of the program.
pub(crate) fn settle_function(id: u32, function: &Function, flow: &FunctionFlow, (scope, decode, records): (&Scope, DecodeContext<'_>, &[Register]),
    resolved: &BTreeMap<Key, u32>, classes: &ClassIds, accessors: &BTreeSet<u32>, types: &Types, facts: &FunctionFacts<'_>, feedback: &Feedback) -> Settled {
    let whole = whole_literals(function);
    let ctx = Ctx {
        id,
        parameter_end: function.parameter_end_pc as usize,
        is_root: function.parent.is_none(),
        scope,
        resolved,
        classes,
        registers: usize::from(function.registers),
        // The settled replay starts each block from the solve's entry registers alone (`FunctionFlow::entries`).
        tracked_locals: 0,
        whole_literals: &whole[..],
        types: Some(types),
        decode,
        records,
        accessors,
        filled: &feedback.filled,
        hole_free_stores: &feedback.hole_free_stores,
        in_bounds_reads: &feedback.in_bounds_reads,
        spans: &feedback.spans,
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
            settled.record(ctx, pc as u32, instruction, &words, &lengths);
        }
        let (_, resized) = advance(ctx, facts, pc, instruction, &mut lengths, &mut words, |words| step(ctx, pc, instruction, words));
        if let Some(settled) = record.as_mut() {
            for class in resized {
                settled.resize(pc as u32, class);
            }
        }
    }
    (words, lengths)
}

/// The lengths a successor edge `index` of `block` carries, from the block's output `out` (its words and lengths): those of the
/// output, and the relations the block's conditional jump proves on that edge (`refine`). An exception edge carries none. An edge into
/// a loop's header (`headers`) also pairs the parameters its words hold unchanged (`Lengths::pair_parameters`): a back edge holds none.
fn edge(ctx: &Ctx<'_>, function: &Function, blocks: &Blocks, block: usize, index: usize, out: &(Vec<Word>, Lengths), headers: &BitSet) -> Lengths {
    if matches!(blocks.kinds[block][index], ControlEdge::Handler) {
        return Lengths::default();
    }
    let target = blocks.targets[block][index];
    let last = blocks.ranges[block].end - 1;
    let instruction = function.instruction_at(last);
    let fallthrough = last + 1;
    let at = blocks.ranges[target].start;
    let mut lengths = match conditional_target(instruction) {
        Some(jump) if jump != fallthrough && (at == jump || at == fallthrough) => refine(ctx, instruction, at == jump, &out.0, &out.1),
        _ => out.1.clone(),
    };
    if headers.contains(target) {
        lengths.pair_parameters(ctx, &out.0);
    }
    lengths
}
