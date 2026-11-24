//! A whole function, or a loop region's span of it, as one flat program of rows, numbered from 0:
//! the stitch index every web, home, liveness set and exit of the code is keyed by.
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
//! renamed into its window. A loop region's rows end in a leave row for each PC control may go to
//! outside the span (the one past its last instruction first, where its last row falls), which
//! resumes the interpreter there, so a jump out of a region is a jump to a row like any other.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::call_plan::inline::{InlineSite, Proof};
use quench_runtime::tier_surface::GlobalFact;

/// What a caller instruction becomes: a row, or a call's inlined site.
pub(crate) enum What {
    Row(Decoded),
    Site(InlineSite),
    /// A PC that plans no row: a field read folded into its call (`fold`), whose call's row leaves at this PC.
    Folded,
    /// A row that leaves at the PC of the instruction it stands for (`fold`), not at its own.
    Resume(Decoded, u32),
}

impl What {
    /// Whether it makes a call or a step the interpreter's operation runs, either of which may
    /// change a global a folded fact holds.
    fn changes_globals(&self) -> bool {
        matches!(self, Self::Row(row) | Self::Resume(row, _) if row.makes_calls())
    }

    /// The rows it places: a site's are its callee's behind the callee's proof, which is a row
    /// unless a Stable fact holds it and nothing before may have changed it (`changed`).
    fn len(&self, changed: bool) -> usize {
        match self {
            Self::Row(_) | Self::Resume(..) => 1,
            Self::Folded => 0,
            Self::Site(site) => usize::from(matches!(site.proof, Proof::Guard(..)) || changed) + site.rows.len(),
        }
    }
}

/// A caller instruction's row or site, and its PC.
pub(crate) struct Part {
    pub(crate) pc: u32,
    pub(crate) what: What,
}

/// The flat program: its rows, where each resumes the interpreter, and what each reads there.
pub(crate) struct Stitched {
    pub(crate) rows: Vec<Decoded>,
    pub(crate) origins: Vec<Origin>,
    pub(crate) live: Live,
    /// The inlined calls, in program order: the method's frames.
    pub(crate) frames: Vec<InlinedCall>,
    /// The copies a leave inside each frame runs first (`InlineSite::copies`), by frame.
    pub(crate) copies: Vec<Vec<(Register, Register)>>,
    /// The Stable facts the sites fold: the code is subscribed to each.
    pub(crate) facts: Vec<GlobalFact>,
    /// The first row of each caller PC of the span that has rows (PC, row): where the interpreter's resume at the PC
    /// re-enters the program (`resume`); a folded read has none.
    pub(crate) entries: Vec<(u32, u32)>,
}

/// The first row of each caller PC's parts (by offset from the span's header), and the row past the last.
fn starts(region: &LoopRegion, parts: &[Part], changed: bool) -> Vec<u32> {
    let count = region.instructions().len();
    let (mut starts, mut at, mut last) = (vec![0; count + 1], 0, None);
    for part in parts {
        if last.replace(part.pc) != Some(part.pc) {
            starts[(part.pc - region.header) as usize] = at;
        }
        at += part.what.len(changed) as u32;
    }
    starts[count] = at;
    starts
}

/// The PCs outside a loop region's span that its rows lead to, each once, in order: the PC past its
/// last instruction first, which its last row falls to, then its jumps' targets. A function's
/// body leads nowhere outside it.
fn outside(region: &LoopRegion, parts: &[Part], ending: Ending) -> Vec<u32> {
    if ending == Ending::Body {
        return Vec::new();
    }
    let span = region.header..=region.backedge;
    let targets = parts.iter().filter_map(|part| if let What::Row(row) = &part.what { row.target } else { None });
    let mut leaves = vec![region.backedge + 1];
    for target in targets.filter(|target| !span.contains(target)) {
        if !leaves.contains(&target) {
            leaves.push(target);
        }
    }
    leaves
}

/// The caller's registers (below `registers`) the interpreter may read resuming at `pc`.
fn callers(region: &LoopRegion, pc: u32, registers: u16) -> impl Iterator<Item = Register> + '_ {
    (0..registers).filter(move |register| region.is_live(pc, *register))
}

/// The parts laid out as one program over the span `region` holds of its function, which has
/// `registers` registers, folding facts under the invalidation word at `invalidation`; a loop
/// region's (`ending`) is followed by its leave rows (`outside`).
pub(crate) fn stitch(region: &LoopRegion, parts: Vec<Part>, registers: u16, invalidation: Option<usize>, ending: Ending) -> Stitched {
    let changed = parts.iter().any(|part| part.what.changes_globals());
    let starts = starts(region, &parts, changed);
    let leaves = outside(region, &parts, ending);
    let tops = parts.iter().filter_map(|part| if let What::Site(site) = &part.what { Some(site.top as usize) } else { None });
    let rows = starts[starts.len() - 1] as usize;
    let capacity = rows + leaves.len();
    // A folded read has no first row to resume at: its PC re-enters nowhere (the interpreter takes it).
    let folded: Vec<u32> = parts.iter().filter(|part| matches!(part.what, What::Folded)).map(|part| part.pc).collect();
    let mut stitched = Stitched { rows: Vec::with_capacity(capacity), origins: Vec::with_capacity(capacity), live: Live::new(tops.fold(usize::from(registers), usize::max)), frames: Vec::new(), copies: Vec::new(), facts: Vec::new(),
        entries: (region.header..).zip(&starts).take(starts.len() - 1).filter(|(pc, _)| !folded.contains(pc)).map(|(pc, row)| (pc, *row)).collect() };
    let row_of = |target: u32| match target.checked_sub(region.header).filter(|local| (*local as usize) < starts.len() - 1) {
        Some(local) => starts[local as usize],
        None => (rows + leaves.iter().position(|leave| *leave == target).expect("a leave row for each target outside")) as u32,
    };
    for part in parts {
        match part.what {
            What::Row(row) => {
                let row = Decoded { target: row.target.map(row_of), ..row };
                stitched.push(row, Origin::Caller(part.pc), callers(region, part.pc, registers));
            }
            What::Resume(row, resume) => {
                let row = Decoded { target: row.target.map(row_of), ..row };
                stitched.push(row, Origin::Caller(resume), callers(region, resume, registers));
            }
            What::Folded => {}
            What::Site(site) => stitched.site(region, part.pc, registers, site, invalidation.filter(|_| changed)),
        }
    }
    for pc in leaves {
        stitched.push(Decoded::step(Step::Leave, None), Origin::Caller(pc), callers(region, pc, registers));
    }
    stitched
}

impl Stitched {
    fn push(&mut self, row: Decoded, origin: Origin, live: impl Iterator<Item = Register>) {
        // A row that folds a global member holds its cell's fact: the body subscribes to it, so a reassignment retires the body.
        self.facts.extend(row.cell.map(|cell| cell.fact));
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
                held.map(|address| Decoded { hole: address as u64, ..Decoded::heap(Heap::Live, None, None, None) })
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
