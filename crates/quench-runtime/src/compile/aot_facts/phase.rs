//! Phase facts (DESIGN 4.12): a holder key (a class's prototype or static object's method, `Key::MemberOn`) whose every write
//! sits in run-once code folds, in the code that runs after its last write, to the closure that write stores. Run-once code
//! is the top-level function, and each function the program calls exactly once, from run-once code outside any loop, and
//! never lets a closure escape. Every run-once pc has an instant: a top-level pc, the call that starts it.

use super::blocks::Blocks;
use super::census::Census;
use super::flow::{Call, FunctionFlow, Target};
use super::value::{Key, Word};
use crate::bytecode::{Function, ResidualProgram, Successors};
use crate::dataflow::BitSet;
use std::collections::{BTreeMap, BTreeSet};

/// A holder key whose value after its last write is one closure.
pub(super) struct Fold {
    /// The closure the last write stores.
    pub(super) closure: u32,
    /// The top-level function, whose pcs order the instants.
    root: u32,
    /// The instant of the last write: a top-level read at a later pc sees the closure.
    last: u32,
    /// The functions that may run at an instant no later than `last`, the top-level function excluded.
    early: BTreeSet<u32>,
}

impl Fold {
    /// Whether a read at `pc` of `function` runs after the last write, so sees the closure. The top-level function's reads
    /// are ordered by pc; any other function's read sees it unless the function may run before the last write.
    pub(super) fn read_sees(&self, function: u32, pc: u32) -> bool {
        if function == self.root { pc > self.last } else { !self.early.contains(&function) }
    }
}

/// What the phase analysis reads from one function's code.
struct Shape {
    /// Whether the function is the top-level one (the program's script, which runs once).
    root: bool,
    /// The pcs of loops: a block on a cycle, so a pc there may run any number of times.
    looping: BitSet,
    blocks: Blocks,
}

impl Shape {
    fn of(function: &Function) -> Self {
        let len = function.code.len();
        let blocks = Blocks::of(&Successors::of_code(function), len);
        // A cycle crosses every cut between its lowest and highest blocks, and its downward edge is a backward edge, so the
        // spans of the backward edges cover every block of every cycle. Over-approximating a loop only keeps facts away.
        let mut looping = BitSet::with_capacity(len);
        for (source, targets) in blocks.targets.iter().enumerate() {
            for &header in targets.iter().filter(|&&header| header <= source) {
                for pc in blocks.ranges[header].start..blocks.ranges[source].end {
                    looping.insert(pc);
                }
            }
        }
        Self { root: function.parent.is_none(), looping, blocks }
    }

