use super::*;
use super::call_plan::DirectFacts;
use super::super::LaneHelper;
use quench_bjit::{call, exit_stub, padded_holes, patch_successor, EXIT_STUB_LEN, HOLES};

/// `jmp rel32`: a resume stub's last row, which jumps to its PC's first row.
#[cfg(target_arch = "x86_64")]
pub(super) use quench_bjit::stub::JMP_REL32;
#[cfg(target_arch = "x86_64")]
pub(super) const JUMP_LEN: usize = 1 + std::mem::size_of::<u32>();
/// `b`: a resume stub's last row, which branches to its PC's first row.
#[cfg(target_arch = "aarch64")]
pub(super) const JUMP_LEN: usize = quench_bjit::patch::INSN_BYTES;

/// The bytes of a resume stub's jump before `patch_jump` sets its target: `jmp rel32` with a zero
/// displacement (x86-64).
#[cfg(target_arch = "x86_64")]
fn jump_row() -> Vec<u8> {
    [&[JMP_REL32][..], &[0; JUMP_LEN - 1]].concat()
}

/// The bytes of a resume stub's jump before `patch_jump` sets its target: `b` with a zero
/// displacement (AArch64).
#[cfg(target_arch = "aarch64")]
fn jump_row() -> Vec<u8> {
    quench_bjit::stub::A64_B.to_le_bytes().to_vec()
}

/// Patches the jump at `at` of `code` to a row `delta` bytes from the jump's start. An x86-64
/// `jmp rel32` displacement counts from the end of its five bytes; an AArch64 `b` displacement
/// counts from its own start.
#[cfg(target_arch = "x86_64")]
fn patch_jump(code: &mut [u8], at: usize, delta: i64) -> Result<(), String> {
    let displacement = (delta - JUMP_LEN as i64) as i32;
    code[at + 1..at + JUMP_LEN].copy_from_slice(&displacement.to_le_bytes());
    Ok(())
}

/// Patches the jump at `at` of `code` to a row `delta` bytes from the jump's start (AArch64: the
/// `b`'s displacement counts from its own start).
#[cfg(target_arch = "aarch64")]
fn patch_jump(code: &mut [u8], at: usize, delta: i64) -> Result<(), String> {
    let insn = quench_bjit::patch::patch_branch(quench_bjit::stub::A64_B, quench_bjit::class::BRANCH26, delta).map_err(|error| format!("resume jump: {error:?}"))?;
    code[at..at + JUMP_LEN].copy_from_slice(&insn.to_le_bytes());
    Ok(())
}

/// How a region hands back its result: a loop region stores its word through the entry's result
/// pointer and returns status 0, or, where a call it made ended the run (`Leave::Status`), that call's
/// status; a function body returns its status and word in two registers, as a lane body does
/// (`lane_body_abi`), a leave with status `NATIVE_JS_STOPPED`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Ending {
    Region,
    Body,
}

impl Ending {
    /// The row of a backward jump: a loop region counts the edge in its entry record
    /// (`JumpPoll`); a function body has no such record, so it only polls (`JumpWatch`).
    /// The one place a poll row is chosen, so no body can reach the counting one.
    pub(super) fn poll(self) -> Kind {
        match self { Self::Region => Kind::JumpPoll, Self::Body => Kind::JumpWatch }
    }

    /// The helper a call row runs: a body's call is made on the activation its native frame runs
    /// (`LaneHelper::Call`), a loop region's on the lane's own (`LaneHelper::RegionCall`).
    pub(super) fn call_helper(self) -> LaneHelper {
        match self { Self::Region => LaneHelper::RegionCall, Self::Body => LaneHelper::Call }
    }

