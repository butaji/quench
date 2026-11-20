//! The exit protocol: how native code says where and why it left, written once.
//!
//! A run ends with a status and a result word. `LaneExit::read` is the only decoder of that
//! pair, for lane bodies, loop regions and bare stencil regions alike (a leave's status is
//! `NATIVE_JS_STOPPED`, entry thunks of stencil regions included); `LaneExit::word` is
//! the only encoder of a leave word, for every tier that emits one. A leave word holds its
//! `ExitKind` in its top bits and the leave's place below: a caller PC, or an inlined frame
//! and its callee PC. `ExitTable` interns, per compiled unit, each leave's `ExitSig` (the
//! kind and the values live across it) and its `ExitSite` (sig, place, dense site id).
//!
//! The b-jit leave stubs (`base_region::link`) emit `LaneExit::Entry.word()` and so share the
//! encoding; their per-stub tables are not built from an `ExitTable` (documented adapter).

use super::{lane_unit, LaneExit, LoopRegion};
use crate::value::Value;
use quench_bjit::call_record::{return_status_pc, LaneStatus};
use std::collections::HashMap;

/// What a leave is, which a leave word holds in its top two bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u64)]
pub(super) enum ExitKind {
    /// At a caller PC.
    At,
    /// At a PC of an inlined frame's callee.
    Inlined,
    /// Into a native call's callee activation.
    Callee,
    /// At the body's entry, whose proof of its inputs failed.
    Entry,
}

impl ExitKind {
    const KINDS: [Self; 4] = [Self::At, Self::Inlined, Self::Callee, Self::Entry];
    const SHIFT: u32 = u64::BITS - Self::KINDS.len().trailing_zeros();
    const PLACE_MASK: u64 = (1 << Self::SHIFT) - 1;
    /// An inlined leave's frame index sits above its callee PC.
    const FRAME_SHIFT: u32 = u32::BITS;
    const PC_MASK: u64 = u32::MAX as u64;

    #[inline]
    fn of(word: u64) -> Self {
        Self::KINDS[(word >> Self::SHIFT) as usize]
    }

    const fn word(self, place: u64) -> u64 {
        (self as u64) << Self::SHIFT | place
    }
}

// The b-jit's callee leave word is this protocol's: one encoding, two crates.
const _: () = assert!(ExitKind::Callee.word(0) == quench_bjit::call_record::CALLEE_EXIT);

impl LaneExit {
    /// Whether the run ended the activation, by its return or by an exception, rather than
    /// leaving it to the lane.
    #[inline]
    pub(in crate::vm) fn completes(self) -> bool {
        matches!(self, Self::Returned { .. } | Self::Threw { .. })
    }
    /// The word a leave is emitted as; a completion is a status, which has none.
    pub(super) fn word(self) -> u64 {
        match self {
            Self::At(pc) => ExitKind::At.word(pc as u64),
            Self::Inlined { frame, pc } => ExitKind::Inlined.word((frame as u64) << ExitKind::FRAME_SHIFT | u64::from(pc)),
            Self::Callee => ExitKind::Callee.word(0),
            Self::Entry => ExitKind::Entry.word(0),
            Self::Returned { .. } | Self::Threw { .. } | Self::Terminated | Self::Stub => unreachable!("a completion, an exception or an unentered call is a status, not a leave word"),
        }
    }

    pub(super) fn decode(word: u64) -> Self {
        let place = word & ExitKind::PLACE_MASK;
        match ExitKind::of(word) {
            ExitKind::At => Self::At((place & ExitKind::PC_MASK) as usize),
            ExitKind::Inlined => Self::Inlined { frame: (place >> ExitKind::FRAME_SHIFT) as usize, pc: (place & ExitKind::PC_MASK) as u32 },
            ExitKind::Callee => Self::Callee,
            ExitKind::Entry => Self::Entry,
        }
    }

