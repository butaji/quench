//! A whole function as one flat program of rows, numbered from 0: the stitch index every web,
//! home, liveness set and exit of the body is keyed by.
//!
//! The rows are what the caller's instructions decode to (`function::cover`), each caller
//! instruction at its row, and in place of a call the stitch inlines, the callee's rows
//! (`call_plan::inline::InlineSite`) behind what proves the callee is the function the site
//! recorded: a Stable fact the code folds (no row, except where a call before it may have changed
//! one: the row that reads the code's invalidation word), or a row that tests the callee register
//! (`Proof`). A jump target is a row; a caller's is the first row of the
//! instruction it names. Each row names where the interpreter resumes if it leaves (`Origin`)
//! and the registers it may read there (`Live`): the caller's own, and around an inlined callee's
//! rows the registers its call reads and the ones that live on past it, with the callee's own,
//! renamed into its window.

use super::*;
use super::call_plan::inline::{InlineSite, Proof};
use crate::heap::GlobalFact;

/// What a caller instruction becomes: a row, or a call's inlined site.
pub(super) enum What {
    Row(Decoded),
    Site(InlineSite),
}

impl What {
    /// Whether it makes a call or a step the interpreter's operation runs, either of which may
    /// change a global a folded fact holds.
    fn changes_globals(&self) -> bool {
        matches!(self, Self::Row(row) if matches!(row.step, Some(Step::Call(_) | Step::Helper)))
    }

    /// The rows it places: a site's are its callee's behind the callee's proof, which is a row
    /// unless a Stable fact holds it and nothing before may have changed it (`changed`).
    fn len(&self, changed: bool) -> usize {
        match self {
            Self::Row(_) => 1,
            Self::Site(site) => usize::from(matches!(site.proof, Proof::Guard(..)) || changed) + site.rows.len(),
        }
    }
}

/// A caller instruction's row or site, and its PC.
pub(super) struct Part {
    pub(super) pc: u32,
    pub(super) what: What,
}

/// The flat program: its rows, where each resumes the interpreter, and what each reads there.
pub(super) struct Stitched {
    pub(super) rows: Vec<Decoded>,
    pub(super) origins: Vec<Origin>,
    pub(super) live: Live,
    /// The inlined calls, in program order: the method's frames.
    pub(super) frames: Vec<InlinedCall>,
    /// The copies a leave inside each frame runs first (`InlineSite::copies`), by frame.
    pub(super) copies: Vec<Vec<(Register, Register)>>,
    /// The Stable facts the sites fold: the code is subscribed to each.
    pub(super) facts: Vec<GlobalFact>,
}

/// The first row of each caller PC's parts, and the row past the last.
fn starts(count: usize, parts: &[Part], changed: bool) -> Vec<u32> {
    let (mut starts, mut at, mut last) = (vec![0; count + 1], 0, None);
    for part in parts {
        if last.replace(part.pc) != Some(part.pc) {
            starts[part.pc as usize] = at;
        }
        at += part.what.len(changed) as u32;
    }
    starts[count] = at;
    starts
}

/// The caller's registers (below `registers`) the interpreter may read resuming at `pc`.
fn callers(region: &LoopRegion, pc: u32, registers: u16) -> impl Iterator<Item = Register> + '_ {
    (0..registers).filter(move |register| region.is_live(pc, *register))
}

/// The parts laid out as one program over the function `region` holds, which has `registers`
/// registers, folding facts under the invalidation word at `invalidation`.
pub(super) fn stitch(region: &LoopRegion, parts: Vec<Part>, registers: u16, invalidation: usize) -> Stitched {
    let changed = parts.iter().any(|part| part.what.changes_globals());
    let starts = starts(region.instructions().len(), &parts, changed);
    let tops = parts.iter().filter_map(|part| if let What::Site(site) = &part.what { Some(site.top as usize) } else { None });
    let rows = starts[starts.len() - 1] as usize;
    let mut stitched = Stitched { rows: Vec::with_capacity(rows), origins: Vec::with_capacity(rows), live: Live::new(tops.fold(usize::from(registers), usize::max)), frames: Vec::new(), copies: Vec::new(), facts: Vec::new() };
    for part in parts {
        match part.what {
            What::Row(row) => {
                let row = Decoded { target: row.target.map(|target| starts[target as usize]), ..row };
                stitched.push(row, Origin::Caller(part.pc), callers(region, part.pc, registers));
            }
            What::Site(site) => stitched.site(region, part.pc, registers, site, changed.then_some(invalidation)),
        }
    }
    stitched
}

impl Stitched {
    fn push(&mut self, row: Decoded, origin: Origin, live: impl Iterator<Item = Register>) {
        self.rows.push(row);
        self.origins.push(origin);
        self.live.push(live);
    }

    /// The rows of `site`, the call at `pc`: the callee's proof (a row, or the read of the
    /// invalidation word at `held` where a call before it may have changed a fact it folds), then its rows, which leave inside
    /// the callee at their instruction, except the copies of the arguments, which leave at the call.
    /// Its registers are live where the callee's are, and where the caller's are live at
    /// the call or past it (the result excepted, which the callee writes; a returning call has no
    /// past).
    fn site(&mut self, region: &LoopRegion, pc: u32, registers: u16, site: InlineSite, held: Option<usize>) {
        let frame = self.frames.len();
        self.frames.push(site.frame);
        self.copies.push(site.copies);
        let proof = match site.proof {
            Proof::Guard(callee, word) => Some(Decoded { hole: word, ..Decoded::heap(Heap::Identity, None, Some(Source::Register(callee)), None) }),
            Proof::Held(fact) => {
                self.facts.push(fact);
                held.map(|address| Decoded { hole: address as u64, ..Decoded::heap(Heap::Held, None, None, None) })
            }
        };
        if let Some(proof) = proof {
            self.push(proof, Origin::Caller(pc), callers(region, pc, registers));
        }
        let (first, result) = (self.rows.len() as u32, site.shape.result);
        let after = callers(region, pc + 1, registers).filter(|register| *register != result && !site.shape.returning);
        let around: Vec<Register> = callers(region, pc, registers).chain(after).collect();
        for row in site.rows {
            let caller = if row.pc.is_some() { around.clone() } else { callers(region, pc, registers).collect() };
            let decoded = Decoded { target: row.decoded.target.map(|target| first + target), ..row.decoded };
            let origin = row.pc.map_or(Origin::Caller(pc), |pc| Origin::Inlined { frame, pc });
            self.push(decoded, origin, caller.into_iter().chain(row.live));
        }
    }
}