    fn thunk(self) -> &'static str {
        match self { Self::Region => "entry_thunk", Self::Body => "entry_body" }
    }

    /// The routine that leaves with a word its holes give, and the one that leaves with the
    /// word an exit stub pushed.
    fn tails(self) -> (&'static str, &'static str) {
        match self { Self::Region => ("leave_word", "leave_pop"), Self::Body => ("leave_stop", "leave_stop_pop") }
    }

    /// The holes of a leave routine that returns `word`.
    fn holes(self, word: u64) -> [u64; HOLES] {
        padded_holes(match self { Self::Region => [word, 0, 0], Self::Body => [word, u64::from(super::super::NATIVE_JS_STOPPED), 0] })
    }
}

/// A routine of the stencil build (`quench-bjit/stencils/entry.rs`) by name.
pub(super) fn routine(name: &str) -> Result<Stencil, String> {
    quench_bjit::rustc::stencil(name).ok_or_else(|| format!("no stencil routine {name}"))
}

/// The entry thunk, its call to the first row (at `rows_at`) patched.
pub(super) fn thunk(ending: Ending, rows_at: usize) -> Result<Vec<u8>, String> {
    let stencil = routine(ending.thunk())?;
    let mut code = stencil.code.clone();
    for reloc in &stencil.relocs {
        let at = reloc.offset as usize;
        patch_successor(&mut code, at, reloc.class, rows_at as i64 + reloc.addend - at as i64).map_err(|error| format!("entry thunk: {error:?}"))?;
    }
    Ok(code)
}

/// The code that writes `spills` back to the register file (a pinned double as a canonical
/// word, a pinned int32 tagged), then copies each of `copies` (destination register, source
/// register) between their frame slots, and ends in the leave routine `tail`, given `leave` as its
/// holes.
pub(super) fn leave_chain(spills: &[(Place, Register)], copies: &[(Register, Register)], tail: &str, leave: [u64; HOLES]) -> Result<Vec<u8>, String> {
    let mut chain = Vec::new();
    for (place, register) in spills {
        let row = numeric::Row { kind: Kind::Spill, dst: Place::Mem, a: *place, b: Place::Mem, speculate: false };
        chain.push((numeric::stencil(row)?.clone(), padded_holes([u64::from(*register) * SLOT])));
    }
    for (dst, src) in copies {
        let row = numeric::Row { kind: Kind::Move, dst: Place::Mem, a: Place::Mem, b: Place::Mem, speculate: false };
        chain.push((numeric::stencil(row)?.clone(), padded_holes([u64::from(*dst) * SLOT, u64::from(*src) * SLOT])));
    }
    chain.push((routine(tail)?, leave));
    let hole_fns: Vec<Box<dyn Fn(u32) -> u64 + '_>> = chain.iter().map(|(_, h)| Box::new(move |i: u32| h[i as usize]) as Box<dyn Fn(u32) -> u64>).collect();
    let rows: Vec<Placed> = chain.iter().enumerate().map(|(k, (stencil, _))| Placed { stencil, hole: &*hole_fns[k], taken: None, cold: false }).collect();
    let mut code = vec![0u8; rows.iter().map(|row| row.stencil.code.len()).sum()];
    // SAFETY: `code` has room for every stencil's full length, which bounds the chain.
    let (len, _) = unsafe { quench_bjit::copy_patch(&rows, code.as_mut_ptr(), |_, _| 0) };
    code.truncate(len);
    Ok(code)
}

