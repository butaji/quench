//! `QUENCH_DEV=fact-stats`: how often each fact cell was retired, printed at exit.
//!
//! A cell is one kind's subject (a shape, a slot owner, a chain's subject, a layout, an atom, a
//! global cell). Each retirement that takes something live counts once against its cell.

use crate::trace::Dev;
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FactKind {
    Shape,
    Slot,
    Chain,
    Layout,
    Atom,
    Global,
}

/// Per kind, in variant order: its name in the dump and the weakenings its ladder allows.
const KINDS: [(&str, u32); 6] = [
    ("shape", 3),
    ("slot", 3),
    ("chain", 3),
    ("layout", 3),
    ("atom", 3),
    ("global", 3),
];

static CELLS: Mutex<BTreeMap<(FactKind, u64), u32>> = Mutex::new(BTreeMap::new());

/// Counts one retirement of `kind`'s cell `id`, when the experiment is on.
#[inline(always)]
pub(crate) fn retired(kind: FactKind, id: u64) {
    if Dev::FactStats.on() {
        count(kind, id);
    }
}

#[inline(never)]
fn count(kind: FactKind, id: u64) {
    if let Ok(mut cells) = CELLS.lock() {
        *cells.entry((kind, id)).or_default() += 1;
    }
}

/// Prints one line per retired cell, then the summary of the cells over their ladder.
pub(crate) fn report() {
    if !Dev::FactStats.on() {
        return;
    }
    let Ok(cells) = CELLS.lock() else { return };
    let mut over_ladder = 0;
    for ((kind, id), weakenings) in cells.iter() {
        let (name, ladder) = KINDS[*kind as usize];
        over_ladder += usize::from(*weakenings > ladder);
        eprintln!(
            "QUENCH_FACT_STATS {{\"kind\":\"{name}\",\"id\":{id},\"weakenings\":{weakenings},\"ladder\":{ladder}}}"
        );
    }
    eprintln!(
        "QUENCH_FACT_STATS_SUMMARY {{\"cells\":{},\"over_ladder\":{over_ladder}}}",
        cells.len()
    );
}
