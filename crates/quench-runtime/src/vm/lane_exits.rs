//! Diagnostic census of lane exits (`profile-lane-exits`): how often the general loop takes
//! over from the lane at each opcode, printed to stderr when the process exits.

use crate::bytecode::Op;
use std::sync::atomic::{AtomicU64, Ordering};

static EXITS: [AtomicU64; Op::COUNT] = [const { AtomicU64::new(0) }; Op::COUNT];
static REPORT: std::sync::Once = std::sync::Once::new();

pub(super) enum CaptureEvent { LaneHit, LaneMiss, WalkHit, WalkMiss }

impl CaptureEvent {
    fn counter(&self) -> (&'static str, &'static AtomicU64) {
        static LANE_HIT: AtomicU64 = AtomicU64::new(0);
        static LANE_MISS: AtomicU64 = AtomicU64::new(0);
        static WALK_HIT: AtomicU64 = AtomicU64::new(0);
        static WALK_MISS: AtomicU64 = AtomicU64::new(0);
        match self {
            Self::LaneHit => ("lane_outer_hit", &LANE_HIT),
            Self::LaneMiss => ("lane_outer_miss", &LANE_MISS),
            Self::WalkHit => ("vm_walk_hit", &WALK_HIT),
            Self::WalkMiss => ("vm_walk_miss", &WALK_MISS),
        }
    }
}

pub(super) fn capture(event: CaptureEvent) {
    event.counter().1.fetch_add(1, Ordering::Relaxed);
    REPORT.call_once(|| {
        // SAFETY: report has process lifetime and the C atexit signature.
        unsafe { libc::atexit(report) };
    });
}
/// Opcodes the report lists, most frequent first.
const REPORTED_OPCODES: usize = 30;
/// Detail lines reported, most frequent first.
const REPORTED_DETAILS: usize = 200;

static DETAILS: std::sync::Mutex<Option<std::collections::HashMap<String, u64>>> =
    std::sync::Mutex::new(None);

/// Lane dispatches: one per record the lane loop executes. The census's opcode counts
/// cover only the general loop's dispatches, so this is what turns retired instructions
/// into a per-executed-instruction cost.
static DISPATCHES: AtomicU64 = AtomicU64::new(0);
/// Lane dispatches by opcode: the mix a row actually executes, which the exit census
/// cannot show because it counts only the general loop's dispatches.
static DISPATCH_OPS: [AtomicU64; Op::COUNT] = [const { AtomicU64::new(0) }; Op::COUNT];

static CALL_TARGET_SAMPLE: AtomicU64 = AtomicU64::new(0);
/// Exact, opt-in census of the general dispatch loop's in-loop call-target probe.
static CALL_ELIGIBILITY: [AtomicU64; CallEligibility::NAMES.len()] = [const { AtomicU64::new(0) }; CallEligibility::NAMES.len()];
static CALL_TARGETS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<(u32, u32, u32), CallTargetProfile>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
static FIELD_PROBES: std::sync::Mutex<
    Option<std::collections::HashMap<(u32, u32, u32, u32, usize, u32, u8, u8), u64>>,
> = std::sync::Mutex::new(None);

/// One sampled call site's target identities and function bodies.
#[derive(Default)]
struct CallTargetProfile {
    samples: u64,
    callees: std::collections::HashMap<crate::value::Value, u64>,
    functions: std::collections::HashMap<u32, u64>,
}

/// Sample one in every 64 script calls for call-site target stability. The
/// diagnostic is opt-in at build time (`profile-lane-exits`) and keeps its per-call cost
/// low enough to preserve useful mono/poly distribution data on long React runs.
pub(super) fn call_target(
    program: u32,
    caller: u32,
    pc: u32,
    callee: crate::value::Value,
    target: u32,
) {
    if CALL_TARGET_SAMPLE.fetch_add(1, Ordering::Relaxed) & 63 != 0 {
        return;
    }
    REPORT.call_once(|| {
        // SAFETY: `report` is a plain function that outlives the process.
        unsafe { libc::atexit(report) };
    });
    let mut profiles = CALL_TARGETS.lock().expect("lane call target profile");
    let profile = profiles.entry((program, caller, pc)).or_default();
    profile.samples += 1;
    *profile.callees.entry(callee).or_default() += 1;
    *profile.functions.entry(target).or_default() += 1;
}

/// An in-loop call-target probe outcome.
#[derive(Clone, Copy)]
pub(super) enum CallEligibility {
    PositiveCacheHit,
    PositiveResolution,
    NonHeapCallee,
    NativeOrWrongRealmOrNonCallable,
    MissingProgramOrFunction,
    UnsupportedCallPlan,
}

impl CallEligibility {
    /// Each outcome's report label, in declaration order.
    const NAMES: [&str; 6] = [
        "positive_cache_hit",
        "positive_resolution",
        "non_heap_callee",
        "native_or_wrong_realm_or_non_callable",
        "missing_program_or_function",
        "unsupported_call_plan",
    ];
}