    /// How a native run that returned `status` and wrote `word` ended: the one reader of the
    /// status word. `returned_pc` names the record that completes the activation, read only then.
    ///
    /// The AOT completion path: the ahead trampoline (`ahead_entry`) returns a completion as
    /// `return_status(pc)`, which is read here as `At(pc)`, and its value is in the entry's result
    /// slot, the `word` the caller passes. `At` carries no value, so the value is read from that
    /// slot by the caller that holds it: `run_js_native_entry`'s linked step (`compiled.rs`).
    ///
    /// Readers of the status word, the ones that decode it: this function (lane bodies, OSR
    /// regions and bare stencils, through `entry_protocol::exit`); the linked step in `compiled.rs`
    /// (`return_status_pc`, the AOT completion's PC); `lane_body_run.rs` and `lane_region/call_frames.rs`
    /// and `stencils/call_rows.rs`, which test a call's `LaneStatus` without decoding a leave.
    pub(super) fn read(status: u32, word: u64, returned_pc: impl FnOnce() -> usize) -> Self {
        match LaneStatus::of(status) {
            Some(LaneStatus::Terminated) => Self::Terminated,
            Some(LaneStatus::Returned) => Self::Returned { pc: returned_pc(), value: Value::from_bits(word) },
            Some(LaneStatus::Threw) => Self::Threw { pc: returned_pc() },
            Some(LaneStatus::NativeJsStopped) => Self::decode(word),
            Some(LaneStatus::NotEntered) => Self::Stub,
            // Dispatch reaches an invalid PC only before any instruction ran.
            None => Self::At(return_status_pc(status).map_or(0, |pc| pc as usize)),
        }
    }

    /// Whether the run entered nothing: its entry contract failed or the cell held the stub, so
    /// the registers are as the run found them.
    #[inline]
    pub(in crate::vm) fn entered_nothing(self) -> bool {
        matches!(self, Self::Entry | Self::Stub)
    }

    /// Whether a leave at a caller PC is one of `range`'s (a loop's PCs); other leaves are
    /// inside the loop unless they complete or terminate the run.
    pub(in crate::vm) fn inside(self, range: std::ops::RangeInclusive<u32>) -> bool {
        match self {
            Self::At(pc) => range.contains(&(pc as u32)),
            Self::Returned { .. } | Self::Threw { .. } | Self::Terminated => false,
            _ => true,
        }
    }
}

/// How a loop region's run ended: its exit, in the one vocabulary every tier's leave word
/// decodes to, and whether it ended inside the native frames a callee's entry pushed on the lane
/// stack, where the lane reads and continues it (`dispatch_fast::region_left_in_lane`). Whether a
/// leave at a caller PC is a guard inside the loop or the loop's normal continuation is a
/// function of the region (`LoopRegion::holds_exit`), not a second exit vocabulary.
#[derive(Clone, Copy, Debug)]
pub(in crate::vm) struct RegionRun {
    pub(in crate::vm) exit: LaneExit,
    pub(in crate::vm) nested: bool,
}

impl RegionRun {
    /// A run that ended in the region's own activation.
    pub(in crate::vm) const fn own(exit: LaneExit) -> Self {
        Self { exit, nested: false }
    }

    /// Whether the run left the region before its loop was done: a guard inside the loop, a leave
    /// in an inlined or entered callee, or a call that made its callee's activation.
    pub(in crate::vm) fn left(&self, region: &LoopRegion) -> bool {
        self.nested || matches!(self.exit, LaneExit::Inlined { .. } | LaneExit::Callee) || region.holds_exit(self.exit)
    }
}

impl LoopRegion {
    /// Whether `exit` is a leave at one of the loop's own PCs: a guard's exit, which the general
    /// path executes, rather than the jump past the loop the lane resumes at. An inlining
    /// region's loop ends at the caller's latch, not at the region's last instruction.
    pub(in crate::vm) fn holds_exit(&self, exit: LaneExit) -> bool {
        let latch = self.inlining.as_ref().map_or(self.backedge, |inlining| inlining.latch);
        matches!(exit, LaneExit::At(_)) && exit.inside(self.header..=latch)
    }
}

