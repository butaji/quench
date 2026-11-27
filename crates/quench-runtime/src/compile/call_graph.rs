//! The program's call graph and the static frequency of each function, solved once per program
//! (`ProgramStore::call_graph`). An edge is a callee the AOT facts name at a call or a construction
//! (`FunctionFacts::call_targets`, `construct_target`); a site the facts name no callee of adds none.
//! A function's frequency is the number of its activations the static model expects per entry of the
//! program: each call site weighs `LOOP_WEIGHT` per loop it sits in, a recursive call cycle weighs as a
//! loop, and the weights propagate from callers to callees. Owner: AOT facts.

use crate::bytecode::{loop_depth, Function, Op, RegExpLiteralSite, ResidualProgram};
use crate::staged::meaning::{means, Meaning};
use super::aot_facts::AotFacts;
use rustc_hash::FxHashMap;

/// The `cycle` entry of a function that is in no call cycle.
const NO_CYCLE: u32 = u32::MAX;

/// The weight of one loop, or of one recursive call cycle, on a function's static frequency (10^depth).
pub const LOOP_WEIGHT: u64 = 10;

/// The `stack_subtree` entry of a function whose calls reach a call cycle or an unnamed call.
const UNBOUNDED: u64 = u64::MAX;

/// The call graph of one program, with its call cycles, its static frequencies and its call sites.
pub struct CallGraph {
    /// Per function, the index into `cycles` of its call cycle, `NO_CYCLE` where it is in none.
    cycle: Box<[u32]>,
    /// The members of each call cycle: a strongly connected component of several functions, or a function that calls itself.
    cycles: Box<[Box<[u32]>]>,
    /// Per function, its static frequency: its expected activations per entry of the program, saturating.
    frequency: Box<[u64]>,
    /// Per function, its work per activation: each instruction weighs `LOOP_WEIGHT` per loop it is in, so a
    /// loop's instructions count as the iterations they run.
    work: Box<[u64]>,
    /// Per function, the calls and constructions that name it, each call site once; saturating.
    sites: Box<[u32]>,
    /// Whether the facts are closed (`AotFacts::closed`): only then does a site they do not name not exist.
    closed: bool,
    /// Per function, the instructions of every function its calls reach (itself included), each counted once per
    /// path; `UNBOUNDED` where a call cycle or a call with no named callee is reached, so depth below it is unbounded.
    stack_subtree: Box<[u64]>,
    /// Per regexp literal site (`ResidualProgram::regexp_literal_sites`), the static frequency of its pattern
    /// (`regexp_site_frequencies`).
    regexp_sites: Box<[u64]>,
}

/// What one scan of the program's bodies reads of its call sites.
struct Scan {
    /// Per function, each call site that names it: its caller and the loop depth of the site.
    callers: Vec<Vec<(usize, u32)>>,
    /// Per function, the call sites that name it, saturating.
    named: Vec<u32>,
    /// Per function, its work per activation.
    work: Vec<u64>,
    /// Per function, its instructions.
    size: Vec<u64>,
    /// Per function, whether it makes a call with no named callee (a method call, or a call the facts name none of).
    unnamed: Vec<bool>,
}

impl CallGraph {
    /// The call graph of `program`, its edges the callees `facts` names, and its frequencies solved from it.
    pub fn of(program: &ResidualProgram, facts: &AotFacts) -> Self {
        let count = program.function_count();
        let mut scan = Scan { callers: vec![Vec::new(); count], named: vec![0; count], work: vec![0; count], size: vec![0; count], unnamed: vec![false; count] };
        let callees: Vec<Box<[usize]>> = (0..count)
            .map(|function| program.function_get(function).map_or_else(Vec::new, |body| scan_body(&mut scan, function, body, facts)).into_boxed_slice())
            .collect();
        let components = strongly_connected(&callees);
        let (cycle, cycles) = cycle_table(&callees, &components);
        let frequency = frequencies(&callees, &components, &scan.callers);
        let stack_subtree = stack_subtrees(&callees, &cycle, &scan.unnamed, &scan.size);
        let regexp_sites = regexp_site_frequencies(program, &frequency);
        Self {
            cycle,
            cycles,
            frequency,
            work: scan.work.into_boxed_slice(),
            sites: scan.named.into_boxed_slice(),
            closed: facts.closed(),
            stack_subtree,
            regexp_sites,
        }
    }

    /// The static frequency of the pattern regexp literal site `site` creates: the sum of `frequency` over the
    /// functions whose bodies create a literal of that pattern and flags (`regexp_site_frequencies`); 0 where none does.
    pub fn regexp_site_frequency(&self, site: usize) -> u64 {
        self.regexp_sites.get(site).copied().unwrap_or(0)
    }

