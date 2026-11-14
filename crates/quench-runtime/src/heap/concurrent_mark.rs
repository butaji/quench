//! Mostly-concurrent full marking, snapshot at the beginning (SATB): which heaps may run a
//! cycle instead of a stop-the-world full collection, and the hook every operation that needs
//! a consistent heap calls first (`finish_cycle_now`).
//!
//! A cycle runs only where every gate passes (`concurrent_cycle_refusal`), which includes
//! the mode (`CycleMode`) and, when cycles are adaptive, their measured cost (`CycleGate`);
//! marking runs on a marker thread beside steps on the mutator at allocation polls.
use super::*;

/// How full collections that pass the static gates choose between a concurrent cycle and
/// stopping the world: `QUENCH_DEV=concurrent-gc` (or `=on`) always runs cycles,
/// `concurrent-gc=adaptive` runs them where they pay (`CycleGate`), `concurrent-gc=off` never.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CycleMode {
    Off,
    Adaptive,
    Always,
}

/// The mode without `QUENCH_DEV=concurrent-gc`.
const DEFAULT_CYCLE_MODE: CycleMode = CycleMode::Off;

impl CycleMode {
    /// The process's mode, from `QUENCH_DEV`.
    pub(crate) fn current() -> Self {
        static MODE: std::sync::OnceLock<CycleMode> = std::sync::OnceLock::new();
        *MODE.get_or_init(|| {
            let dev = crate::trace::Dev::ConcurrentGc;
            if !dev.on() {
                return DEFAULT_CYCLE_MODE;
            }
            match dev.value() {
                None | Some("on") => CycleMode::Always,
                Some("adaptive") => CycleMode::Adaptive,
                Some("off") => CycleMode::Off,
                Some(other) => {
                    eprintln!("Quench: unknown concurrent-gc mode `{other}`; known: on,adaptive,off");
                    DEFAULT_CYCLE_MODE
                }
            }
        })
    }
}

/// A stop-the-world full mark shorter than this pauses too little for a cycle to pay for its
/// marker thread, barrier and floating garbage (V8's minimum heap size for concurrent marking,
/// in time).
const CYCLE_MIN_MARK_MICROS: u64 = 3_000;
/// The share of the time between full collections, in thousandths, that their stop-the-world
/// mark takes, below which cycles do not pay.
const CYCLE_MIN_MARK_SHARE_PERMILLE: u64 = 50;
/// A cycle interval whose peak occupancy (with what the cycle held beside the heap,
/// `CycleExtent::overhead_bytes`) passes, by more than this share (one in N), the peak either
/// of the last two stop-the-world intervals reached for the same live weight shows the floating
/// garbage and the cycle's own memory costing more than stopping the world.
const CYCLE_PEAK_MARGIN_SHARE: u64 = 16;
/// Full collections a heap stops the world for after its cycles fell back, doubling with each
/// fallback up to this shift.
const CYCLE_MAX_BLOCK_SHIFT: u32 = 6;

/// Whether cycles pay for this heap (`CycleMode::Adaptive`): a heap starts stopping the world,
/// takes cycles once a stop-the-world full mark is long both in time and as a share of the
/// time between full collections, and stops the world again (for a while that doubles each
/// time) when a cycle shows it does not pay. A cycle is judged by what its interval produced
/// against what stop-the-world intervals measured: the peak occupancy (records and payload,
/// the resident measure, with the cycle's mark stacks and quarantined backings) per live
/// weight (the larger of the interval's start's and end's), where its floating garbage shows,
/// and the mutator's pause time (the cycle's start, polls and assists, and its remark)
/// per allocated weight.
#[derive(Debug, Default)]
pub(super) struct CycleGate {
    /// When the last full collection ended.
    full_ended: Option<std::time::Instant>,
    /// Whether the next full collection may run as a cycle.
    cycles: bool,
    /// The live weight the last full collection kept (its snapshot's, for a remark).
    live_weight: usize,
    /// The last two stop-the-world intervals' peak bytes per thousand weight of live (the
    /// larger of their start's and end's), and the larger of them; their pause nanoseconds
    /// per allocated weight, averaged.
    stw_peaks: [u64; 2],
    stw_peak: Option<u64>,
    stw_cost: Option<u64>,
    /// Stop-the-world full collections left before cycles may be taken again, and fallbacks.
    blocked: u32,
    fallbacks: u32,
    /// The last full collection's measures, for the trace.
    mark_micros: u64,
    interval_micros: u64,
    peak: u64,
    cost: u64,
}

