use super::*;
use super::call_plan::DirectFacts;
use super::declared::EntryGuard;
use super::super::LaneHelper;
use quench_bjit::{call, exit_stub, padded_holes, patch_successor, EXIT_STUB_LEN, HOLES};

mod entry;
pub(super) use entry::{EntryRow, entry_rows};
use entry::guard_mask;
mod exits;
pub(super) use exits::Target;
use exits::{append_routines, exit_addresses, exit_targets, Leave, Leaves};

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

/// The entry thunk, its calls to the first row (at `rows_at`) patched. A body's is followed by its row entry
/// (`entry_rows`, at `rows_entry_offset`), which calls the same first row, and its proven-arguments entry (a second
/// `entry_rows`, at `proven_entry_offset`), which calls the row at `proven_at`: the first row past the Number guards.
pub(super) fn thunk(ending: Ending, rows_at: usize, proven_at: usize) -> Result<Vec<u8>, String> {
    let mut code = Vec::new();
    let names: Vec<(&str, usize)> = match ending {
        Ending::Region => vec![("entry_thunk", rows_at)],
        Ending::Body => vec![("entry_body", rows_at), ("entry_rows", rows_at), ("entry_rows", proven_at)],
    };
    for (name, target) in names {
        let stencil = routine(name)?;
        let start = code.len();
        code.extend_from_slice(&stencil.code);
        for reloc in &stencil.relocs {
            let at = start + reloc.offset as usize;
            patch_successor(&mut code, at, reloc.class, target as i64 + reloc.addend - at as i64).map_err(|error| format!("entry thunk: {error:?}"))?;
        }
    }
    Ok(code)
}

/// Where a body's row entry (`entry_rows`) lies in its code: past `entry_body`, which starts it.
pub(in crate::vm::compiled) fn rows_entry_offset() -> Option<usize> {
    routine("entry_body").ok().map(|stencil| stencil.code.len())
}

/// Where a body's proven-arguments row entry lies in its code: past `entry_body` and the row entry (`entry_rows`).
pub(in crate::vm::compiled) fn proven_entry_offset() -> Option<usize> {
    Some(rows_entry_offset()? + routine("entry_rows").ok()?.code.len())
}