const _: () = assert!(CallEligibility::UnsupportedCallPlan as usize + 1 == CallEligibility::NAMES.len());

/// Record an in-loop call-target probe outcome.
pub(super) fn call_eligibility(outcome: CallEligibility) {
    if let Some(counter) = CALL_ELIGIBILITY.get(outcome as usize) {
        counter.fetch_add(1, Ordering::Relaxed);
        REPORT.call_once(|| {
            // SAFETY: `report` is a plain function that outlives the process.
            unsafe { libc::atexit(report) };
        });
    }
}

/// Count a field-cache outcome at a precise bytecode access. Outcomes are primary hit,
/// primary miss, primary ineligible, inherited cache hit, shaped data read, inherited
/// lookup, getter call, and generic property lookup (0–7). Available only in diagnostics.
pub(super) fn field_probe(
    program: u32,
    function: u32,
    pc: u32,
    field_site: u32,
    site: usize,
    atom: u32,
    part: u8,
    outcome: u8,
) {
    let mut probes = FIELD_PROBES.lock().expect("field probe census");
    *probes
        .get_or_insert_with(Default::default)
        .entry((program, function, pc, field_site, site, atom, part, outcome))
        .or_default() += 1;
    REPORT.call_once(|| {
        // SAFETY: `report` is a plain function that outlives the process.
        unsafe { libc::atexit(report) };
    });
}

/// Counts one exit by a description of its operands (operator and value kinds).
pub(super) fn record_detail(detail: String) {
    let mut details = DETAILS.lock().expect("lane exit details");
    *details.get_or_insert_with(Default::default).entry(detail).or_default() += 1;
}

/// Record one executed lane instruction of `op`.
pub(super) fn dispatch(op: Op) {
    REPORT.call_once(|| {
        // SAFETY: `report` is a plain function that outlives the process.
        unsafe { libc::atexit(report) };
    });
    DISPATCHES.fetch_add(1, Ordering::Relaxed);
    DISPATCH_OPS[op as usize].fetch_add(1, Ordering::Relaxed);
}

pub(super) fn record(op: Op) {
    REPORT.call_once(|| {
        // SAFETY: `report` is a plain function that outlives the process.
        unsafe { libc::atexit(report) };
    });
    EXITS[op as usize].fetch_add(1, Ordering::Relaxed);
}

extern "C" fn report() {
    for event in [CaptureEvent::LaneHit, CaptureEvent::LaneMiss, CaptureEvent::WalkHit, CaptureEvent::WalkMiss] {
        let (name, counter) = event.counter();
        eprintln!("capture {name}: {}", counter.load(Ordering::Relaxed));
    }
    let call_eligibility: Vec<_> = CallEligibility::NAMES
        .iter()
        .enumerate()
        .map(|(index, name)| (*name, CALL_ELIGIBILITY[index].load(Ordering::Relaxed)))
        .filter(|(_, count)| *count > 0)
        .collect();
    if !call_eligibility.is_empty() {
        eprintln!("in-loop call eligibility: {call_eligibility:?}");
    }
    if let Some(probes) = FIELD_PROBES.lock().expect("field probe census").as_ref() {
        let mut probes: Vec<_> = probes.iter().collect();
        probes.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(**count));
        eprintln!("field cache probes (program,function,pc,field_site,cache_site,atom,part,outcome):");
        for (key, count) in probes.into_iter().take(REPORTED_DETAILS) {
            eprintln!("  {count:>12} {key:?}");
        }
    }
    let mut counts: Vec<(u64, usize)> = EXITS
        .iter()
        .enumerate()
        .map(|(op, count)| (count.load(Ordering::Relaxed), op))
        .filter(|(count, _)| *count > 0)
        .collect();
    counts.sort_unstable_by(|a, b| b.cmp(a));
    let total: u64 = counts.iter().map(|(count, _)| count).sum();
    eprintln!("lane dispatches: {}", DISPATCHES.load(Ordering::Relaxed));
    let mut dispatch_ops: Vec<_> = DISPATCH_OPS
        .iter()
        .enumerate()
        .map(|(op, count)| (count.load(Ordering::Relaxed), op))
        .filter(|(count, _)| *count > 0)
        .collect();
    dispatch_ops.sort_unstable_by(|a, b| b.cmp(a));
    for (count, op) in dispatch_ops.into_iter().take(REPORTED_OPCODES) {
        eprintln!("  lane op {count:>12} {}", Op::NAMES[op]);
    }
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
    let profiles = CALL_TARGETS.lock().expect("lane call target profile");
    let mut sites: Vec<_> = profiles.iter().collect();
    sites.sort_unstable_by_key(|(_, profile)| std::cmp::Reverse(profile.samples));
    eprintln!("lane call targets (1/64 sample):");
    for ((program, caller, pc), profile) in sites.into_iter().take(REPORTED_DETAILS) {
        let top_callees = top_counts(&profile.callees, 3);
        let top_functions = top_counts(&profile.functions, 3);
        eprintln!(
            "  program={program} caller={caller} pc={pc} samples={} distinct_callees={} distinct_functions={} callees={top_callees:?} functions={top_functions:?}",
            profile.samples,
            profile.callees.len(),
            profile.functions.len(),
        );
    }
}