impl CycleGate {
    /// The last full collection's mark time and the time since the one before, in
    /// microseconds; an interval of zero when unknown.
    fn last_mark_and_interval(&self) -> (u64, u64) {
        (self.mark_micros, self.interval_micros)
    }
}

/// `average`, taking in `sample`.
fn blend(average: Option<u64>, sample: u64) -> Option<u64> {
    Some(average.map_or(sample, |average| (average + sample) / 2))
}

impl std::fmt::Display for CycleGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "gate_cycles={} full_mark_us={} full_interval_us={} peak={} stw_peak={:?} cost={} stw_cost={:?} blocked={} fallbacks={}",
            self.cycles, self.mark_micros, self.interval_micros, self.peak, self.stw_peak, self.cost, self.stw_cost, self.blocked, self.fallbacks
        )
    }
}

/// Live legacy cells past which no cycle starts: the initial mark pushes every live cell's
/// children in its pause, as cells pass no snapshot barrier.
pub(super) const CONCURRENT_CYCLE_MAX_LEGACY_CELLS: usize = 64 * 1024;

/// The first gate a heap fails, which makes its next full collection stop the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CycleRefusal {
    /// Concurrent cycles are an experiment, on only under `QUENCH_DEV=concurrent-gc`.
    Disabled,
    /// A JIT tier may run: native stores inline the generational guard
    /// (`NATIVE_RECORD_MARK_OFFSET`) and would bypass the snapshot barrier. `--no-jit` only.
    NativeCode,
    /// Pacing favors minor collections; a cycle replaces full collections only where they
    /// alone reclaim memory (`Regime::FullOnly`).
    Generational,
    /// The property arena lives in a moving allocation, not its fixed reservation, so a
    /// marker could read a buffer the mutator freed.
    MovingProperties,
    /// More live legacy cells than the initial mark's pause should visit
    /// (`CONCURRENT_CYCLE_MAX_LEGACY_CELLS`).
    LegacyCells,
    /// The memory profile attributes each record as the stop-the-world sweep frees it.
    ProfilingMemory,
    /// Cycles are adaptive and this heap's measures say they do not pay (`CycleGate`).
    NotPaying,
}

impl Heap {
    /// Why this heap's next full collection cannot run as a concurrent cycle, or `None` when
    /// every gate passes.
    pub(crate) fn concurrent_cycle_refusal(&self) -> Option<CycleRefusal> {
        self.concurrent_cycle_refusal_with(
            CycleMode::current(),
            crate::jit_mode::tiers_allowed(crate::jit_mode::Tier::ANY),
        )
    }

    /// `concurrent_cycle_refusal` with the process-wide inputs given.
    pub(super) fn concurrent_cycle_refusal_with(&self, mode: CycleMode, native_code: bool) -> Option<CycleRefusal> {
        if mode == CycleMode::Off {
            Some(CycleRefusal::Disabled)
        } else if native_code {
            Some(CycleRefusal::NativeCode)
        } else if self.pacing_state.regime(&self.pacing()) != pacing::Regime::FullOnly {
            Some(CycleRefusal::Generational)
        } else if !self.properties.values_stay_in_place() {
            Some(CycleRefusal::MovingProperties)
        } else if self.slots.len() - self.free_slots > CONCURRENT_CYCLE_MAX_LEGACY_CELLS {
            Some(CycleRefusal::LegacyCells)
        } else if cfg!(feature = "profile-memory") {
            Some(CycleRefusal::ProfilingMemory)
        } else if mode == CycleMode::Adaptive && !self.cycle_gate.cycles {
            Some(CycleRefusal::NotPaying)
        } else {
            None
        }
    }