/// Where a live value is when its leave happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Source {
    /// An inlined callee's register, which the leave stores to this slot of the entry's
    /// scratch window (`BodyRunContext::scratch`).
    Scratch(u16),
}

/// A value a leave stores: its unit register and where the leave stores it. Every stored
/// value is stored tagged, so no representation is recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct LiveValue {
    pub(super) slot: u16,
    pub(super) source: Source,
}

/// What a leave is and the values it stores (`Lowering::inlined_stores`, the emitter's own
/// query, which a leave inside an inlined frame stores and no other).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ExitSig {
    pub(super) kind: ExitKind,
    pub(super) live: Box<[LiveValue]>,
}

/// One leave of a unit: its sig, its place, its dense site id, and the first unit PC that
/// leaves there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ExitSite {
    /// The leave's word (`LaneExit::word`), which `LaneExit::decode` reads back; the table's key.
    /// Kept as a word, not a `LaneExit`, which holds a `Value` and is three words wide.
    word: u64,
    pub(super) sig: u32,
    pub(super) id: u32,
    pub(super) unit_pc: u32,
}

/// A unit's leaves, each place once, ordered by leave word, over its interned sigs. Built once per
/// compile and kept by the body it publishes, so its tables hold no capacity past their length.
#[derive(Default)]
pub(in crate::vm) struct ExitTable {
    sigs: Vec<ExitSig>,
    sites: Vec<ExitSite>,
}

impl ExitTable {
    /// The table of the places `unit`'s PCs leave at. `stored` is the emitter's store set of a
    /// leave at an inlined PC (`unit_pc`, its `frame`), so the table records what the leave writes.
    pub(super) fn of_unit(unit: &lane_unit::Unit, stored: impl Fn(usize, usize) -> Vec<LiveValue>) -> Self {
        let mut table = Self::default();
        let mut known: HashMap<ExitSig, usize> = HashMap::new();
        for (unit_pc, origin) in unit.origins.iter().enumerate() {
            let (exit, sig) = match *origin {
                lane_unit::Origin::Caller(pc) => (LaneExit::At(pc as usize), ExitSig { kind: ExitKind::At, live: Box::default() }),
                lane_unit::Origin::Inlined { frame, pc } => {
                    (LaneExit::Inlined { frame, pc }, ExitSig { kind: ExitKind::Inlined, live: stored(unit_pc, frame).into_boxed_slice() })
                }
            };
            let sig = table.intern(&mut known, sig);
            table.add(exit, sig, unit_pc);
        }
        table.sigs.shrink_to_fit();
        table.sites.shrink_to_fit();
        table
    }

    fn intern(&mut self, known: &mut HashMap<ExitSig, usize>, sig: ExitSig) -> usize {
        if let Some(&index) = known.get(&sig) { return index; }
        self.sigs.push(sig.clone());
        known.insert(sig, self.sigs.len() - 1);
        self.sigs.len() - 1
    }

    /// Adds the leave `exit` unless an earlier unit PC already leaves there.
    fn add(&mut self, exit: LaneExit, sig: usize, unit_pc: usize) {
        let word = exit.word();
        let Err(at) = self.sites.binary_search_by_key(&word, |site| site.word) else { return };
        let id = self.sites.len() as u32;
        let sig = u32::try_from(sig).expect("a unit interns fewer sigs than a u32 holds");
        let unit_pc = u32::try_from(unit_pc).expect("a unit has fewer PCs than a u32 holds");
        self.sites.insert(at, ExitSite { word, sig, id, unit_pc });
    }

    /// The site at the place `exit` leaves, which only a leave at a PC has.
    pub(super) fn site(&self, exit: LaneExit) -> Option<&ExitSite> {
        if !matches!(exit, LaneExit::At(_) | LaneExit::Inlined { .. }) { return None; }
        self.sites.binary_search_by_key(&exit.word(), |site| site.word).ok().map(|index| &self.sites[index])
    }

