//! Where a lane lowering's CLIF goes: each IR instruction the lowering emits charged to the
//! bytecode op it lowers, or to the shared parts around them (the entry, block edges, leaves,
//! exits), printed per lowering under `QUENCH_CLIF_CENSUS` for the CLIF-per-op budget.

#[allow(unused_imports)]
use crate::ext_traits::*;
use std::collections::BTreeMap;

/// The parts of a lowering that no single bytecode op owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Part { Entry, Edges, Leaves, Exits, Tail }

pub(crate) struct Census {
    ops: BTreeMap<String, (u32, u32)>,
    parts: BTreeMap<Part, u32>,
    blocks: BTreeMap<String, u32>,
    units: u32,
}

pub(crate) fn enabled() -> bool {
    quench_runtime::vm::switches::clif_census()
}

impl Census {
    pub(crate) fn new() -> Option<Self> {
        enabled().then(|| Self { ops: BTreeMap::new(), parts: BTreeMap::new(), blocks: BTreeMap::new(), units: 0 })
    }

    pub(crate) fn op(&mut self, name: impl std::fmt::Debug, emitted: usize) {
        let entry = self.ops.entry(format!("{name:?}")).or_default();
        entry.0 += 1;
        entry.1 += emitted as u32;
        self.units += 1;
    }

    /// Whether to print the CLIF of this op's lowering: the first few of each op where
    /// `QUENCH_CLIF_CENSUS=sample`.
    pub(crate) fn sample(&self, name: impl std::fmt::Debug) -> bool {
        quench_runtime::vm::switches::clif_census_sample()
            && self.ops.get(&format!("{name:?}")).is_none_or(|(count, _)| *count < 2)
    }

    pub(crate) fn blocks(&mut self, name: impl std::fmt::Debug, made: usize) {
        *self.blocks.entry(format!("{name:?}")).or_default() += made as u32;
    }

    pub(crate) fn part(&mut self, part: Part, emitted: usize) {
        *self.parts.entry(part).or_default() += emitted as u32;
    }

    pub(crate) fn report(&self, function: u32, total: usize) {
        let ops: Vec<String> = self.ops.iter().map(|(name, (count, emitted))| format!("\"{name}\":[{count},{emitted}]")).collect();
        let blocks: Vec<String> = self.blocks.iter().map(|(name, made)| format!("\"{name}\":{made}")).collect();
        let parts: Vec<String> = self.parts.iter().map(|(part, emitted)| format!("\"{part:?}\":{emitted}")).collect();
        eprintln!("QUENCH_CLIF_CENSUS {{\"function\":{function},\"ops\":{},\"ir\":{total},\"by_op\":{{{}}},\"parts\":{{{}}},\"blocks\":{{{}}}}}", self.units, ops.join(","), parts.join(","), blocks.join(","));
    }
}