    /// Weighs a full collection that just ended, which paused the mutator for `pause_micros`
    /// and marked for `mark_micros` of it, for the gate (`CycleGate`).
    pub(super) fn judge_cycles(&mut self, premarked: bool, mark_micros: u64, pause_micros: u64) {
        let now = std::time::Instant::now();
        let gate = &mut self.cycle_gate;
        let interval = gate.full_ended.map(|ended| (now - ended).as_micros() as u64);
        gate.full_ended = Some(now);
        let live_before = std::mem::replace(&mut gate.live_weight, self.live_weight_after_full);
        let (paused, peak) = if premarked {
            let extent = &self.last_cycle_extent;
            (pause_micros + extent.mutator_micros, self.resident_at_full_start + extent.overhead_bytes())
        } else {
            (pause_micros, self.resident_at_full_start)
        };
        // The live weight the interval's peak held: what it started with, or what it ended
        // with where the live set grew through it.
        let live = live_before.max(self.live_weight_after_full);
        gate.peak = (peak as u128 * 1000 / live.max(1) as u128) as u64;
        gate.cost = paused * 1000 / self.weight_between_fulls.max(1) as u64;
        gate.mark_micros = mark_micros;
        gate.interval_micros = interval.unwrap_or(0);
        let measured = live_before != 0 && self.weight_between_fulls != 0;
        if premarked {
            let over_peak = gate.stw_peak.is_some_and(|stw| gate.peak > stw + stw / CYCLE_PEAK_MARGIN_SHARE);
            let costly = gate.stw_cost.is_some_and(|stw| gate.cost > stw);
            if measured && self.last_cycle_extent.planned && (over_peak || costly) {
                gate.cycles = false;
                gate.blocked = 1 << gate.fallbacks.min(CYCLE_MAX_BLOCK_SHIFT);
                gate.fallbacks += 1;
            }
            return;
        }
        if measured {
            gate.stw_peaks = [gate.stw_peaks[1], gate.peak];
            gate.stw_peak = Some(gate.stw_peaks[0].max(gate.stw_peaks[1]));
            gate.stw_cost = blend(gate.stw_cost, gate.cost);
        }
        if gate.blocked > 0 {
            gate.blocked -= 1;
            gate.cycles = false;
            return;
        }
        gate.cycles = gate.stw_peak.is_some()
            && interval.is_some_and(|interval| {
                mark_micros >= CYCLE_MIN_MARK_MICROS
                    && mark_micros * 1000 >= interval * CYCLE_MIN_MARK_SHARE_PERMILLE
            });
    }

    /// Whether a concurrent cycle is marking: the snapshot barrier is on and allocation is
    /// black.
    #[inline(always)]
    pub(crate) fn satb_active(&self) -> bool {
        self.satb_active
    }

    /// Brings a running cycle's marking to its end on this thread, before an operation that
    /// needs it: an explicit collection, a snapshot. The barrier stays on until the collection
    /// that follows (any collection while a cycle runs is full and finishes it,
    /// `collect_with_object_roots`), so writes meanwhile are still snapshotted.
    pub(crate) fn finish_cycle_now(&mut self) {
        if self.cycle.is_some() {
            while !self.mark_step(usize::MAX) {}
        }
    }
}

/// The snapshot barrier's copy of a record's children (`RecordSpace::satb_note`): what the
/// mark would push for it, with no shape roots (pushed per shape when a cycle starts) and
/// every string left to the cycle's mark.
fn snapshot_object(object: &Object, properties: &ValueArena, out: &mut Vec<Value>) {
    Heap::object_children(
        Value::UNDEFINED,
        object,
        properties,
        out,
        &mut |_, _, _, _, _| {},
        GcTraceKind::Collection,
        &mut PushedProtos::default(),
    );
}