/// What the linker needs of the region: its span, the registers live at a row, and the entry
/// loads.
pub(super) struct Layout<'a> {
    pub(super) span: Span<'a>,
    pub(super) homes: Homes,
    pub(super) webs: Webs,
    pub(super) ending: Ending,
    /// Per caller PC, what the call there recorded of each callee it reached, latest first: the code
    /// calls each by its entry.
    pub(super) callees: &'a [Vec<DirectFacts>],
    /// Per row of a stitched program, where the interpreter resumes if the row leaves.
    pub(super) origins: &'a [Origin],
    /// Per inlined frame, the (parameter register, argument register) copies a leave inside it runs
    /// first: a parameter the callee never writes lives in the caller's argument register.
    pub(super) copies: &'a [Vec<(Register, Register)>],
    /// The first row of each caller PC of the span (`Stitched::entries`), by PC.
    pub(super) entries: &'a [(u32, u32)],
    /// The address of the invalidation word the code reads before each row that holds a Stable fact a
    /// call may have changed (`Decoded::holds_stable`): a body that folds one and makes a call.
    pub(super) invalidation: Option<usize>,
    /// The realm the entry proves active, where the code inlines callees.
    pub(super) realm: Option<RealmProof>,
    /// Per row, whether a receiver proof reaches it (`Decoded::relies`): no resume stub enters there.
    pub(super) relies: &'a [bool],
}

impl Layout<'_> {
    /// What the call at caller PC `pc` recorded of each callee it reached, latest first.
    pub(super) fn callees(&self, pc: u32) -> &[DirectFacts] {
        self.callees.get(pc as usize).map_or(&[], Vec::as_slice)
    }

    fn origin(&self, row: u32) -> Option<Origin> {
        self.origins.get(row as usize).copied()
    }

    /// The word a leave at `row` returns: the caller's PC, or the callee's inside its inlined frame.
    pub(super) fn exit_word(&self, row: u32) -> u64 {
        match self.origin(row) {
            Some(Origin::Caller(pc)) => u64::from(pc),
            Some(Origin::Inlined { frame, pc }) => super::super::LaneExit::Inlined { frame, pc }.word(),
            None => u64::from(row),
        }
    }

    /// The caller's record the instruction of `row` is: where a helper row names it and a
    /// completion records it.
    pub(super) fn record(&self, row: u32) -> u32 {
        match self.origin(row) {
            Some(Origin::Caller(pc)) => pc,
            _ => row,
        }
    }

    /// Whether `row` is inside an inlined callee.
    pub(super) fn inlined(&self, row: u32) -> bool {
        matches!(self.origin(row), Some(Origin::Inlined { .. }))
    }

    /// The copies a leave at `row` runs before it leaves: its inlined frame's, else none.
    fn copies(&self, row: u32) -> &[(Register, Register)] {
        match self.origin(row) {
            Some(Origin::Inlined { frame, .. }) => &self.copies[frame],
            _ => &[],
        }
    }

    /// The pinned registers live at `pc`: the ones a stub there writes back.
    pub(super) fn live_pins(&self, pc: u32) -> Vec<Pin> {
        self.live_homes(&self.homes.pins, pc)
    }

    pub(super) fn live_floats(&self, pc: u32) -> Vec<Pin> {
        self.live_homes(&self.homes.floats, pc)
    }

    /// The pins of `homes` whose web is live at `pc`, each with the register it writes back to.
    fn live_homes(&self, homes: &[Pin], pc: u32) -> Vec<Pin> {
        let live = self.webs.live_at(&self.span, pc);
        homes.iter().filter(|(_, web)| live.contains(&usize::from(*web))).map(|(pin, web)| (*pin, self.homes.orig[usize::from(*web)])).collect()
    }

    /// The registers a leave at `pc` writes back: the live pinned doubles, then the ints.
    pub(super) fn spills(&self, pc: u32) -> Vec<(Place, Register)> {
        let doubles = self.live_floats(pc).into_iter().map(|(pin, register)| (Place::PinF(pin as u8), register));
        doubles.chain(self.live_pins(pc).into_iter().map(|(pin, register)| (Place::Pin(pin as u8), register))).collect()
    }

    /// The routine a row's exit stub jumps to: it writes the live pinned registers back and returns the PC the stub pushed.
    pub(super) fn routine(&self, pc: u32) -> Result<Vec<u8>, String> {
        leave_chain(&self.spills(pc), &[], self.ending.tails().1, self.ending.holes(0))
    }
}