fn top_counts<K: Copy + std::hash::Hash + Eq>(
    counts: &std::collections::HashMap<K, u64>,
    limit: usize,
) -> Vec<(K, u64)> {
    let mut top: Vec<_> = counts.iter().map(|(key, count)| (*key, *count)).collect();
    top.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(*count));
    top.truncate(limit);
    top
}

/// Per-function instruction census (`profile-lane-exits`): lane records dispatched and
/// general-loop instructions stepped, by the function whose view or frame ran them, and the
/// most dispatched records by function and instruction index.
#[derive(Default)]
struct FunctionCensus {
    /// Each lane view's record address range and its function's index, sorted by start.
    views: Vec<(usize, usize, usize)>,
    /// Each lane view's opcode per record, by its start address.
    view_ops: std::collections::HashMap<usize, Vec<Op>>,
    /// Lane records dispatched by opcode, and by the opcode dispatched just before.
    op_counts: Vec<u64>,
    /// General-loop instructions run, by opcode.
    general_op_counts: Vec<u64>,
    pair_counts: std::collections::HashMap<(usize, usize), u64>,
    previous_op: Option<Op>,
    labels: Vec<String>,
    label_index: std::collections::HashMap<String, usize>,
    lane: Vec<u64>,
    general: Vec<u64>,
    general_keys: std::collections::HashMap<(u32, u32), usize>,
    last: (usize, usize, usize),
    /// Dispatches by record address.
    records: std::collections::HashMap<usize, u64>,
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
    REPORT_FUNCTIONS.call_once(|| {
        // SAFETY: `report_functions` is a plain function that outlives the process.
        unsafe { libc::atexit(report_functions) };
    });
    with_functions(|census| {
        *census.records.entry(address).or_default() += 1;
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
pub(super) fn count_general(label: impl FnOnce() -> String, key: (u32, u32), op: Op) {
    with_functions(|census| {
        if census.general_op_counts.is_empty() {
            census.general_op_counts = vec![0; Op::COUNT];
        }
        census.general_op_counts[op as usize] += 1;
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
/// Records the record census lists, most dispatched first.
const REPORTED_RECORDS: usize = 600;

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
        let mut general_ops: Vec<_> =
            census.general_op_counts.iter().enumerate().filter(|(_, count)| **count > 0).collect();
        general_ops.sort_unstable_by(|a, b| b.1.cmp(a.1));
        for (op, count) in general_ops.into_iter().take(REPORTED_DISPATCHES) {
            eprintln!("  general op {count:>12} {}", Op::NAMES[op]);
        }
        let mut pairs: Vec<_> = census.pair_counts.iter().collect();
        pairs.sort_unstable_by(|a, b| b.1.cmp(a.1));
        for ((first, second), count) in pairs.into_iter().take(REPORTED_DISPATCHES) {
            eprintln!("  lane pair {count:>12} {} -> {}", Op::NAMES[*first], Op::NAMES[*second]);
        }
        for (sum, lane, general, label) in rows.into_iter().take(REPORTED_FUNCTIONS) {
            eprintln!("  {sum:>12} lane={lane:>11} general={general:>10} {label}");
        }
        let record_size = std::mem::size_of::<super::dispatch_fast::LaneInstruction>();
        let mut records: Vec<_> = census.records.iter().map(|(address, count)| (*count, *address)).collect();
        records.sort_unstable_by(|a, b| b.cmp(a));
        eprintln!("record census:");
        for (count, address) in records.into_iter().take(REPORTED_RECORDS) {
            let at = census.views.partition_point(|view| view.0 <= address);
            if let Some(&(start, _, index)) = at.checked_sub(1).and_then(|at| census.views.get(at)) {
                eprintln!("  {count:>12} {} @{}", census.labels[index], (address - start) / record_size);
            }
        }
    });
}

impl<H: crate::Host> super::Vm<H> {
    /// Records a guarded native call's kind (a host function by its name), argument count and
    /// the string lengths of its receiver and first argument.
    pub(super) fn record_native_call(&self, native: crate::heap::Native, callee: crate::Value, this: crate::Value, args: &[crate::Value]) {
        let name = if native == crate::heap::Native::HostFunction {
            format!("host {}", self.host_function_name(callee).unwrap_or_default())
        } else {
            format!("{native:?}")
        };
        let units = |value: crate::Value| self.heap.string(value).map(|text| text.len());
        record_detail(format!("native-call {name} argc={} receiver_units={:?} arg0_units={:?}",
            args.len(), units(this), args.first().and_then(|value| units(*value))));
    }
}