impl records::SatbSnapshot for Object {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(self, properties, out);
    }
}

impl records::SatbSnapshot for ArrayObject {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        Heap::array_children(
            Value::UNDEFINED,
            self,
            properties,
            |_| false,
            out,
            &mut |_, _, _, _, _| {},
            GcTraceKind::Collection,
            &mut PushedProtos::default(),
        );
    }
}

impl records::SatbSnapshot for FunctionRecord {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        out.extend([self.realm, self.env]);
    }
}

impl records::SatbSnapshot for PromiseCell {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        self.record.append_roots(out);
    }
}

impl records::SatbSnapshot for IteratorRecord {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        Heap::iterator_edges(self, out);
    }
}

impl records::SatbSnapshot for AwaitState {
    fn snapshot(&self, _properties: &ValueArena, out: &mut Vec<Value>) {
        self.extend_roots(out);
    }
}

impl Heap {
    /// Turns the snapshot barrier on or off in every record family, as a concurrent cycle's
    /// start and remark will; the children it copies are taken with `take_satb_values`.
    pub(super) fn set_satb(&mut self, active: bool) {
        self.satb_active = active;
        for (family, _) in self.record_families_mut() {
            if active { family.begin_satb() } else { family.end_satb() }
        }
    }

    /// Allocation is black while a cycle marks: a cell made now is marked at once, newer than
    /// the snapshot, so the cycle neither traces nor frees it.
    #[cold]
    #[inline(never)]
    pub(super) fn blacken_slot(&mut self, index: usize) {
        let (word, bit) = slot_bit(index);
        *self.marks[word].get_mut() |= bit;
    }

    /// The children the snapshot barrier copied in every family since the last call.
    pub(super) fn take_satb_values(&mut self) -> Vec<Value> {
        let mut values = Vec::new();
        for (family, _) in self.record_families_mut() {
            values.append(&mut family.take_satb_values());
        }
        values
    }
}

/// An incremental cycle's mark state between steps: the values left to mark and the state the
/// serial mark keeps across them. Steps run on the mutator thread (`Heap::mark_step`), between
/// which the mutator writes through the snapshot barrier and allocates black.
pub(super) struct ConcurrentCycle {
    /// The marker thread, when the cycle marks on one (`begin_cycle_with`); `work` is then
    /// the mutator's share left over. Before the quarantine, so it is joined first.
    marker: Option<concurrent_view::MarkerThread>,
    work: Vec<Value>,
    ephemerons: weak::EphemeronWork,
    last_proto: PushedProtos,
    /// Values a poll marks (`Heap::cycle_poll`), and the allocation weight between polls.
    step_values: usize,
    poll_weight: usize,
    /// Polls taken, and the allocation count at the start (`last_cycle_extent`).
    polls: u32,
    assists: u32,
    planned: bool,
    /// The mutator's time in the cycle's start and polls.
    mutator_nanos: u64,
    started_at: usize,
    started: std::time::Instant,
    /// Holds the array backings freed during the cycle until it is dropped.
    _quarantine: BackingQuarantine,
}

/// Array backings freed while it lives wait for its drop (`array_elements::begin_quarantine`),
/// as a concurrent marker may read them until then.
struct BackingQuarantine;

impl BackingQuarantine {
    fn begin() -> Self {
        array_elements::begin_quarantine();
        Self
    }
}

impl Drop for BackingQuarantine {
    fn drop(&mut self) {
        array_elements::end_quarantine();
    }
}

