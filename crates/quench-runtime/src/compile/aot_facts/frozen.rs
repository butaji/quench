//! Frozen facts: a binding or a named field the program writes exactly once, with an immediate, at the root, where that store
//! dominates every read of the key. Such a value never changes after the store, so a frozen image folds it at each read
//! (`AotFacts::dominated_root_store`). The decision lives here, over the closed world's own call graph and dominators, so no
//! tier re-derives it.
//!
//! The fact for a key `K` holds when all of these hold in the closed world (`World::Closed`, which `analyze` returns only once
//! `gate::verify` has checked it):
//! - Single write. The program's flows record exactly one write of `K` (fields: one write of the name, on any object). A write
//!   the census cannot follow (`Census::wild`) reaches the key and kills the fact.
//! - Root store. That one write is in the root function (id 0) and reached from the root's entry. Its source register holds an
//!   immediate constant loaded in the same basic block, with no write of the register between, so the stored value is that
//!   constant on every path to the store.
//! - Dominance. Every read of `K` (`FunctionFlow::key_reads`) runs after the store. A read in the root must be dominated by the
//!   store's block (or follow it in the same block). A read in another function must run only after the store: that function
//!   is not a seed (`gate::seeds`: a root, a constructor, an accessor, an escaped or named closure, a Module's declaration), and
//!   every call it may be reached by is made in the root after the store, or from a function that itself runs only after it.
//!
//! Each argument is a greatest fixpoint: a function is late until one of its calls may run before the store.

use std::collections::BTreeMap;

use crate::bytecode::{Constant, Successors};
use crate::staged::Operation;
use crate::typed_ir::{Cell, FactKey, Type};
use crate::value::Value;
use super::blocks::Blocks;
use super::census::Census;
use super::escapes::Escapes;
use super::flow::FunctionFlow;
use super::gate::{self, World};
use super::types::targets;
use super::value::{Key, Word};
use super::AotFacts;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// The single store of a frozen key: the root's store of an immediate, which dominates every read of the key.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RootStore {
    /// The function the store lies in: always the root (id 0).
    pub function: u32,
    /// The store's PC in `function`.
    pub pc: u32,
    /// The cell the solve settled for the value the store writes.
    pub cell: Cell,
    /// The immediate the store writes (a number, a Boolean, `null` or `undefined`), which the block of the store loads.
    pub value: Value,
}

impl AotFacts {
    /// The root store that dominates every read of `key`, when the closed world writes `key` exactly once, with an immediate,
    /// before any code that reads it can run (this module's header states the rules). A field's key is the name's: a read or a
    /// store of any object's property of that name counts, so the store fixes the value every store of the name writes; it does
    /// not say which object holds a read's value, so a read still resolves its holder. None in an open world, and for any key
    /// the rules do not hold.
    pub fn dominated_root_store(&self, key: FactKey) -> Option<RootStore> {
        let key = match key {
            FactKey::Field { atom, .. } => FactKey::AnyField(atom),
            other => other,
        };
        self.root_stores.get(&key).copied()
    }
}

/// The frozen keys of `flows` in `world`: each key that has one root store (`RootStore`) and no other write.
pub(super) fn root_stores(program: &crate::bytecode::ResidualProgram, world: World, flows: &[FunctionFlow], facts: &AotFacts,
    census: &Census, escaped: &Escapes,
) -> BTreeMap<FactKey, RootStore> {
    if world != World::Closed || flows.is_empty() { return BTreeMap::new(); }
    let root = program.function(0);
    let blocks = Blocks::of(&Successors::of_code(root), root.code.len());
    let dominators = Dominators::of(&blocks, root.code.len());
    let mut seeded = vec![false; flows.len()];
    for seed in gate::seeds(program, flows, census, escaped) {
        if let Some(slot) = seeded.get_mut(seed as usize) { *slot = true; }
    }
    let mut writes: BTreeMap<FactKey, Vec<(u32, u32, Word)>> = BTreeMap::new();
    let mut reads: BTreeMap<FactKey, Vec<(u32, u32)>> = BTreeMap::new();
    for (function, flow) in flows.iter().enumerate() {
        for &(pc, key, word) in &flow.writes {
            if let Some(group) = fact_group(key) { writes.entry(group).or_default().push((function as u32, pc, word)); }
        }
        for &(pc, key) in &flow.key_reads {
            if let Some(group) = fact_group(key) { reads.entry(group).or_default().push((function as u32, pc)); }
        }
    }
    let mut roots = BTreeMap::new();
    for (&key, sites) in &writes {
        let [(function, pc, word)] = sites.as_slice() else { continue };
        if *function != 0 || wild(census, key) { continue; }
        let Some(value) = store_value(program, root, &blocks, &dominators, *pc) else { continue };
        // The solve's cell of the stored word is no wider than the constant it holds.
        if !word.cell.ty.is_subtype(Type::immediate(value)) { continue; }
        // A store no path from the entry reaches never writes the key.
        if !dominators.reached(*pc) { continue; }
        let key_reads = reads.get(&key).map_or(&[][..], Vec::as_slice);
        if !dominates_reads(flows, facts, census, &seeded, &dominators, *pc, key_reads) { continue; }
        roots.insert(key, RootStore { function: 0, pc: *pc, cell: word.cell, value });
    }
    roots
}