/// The row entries of a body whose code lies at `base` (`EntryCell::rows`, `proven`), with its guard and `this` facts.
pub(in crate::vm::compiled) fn row_entries(base: usize, numbers: u64, uses_this: bool) -> Option<crate::vm::compiled::entry_cell::RowEntries> {
    Some(crate::vm::compiled::entry_cell::RowEntries { rows: base + rows_entry_offset()?, proven: base + proven_entry_offset()?, numbers, uses_this })
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
    /// Per row, whether a proof the body established reaches it (`Decoded::relies`): no resume stub enters there.
    /// Proofs the entry seed alone reaches get a stub, whose entry rows re-run the declared guards (`guards`).
    pub(super) relies: &'a [bool],
    /// Per row, whether its checked operand is a Number on every path into it, so its number guard is redundant
    /// (`numbers::proven_operands`).
    pub(super) number_held: &'a [bool],
    /// Per caller PC, the mask of the call's arguments proven Numbers (bit `k`: argument `k`, `call_args::proven_arguments`);
    /// empty where none is proven.
    pub(super) proven_args: &'a [u64],
    /// The Number guards a Mem slot's entry keeps, by row (`numbers::entry_reads`).
    pub(super) entry_reads: &'a super::numbers::EntryReads,
    /// The entry tests of the declared parameters the function never writes (`declared::entry_guards`).
    pub(super) guards: Vec<EntryGuard>,
    /// The declared Number parameters the function never writes, which the entry's Number guards prove
    /// (`declared::entry_numbers`).
    pub(super) numbers: Vec<Register>,
    /// The class-typed parameters the function never writes, with the prototype each entry checks (`declared::class_prototypes`).
    pub(super) prototypes: Vec<super::function::ClassProto>,
    /// The loop-invariant receivers whose exact shape the entry checks, and the proofs seed (`Known::shapes`, `Heap::Shape`).
    pub(super) shapes: Vec<super::function::ShapeProto>,
    /// Whether a store is admitted on the run's stores word the entry checks (`Heap::Stores`, `proofs::prove`).
    pub(super) stores: bool,
    /// The captured declarations the body calls by identity, each with the function its entry checks (`Callees::captured_identities`).
    pub(super) identities: Vec<(u16, u64)>,
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
pub(super) fn link(layout: &Layout, planned: &[Planned]) -> Result<(Vec<u8>, ResumeTable, u64), String> {
    // A row's own rows run before and after it at its PC: a branch to the PC lands on the first.
    let (starts, mut parts) = (row_starts(planned), Vec::new());
    for (row, start) in planned.iter().zip(&starts) {
        parts.extend(row.pre.iter().chain(std::iter::once(row)).chain(&row.post).map(|part| (part, *start)));
    }
    let body: Vec<&Planned> = parts.iter().map(|(part, _)| *part).collect();
    let entry = entry_rows(layout, &layout.webs.entry_live(&layout.span), 0)?;
    let (g, n) = (entry.len(), body.len());
    let (guards, proven_row) = (guard_mask(&entry), entry.iter().take_while(|row| row.number.is_some()).count());
    let (mut leaves, failed) = Leaves::open(layout)?;
    let targets = parts.iter().map(|part| branch_target(layout, &mut leaves, &starts, *part, n, g)).collect::<Result<Vec<_>, _>>()?;
    let (body_exits, routines) = exit_targets(layout, &body, (g, g + n), &mut leaves)?;
    let stubs = if layout.ending == Ending::Region { resume_stubs(layout, planned, (g, &starts))? } else { Vec::new() };
    let jump = leave_row(jump_row());
    let mut holes: Vec<[u64; HOLES]> = entry.iter().map(|row| row.holes).collect();
    holes.extend(body.iter().map(|row| row.holes));
    holes.extend(std::iter::repeat_n([0; HOLES], leaves.0.len()));
    holes.extend(stubs.iter().flat_map(Stub::holes));
    let hole_fns: Vec<Box<dyn Fn(u32) -> u64 + '_>> =
        holes.iter().map(|h| Box::new(move |i: u32| h[i as usize]) as Box<dyn Fn(u32) -> u64>).collect();
    let failed_row = g + n + failed;
    let mut rows: Vec<Placed> = Vec::with_capacity(g + n + leaves.0.len());
    rows.extend(entry.iter().enumerate().map(|(k, row)| Placed { stencil: row.stencil, hole: &*hole_fns[k], taken: row.taken(failed_row), cold: false }));
    rows.extend(body.iter().enumerate().map(|(k, row)| Placed { stencil: row.stencil, hole: &*hole_fns[g + k], taken: targets[k], cold: row.cold }));
    rows.extend(leaves.0.iter().enumerate().map(|(k, (_, stencil))| Placed { stencil, hole: &*hole_fns[g + n + k], taken: None, cold: false }));
    let jumps = place_stubs(&mut rows, &stubs, &jump, &hole_fns, failed_row);
    // Behind the body: the leave rows, then each stub's rows and jump, which are the rows `place_stubs` appended.
    let exits: Vec<Vec<Target>> = entry.iter().map(|row| row.exits(failed_row)).chain(body_exits)
        .chain(std::iter::repeat_with(Vec::new).take(leaves.0.len()))
        .chain(stubs.iter().flat_map(|stub| stub.exits(failed_row)))
        .collect();
    debug_assert_eq!(exits.len(), rows.len());
    let (code, offsets, thunk) = place_rows(layout.ending, &rows, (&exits, &routines), &jumps, proven_row)?;
    Ok((code, ResumeTable::of(thunk, &stubs, &jumps, &offsets)?, guards))
}


/// Copies the rows behind the thunk, then the exit routines and each stub, and patches each
/// row's exits to their targets, `exits` by row and exit kind (a leave row's start, or a stub);
/// the code is position independent, so the buffer can move to executable memory. The rows are
/// in layout order: the hot rows first, so nothing cold sits between them. Also the code offset of
/// each row and, for a loop region, of the resume thunk (`entry_resume`), which follows the stubs;
/// each resume stub's `jumps` is patched to its row.
pub(super) fn place_rows(ending: Ending, rows: &[Placed], (exits, routines): (&[Vec<Target>], &[Vec<u8>]), jumps: &[Jump], proven_row: usize) -> Result<(Vec<u8>, Vec<usize>, usize), String> {
    let head = thunk(ending, 0, 0)?.len();
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
    let proven_at = head + starts.get(proven_row).copied().unwrap_or(0);
    let thunk_bytes = thunk(ending, head, proven_at)?;
    code[..head].copy_from_slice(&thunk_bytes);
    code[at..at + resume.len()].copy_from_slice(&resume);
    let resume_at = at;
    at += resume.len();
    code.truncate(at);
    Ok((code, starts.iter().map(|start| head + start).collect(), resume_at))
}

#[cfg(test)]
mod tests;