/// What the last cycle took, for pacing and the trace (`QUENCH_TRACE=gc`).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct CycleExtent {
    pub(super) polls: u32,
    /// Allocation weight charged during the cycle: what its remark keeps black.
    pub(super) weight: usize,
    pub(super) wall_micros: u128,
    /// Values left for the remark to mark.
    pub(super) remark_values: usize,
    /// Marks the marker thread and assists set, and the marker's busy time.
    pub(super) marked: u64,
    pub(super) marker_busy_micros: u64,
    /// Polls that assisted the marker (`MarkerThread::assist`).
    pub(super) assists: u32,
    /// Allocation weight charged past the cycle's due point, where every poll assists.
    pub(super) late_weight: usize,
    /// The mutator's time in the cycle's start and polls (`Heap::cycle_poll`).
    pub(super) mutator_micros: u64,
    /// The marker stack's largest capacity, in values, and the array backing bytes the
    /// cycle's quarantine held at its end.
    pub(super) marker_peak_work: u64,
    pub(super) quarantined: usize,
    /// The initial mark's stack, in values.
    pub(super) initial_work: usize,
    /// Whether the cycle ran to a planned due point (`Heap::plan_cycle`): one the last full
    /// collection's gates refused but a later one's passed runs with none.
    pub(super) planned: bool,
}

impl CycleExtent {
    /// What the cycle held beside the heap at its height: its mark stacks (the initial mark's
    /// and the marker's) and the array backings it quarantined.
    fn overhead_bytes(&self) -> usize {
        (self.initial_work + self.marker_peak_work as usize) * std::mem::size_of::<Value>() + self.quarantined
    }
}

impl std::fmt::Display for CycleExtent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "remark_after_polls={} cycle_weight={} late_weight={} cycle_mutator_us={} initial_work={} marker_peak_work={} quarantined={} cycle_us={} remark_values={} marker_marked={} marker_busy_us={} assists={}",
            self.polls, self.weight, self.late_weight, self.mutator_micros, self.initial_work, self.marker_peak_work, self.quarantined, self.wall_micros, self.remark_values, self.marked, self.marker_busy_micros, self.assists
        )
    }
}

/// Polls a cycle takes to mark the live set, at most: the mutator marks a share of it at each,
/// so the cycle ends within about `1 / CYCLE_POLLS_PER_LIVE_SET`'s worth of the interval since
/// the last full collection, past whatever the barrier adds.
const CYCLE_POLLS_PER_LIVE_SET: usize = 8;
/// An assist marks this share of a poll's step (`ConcurrentCycle::step_values`): past its
/// budget the mutator polls four times as often, so it marks a quarter of the live set over
/// what would have been sixteen ordinary polls, the marker doing the rest beside it.
const CYCLE_ASSIST_SHARE: usize = 16;
/// Polls a cycle spreads over the allocation one full collection interval charges.
const CYCLE_POLLS_PER_INTERVAL: usize = 128;

impl Heap {
    /// Starts an incremental cycle, as a full collection starts: the marks are cleared, and
    /// the snapshot is pushed: `roots`, the root table, and the children of every live cell
    /// (cells pass no snapshot barrier, so what one holds now is all the cycle needs of it),
    /// with sparse elements. Weak entries are held strongly for the cycle (a weak map's keys
    /// and values, a weak reference's target), so no ephemeron waits on a key and nothing a
    /// weak read hands out goes unmarked; the remark's own trace prunes them as any full
    /// collection does. The snapshot barrier and black allocation then run until the next
    /// collection finishes the cycle (`collect_with_object_roots`).
    ///
    /// The caller has passed the gates (`concurrent_cycle_refusal`); shape roots and every
    /// weakly held VM value a weak read cannot shade are among `roots`.
    pub(crate) fn begin_cycle(&mut self, roots: impl IntoIterator<Item = Value>) {
        self.begin_cycle_with(roots, true);
    }