    /// Whether the block of `pc` runs on every path from the entry to the function's exits: no exit is reachable from the entry
    /// without passing through it. A write there is the one its key holds from then on.
    fn unconditional(&self, pc: usize) -> bool {
        let Some(block) = self.blocks.ranges.iter().position(|range| range.contains(&pc)) else { return false };
        if block == 0 { return true; }
        let mut seen = vec![false; self.blocks.ranges.len()];
        seen[block] = true;
        seen[0] = true;
        let mut stack = vec![0];
        while let Some(current) = stack.pop() {
            if self.blocks.targets[current].is_empty() { return false; }
            for &next in &self.blocks.targets[current] {
                if !seen[next] {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        true
    }
}

/// The functions a call may run: its known callee, the closed set of closures stored under its name (`Census::candidates`),
/// or the one closure the census names. `None` where the callee is unknown: a method or accessor the solve names no closure for,
/// or a read from a key new code can move.
fn callees(census: &Census, call: &Call) -> Option<Vec<u32>> {
    match call.target {
        Target::Known(function) => Some(vec![function]),
        Target::Register if census.live_callee(call.callee) => None,
        Target::Register => match census.candidates(call.callee) {
            Some(set) => Some(set.iter().copied().collect()),
            None => census.target(call.callee).map(|function| vec![function]),
        },
        Target::Method(_) | Target::Accessor { .. } => None,
    }
}

/// Adds to `work` the functions a call may run: its callees, and the closures it hands its receiver and arguments, which the
/// callee may call. Returns whether a callee is unknown, so any escaped closure may run.
fn reach(census: &Census, call: &Call, work: &mut Vec<u32>) -> bool {
    work.extend(call.receiver.cell.ty.body_index());
    work.extend(call.arguments.iter().filter_map(|word| word.cell.ty.body_index()));
    match callees(census, call) {
        Some(set) => {
            work.extend(set);
            false
        }
        None => true,
    }
}

/// The functions whose closure a value the solve cannot follow may hold: stored, handed out, or passed to a call.
fn escaped(flows: &[FunctionFlow]) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    for flow in flows {
        let stored = flow.writes.iter().map(|&(_, _, word)| word);
        let handed = flow.handed.iter().copied();
        let passed = flow.calls.iter().flat_map(|call| std::iter::once(call.receiver).chain(call.arguments.iter().copied()));
        out.extend(stored.chain(handed).chain(passed).filter_map(|word| word.cell.ty.body_index()));
    }
    out
}

/// Per function, the instant its code runs at, where it is run-once: a function the program calls exactly once, from a run-once
/// call outside any loop, that never escapes. Its instant is the top-level pc of the call that starts it. The top-level function
/// has no instant of its own: its pcs are the instants.
fn instants(census: &Census, flows: &[FunctionFlow], shapes: &[Shape], root: usize, escaped: &BTreeSet<u32>) -> Vec<Option<u32>> {
    let mut calls_to = vec![0u32; flows.len()];
    for call in flows.iter().flat_map(|flow| &flow.calls) {
        for function in callees(census, call).unwrap_or_default() {
            if let Some(count) = calls_to.get_mut(function as usize) { *count += 1; }
        }
    }
    let mut instant: Vec<Option<u32>> = vec![None; flows.len()];
    let mut queue = vec![root];
    while let Some(caller) = queue.pop() {
        for call in &flows[caller].calls {
            let at = if caller == root { call.pc } else {
                match instant[caller] {
                    Some(at) => at,
                    None => continue,
                }
            };
            if shapes[caller].looping.contains(call.pc as usize) { continue; }
            let Some(set) = callees(census, call) else { continue };
            let [function] = set.as_slice() else { continue };
            let function = *function as usize;
            if function == root || instant[function].is_some() || escaped.contains(&(function as u32)) || calls_to[function] != 1 { continue; }
            instant[function] = Some(at);
            queue.push(function);
        }
    }
    instant
}

/// The program's folds, keyed by holder key. Nothing folds when a write the solve cannot follow may name a property
/// (`member_wild`), or when a store replaces a `prototype` with an object whose properties the solve never saw.
pub(super) fn folds(program: &ResidualProgram, flows: &[FunctionFlow], census: &Census) -> BTreeMap<(u32, bool, u32), Fold> {
    if flows.iter().any(|flow| flow.member_wild) { return BTreeMap::new(); }
    let shapes: Vec<Shape> = (0..flows.len()).map(|id| program.with_record(id, Shape::of)).collect();
    let Some(root) = shapes.iter().position(|shape| shape.root) else { return BTreeMap::new() };
    let prototype: BTreeSet<u32> = program.atoms.iter().enumerate().filter(|&(_, text)| text == "prototype").map(|(atom, _)| atom as u32).collect();
    let mut sites: BTreeMap<(u32, bool, u32), Vec<(u32, u32, Word)>> = BTreeMap::new();
    let mut named: BTreeSet<(u32, u32, u32)> = BTreeSet::new();
    let mut stores: Vec<(u32, u32, u32)> = Vec::new();
    for (id, flow) in flows.iter().enumerate() {
        let id = id as u32;
        for &(pc, key, fact) in &flow.writes {
            match key {
                Key::MemberOn { class, instance, atom } => {
                    sites.entry((class, instance, atom)).or_default().push((id, pc, fact));
                    named.insert((id, pc, atom));
                }
                Key::Member(atom) | Key::MemberAny(atom) => {
                    if prototype.contains(&atom) && fact.cell.ty.body_index().is_none() { return BTreeMap::new(); }
                    stores.push((id, pc, atom));
                }
                _ => {}
            }
        }
    }
    // A named store no holder write of its own site accounts for may write any holder's method of that name.
    let unnamed: BTreeSet<u32> = stores.iter().filter(|store| !named.contains(*store)).map(|&(_, _, atom)| atom).collect();
    let escaped = escaped(flows);
    let instant = instants(census, flows, &shapes, root, &escaped);
    let analysis = Analysis { census, flows, shapes, root, instant, escaped };
    sites.into_iter()
        .filter(|((_, _, atom), _)| !unnamed.contains(atom))
        .filter_map(|(key, writes)| analysis.fold(&writes).map(|fold| (key, fold)))
        .collect()
}

/// The program's facts the folds read, bundled for one holder key at a time.
struct Analysis<'a> {
    census: &'a Census,
    flows: &'a [FunctionFlow],
    shapes: Vec<Shape>,
    root: usize,
    instant: Vec<Option<u32>>,
    escaped: BTreeSet<u32>,
}

impl Analysis<'_> {
    /// The instant of a write at `pc` of `function`: its top-level pc, or the instant its run-once function runs at. None where
    /// the write is not run-once: a loop's, or a function that runs any number of times.
    fn when(&self, function: u32, pc: u32) -> Option<u32> {
        let function = function as usize;
        if self.shapes[function].looping.contains(pc as usize) { return None; }
        if function == self.root { Some(pc) } else { self.instant[function] }
    }