/// The entry rows: the realm a body inlined callees in, or its rows' global slots belong to, is the
/// active one, each pinned register live at the header (or, for a resume stub, at its PC; `entered` are
/// the webs live there) loads its int32, each int slot is checked
/// (or, dead there, tagged), and each other Number register live there is checked, so a loop whose
/// values are no Numbers is never entered.
pub(super) fn entry_rows(layout: &Layout, entered: &[usize]) -> Result<Vec<(&'static Stencil, [u64; HOLES])>, String> {
    let guard = |kind, dst, offset: u64| {
        numeric::stencil(numeric::Row { kind, dst, a: Place::Mem, b: Place::Mem, speculate: true }).map(|stencil| {
            (stencil, padded_holes(if kind == Kind::TagInit { [offset, 0, 0] } else { [0, offset, 0] }))
        })
    };
    let mut rows = Vec::new();
    if let Some(realm) = layout.realm {
        let row = HeapRow { heap: Heap::Realm, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
        rows.push((base_rows::stencil(row)?, padded_holes([0, realm.offset, realm.global])));
    }
    let slot = |web: Register| u64::from(layout.homes.orig[usize::from(web)]) * SLOT;
    for (pin, web) in layout.homes.pins.iter().filter(|(_, web)| entered.contains(&usize::from(*web))) {
        rows.push(guard(Kind::Guard, Place::Pin(*pin as u8), slot(*web))?);
    }
    for (pin, web) in layout.homes.floats.iter().filter(|(_, web)| entered.contains(&usize::from(*web))) {
        rows.push(guard(Kind::Guard, Place::PinF(*pin as u8), slot(*web))?);
    }
    for web in &layout.homes.slots {
        let live = entered.contains(&usize::from(*web));
        rows.push(guard(if live { Kind::Guard } else { Kind::TagInit }, Place::MemInt, slot(*web))?);
    }
    for web in entered.iter().map(|web| *web as Register) {
        let number = layout.span.number_registers().contains(&layout.homes.orig[usize::from(web)]);
        if number && layout.homes.place(web) == Place::Mem {
            rows.push(guard(Kind::Guard, Place::Mem, slot(web))?);
        }
    }
    Ok(rows)
}

/// A leave row of the cold area, behind the body: what it returns and what it writes back first.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Leave {
    /// Leaves at `pc`, the live pinned registers there written back first.
    Pc(u32),
    /// Returns this word; nothing is written back (the chain's spills already ran, or no row has).
    Word(u64),
    /// `LEAVE_STATUS`.
    Status,
    /// Completes the activation at `pc` with the register slot at byte offset `slot`, the live
    /// pinned registers written back first (a body's `Return`).
    Return { pc: u32, slot: u64 },
}

impl Stop {
    /// The leave row a stop jumps to.
    fn leave(self) -> Leave {
        self.returns.map_or(Leave::Pc(self.pc), |slot| Leave::Return { pc: self.pc, slot })
    }
}

/// The leave rows, each once, in the order they were asked for.
struct Leaves(Vec<(Leave, Stencil)>);

impl Leaves {
    /// The leave rows every link starts with, and the index of the failed entry's (nothing to
    /// write back).
    fn open(layout: &Layout) -> Result<(Self, usize), String> {
        let mut leaves = Self(Vec::new());
        let failed = leaves.index(layout, Leave::Word(super::super::LaneExit::Entry.word()))?;
        Ok((leaves, failed))
    }

    /// The index of `leave` among the rows, which it joins if it is new.
    fn index(&mut self, layout: &Layout, leave: Leave) -> Result<usize, String> {
        if let Some(found) = self.0.iter().position(|(candidate, _)| *candidate == leave) {
            return Ok(found);
        }
        let ending = layout.ending;
        let stencil = match leave {
            Leave::Pc(pc) => leave_row(leave_chain(&layout.spills(pc), layout.copies(pc), ending.tails().0, ending.holes(layout.exit_word(pc)))?),
            Leave::Word(word) => leave_row(leave_chain(&[], &[], ending.tails().0, ending.holes(word))?),
            Leave::Status => routine(call::LEAVE_STATUS)?,
            Leave::Return { pc, slot } => {
                let status = u64::from(super::super::LANE_BODY_RETURNED);
                leave_row(leave_chain(&layout.spills(pc), &[], "leave_returned", padded_holes([slot, status, u64::from(layout.record(pc))]))?)
            }
        };
        self.0.push((leave, stencil));
        Ok(self.0.len() - 1)
    }
}