    /// `begin_cycle`, marking on a marker thread (`concurrent_view`) or, without `threaded`,
    /// in the mutator's steps alone.
    pub(super) fn begin_cycle_with(&mut self, roots: impl IntoIterator<Item = Value>, threaded: bool) {
        assert!(self.cycle.is_none() && !self.satb_active, "one cycle at a time");
        let begun = std::time::Instant::now();
        // The shared unit strings are handed out with no barrier: a cycle starts them anew,
        // as a collection does.
        if let Some(strings) = &mut self.ascii_unit_strings {
            strings.fill(Value::UNDEFINED);
        }
        Self::clear_marks(&self.marks);
        self.remembered.clear();
        self.remembered_marks.fill(0);
        self.carried_owners.clear();
        self.aged.clear();
        for (family, _) in self.record_families_mut() {
            family.clear_marks();
            family.forget_ages();
        }
        let mut work: Vec<Value> = roots.into_iter().collect();
        work.extend(self.roots.values());
        let mut last_proto = PushedProtos::default();
        for (index, slot) in self.slots.iter().enumerate() {
            let Some(cell) = &slot.cell else { continue };
            Self::children(
                Value::heap(index as u32),
                cell,
                &self.properties,
                &mut work,
                &mut |_, _, _, _, _| {},
                GcTraceKind::Collection,
                &mut last_proto,
            );
            match cell {
                Cell::WeakMap { entries, .. } => work.extend(entries.iter().flat_map(|(key, value)| [key, value])),
                Cell::WeakSet { entries, .. } => work.extend(entries.iter()),
                Cell::WeakRef { target: Some(target), .. } => work.push(target.value),
                _ => {}
            }
        }
        if let Some(arrays) = &self.sparse_arrays {
            for elements in arrays.values() {
                work.extend(elements.values.values().copied());
            }
        }
        let live = self.live_after_full.max(work.len());
        self.last_cycle_extent.initial_work = work.len();
        let interval = self.weight_between_fulls.max(self.pacing().minimum_young);
        self.set_satb(true);
        let quarantine = BackingQuarantine::begin();
        let (marker, work) = if threaded {
            let view = concurrent_view::ConcurrentMarkView::new(self);
            (Some(concurrent_view::MarkerThread::start(view, work)), Vec::new())
        } else {
            (None, work)
        };
        self.cycle = Some(Box::new(ConcurrentCycle {
            marker,
            work,
            ephemerons: weak::EphemeronWork::default(),
            last_proto: PushedProtos::default(),
            step_values: (live / CYCLE_POLLS_PER_LIVE_SET).max(1024),
            poll_weight: (interval / CYCLE_POLLS_PER_INTERVAL).max(1),
            polls: 0,
            assists: 0,
            planned: self.cycle_due_at != usize::MAX,
            mutator_nanos: begun.elapsed().as_nanos() as u64,
            started_at: self.allocated,
            started: std::time::Instant::now(),
            _quarantine: quarantine,
        }));
        self.arm_cycle_poll();
    }

    /// Makes `should_collect` answer at the cycle's next poll.
    fn arm_cycle_poll(&mut self) {
        let weight = self.cycle.as_ref().map_or(0, |cycle| cycle.poll_weight);
        // Past the budget, polls (each an assist) come four times as often.
        let weight = if self.allocated >= self.cycle_due_at { weight / 4 } else { weight };
        self.collect_at.set(self.allocated.saturating_add(weight.max(1)));
    }

    /// Plans the cycle a full collection's `threshold` (its interval's garbage budget, from
    /// now) makes due: its remark is due once the budget is spent, counting what the last
    /// remark kept black (`premarked`), and it starts early by what cycles allocate, so the
    /// peak stays within the budget; the start is the next collection's trigger.
    pub(super) fn plan_cycle(&mut self, premarked: bool, threshold: usize) {
        if self.concurrent_cycle_refusal().is_some() {
            return;
        }
        let floating = if premarked { self.last_cycle_extent.weight } else { 0 };
        let budget = threshold.saturating_sub(floating);
        // Before any cycle's measure, a cycle is taken to allocate twice what the interval
        // allocates over the last stop-the-world mark's time (the marker marks at about the
        // stop-the-world mark's wall rate), at most half the budget.
        let weight = if self.cycle_weight_average != 0 {
            self.cycle_weight_average
        } else {
            let (mark, interval) = self.cycle_gate.last_mark_and_interval();
            (2 * threshold as u128 * mark as u128 / interval.max(1) as u128).min(budget as u128 / 2) as usize
        };
        self.cycle_due_at = self.allocated.saturating_add(budget);
        let start = budget.saturating_sub(weight).max(1);
        self.collect_at.set(self.allocated.saturating_add(start));
    }