    /// The instructions of the functions `function`'s calls reach, when they are a finite acyclic set of named
    /// bodies: `None` where a call cycle or an unnamed call is reached, so the depth below `function` is unbounded.
    pub fn stack_subtree_instructions(&self, function: u32) -> Option<u64> {
        match self.stack_subtree.get(function as usize) {
            Some(&total) if total != UNBOUNDED => Some(total),
            _ => None,
        }
    }

    /// Whether `function` has exactly one call site in the closed world: one call or construction, of the one
    /// program, names it. False where the facts are not closed, since a site they do not name may call it.
    pub fn single_call_site(&self, function: u32) -> bool {
        self.closed && self.sites.get(function as usize) == Some(&1)
    }

    /// The static frequency of `function`: its expected activations per entry of the program. A loop
    /// multiplies a call inside it by `LOOP_WEIGHT`, a recursive call cycle weighs as a loop, and the
    /// caller's frequency propagates through the call sites.
    pub fn frequency(&self, function: u32) -> u64 {
        self.frequency.get(function as usize).copied().unwrap_or(1)
    }

    /// The work of one activation of `function`: its instructions, each weighing `LOOP_WEIGHT` per loop it sits in.
    pub fn loop_work(&self, function: u32) -> u64 {
        self.work.get(function as usize).copied().unwrap_or(0)
    }

    /// Whether `function` is in a call cycle: it calls itself, directly or through other functions.
    pub fn in_cycle(&self, function: u32) -> bool {
        !self.cycle_of(function).is_empty()
    }

    /// The members of `function`'s call cycle, itself included; none where it is in no cycle.
    pub fn cycle_of(&self, function: u32) -> &[u32] {
        match self.cycle.get(function as usize) {
            Some(&index) if index != NO_CYCLE => &self.cycles[index as usize][..],
            _ => &[],
        }
    }
}

