//! The concurrent-cycle gate: which full collections may run as a cycle (`CycleRefusal`)
//! and whether cycles pay for this heap (`CycleGate`, judged at each full collection).

use super::*;

/// How full collections that pass the static gates choose between a concurrent cycle and
/// stopping the world: `QUENCH_DEV=concurrent-gc` (or `=on`) always runs cycles,
/// `concurrent-gc=adaptive` runs them where they pay (`CycleGate`), `concurrent-gc=off` never.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::heap) enum CycleMode {
    Off,
    Adaptive,
    Always,
}

/// The mode without `QUENCH_DEV=concurrent-gc`.
const DEFAULT_CYCLE_MODE: CycleMode = CycleMode::Off;

impl CycleMode {
    /// The process's mode, from `QUENCH_DEV`.
    pub(in crate::heap) fn current() -> Self {
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
pub(in crate::heap) struct CycleGate {
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
    pub(super) fn last_mark_and_interval(&self) -> (u64, u64) {
        (self.mark_micros, self.interval_micros)
    }
}

/// `average`, taking in `sample`.
fn blend(average: Option<u64>, sample: u64) -> Option<u64> {
    Some(average.map_or(sample, |average| (average + sample) / AVERAGE_OF_TWO))
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

/// The units in a thousand: the gate measures per thousand weight and per thousand time.
const PER_THOUSAND: u64 = 1000;

/// The divisor that averages two samples, an old average and a new one, equally.
const AVERAGE_OF_TWO: u64 = 2;

/// Live legacy cells past which no cycle starts: the initial mark pushes every live cell's
/// children in its pause, as cells pass no snapshot barrier.
pub(in crate::heap) const CONCURRENT_CYCLE_MAX_LEGACY_CELLS: usize = 64 * 1024;

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
    /// Old records keep reference counts (`super::rc`), whose barrier the snapshot barrier
    /// would replace.
    ReferenceCounting,
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
    pub(in crate::heap) fn concurrent_cycle_refusal_with(&self, mode: CycleMode, native_code: bool) -> Option<CycleRefusal> {
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
        } else if self.rc_active {
            Some(CycleRefusal::ReferenceCounting)
        } else if mode == CycleMode::Adaptive && !self.cycle_gate.cycles {
            Some(CycleRefusal::NotPaying)
        } else {
            None
        }
    }

    /// Weighs a full collection that just ended, which paused the mutator for `pause_micros`
    /// and marked for `mark_micros` of it, for the gate (`CycleGate`).
    pub(in crate::heap) fn judge_cycles(&mut self, premarked: bool, mark_micros: u64, pause_micros: u64) {
        let now = std::time::Instant::now();
        let interval = self.cycle_gate.full_ended.map(|ended| (now - ended).as_micros() as u64);
        self.cycle_gate.full_ended = Some(now);
        let live_before = std::mem::replace(&mut self.cycle_gate.live_weight, self.live_weight_after_full);
        let measured = live_before != 0 && self.weight_between_fulls != 0;
        self.record_interval_measures(premarked, live_before, mark_micros, pause_micros, interval);
        if premarked {
            self.judge_premarked_interval(measured);
        } else {
            self.judge_stw_interval(measured, mark_micros, interval);
        }
    }

    /// Records a full collection's peak and cost per thousand weight, and its mark time and
    /// interval. A remark's peak and pause include the cycle's own (`CycleExtent`).
    fn record_interval_measures(
        &mut self,
        premarked: bool,
        live_before: usize,
        mark_micros: u64,
        pause_micros: u64,
        interval: Option<u64>,
    ) {
        let (paused, peak) = if premarked {
            let extent = &self.last_cycle_extent;
            (pause_micros + extent.mutator_micros, self.resident_at_full_start + extent.overhead_bytes())
        } else {
            (pause_micros, self.resident_at_full_start)
        };
        // The live weight the interval's peak held: what it started with, or what it ended
        // with where the live set grew through it.
        let live = live_before.max(self.live_weight_after_full);
        let weight_between = self.weight_between_fulls.max(1) as u64;
        let gate = &mut self.cycle_gate;
        gate.peak = (peak as u128 * u128::from(PER_THOUSAND) / live.max(1) as u128) as u64;
        gate.cost = paused * PER_THOUSAND / weight_between;
        gate.mark_micros = mark_micros;
        gate.interval_micros = interval.unwrap_or(0);
    }

    /// Judges a remark's interval against the last stop-the-world intervals: a cycle whose
    /// peak or cost exceeds theirs stops the world for a doubling while (`CycleGate::blocked`).
    fn judge_premarked_interval(&mut self, measured: bool) {
        let planned = self.last_cycle_extent.planned;
        let gate = &mut self.cycle_gate;
        let over_peak = gate.stw_peak.is_some_and(|stw| gate.peak > stw + stw / CYCLE_PEAK_MARGIN_SHARE);
        let costly = gate.stw_cost.is_some_and(|stw| gate.cost > stw);
        if measured && planned && (over_peak || costly) {
            gate.cycles = false;
            gate.blocked = 1 << gate.fallbacks.min(CYCLE_MAX_BLOCK_SHIFT);
            gate.fallbacks += 1;
        }
    }

    /// Judges a stop-the-world interval: it refreshes the baseline peak and cost, then takes
    /// cycles next time when its mark was long enough in time and share of the interval.
    fn judge_stw_interval(&mut self, measured: bool, mark_micros: u64, interval: Option<u64>) {
        let gate = &mut self.cycle_gate;
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
                    && mark_micros * PER_THOUSAND >= interval * CYCLE_MIN_MARK_SHARE_PERMILLE
            });
    }
}