    /// Whether an incremental cycle is running (`begin_cycle`).
    pub(crate) fn cycle_running(&self) -> bool {
        self.cycle.is_some()
    }

    /// One poll of the running cycle, at an allocation safepoint once `should_collect` says
    /// so: marks a step's share, then arms the next poll. Whether its marking is done, so the
    /// caller's full collection, the remark, is due.
    pub(crate) fn cycle_poll(&mut self) -> bool {
        let Some(cycle) = self.cycle.as_mut() else { return true };
        let polled = std::time::Instant::now();
        cycle.polls += 1;
        let budget = cycle.step_values;
        let done = self.mark_step(budget);
        self.arm_cycle_poll();
        if let Some(cycle) = self.cycle.as_mut() {
            cycle.mutator_nanos += polled.elapsed().as_nanos() as u64;
        }
        done
    }

    /// Marks up to `budget` values of the running cycle on this thread, after taking what the
    /// snapshot barrier copied and weak reads shaded since the last step; whether its marking
    /// is done (nothing left to mark, nothing copied).
    pub(super) fn mark_step(&mut self, budget: usize) -> bool {
        let mut cycle = self.cycle.take().expect("a running cycle");
        if let Some(marker) = cycle.marker.take() {
            let done = self.share_marking(&marker, &mut cycle);
            cycle.marker = Some(marker);
            self.cycle = Some(cycle);
            return done;
        }
        cycle.work.append(&mut self.take_satb_values());
        cycle.work.append(self.satb_shaded.get_mut());
        // No closure-read source: closure environment edges are ordinary edges, and every
        // environment is traced whole.
        let mut environments = EnvironmentTrace::new(None, EnvironmentReads::Projections);
        let ConcurrentCycle { work, ephemerons, last_proto, .. } = &mut *cycle;
        for _ in 0..budget {
            let Some(value) = work.pop() else { break };
            self.mark_value(value, work, &mut |_, _, _, _, _| {}, ephemerons, GcTraceKind::Collection, &mut environments, last_proto);
        }
        debug_assert!(!environments.has_pending());
        let done = cycle.work.is_empty();
        self.cycle = Some(cycle);
        done && self.satb_values_empty() && self.satb_shaded.get_mut().is_empty()
    }

    /// The mutator's step while the marker thread marks: what the marker deferred and the
    /// mutator's own leftovers are traced here, and the barrier's copies, weak reads and the
    /// records reached go to the marker. Done once the marker idles with nothing sent or
    /// deferred and this step sent it nothing.
    fn share_marking(&mut self, marker: &concurrent_view::MarkerThread, cycle: &mut ConcurrentCycle) -> bool {
        let mut for_marker = self.take_satb_values();
        for_marker.append(self.satb_shaded.get_mut());
        if self.allocated >= self.cycle_due_at {
            // The interval's budget is spent: the mutator marks a step's share itself rather
            // than let the heap grow past it.
            cycle.assists += 1;
            marker.assist((cycle.step_values / CYCLE_ASSIST_SHARE).max(1), &mut cycle.work);
        }
        let quiet = marker.take_deferred(&mut cycle.work);
        if quiet && cycle.work.is_empty() {
            // The marker drained what it was given: the remark marks what the mutator wrote
            // since, which the barrier still holds, as the mutator keeps writing.
            self.satb_shaded.get_mut().append(&mut for_marker);
            return true;
        }
        let ConcurrentCycle { work, ephemerons, last_proto, .. } = cycle;
        self.trace_deferred(marker.view(), work, &mut for_marker, ephemerons, last_proto);
        let done = quiet && for_marker.is_empty();
        marker.send(&mut for_marker);
        done
    }