/// Scans `body` (function `function`): its work per activation, its call sites into `scan`, and the distinct
/// callees it names, sorted.
fn scan_body(scan: &mut Scan, function: usize, body: &Function, facts: &AotFacts) -> Vec<usize> {
    let backward: Vec<(usize, usize)> = body.back_edges().map(|(source, target)| (target, source)).collect();
    let calls = facts.function_facts(function as u32);
    let count = scan.named.len();
    let mut out: Vec<usize> = Vec::new();
    for (pc, instruction) in body.instructions().enumerate() {
        let depth = loop_depth(&backward, pc);
        scan.work[function] = scan.work[function].saturating_add(LOOP_WEIGHT.saturating_pow(depth));
        scan.size[function] += 1;
        // A method call's callee is not an edge here, so it is unnamed for stack depth too.
        if means(instruction, Meaning::MethodCall) { scan.unnamed[function] = true; }
        let at = pc as u32;
        let mut targets: Vec<u32> = if means(instruction, Meaning::Construct) {
            calls.construct_target(at).into_iter().collect()
        } else if means(instruction, Meaning::Call) {
            calls.call_targets(at)
        } else {
            continue;
        };
        if targets.is_empty() { scan.unnamed[function] = true; }
        targets.sort_unstable();
        targets.dedup();
        for callee in targets.into_iter().map(|callee| callee as usize).filter(|callee| *callee < count) {
            out.push(callee);
            scan.named[callee] = scan.named[callee].saturating_add(1);
            scan.callers[callee].push((function, depth));
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The call cycles of the components: each component that is one (`is_cycle`), as its members, and the index of
/// its cycle per function.
/// Per regexp literal site of `program`, the static frequency of its pattern: each function whose body creates a
/// literal of the pattern (`Op::CreateRegExpLiteral` at a site with the same pattern and flags constants) adds its
/// `frequency` once, saturating. The creating functions stand for the pattern's exec sites: a regexp that a function
/// receives from elsewhere is counted where it was created.
fn regexp_site_frequencies(program: &ResidualProgram, frequency: &[u64]) -> Box<[u64]> {
    let pattern = |site: &RegExpLiteralSite| (site.pattern_constant, site.flags_constant);
    let mut by_pattern: FxHashMap<(u32, u32), u64> = FxHashMap::default();
    for function in 0..program.function_count() {
        let Some(body) = program.function_get(function) else { continue };
        let mut created: Vec<(u32, u32)> = Vec::new();
        for instruction in body.instructions().filter(|instruction| instruction.op() == Op::CreateRegExpLiteral) {
            if let Some(site) = program.regexp_literal_sites.get(instruction.regexp_literal_site_index()) {
                if !created.contains(&pattern(site)) {
                    created.push(pattern(site));
                }
            }
        }
        for key in created {
            let total = by_pattern.entry(key).or_insert(0);
            *total = total.saturating_add(frequency[function]);
        }
    }
    program.regexp_literal_sites.iter().map(|site| by_pattern.get(&pattern(site)).copied().unwrap_or(0)).collect()
}

fn cycle_table(callees: &[Box<[usize]>], components: &[Vec<usize>]) -> (Box<[u32]>, Box<[Box<[u32]>]>) {
    let mut cycle = vec![NO_CYCLE; callees.len()];
    let mut cycles: Vec<Box<[u32]>> = Vec::new();
    for members in components.iter().filter(|members| is_cycle(callees, members)) {
        for &function in members { cycle[function] = cycles.len() as u32; }
        cycles.push(members.iter().map(|&function| function as u32).collect());
    }
    (cycle.into_boxed_slice(), cycles.into_boxed_slice())
}

/// Each function's static frequency. Components come callees first, so the reverse visits every caller before the
/// functions it calls; a call from within its own component adds nothing, and a recursive component weighs as a loop.
fn frequencies(callees: &[Box<[usize]>], components: &[Vec<usize>], callers: &[Vec<(usize, u32)>]) -> Box<[u64]> {
    let mut component = vec![0usize; callees.len()];
    for (index, members) in components.iter().enumerate() {
        for &function in members { component[function] = index; }
    }
    let mut frequency = vec![0u64; callees.len()];
    for (index, members) in components.iter().enumerate().rev() {
        let recursive = is_cycle(callees, members);
        for &function in members {
            let weight = callers[function]
                .iter()
                .filter(|&&(caller, _)| component[caller] != index)
                .fold(1u64, |weight, &(caller, depth)| weight.saturating_add(frequency[caller].saturating_mul(LOOP_WEIGHT.saturating_pow(depth))));
            frequency[function] = if recursive { weight.saturating_mul(LOOP_WEIGHT) } else { weight };
        }
    }
    frequency.into_boxed_slice()
}

/// Whether a component of the call graph is a call cycle: several functions, or one that calls itself.
fn is_cycle(callees: &[Box<[usize]>], members: &[usize]) -> bool {
    members.len() > 1 || callees[members[0]].contains(&members[0])
}

/// The strongly connected components of `callees` by Tarjan's algorithm in one pass, each as its members. A
/// component is emitted after every component its members call, so the list runs callees first.
fn strongly_connected(callees: &[Box<[usize]>]) -> Vec<Vec<usize>> {
    const UNVISITED: u32 = u32::MAX;
    let count = callees.len();
    let mut index = vec![UNVISITED; count];
    let mut low = vec![0u32; count];
    let mut on_stack = vec![false; count];
    let mut stack: Vec<usize> = Vec::new();
    let mut next = 0u32;
    let mut found = Vec::new();
    for root in 0..count {
        if index[root] != UNVISITED { continue; }
        // The walk: each function entered and not yet left, with the number of its callees read so far.
        let mut walk: Vec<(usize, usize)> = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(v, edge)) = walk.last() {
            if let Some(&w) = callees[v].get(edge) {
                walk.last_mut().expect("the walk's top").1 += 1;
                if index[w] == UNVISITED {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    walk.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                walk.pop();
                if let Some(&(parent, _)) = walk.last() { low[parent] = low[parent].min(low[v]); }
                if low[v] == index[v] {
                    let mut members = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        members.push(w);
                        if w == v { break; }
                    }
                    found.push(members);
                }
            }
        }
    }
    found
}

/// The `CallGraph::stack_subtree` of every function: a function reaching a call cycle (`cycle`) or an unnamed call
/// (`unnamed`) is `UNBOUNDED`. The rest reach an acyclic set of named bodies, whose instructions (`size`) sum over
/// that DAG; a shared callee is counted once per path, which only overstates the bound.
fn stack_subtrees(callees: &[Box<[usize]>], cycle: &[u32], unnamed: &[bool], size: &[u64]) -> Box<[u64]> {
    let count = callees.len();
    // A function reaches a bad one where a reverse walk from the bad functions over caller edges reaches it.
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (caller, out) in callees.iter().enumerate() {
        for &callee in out.iter() { callers[callee].push(caller); }
    }
    let mut bad: Vec<bool> = (0..count).map(|function| cycle[function] != NO_CYCLE || unnamed[function]).collect();
    let mut pending: Vec<usize> = (0..count).filter(|&function| bad[function]).collect();
    while let Some(function) = pending.pop() {
        for &caller in &callers[function] {
            if !bad[caller] {
                bad[caller] = true;
                pending.push(caller);
            }
        }
    }
    let mut total = vec![UNBOUNDED; count];
    let mut done = vec![false; count];
    for root in (0..count).filter(|&function| !bad[function]) {
        if done[root] { continue; }
        // Post-order over the acyclic reach of `root`: each function's callees are finished before it.
        let mut walk: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(function, edge)) = walk.last() {
            if let Some(&callee) = callees[function].get(edge) {
                walk.last_mut().expect("the walk's top").1 += 1;
                if !done[callee] { walk.push((callee, 0)); }
            } else {
                walk.pop();
                total[function] = callees[function].iter().fold(size[function], |sum, &callee| sum.saturating_add(total[callee]));
                done[function] = true;
            }
        }
    }
    total.into_boxed_slice()
}