/// The key a write or a read of `key` belongs to: a global's own, a slot's own, and a named property's name-wide group (every
/// class's and every object's property of the name, `FactKey::AnyField`). A return or an element is in no frozen group.
fn fact_group(key: Key) -> Option<FactKey> {
    match key {
        Key::Global(atom) => Some(FactKey::Global(atom)),
        Key::Slot { owner, slot } => Some(FactKey::Slot { owner, slot }),
        Key::Member(atom) | Key::MemberAny(atom) | Key::MemberOn { atom, .. } => Some(FactKey::AnyField(atom)),
        Key::Return(_) | Key::Element(_) => None,
    }
}

/// Whether a write the analysis cannot follow reaches `key` (`Census::wild`, by the key's own kind).
fn wild(census: &Census, key: FactKey) -> bool {
    match key {
        FactKey::Global(atom) => census.wild(Key::Global(atom)),
        FactKey::Slot { owner, slot } => census.wild(Key::Slot { owner, slot }),
        FactKey::AnyField(atom) => census.wild(Key::Member(atom)),
        _ => true,
    }
}

/// The immediate a root store of `pc` writes: the constant its source register loads earlier in the same basic block, with
/// no write of that register in between. None where the block does not load one.
fn store_value(program: &crate::bytecode::ResidualProgram, root: &crate::bytecode::Function, blocks: &Blocks, dominators: &Dominators,
    pc: u32,
) -> Option<Value> {
    let pc = pc as usize;
    let source = root.instruction_at(pc).register_a();
    let block = dominators.block_of.get(pc).copied()?;
    let start = blocks.ranges.get(block)?.start;
    for at in (start..pc).rev() {
        let instruction = root.instruction_at(at);
        if !instruction.written_registers().any(|written| written == source) { continue; }
        let (destination, constant) = Operation::<(), ()>::decode_constant_load(instruction)?;
        if destination != source { return None; }
        return immediate(program.constants.get(constant)?);
    }
    None
}

/// The value a constant denotes where it is an immediate (no heap allocation): numbers, booleans, `null` and `undefined`.
fn immediate(constant: &Constant) -> Option<Value> {
    match constant {
        Constant::Number(number) => Some(Value::number(*number)),
        Constant::Boolean(value) => Some(Value::boolean(*value)),
        Constant::Null => Some(Value::NULL),
        Constant::Undefined => Some(Value::UNDEFINED),
        _ => None,
    }
}

/// Whether every read in `key_reads` runs after the store at root `store`: a read in the root is dominated by it, and a read in
/// another function runs only after it (`late`).
fn dominates_reads(flows: &[FunctionFlow], facts: &AotFacts, census: &Census, seeded: &[bool], dominators: &Dominators,
    store: u32, key_reads: &[(u32, u32)],
) -> bool {
    if key_reads.is_empty() { return true; }
    let late = late_functions(flows, facts, census, seeded, dominators, store);
    key_reads.iter().all(|&(function, pc)| {
        if function == 0 { dominators.dominates_pc(store, pc) } else { late.get(function as usize).copied().unwrap_or(false) }
    })
}

/// Per function, whether it runs only after the root store at `store`: not a seed, and every call the solve may reach it by is
/// made in the root after the store, or from a function that is itself late. A greatest fixpoint: a function starts late and
/// loses the mark at the first call that may run before the store.
fn late_functions(flows: &[FunctionFlow], facts: &AotFacts, census: &Census, seeded: &[bool], dominators: &Dominators, store: u32) -> Vec<bool> {
    let mut late: Vec<bool> = (0..flows.len()).map(|function| function != 0 && !seeded[function]).collect();
    loop {
        let mut changed = false;
        for (caller, flow) in flows.iter().enumerate() {
            for call in &flow.calls {
                let after_store = if caller == 0 { dominators.dominates_pc(store, call.pc) } else { late[caller] };
                if after_store { continue; }
                for callee in targets(caller as u32, call, facts, census) {
                    if let Some(mark) = late.get_mut(callee as usize) && *mark {
                        *mark = false;
                        changed = true;
                    }
                }
            }
        }
        if !changed { return late; }
    }
}