/// Where an exit hole of a placed row goes: a stub in the cold area behind the leave rows, or a
/// leave row.
pub(super) enum Target {
    Stub { pc: u32, routine: usize },
    Row(usize),
}

/// The index of the routine of the pinned registers live at `pc`, one per set of them.
fn routine_index(layout: &Layout, routines: &mut Vec<(Vec<Pin>, Vec<Pin>, Vec<u8>)>, pc: u32) -> Result<usize, String> {
    let (ints, floats) = (layout.live_pins(pc), layout.live_floats(pc));
    if let Some(found) = routines.iter().position(|(i, f, _)| *i == ints && *f == floats) {
        return Ok(found);
    }
    routines.push((ints, floats, layout.routine(pc)?));
    Ok(routines.len() - 1)
}

/// The target of each exit of each body row, and the routines the stubs enter. `entry` is the number
/// of entry rows, which precede the body, and `leaf_base` the index of the first leave row among the
/// placed rows.
fn exit_targets(layout: &Layout, body: &[&Planned], (entry, leaf_base): (usize, usize), leaves: &mut Leaves) -> Result<(Vec<Vec<Target>>, Vec<Vec<u8>>), String> {
    let mut routines = Vec::new();
    let mut targets = Vec::with_capacity(body.len());
    for (at, row) in body.iter().enumerate() {
        let mut row_targets = Vec::with_capacity(row.exits.len());
        for exit in &row.exits {
            row_targets.push(match *exit {
                Exit::Spilling(pc) if layout.inlined(pc) => Target::Row(leaf_base + leaves.index(layout, Leave::Pc(pc))?),
                Exit::Spilling(pc) => Target::Stub { pc: layout.record(pc), routine: routine_index(layout, &mut routines, pc)? },
                Exit::At(pc) => Target::Row(leaf_base + leaves.index(layout, Leave::Word(u64::from(pc)))?),
                Exit::Status => Target::Row(leaf_base + leaves.index(layout, Leave::Status)?),
                Exit::Ahead(parts) => Target::Row(entry + at + parts),
            });
        }
        targets.push(row_targets);
    }
    Ok((targets, routines.into_iter().map(|(_, _, code)| code).collect()))
}

/// The index of each planned row's first part among all parts.
fn row_starts(planned: &[Planned]) -> Vec<usize> {
    let mut at = 0;
    planned
        .iter()
        .map(|row| {
            let start = at;
            at += row.pre.len() + 1 + row.post.len();
            start
        })
        .collect()
}

/// A part of a planned row's run, and the index of the run's first part.
type Part<'a> = (&'a Planned, usize);

/// The index among the placed rows that `row` branches to, given the planned rows' `starts`: a
/// stop is its leave row, a branch inside the region its row, a branch out of it a leave row,
/// and a part of the row's own run the part.
fn branch_target(layout: &Layout, leaves: &mut Leaves, starts: &[usize], (row, run): Part, parts: usize, entry: usize) -> Result<Option<usize>, String> {
    let (header, backedge) = (layout.span.header, layout.span.backedge);
    let leave = match (row.stop, row.branch) {
        (Some(stop), _) => stop.leave(),
        (None, Some(Branch::Part(part))) => return Ok(Some(entry + run + part)),
        (None, Some(Branch::Pc(pc))) if (header..=backedge).contains(&pc) => return Ok(Some(entry + starts[(pc - header) as usize])),
        (None, Some(Branch::Pc(pc))) => Leave::Pc(pc),
        (None, None) => return Ok(None),
    };
    Ok(Some(entry + parts + leaves.index(layout, leave)?))
}