    /// The diagnostic name of the leave `exit` (`QUENCH_LANE_BODY`): its site id and, for an
    /// inlined leave, where its sig stores each live value (`slot>scratch`).
    pub(in crate::vm) fn describe(&self, exit: LaneExit) -> String {
        let Some(site) = self.site(exit) else { return String::new() };
        let sig = &self.sigs[site.sig as usize];
        if sig.kind != ExitKind::Inlined { return format!(" s{}", site.id); }
        let stores = sig.live.iter().map(|value| match value.source {
            Source::Scratch(at) => format!("{}>{at}", value.slot),
        });
        format!(" s{} {}", site.id, stores.collect::<Vec<_>>().join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_bjit::call_record::return_status;

    const EXITS: [LaneExit; 7] = [
        LaneExit::At(0), LaneExit::At(u32::MAX as usize), LaneExit::Inlined { frame: 0, pc: 0 },
        LaneExit::Inlined { frame: 3, pc: 9 }, LaneExit::Inlined { frame: 1 << 20, pc: u32::MAX }, LaneExit::Callee, LaneExit::Entry,
    ];

    #[test]
    fn a_leave_word_decodes_to_its_leave() {
        for exit in EXITS {
            assert_eq!(LaneExit::decode(exit.word()), exit);
        }
        assert_eq!(LaneExit::At(7).word(), 7, "a caller PC is its own word");
    }

    #[test]
    fn each_exit_kind_round_trips_and_names_its_own_word() {
        for kind in ExitKind::KINDS {
            assert_eq!(ExitKind::of(kind.word(ExitKind::PLACE_MASK)), kind);
            assert_eq!(ExitKind::of(kind.word(0)), kind);
        }
        let words: Vec<u64> = EXITS.iter().map(|exit| exit.word()).collect();
        assert!(words.iter().enumerate().all(|(at, word)| !words[..at].contains(word)), "each leave has its own word");
    }

    #[test]
    fn the_reader_separates_completion_leaves_and_termination() {
        let word = LaneExit::Inlined { frame: 1, pc: 2 }.word();
        assert_eq!(LaneExit::read(LaneStatus::NativeJsStopped as u32, word, || unreachable!()), LaneExit::Inlined { frame: 1, pc: 2 });
        assert_eq!(LaneExit::read(LaneStatus::Terminated as u32, 0, || unreachable!()), LaneExit::Terminated);
        let returned = LaneExit::read(LaneStatus::Returned as u32, Value::UNDEFINED.to_bits(), || 5);
        assert!(matches!(returned, LaneExit::Returned { pc: 5, .. }));
    }

    #[test]
    fn each_named_status_round_trips_and_no_return_status_is_named() {
        for status in LaneStatus::ALL {
            assert_eq!(LaneStatus::of(status as u32), Some(status));
            assert!((status as u32) < return_status(0), "a named status is below every return status");
        }
        assert_eq!(LaneStatus::of(return_status(0)), None);
        assert_eq!(LaneStatus::of(0), None);
    }

    /// The AOT completion's status decodes to the record it names, and only return statuses name one.
    #[test]
    fn an_aot_completion_reads_as_the_leave_at_its_record() {
        for pc in [0, 1, 2047] {
            assert_eq!(return_status_pc(return_status(pc)), Some(pc));
            assert_eq!(LaneExit::read(return_status(pc), Value::UNDEFINED.to_bits(), || unreachable!()), LaneExit::At(pc as usize));
        }
        assert_eq!(return_status_pc(LaneStatus::Threw as u32), None);
    }

    /// Every producer of a leave word stops with `NATIVE_JS_STOPPED` (the stencil entry thunks
    /// return it, `stencil_layout_matches`), which the one reader decodes.
    #[test]
    fn a_leave_status_decodes_its_word() {
        for exit in EXITS {
            assert_eq!(LaneExit::read(LaneStatus::NativeJsStopped as u32, exit.word(), || unreachable!()), exit);
        }
        assert_eq!(LaneExit::read(0, LaneExit::Entry.word(), || unreachable!()), LaneExit::At(0), "status 0 is no leave");
    }
}
