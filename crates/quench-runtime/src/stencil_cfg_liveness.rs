//! Register-liveness fixed point over canonical CFG successors.

use super::{BaselineEntry, Successors};
use std::collections::BTreeSet;

pub(super) fn register_liveness(
    entries: &[BaselineEntry],
    operand_windows: &[Option<&[u16]>],
    successors: &[Successors],
) -> Vec<BTreeSet<u16>> {
    fixed_point_register_liveness(entries, operand_windows, successors)
}

/// Compute the exact monotone liveness fixed point without tying convergence
/// to source size. The old round cap (`2 * instruction_count + 1`) was only a
/// heuristic: long diamonds and loop nests could exhaust it and pessimistically
/// mark every register live, which then prevented otherwise valid native
/// regions. A worklist visits a node again only when one of its successor facts
/// changes, so the only termination bound is the finite register/edge lattice.
fn fixed_point_register_liveness(
    entries: &[BaselineEntry],
    operand_windows: &[Option<&[u16]>],
    successors: &[Successors],
) -> Vec<BTreeSet<u16>> {
    let conservative = conservative_registers(entries, operand_windows);
    let mut live_in = vec![BTreeSet::new(); entries.len()];
    let mut live_out = live_in.clone();
    let mut predecessors = match predecessor_table(successors) {
        Some(predecessors) => predecessors,
        None => return vec![conservative; entries.len()],
    };
    let mut worklist = Vec::new();
    if worklist.try_reserve(entries.len()).is_err() {
        return vec![conservative; entries.len()];
    }
    worklist.extend(0..entries.len());
    let mut queued = vec![true; entries.len()];
    while let Some(pc) = worklist.pop() {
        queued[pc] = false;
        let output = successor_input_union(&successors[pc], &live_in);
        let flow = entries[pc].instruction.register_flow();
        let input = live_input(
            &output,
            flow,
            operand_windows.get(pc).copied().flatten(),
            &conservative,
        );
        if live_out[pc] == output && live_in[pc] == input {
            continue;
        }
        live_out[pc] = output;
        live_in[pc] = input;
        let Some(incoming) = predecessors.get(pc) else {
            return vec![conservative; entries.len()];
        };
        // A predecessor is enqueued when this node changes. The queued bit is
        // only scheduler state, not another liveness fact; it prevents a
        // high-fan-in loop from accumulating redundant visits.
        for &predecessor in incoming {
            if !queued[predecessor] {
                queued[predecessor] = true;
                worklist.push(predecessor);
            }
        }
    }
    live_out
}

fn predecessor_table(successors: &[Successors]) -> Option<Vec<Vec<usize>>> {
    let mut predecessors = Vec::new();
    predecessors.try_reserve(successors.len()).ok()?;
    predecessors.resize_with(successors.len(), Vec::new);
    for (pc, edges) in successors.iter().enumerate() {
        for successor in edges.iter() {
            let incoming = predecessors.get_mut(successor)?;
            incoming.try_reserve(1).ok()?;
            incoming.push(pc);
        }
    }
    Some(predecessors)
}

pub(super) fn live_inputs(
    entries: &[BaselineEntry],
    windows: &[Option<&[u16]>],
    live_out: &[BTreeSet<u16>],
) -> Vec<BTreeSet<u16>> {
    let conservative = conservative_registers(entries, windows);
    entries
        .iter()
        .enumerate()
        .map(|(pc, entry)| {
            live_input(
                &live_out[pc],
                entry.instruction.register_flow(),
                windows.get(pc).copied().flatten(),
                &conservative,
            )
        })
        .collect()
}

pub(super) fn bounded_register_liveness(
    entries: &[BaselineEntry],
    operand_windows: &[Option<&[u16]>],
    successors: &[Successors],
    round_limit: usize,
) -> Vec<BTreeSet<u16>> {
    let conservative = conservative_registers(entries, operand_windows);
    let mut live_in = vec![BTreeSet::new(); entries.len()];
    let mut live_out = live_in.clone();
    for _ in 0..round_limit {
        if !liveness_round(
            entries,
            operand_windows,
            successors,
            &conservative,
            &mut live_in,
            &mut live_out,
        ) {
            return live_out;
        }
    }
    vec![conservative; entries.len()]
}

fn liveness_round(
    entries: &[BaselineEntry],
    windows: &[Option<&[u16]>],
    successors: &[Successors],
    conservative: &BTreeSet<u16>,
    live_in: &mut [BTreeSet<u16>],
    live_out: &mut [BTreeSet<u16>],
) -> bool {
    let mut changed = false;
    for pc in (0..entries.len()).rev() {
        let output = successor_input_union(&successors[pc], live_in);
        let flow = entries[pc].instruction.register_flow();
        let input = live_input(
            &output,
            flow,
            windows.get(pc).copied().flatten(),
            conservative,
        );
        changed |= live_out[pc] != output || live_in[pc] != input;
        live_out[pc] = output;
        live_in[pc] = input;
    }
    changed
}

fn successor_input_union(successors: &Successors, live_in: &[BTreeSet<u16>]) -> BTreeSet<u16> {
    let mut output = BTreeSet::new();
    for successor in successors.iter() {
        if let Some(input) = live_in.get(successor) {
            output.extend(input.iter().copied());
        }
    }
    output
}

fn conservative_registers(
    entries: &[BaselineEntry],
    operand_windows: &[Option<&[u16]>],
) -> BTreeSet<u16> {
    let mut registers = entries
        .iter()
        .flat_map(|entry| entry.instruction.register_flow().uses)
        .flatten()
        .collect::<BTreeSet<_>>();
    // An opaque structured operation may read a value produced earlier even
    // when that register has no later compact use. Include definitions so an
    // Unknown edge cannot authorize a fusion to clear such a live value.
    registers.extend(
        entries
            .iter()
            .filter_map(|entry| entry.instruction.register_flow().definition),
    );
    registers.extend(
        operand_windows
            .iter()
            .flatten()
            .flat_map(|window| window.iter().copied()),
    );
    registers
}

fn live_input(
    output: &BTreeSet<u16>,
    flow: crate::ir::RegisterFlow,
    window: Option<&[u16]>,
    conservative: &BTreeSet<u16>,
) -> BTreeSet<u16> {
    let mut input = if flow.complete {
        output.clone()
    } else {
        conservative.clone()
    };
    if let Some(definition) = flow.definition {
        input.remove(&definition);
    }
    input.extend(flow.uses.into_iter().flatten());
    input.extend(window.into_iter().flatten().copied());
    input
}