/// Lays the entry rows, rows, leave stubs and, for a loop region, resume stubs out as one code buffer
/// behind the entry thunk, with the table of the resume stubs. The hot code is the entry rows and the
/// body, which fall through to their successors; everything a guard, a miss or a branch out of the
/// region reaches (the leave rows, the exit routines and each exiting row's stub) is placed after it
/// (`place_rows`).
pub(super) fn link(layout: &Layout, planned: &[Planned]) -> Result<(Vec<u8>, ResumeTable), String> {
    // A row's own rows run before and after it at its PC: a branch to the PC lands on the first.
    let (starts, mut parts) = (row_starts(planned), Vec::new());
    for (row, start) in planned.iter().zip(&starts) {
        parts.extend(row.pre.iter().chain(std::iter::once(row)).chain(&row.post).map(|part| (part, *start)));
    }
    let body: Vec<&Planned> = parts.iter().map(|(part, _)| *part).collect();
    let entry = entry_rows(layout, &layout.webs.entry_live(&layout.span))?;
    let (g, n) = (entry.len(), body.len());
    let (mut leaves, failed) = Leaves::open(layout)?;
    let targets = parts.iter().map(|part| branch_target(layout, &mut leaves, &starts, *part, n, g)).collect::<Result<Vec<_>, _>>()?;
    let (body_exits, routines) = exit_targets(layout, &body, (g, g + n), &mut leaves)?;
    let stubs = if layout.ending == Ending::Region { resume_stubs(layout, planned, (g, &starts))? } else { Vec::new() };
    let jump = leave_row(jump_row());
    let mut holes: Vec<[u64; HOLES]> = entry.iter().map(|(_, holes)| *holes).collect();
    holes.extend(body.iter().map(|row| row.holes));
    holes.extend(std::iter::repeat_n([0; HOLES], leaves.0.len()));
    holes.extend(stubs.iter().flat_map(Stub::holes));
    let hole_fns: Vec<Box<dyn Fn(u32) -> u64 + '_>> =
        holes.iter().map(|h| Box::new(move |i: u32| h[i as usize]) as Box<dyn Fn(u32) -> u64>).collect();
    let mut rows: Vec<Placed> = Vec::with_capacity(g + n + leaves.0.len());
    rows.extend(entry.iter().enumerate().map(|(k, (stencil, _))| Placed { stencil, hole: &*hole_fns[k], taken: Some(g + n + failed), cold: false }));
    rows.extend(body.iter().enumerate().map(|(k, row)| Placed { stencil: row.stencil, hole: &*hole_fns[g + k], taken: targets[k], cold: row.cold }));
    rows.extend(leaves.0.iter().enumerate().map(|(k, (_, stencil))| Placed { stencil, hole: &*hole_fns[g + n + k], taken: None, cold: false }));
    let jumps = place_stubs(&mut rows, &stubs, &jump, &hole_fns, g + n + failed);
    let exits: Vec<Vec<Target>> = std::iter::repeat_with(Vec::new).take(g).chain(body_exits).chain(std::iter::repeat_with(Vec::new).take(rows.len() - g - n)).collect();
    let (code, offsets, thunk) = place_rows(layout.ending, &rows, (&exits, &routines), &jumps)?;
    Ok((code, ResumeTable::of(thunk, &stubs, &jumps, &offsets)?))
}

/// Writes the exit stub of `pc` at `at`: it pushes the PC and jumps to the routine at `routine_at`.
fn write_stub(code: &mut [u8], at: usize, pc: u32, routine_at: usize) -> Result<(), String> {
    let stub = exit_stub(pc, routine_at as i64 - at as i64).map_err(|error| format!("exit stub: {error:?}"))?;
    code[at..at + EXIT_STUB_LEN].copy_from_slice(&stub);
    Ok(())
}

