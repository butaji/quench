//! The program's call graph and the functions a loop or a call cycle reaches, solved once per program
//! (`ProgramStore::call_graph`). An edge is a callee the AOT facts name at a call or a construction
//! (`FunctionFacts::call_targets`, `construct_target`); a site the facts name no callee of adds none.
//! A function is in a loop or a cycle where it has a back edge, a loop body calls it, it is in a call
//! cycle, or every caller of it is in a loop or a cycle (`CallGraph::in_loop_or_cycle`). Owner: AOT facts.

use crate::bytecode::{may_repeat, ResidualProgram};
use crate::dataflow::{self, Dataflow};
use crate::staged::meaning::{means, Meaning};
use super::aot_facts::AotFacts;

/// The `cycle` entry of a function that is in no call cycle.
const NO_CYCLE: u32 = u32::MAX;

/// The call graph of one program, with its call cycles and its loop-or-cycle facts.
pub struct CallGraph {
    /// Per function, the index into `cycles` of its call cycle, `NO_CYCLE` where it is in none.
    cycle: Box<[u32]>,
    /// The members of each call cycle: a strongly connected component of several functions, or a function that calls itself.
    cycles: Box<[Box<[u32]>]>,
    /// Per function, whether a loop or a call cycle reaches it.
    loop_or_cycle: Box<[bool]>,
}

impl CallGraph {
    /// The call graph of `program`, its edges the callees `facts` names, and its facts solved from it.
    pub fn of(program: &ResidualProgram, facts: &AotFacts) -> Self {
        let count = program.function_count();
        let mut callees: Vec<Box<[usize]>> = Vec::with_capacity(count);
        let mut own_loop = vec![false; count];
        let mut called_in_loop = vec![false; count];
        for function in 0..count {
            let mut out: Vec<usize> = Vec::new();
            if let Some(body) = program.function_get(function) {
                let backward: Vec<(usize, usize)> = body.back_edges().map(|(source, target)| (target, source)).collect();
                own_loop[function] = !backward.is_empty();
                let calls = facts.function_facts(function as u32);
                for (pc, instruction) in body.instructions().enumerate() {
                    let at = pc as u32;
                    let targets: Vec<u32> = if means(instruction, Meaning::Construct) {
                        calls.construct_target(at).into_iter().collect()
                    } else if means(instruction, Meaning::Call) {
                        calls.call_targets(at)
                    } else {
                        continue;
                    };
                    let looped = may_repeat(&backward, pc);
                    for callee in targets.into_iter().map(|callee| callee as usize).filter(|callee| *callee < count) {
                        out.push(callee);
                        if looped { called_in_loop[callee] = true; }
                    }
                }
            }
            out.sort_unstable();
            out.dedup();
            callees.push(out.into_boxed_slice());
        }
        let members = cycles(&callees);
        let mut cycle = vec![NO_CYCLE; count];
        for (index, members) in members.iter().enumerate() {
            for &function in members { cycle[function as usize] = index as u32; }
        }
        let local: Vec<bool> = (0..count).map(|function| own_loop[function] || called_in_loop[function] || cycle[function] != NO_CYCLE).collect();
        let mut has_caller = vec![false; count];
        for out in &callees { for &callee in out.iter() { has_caller[callee] = true; } }
        let states = dataflow::solve(&mut ColdReach { local: &local, callees: &callees, has_caller: &has_caller });
        let loop_or_cycle: Box<[bool]> = (0..count).map(|function| local[function] || states[function] != Some(true)).collect();
        Self {
            cycle: cycle.into_boxed_slice(),
            cycles: members.into_iter().map(Vec::into_boxed_slice).collect(),
            loop_or_cycle,
        }
    }

    /// Whether a loop or a call cycle reaches `function`: it has a back edge, a loop body calls it, it is in a call
    /// cycle, or every caller of it is in a loop or a cycle.
    pub fn in_loop_or_cycle(&self, function: u32) -> bool {
        self.loop_or_cycle.get(function as usize).copied().unwrap_or(false)
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

/// The call cycles of `callees`, by Tarjan's strongly connected components in one pass: each component of several
/// functions, and each function that calls itself, as its members.
fn cycles(callees: &[Box<[usize]>]) -> Vec<Vec<u32>> {
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
                        members.push(w as u32);
                        if w == v { break; }
                    }
                    if members.len() > 1 || callees[v].contains(&v) { found.push(members); }
                }
            }
        }
    }
    found
}

/// Where "cold" reaches: a function that no loop or cycle reaches on its own, and that a cold caller calls, or that
/// no function calls (entered from outside the program). A function a loop or a cycle reaches on its own is never cold,
/// so a function is cold exactly when every caller it has is outside a loop or a cycle.
struct ColdReach<'a> {
    local: &'a [bool],
    callees: &'a [Box<[usize]>],
    has_caller: &'a [bool],
}

impl Dataflow for ColdReach<'_> {
    type State = bool;

    fn blocks(&self) -> usize {
        self.callees.len()
    }

    fn edges(&self, block: usize) -> &[usize] {
        &self.callees[block]
    }

    fn entry_state(&self, block: usize) -> Option<bool> {
        (!self.local[block] && !self.has_caller[block]).then_some(true)
    }

    fn join(&self, into: &mut bool, incoming: &bool) -> bool {
        let changed = *incoming && !*into;
        *into |= *incoming;
        changed
    }

    fn transfer(&mut self, block: usize, state: &bool) -> bool {
        *state && !self.local[block]
    }
}