    fn satb_values_empty(&mut self) -> bool {
        self.record_families_mut().iter_mut().all(|(family, _)| family.satb_values_empty())
    }

    /// A weak read handed out `value` while a cycle marks: the cycle marks it, as its snapshot
    /// may not reach it and the mutator may now store it anywhere.
    #[inline(always)]
    pub(super) fn shade(&self, value: Value) {
        if self.satb_active {
            self.shade_slow(value);
        }
    }

    #[cold]
    #[inline(never)]
    fn shade_slow(&self, value: Value) {
        self.satb_shaded.borrow_mut().push(value);
    }

    /// Marks what is left of the running cycle and ends its snapshot barrier (which marks
    /// what it allocated): the start of its remark, the collection that keeps these marks.
    /// `false` when no cycle runs.
    pub(super) fn complete_cycle_marking(&mut self) -> bool {
        let Some(cycle) = self.cycle.as_mut() else {
            return false;
        };
        // The marker stops first: the rest is marked here, serially.
        if let Some(marker) = cycle.marker.take() {
            let (marked, busy) = marker.stats();
            self.last_cycle_extent.marker_peak_work = marker.peak_work();
            self.last_cycle_extent.quarantined = array_elements::quarantined_bytes();
            self.last_cycle_extent.marked = marked;
            self.last_cycle_extent.marker_busy_micros = busy / 1000;
            let mut left = marker.stop();
            cycle.work.append(&mut left);
        }
        let remark_values = cycle.work.len() + self.satb_shaded.get_mut().len()
            + self.record_families_mut().iter_mut().map(|(family, _)| family.satb_values_len()).sum::<usize>();
        let cycle = self.cycle.as_ref().expect("a running cycle");
        self.last_cycle_extent = CycleExtent {
            polls: cycle.polls,
            weight: self.allocated - cycle.started_at,
            wall_micros: cycle.started.elapsed().as_micros(),
            remark_values,
            marked: self.last_cycle_extent.marked,
            marker_busy_micros: self.last_cycle_extent.marker_busy_micros,
            assists: cycle.assists,
            late_weight: self.allocated.saturating_sub(self.cycle_due_at.max(cycle.started_at)),
            mutator_micros: cycle.mutator_nanos / 1000,
            marker_peak_work: self.last_cycle_extent.marker_peak_work,
            quarantined: self.last_cycle_extent.quarantined,
            initial_work: self.last_cycle_extent.initial_work,
            planned: cycle.planned,
        };
        self.cycle_weight_average = if self.cycle_weight_average == 0 {
            self.last_cycle_extent.weight
        } else {
            (self.cycle_weight_average + self.last_cycle_extent.weight) / 2
        };
        while !self.mark_step(usize::MAX) {}
        self.cycle = None;
        self.set_satb(false);
        true
    }

    /// Drops a running cycle without finishing it, for a heap discarding everything
    /// (`Heap::reset`).
    pub(super) fn abandon_cycle(&mut self) {
        if self.cycle.take().is_some() {
            self.set_satb(false);
        }
        self.satb_shaded.get_mut().clear();
    }

    /// Whether the last collection kept the marks a finished cycle made: its own trace reached
    /// only what the cycle had not, so a by-product of that trace (the VM's reached shapes) is
    /// incomplete.
    pub(crate) fn last_collection_kept_cycle_marks(&self) -> bool {
        self.last_collection_premarked
    }

    /// Finishes the running cycle with a full collection from `roots` (the remark).
    #[cfg(test)]
    pub(super) fn finish_cycle(&mut self, roots: impl IntoIterator<Item = Value>) -> Vec<(Value, Value)> {
        self.collect_with_object_roots(roots, |_, _, _, _, _| {}, None, None, true)
    }
}

