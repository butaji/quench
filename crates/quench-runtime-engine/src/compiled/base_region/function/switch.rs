//! A `WasmBranchTable` (`Step::Switch`) as a balanced tree of int compare rows on the selector slot, mirroring the Wasm
//! tier's `wasm_stitch` `table`: a `Ge` split takes its left half where the slot is below the middle (the JS compare
//! branches where its relation fails), a `Ne` leaf takes its table entry where the slot equals it, and the last leaf
//! jumps to its entry. The tree is one run of parts at the switch's PC: the root first, the rest in layout order.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// Subtrees of at most this many slots are a chain of leaves rather than a split.
const LINEAR: usize = 4;

/// A node of the tree: a split of the slots at `Split` (`slot >= value` falls through), a leaf for one slot, or the
/// unconditional jump to the last slot of its range.
#[derive(Clone, Copy)]
enum Node {
    Split(i32),
    Leaf(usize),
    Jump(usize),
}

/// Appends the nodes of the subtree over slots `low..high` (its first node first, the right half directly after a split,
/// the left half last) and returns the subtree's first node. A split's `taken` is the index of its left half. A subtree
/// whose slots all enter one target is a single `Jump` to its first slot's entry, which the others share.
fn build(nodes: &mut Vec<(Node, Option<usize>)>, targets: &[Option<u32>], low: usize, high: usize) -> usize {
    let first = nodes.len();
    if targets[low].is_some() && targets[low..high].iter().all(|target| *target == targets[low]) {
        nodes.push((Node::Jump(low), None));
        return first;
    }
    if high - low <= LINEAR {
        nodes.extend((low..high - 1).map(|slot| (Node::Leaf(slot), None)));
        nodes.push((Node::Jump(high - 1), None));
        return first;
    }
    let middle = (low + high) / 2;
    let split = nodes.len();
    nodes.push((Node::Split(middle as i32), None));
    build(nodes, targets, middle, high);
    let left = build(nodes, targets, low, middle);
    nodes[split].1 = Some(left);
    first
}

/// The plan of the switch row `row` at `pc` (its selector in `a`, its slot count in `hole`): the tree's root, with the
/// rest of its nodes as the run's later parts. The entry of slot `k` is the `Jump` at `pc + 1 + k`.
pub(crate) fn switch_run(layout: &Layout, (reprs, kinds): (&Reprs, Option<&OperandKinds>), rows: &[Decoded], pc: u32, row: &Decoded, helpers: LaneHelpers) -> Result<Planned, String> {
    let selector = row.a.ok_or("switch: no selector")?;
    let targets: Vec<Option<u32>> = (0..=row.hole as usize).map(|slot| rows.get(pc as usize + 1 + slot).and_then(|entry| entry.target)).collect();
    let mut nodes = Vec::new();
    build(&mut nodes, &targets, 0, row.hole as usize + 1);
    let entry = |slot: usize| pc + 1 + slot as u32;
    let constant = |value: i64| Source::Constant(quench_runtime::Value::integer(value as i32).to_bits());
    let mut parts = Vec::with_capacity(nodes.len());
    for (node, left) in nodes {
        let (decoded, taken) = match node {
            Node::Split(value) => (Decoded::number(Kind::Ge, None, Some(selector), Some(constant(i64::from(value))), None), left.map(Branch::Part)),
            Node::Leaf(slot) => (Decoded::number(Kind::Ne, None, Some(selector), Some(constant(slot as i64)), Some(entry(slot))), None),
            Node::Jump(slot) => (Decoded::number(Kind::Jump, None, None, None, Some(entry(slot))), None),
        };
        let mut part = plan_row(layout, (reprs, kinds), rows, true, pc, &decoded, helpers)?;
        if taken.is_some() {
            part.branch = taken;
        }
        parts.push(part);
    }
    let mut root = parts.remove(0);
    root.post = parts;
    Ok(root)
}
