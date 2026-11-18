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

use super::{lane_unit, LaneExit, LoopRegion, RETURN_STATUS_BASE, LANE_BODY_RETURNED, LANE_BODY_THREW, NATIVE_JS_STOPPED, TERMINATED_STATUS};
use super::entry_cell::LANE_BODY_NOT_ENTERED;
use crate::value::Value;

/// What a leave is, which a leave word holds in its top two bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    fn word(self, place: u64) -> u64 {
        (self as u64) << Self::SHIFT | place
    }
}

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
    /// result. `returned_pc` names the record that completes the activation, read only then.
    pub(super) fn read(status: u32, word: u64, returned_pc: impl FnOnce() -> usize) -> Self {
        match status {
            _ if i64::from(status) == TERMINATED_STATUS => Self::Terminated,
            LANE_BODY_RETURNED => Self::Returned { pc: returned_pc(), value: Value::from_bits(word) },
            LANE_BODY_THREW => Self::Threw { pc: returned_pc() },
            NATIVE_JS_STOPPED => Self::decode(word),
            LANE_BODY_NOT_ENTERED => Self::Stub,
            _ if status >= RETURN_STATUS_BASE => Self::At((status - RETURN_STATUS_BASE) as usize),
            // Dispatch reaches an invalid PC only before any instruction ran.
            _ => Self::At(0),
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
/// stack, where the lane reads and continues it (`dispatch_fast::lane_region_left`). Whether a
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    /// An inlined callee's register, which the leave stores to this slot of the entry's
    /// scratch window (`LaneFrame::scratch`).
    Scratch(u16),
}

/// A value live across a leave: its unit register and where the leave stores it. Every live
/// value is stored tagged, so no representation is recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LiveValue {
    pub(super) slot: u16,
    pub(super) source: Source,
}

/// What a leave is and the values it stores that its caller's registers do not already hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExitSig {
    pub(super) kind: ExitKind,
    pub(super) live: Box<[LiveValue]>,
}

/// One leave of a unit: its sig, its place, its dense site id, and the first unit PC that
/// leaves there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ExitSite {
    pub(super) sig: usize,
    pub(super) exit: LaneExit,
    pub(super) id: u32,
    pub(super) unit_pc: usize,
}

/// A unit's leaves, each place once, ordered by leave word, over its interned sigs.
#[derive(Default)]
pub(in crate::vm) struct ExitTable {
    sigs: Vec<ExitSig>,
    sites: Vec<ExitSite>,
}

impl ExitTable {
    /// The table of the places `unit`'s PCs leave at.
    pub(super) fn of_unit(unit: &lane_unit::Unit) -> Self {
        let mut table = Self::default();
        let mut frame_sigs: Vec<Option<usize>> = vec![None; unit.frames.len()];
        for (unit_pc, origin) in unit.origins.iter().enumerate() {
            let (exit, sig) = match *origin {
                lane_unit::Origin::Caller(pc) => (LaneExit::At(pc as usize), table.intern(ExitSig { kind: ExitKind::At, live: Box::default() })),
                lane_unit::Origin::Inlined { frame, pc } => {
                    let sig = *frame_sigs[frame].get_or_insert_with(|| table.intern(Self::inlined_sig(unit, frame)));
                    (LaneExit::Inlined { frame, pc }, sig)
                }
            };
            table.add(exit, sig, unit_pc);
        }
        table
    }

    /// The sig of a leave inside `frame`: every register of its chain's frames, stored tagged.
    fn inlined_sig(unit: &lane_unit::Unit, frame: usize) -> ExitSig {
        let live = unit.chain(frame).into_iter().flat_map(|index| {
            let inlined = unit.frames[index];
            inlined.base..inlined.base + inlined.registers
        }).map(|register| LiveValue { slot: register, source: Source::Scratch(register - unit.caller_registers) });
        ExitSig { kind: ExitKind::Inlined, live: live.collect() }
    }

    fn intern(&mut self, sig: ExitSig) -> usize {
        self.sigs.iter().position(|known| *known == sig).unwrap_or_else(|| {
            self.sigs.push(sig);
            self.sigs.len() - 1
        })
    }

    /// Adds the leave `exit` unless an earlier unit PC already leaves there.
    fn add(&mut self, exit: LaneExit, sig: usize, unit_pc: usize) {
        let Err(at) = self.sites.binary_search_by_key(&exit.word(), |site| site.exit.word()) else { return };
        let id = self.sites.len() as u32;
        self.sites.insert(at, ExitSite { sig, exit, id, unit_pc });
    }

    /// The site at the place `exit` leaves, which only a leave at a PC has.
    pub(super) fn site(&self, exit: LaneExit) -> Option<&ExitSite> {
        if !matches!(exit, LaneExit::At(_) | LaneExit::Inlined { .. }) { return None; }
        self.sites.binary_search_by_key(&exit.word(), |site| site.exit.word()).ok().map(|index| &self.sites[index])
    }

    /// The diagnostic name of the leave `exit` (`QUENCH_LANE_BODY`): its site id and, for an
    /// inlined leave, where its sig stores each live value (`slot>scratch`).
    pub(in crate::vm) fn describe(&self, exit: LaneExit) -> String {
        let Some(site) = self.site(exit) else { return String::new() };
        let sig = &self.sigs[site.sig];
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
        assert_eq!(LaneExit::read(NATIVE_JS_STOPPED, word, || unreachable!()), LaneExit::Inlined { frame: 1, pc: 2 });
        assert_eq!(LaneExit::read(TERMINATED_STATUS as u32, 0, || unreachable!()), LaneExit::Terminated);
        let returned = LaneExit::read(LANE_BODY_RETURNED, Value::UNDEFINED.to_bits(), || 5);
        assert!(matches!(returned, LaneExit::Returned { pc: 5, .. }));
    }

    /// Every producer of a leave word stops with `NATIVE_JS_STOPPED` (the stencil entry thunks
    /// return it, `stencil_layout_matches`), which the one reader decodes.
    #[test]
    fn a_leave_status_decodes_its_word() {
        for exit in EXITS {
            assert_eq!(LaneExit::read(NATIVE_JS_STOPPED, exit.word(), || unreachable!()), exit);
        }
        assert_eq!(LaneExit::read(0, LaneExit::Entry.word(), || unreachable!()), LaneExit::At(0), "status 0 is no leave");
    }
}