    /// The fold of a holder key written at `writes` (function, pc, fact): every write run-once, the last instant holding one write
    /// that stores a closure on every path through its function.
    fn fold(&self, writes: &[(u32, u32, Word)]) -> Option<Fold> {
        let timed: Vec<(u32, u32, u32, Word)> = writes.iter()
            .map(|&(function, pc, fact)| Some((self.when(function, pc)?, function, pc, fact)))
            .collect::<Option<_>>()?;
        let last = timed.iter().map(|write| write.0).max()?;
        let mut at_last = timed.iter().filter(|write| write.0 == last);
        let (_, function, pc, fact) = *at_last.next()?;
        if at_last.next().is_some() { return None; }
        let closure = fact.cell.ty.body_index()?;
        if !self.shapes[function as usize].unconditional(pc as usize) { return None; }
        Some(Fold { closure, root: self.root as u32, last, early: self.early(last) })
    }

    /// The functions that may run at an instant no later than `last`: the run-once functions with such an instant, the top-level
    /// calls before `last`, and whatever those functions call (and are handed). An unknown callee may run any escaped closure.
    fn early(&self, last: u32) -> BTreeSet<u32> {
        let mut early = BTreeSet::new();
        let mut work: Vec<u32> = (0..self.flows.len())
            .filter(|&function| function != self.root && self.instant[function].is_some_and(|at| at <= last))
            .map(|function| function as u32)
            .collect();
        let mut unknown = self.flows[self.root].calls.iter().filter(|call| call.pc <= last)
            .fold(false, |unknown, call| reach(self.census, call, &mut work) || unknown);
        let mut spilled = false;
        loop {
            if unknown && !spilled {
                spilled = true;
                work.extend(self.escaped.iter().copied());
            }
            let Some(function) = work.pop() else { break };
            if function as usize == self.root || !early.insert(function) { continue; }
            for call in &self.flows[function as usize].calls {
                unknown |= reach(self.census, call, &mut work);
            }
        }
        early
    }
}
