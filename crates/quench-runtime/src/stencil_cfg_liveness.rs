//! Bounded register-liveness fixed point over canonical CFG successors.

use super::{BaselineEntry, Successors};
use std::collections::BTreeSet;

pub(super) fn register_liveness(
    entries: &[BaselineEntry],
    operand_windows: &[Option<&[u16]>],
    successors: &[Successors],
) -> Vec<BTreeSet<u16>> {
    bounded_register_liveness(
        entries,
        operand_windows,
        successors,
        entries.len().saturating_mul(2).saturating_add(1),
    )
}

pub(super) fn live_inputs(
    entries: &[BaselineEntry],
    windows: &[Option<&[u16]>],
    live_out: &[BTreeSet<u16>],
) -> Vec<BTreeSet<u16>> {
    let conservative = all_register_uses(entries, windows);
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
    let conservative = all_register_uses(entries, operand_windows);
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

fn all_register_uses(
    entries: &[BaselineEntry],
    operand_windows: &[Option<&[u16]>],
) -> BTreeSet<u16> {
    let mut uses = entries
        .iter()
        .flat_map(|entry| entry.instruction.register_flow().uses)
        .flatten()
        .collect::<BTreeSet<_>>();
    uses.extend(
        operand_windows
            .iter()
            .flatten()
            .flat_map(|window| window.iter().copied()),
    );
    uses
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