/// The immediate dominators of a function's basic blocks, from its entry block (block 0), over the blocks the entry reaches.
struct Dominators {
    /// The block each PC lies in.
    block_of: Vec<usize>,
    /// Per block, its immediate dominator (the entry's is itself); None where no path from the entry reaches the block.
    idom: Vec<Option<usize>>,
}

impl Dominators {
    /// The dominators of `blocks`, a function of `len` instructions (Cooper, Harvey and Kennedy's iteration over the reverse
    /// postorder of the reached blocks).
    fn of(blocks: &Blocks, len: usize) -> Self {
        let count = blocks.ranges.len();
        let mut block_of = vec![0; len];
        for (block, range) in blocks.ranges.iter().enumerate() { block_of[range.clone()].fill(block); }
        if count == 0 { return Self { block_of, idom: Vec::new() }; }
        let rpo = reverse_postorder(blocks);
        let mut order = vec![usize::MAX; count];
        for (index, &block) in rpo.iter().enumerate() { order[block] = index; }
        let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); count];
        for &block in &rpo {
            for &target in &blocks.targets[block] { predecessors[target].push(block); }
        }
        let mut idom: Vec<Option<usize>> = vec![None; count];
        idom[0] = Some(0);
        let mut changed = true;
        while changed {
            changed = false;
            for &block in rpo.iter().skip(1) {
                let found = predecessors[block].iter().filter(|&&pred| idom[pred].is_some()).fold(None, |found, &pred| {
                    Some(found.map_or(pred, |current| intersect(&idom, &order, pred, current)))
                });
                if found.is_some() && idom[block] != found {
                    idom[block] = found;
                    changed = true;
                }
            }
        }
        Self { block_of, idom }
    }

    /// Whether block `above` dominates block `below`: every path from the entry to `below` passes through `above`.
    fn dominates(&self, above: usize, below: usize) -> bool {
        let mut current = below;
        loop {
            if current == above { return true; }
            match self.idom.get(current).copied().flatten() {
                Some(up) if up != current => current = up,
                _ => return false,
            }
        }
    }

    /// Whether the instruction at `store` runs before the one at `pc` on every path to `pc` (strictly before). A `pc` no path
    /// from the entry reaches never runs, so it is ordered after any store.
    fn dominates_pc(&self, store: u32, pc: u32) -> bool {
        let (Some(&store_block), Some(&block)) = (self.block_of.get(store as usize), self.block_of.get(pc as usize)) else { return false };
        match self.idom.get(block).copied().flatten() {
            None => true,
            Some(_) if store_block == block => store < pc,
            Some(_) => self.dominates(store_block, block),
        }
    }

    /// Whether a path from the entry reaches the instruction at `pc`.
    fn reached(&self, pc: u32) -> bool {
        self.block_of.get(pc as usize).is_some_and(|&block| self.idom.get(block).copied().flatten().is_some())
    }
}

/// The blocks the entry reaches, in reverse postorder of a depth-first walk from the entry (which comes first).
fn reverse_postorder(blocks: &Blocks) -> Vec<usize> {
    let mut visited = vec![false; blocks.ranges.len()];
    let mut postorder = Vec::with_capacity(blocks.ranges.len());
    let mut stack = vec![(0_usize, 0_usize)];
    visited[0] = true;
    while let Some((block, next)) = stack.pop() {
        match blocks.targets[block].get(next).copied() {
            Some(target) => {
                stack.push((block, next + 1));
                if !visited[target] {
                    visited[target] = true;
                    stack.push((target, 0));
                }
            }
            None => postorder.push(block),
        }
    }
    postorder.reverse();
    postorder
}

/// The nearest common dominator of two reached blocks, by their reverse-postorder numbers.
fn intersect(idom: &[Option<usize>], order: &[usize], mut first: usize, mut second: usize) -> usize {
    while first != second {
        while order[first] > order[second] { first = idom[first].unwrap_or(0); }
        while order[second] > order[first] { second = idom[second].unwrap_or(0); }
    }
    first
}
