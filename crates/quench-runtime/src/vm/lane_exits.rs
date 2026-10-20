//! Diagnostic census of lane exits (`profile-lane-exits`): how often the general loop takes
//! over from the lane at each opcode, printed to stderr when the process exits.

use crate::bytecode::Op;
use std::sync::atomic::{AtomicU64, Ordering};

static EXITS: [AtomicU64; Op::COUNT] = [const { AtomicU64::new(0) }; Op::COUNT];
static REPORT: std::sync::Once = std::sync::Once::new();
/// Opcodes the report lists, most frequent first.
const REPORTED_OPCODES: usize = 30;

static DETAILS: std::sync::Mutex<Option<std::collections::HashMap<String, u64>>> =
    std::sync::Mutex::new(None);

/// Counts one exit by a description of its operands (operator and value kinds).
pub(super) fn record_detail(detail: String) {
    let mut details = DETAILS.lock().expect("lane exit details");
    *details.get_or_insert_with(Default::default).entry(detail).or_default() += 1;
}

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
    if let Some(details) = DETAILS.lock().expect("lane exit details").as_ref() {
        let mut details: Vec<_> = details.iter().collect();
        details.sort_unstable_by(|a, b| b.1.cmp(a.1));
        for (detail, count) in details.into_iter().take(REPORTED_OPCODES) {
            eprintln!("  {count:>12} {detail}");
        }
    }
}
