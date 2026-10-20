//! Diagnostic census of lane exits (`profile-lane-exits`): how often the general loop takes
//! over from the lane at each opcode, printed to stderr when the process exits.

use crate::bytecode::Op;
use std::sync::atomic::{AtomicU64, Ordering};

static EXITS: [AtomicU64; Op::COUNT] = [const { AtomicU64::new(0) }; Op::COUNT];
static REPORT: std::sync::Once = std::sync::Once::new();
/// Opcodes the report lists, most frequent first.
const REPORTED_OPCODES: usize = 30;

pub(super) fn record(op: Op) {
    REPORT.call_once(|| {
        // SAFETY: `report` is a plain function that outlives the process.
        unsafe { libc::atexit(report) };
    });
    EXITS[op as usize].fetch_add(1, Ordering::Relaxed);
}

extern "C" fn report() {
    let mut counts: Vec<(u64, usize)> = EXITS
        .iter()
        .enumerate()
        .map(|(op, count)| (count.load(Ordering::Relaxed), op))
        .filter(|(count, _)| *count > 0)
        .collect();
    counts.sort_unstable_by(|a, b| b.cmp(a));
    let total: u64 = counts.iter().map(|(count, _)| count).sum();
    eprintln!("lane exits: {total}");
    for (count, op) in counts.into_iter().take(REPORTED_OPCODES) {
        eprintln!("  {count:>12} {}", Op::NAMES[op]);
    }
}