/// Copies each of `routines` to `at`, returning where each starts and the end.
fn append_routines(code: &mut [u8], mut at: usize, routines: &[Vec<u8>]) -> (Vec<usize>, usize) {
    let mut starts = Vec::with_capacity(routines.len());
    for routine in routines {
        code[at..at + routine.len()].copy_from_slice(routine);
        starts.push(at);
        at += routine.len();
    }
    (starts, at)
}

/// The address each exit of each row goes to, writing the stubs it names at `at` (each entering its
/// routine of `routine_at`); the end of the stubs too. `rows_at` is the address of the first row's
/// code and `starts` each row's offset from it.
fn exit_addresses(code: &mut [u8], (rows_at, starts): (usize, &[usize]), mut at: usize, (exits, routine_at): (&[Vec<Target>], &[usize])) -> Result<(Vec<Vec<usize>>, usize), String> {
    let base = code.as_ptr() as usize;
    let mut addresses: Vec<Vec<usize>> = Vec::with_capacity(exits.len());
    for row_exits in exits {
        let mut row_addresses = Vec::with_capacity(row_exits.len());
        for target in row_exits {
            row_addresses.push(match *target {
                Target::Row(row) => rows_at + starts[row],
                Target::Stub { pc, routine } => {
                    write_stub(code, at, pc, routine_at[routine])?;
                    at += EXIT_STUB_LEN;
                    base + at - EXIT_STUB_LEN
                }
            });
        }
        addresses.push(row_addresses);
    }
    Ok((addresses, at))
}

