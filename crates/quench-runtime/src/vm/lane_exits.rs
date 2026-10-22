//! Diagnostic census of lane exits (`profile-lane-exits`): how often the general loop takes
//! over from the lane at each opcode, printed to stderr when the process exits.

use crate::bytecode::Op;
use std::sync::atomic::{AtomicU64, Ordering};

static EXITS: [AtomicU64; Op::COUNT] = [const { AtomicU64::new(0) }; Op::COUNT];
static REPORT: std::sync::Once = std::sync::Once::new();
/// Opcodes the report lists, most frequent first.
const REPORTED_OPCODES: usize = 30;
/// Detail lines reported, most frequent first.
const REPORTED_DETAILS: usize = 200;

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
        for (detail, count) in details.into_iter().take(REPORTED_DETAILS) {
            eprintln!("  {count:>12} {detail}");
        }
    }
}

/// Per-function instruction census (`profile-lane-exits`): lane records dispatched and
/// general-loop instructions stepped, by the function whose view or frame ran them.
#[derive(Default)]
struct FunctionCensus {
    /// Each lane view's record address range and its function's index, sorted by start.
    views: Vec<(usize, usize, usize)>,
    /// Each lane view's opcode per record, by its start address.
    view_ops: std::collections::HashMap<usize, Vec<Op>>,
    /// Lane records dispatched by opcode, and by the opcode dispatched just before.
    op_counts: Vec<u64>,
    pair_counts: std::collections::HashMap<(usize, usize), u64>,
    previous_op: Option<Op>,
    labels: Vec<String>,
    label_index: std::collections::HashMap<String, usize>,
    lane: Vec<u64>,
    general: Vec<u64>,
    general_keys: std::collections::HashMap<(u32, u32), usize>,
    last: (usize, usize, usize),
}

static FUNCTIONS: std::sync::Mutex<Option<FunctionCensus>> = std::sync::Mutex::new(None);

fn with_functions<T>(f: impl FnOnce(&mut FunctionCensus) -> T) -> T {
    let mut census = FUNCTIONS.lock().expect("function census");
    f(census.get_or_insert_with(Default::default))
}

fn function_index(census: &mut FunctionCensus, label: String) -> usize {
    if let Some(index) = census.label_index.get(&label) {
        return *index;
    }
    let index = census.labels.len();
    census.labels.push(label.clone());
    census.label_index.insert(label, index);
    census.lane.push(0);
    census.general.push(0);
    index
}

/// Record that the records `start..end` are a view of the function `label` names, whose
/// records are `ops`, in order.
pub(super) fn register_view(start: usize, end: usize, label: String, ops: Vec<Op>) {
    with_functions(|census| {
        census.view_ops.insert(start, ops);
        let index = function_index(census, label);
        let at = census.views.partition_point(|view| view.0 < start);
        census.views.insert(at, (start, end, index));
    });
}

/// Count one dispatch of the lane record at `address`, `record_size` bytes per record.
pub(super) fn count_record(address: usize, record_size: usize) {
    with_functions(|census| {
        let (mut start, end, index) = census.last;
        if !(address >= start && address < end) {
            let at = census.views.partition_point(|view| view.0 <= address);
            let Some(&(view_start, view_end, view_index)) =
                at.checked_sub(1).and_then(|at| census.views.get(at))
            else {
                return;
            };
            if address >= view_end {
                return;
            }
            census.last = (view_start, view_end, view_index);
            start = view_start;
            census.lane[view_index] += 1;
        } else {
            census.lane[index] += 1;
        }
        let Some(&op) = census.view_ops.get(&start).and_then(|ops| ops.get((address - start) / record_size)) else {
            return;
        };
        if census.op_counts.is_empty() {
            census.op_counts = vec![0; Op::COUNT];
        }
        census.op_counts[op as usize] += 1;
        if let Some(previous) = census.previous_op.replace(op) {
            *census.pair_counts.entry((previous as usize, op as usize)).or_default() += 1;
        }
    });
}

/// Opcodes and opcode pairs the dispatch census lists, most frequent first.
const REPORTED_DISPATCHES: usize = 40;

/// Count one general-loop instruction of the function `label` names.
pub(super) fn count_general(label: impl FnOnce() -> String, key: (u32, u32)) {
    with_functions(|census| {
        let index = match census.general_keys.get(&key) {
            Some(index) => *index,
            None => {
                let index = function_index(census, label());
                census.general_keys.insert(key, index);
                index
            }
        };
        census.general[index] += 1;
    });
    REPORT_FUNCTIONS.call_once(|| {
        // SAFETY: `report_functions` is a plain function that outlives the process.
        unsafe { libc::atexit(report_functions) };
    });
}

static REPORT_FUNCTIONS: std::sync::Once = std::sync::Once::new();
/// Functions the per-function census lists, most instructions first.
const REPORTED_FUNCTIONS: usize = 150;

extern "C" fn report_functions() {
    with_functions(|census| {
        let mut rows: Vec<_> = (0..census.labels.len())
            .map(|index| (census.lane[index] + census.general[index], census.lane[index], census.general[index], &census.labels[index]))
            .collect();
        rows.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        let total: u64 = rows.iter().map(|row| row.0).sum();
        let general: u64 = rows.iter().map(|row| row.2).sum();
        eprintln!("function census: {total} instructions, {general} general");
        let mut ops: Vec<_> = census.op_counts.iter().enumerate().filter(|(_, count)| **count > 0).collect();
        ops.sort_unstable_by(|a, b| b.1.cmp(a.1));
        for (op, count) in ops.into_iter().take(REPORTED_DISPATCHES) {
            eprintln!("  lane op {count:>12} {}", Op::NAMES[op]);
        }
        let mut pairs: Vec<_> = census.pair_counts.iter().collect();
        pairs.sort_unstable_by(|a, b| b.1.cmp(a.1));
        for ((first, second), count) in pairs.into_iter().take(REPORTED_DISPATCHES) {
            eprintln!("  lane pair {count:>12} {} -> {}", Op::NAMES[*first], Op::NAMES[*second]);
        }
        for (sum, lane, general, label) in rows.into_iter().take(REPORTED_FUNCTIONS) {
            eprintln!("  {sum:>12} lane={lane:>11} general={general:>10} {label}");
        }
    });
}