/// Copies the rows behind the thunk, then the exit routines and each stub, and patches each
/// row's exits to their targets, `exits` by row and exit kind (a leave row's start, or a stub);
/// the code is position independent, so the buffer can move to executable memory. The rows are
/// in layout order: the hot rows first, so nothing cold sits between them. Also the code offset of
/// each row and, for a loop region, of the resume thunk (`entry_resume`), which follows the stubs;
/// each resume stub's `jumps` is patched to its row.
pub(super) fn place_rows(ending: Ending, rows: &[Placed], (exits, routines): (&[Vec<Target>], &[Vec<u8>]), jumps: &[Jump]) -> Result<(Vec<u8>, Vec<usize>, usize), String> {
    let head = thunk(ending, 0)?.len();
    let resume = if ending == Ending::Region { routine("entry_resume")?.code } else { Vec::new() };
    let bound: usize = rows.iter().map(|row| row.stencil.code.len()).sum();
    let stubs = exits.iter().flatten().filter(|target| matches!(target, Target::Stub { .. })).count();
    let tail: usize = routines.iter().map(Vec::len).sum::<usize>() + stubs * EXIT_STUB_LEN + resume.len();
    let mut code = vec![0u8; head + bound + tail];
    let base = code.as_mut_ptr();
    // SAFETY: `base + head` has room for every stencil's full length, which bounds the rows.
    let (len, starts) = unsafe { quench_bjit::copy_patch(rows, base.add(head), |_, _| base as usize) };
    let (routine_at, at) = append_routines(&mut code, head + len, routines);
    let (addresses, mut at) = exit_addresses(&mut code, (base as usize + head, &starts), at, (exits, &routine_at))?;
    // SAFETY: as above; the stubs lie after the copied code inside `code`.
    let (_, starts) = unsafe { quench_bjit::copy_patch(rows, base.add(head), |row, kind| addresses[row].get(kind as usize).copied().unwrap_or(base as usize)) };
    for jump in jumps {
        patch_jump(&mut code, head + starts[jump.row], starts[jump.target] as i64 - starts[jump.row] as i64)?;
    }
    let thunk_bytes = thunk(ending, head)?;
    code[..head].copy_from_slice(&thunk_bytes);
    code[at..at + resume.len()].copy_from_slice(&resume);
    let resume_at = at;
    at += resume.len();
    code.truncate(at);
    Ok((code, starts.iter().map(|start| head + start).collect(), resume_at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_bjit::Reloc;

    /// The machine code of a bare return on the host architecture.
    #[cfg(target_arch = "x86_64")]
    fn ret() -> Vec<u8> {
        vec![quench_bjit::RET]
    }

    /// The machine code of a bare return on the host architecture.
    #[cfg(target_arch = "aarch64")]
    fn ret() -> Vec<u8> {
        quench_bjit::RET.to_vec()
    }

    /// A row that is one `jmp rel32` through its first exit hole.
    #[cfg(target_arch = "x86_64")]
    fn exiting() -> Stencil {
        let exit = Reloc { offset: 1, class: quench_bjit::REL32, index: quench_bjit::EXIT, addend: -(std::mem::size_of::<i32>() as i64) };
        Stencil { code: vec![quench_bjit::stub::JMP_REL32, 0, 0, 0, 0], relocs: vec![exit], imm32: Vec::new(), next_jump: None, tail_len: 0, raw_len: 5, hot_len: 5 }
    }

    /// A row that is one `b` through its first exit hole (its displacement counts from its start).
    #[cfg(target_arch = "aarch64")]
    fn exiting() -> Stencil {
        let exit = Reloc { offset: 0, class: quench_bjit::BRANCH26, index: quench_bjit::EXIT, addend: 0 };
        let code = quench_bjit::stub::A64_B.to_le_bytes().to_vec();
        Stencil { code, relocs: vec![exit], imm32: Vec::new(), next_jump: None, tail_len: 0, raw_len: 4, hot_len: 4 }
    }

    /// Where the row at `at` of `code` jumps to.
    #[cfg(target_arch = "x86_64")]
    fn jump_target(code: &[u8], at: usize) -> usize {
        let field = at + 1;
        let displacement = i32::from_le_bytes(code[field..field + std::mem::size_of::<i32>()].try_into().expect("a displacement"));
        (field as i64 + std::mem::size_of::<i32>() as i64 + i64::from(displacement)) as usize
    }

    /// Where the row at `at` of `code` branches to (its `b` displacement counts from `at`).
    #[cfg(target_arch = "aarch64")]
    fn jump_target(code: &[u8], at: usize) -> usize {
        let insn = u32::from_le_bytes(code[at..at + JUMP_LEN].try_into().expect("an instruction"));
        (at as i64 + quench_bjit::patch::branch_delta(insn, quench_bjit::class::BRANCH26)) as usize
    }

    /// A row's stub and a leave row lie after every hot row, the stub after the routines, and each
    /// exit reaches its own target.
    #[test]
    fn exits_land_in_the_cold_area_behind_the_hot_rows() {
        let (hot, leave) = (exiting(), leave_row(ret()));
        let hole = |_: u32| 0u64;
        let rows = [Placed { stencil: &hot, hole: &hole, taken: None, cold: false }, Placed { stencil: &hot, hole: &hole, taken: None, cold: false }, Placed { stencil: &leave, hole: &hole, taken: None, cold: false }];
        let exits = [vec![Target::Stub { pc: 7, routine: 0 }], vec![Target::Row(2)], Vec::new()];
        let routines = [ret()];
        let (code, ..) = place_rows(Ending::Body, &rows, (&exits, &routines), &[]).expect("a layout");
        let head = thunk(Ending::Body, 0).expect("a thunk").len();
        let (second, leave_at) = (head + hot.code.len(), head + 2 * hot.code.len());
        let stub = leave_at + leave.code.len() + routines[0].len();
        assert_eq!(jump_target(&code, head), stub, "the first row's exit is its stub");
        assert_eq!(jump_target(&code, second), leave_at, "the second row's exit is the leave row");
        assert_eq!(code.len(), stub + EXIT_STUB_LEN);
    }
}
