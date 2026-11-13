use super::*;
use quench_bjit::{exit_stub, padded_holes, patch_successor, EXIT_STUB_LEN, HOLES};

/// `jmp rel32`: a resume stub's last row, which jumps to its PC's first row.
pub(super) use quench_bjit::stub::JMP_REL32;
pub(super) const JUMP_LEN: usize = 1 + std::mem::size_of::<u32>();

/// A routine of the stencil build (`quench-bjit/stencils/entry.rs`) by name.
pub(super) fn routine(name: &str) -> Result<Stencil, String> {
    quench_bjit::rustc::stencil(name).ok_or_else(|| format!("no stencil routine {name}"))
}

/// The entry thunk, its call to the first row (at `rows_at`) patched.
pub(super) fn thunk(rows_at: usize) -> Result<Vec<u8>, String> {
    let stencil = routine("entry_thunk")?;
    let mut code = stencil.code.clone();
    for reloc in &stencil.relocs {
        let at = reloc.offset as usize;
        patch_successor(&mut code, at, reloc.class, rows_at as i64 + reloc.addend - at as i64).map_err(|error| format!("entry thunk: {error:?}"))?;
    }
    Ok(code)
}

/// The code that writes `spills` back to the register file (a pinned double as a canonical
/// word, a pinned int32 tagged) and ends in the leave routine `tail`, given `word` in hole 0.
pub(super) fn leave_chain(spills: &[(Place, Register)], tail: &str, word: u64) -> Result<Vec<u8>, String> {
    let mut chain = Vec::new();
    for (place, register) in spills {
        let row = numeric::Row { kind: Kind::Spill, dst: Place::Mem, a: *place, b: Place::Mem, speculate: false };
        chain.push((numeric::stencil(row)?.clone(), [u64::from(*register) * SLOT, 0, 0]));
    }
    chain.push((routine(tail)?, [word, 0, 0]));
    let hole_fns: Vec<Box<dyn Fn(u32) -> u64 + '_>> = chain.iter().map(|(_, h)| Box::new(move |i: u32| h[i as usize]) as Box<dyn Fn(u32) -> u64>).collect();
    let rows: Vec<Placed> = chain.iter().enumerate().map(|(k, (stencil, _))| Placed { stencil, hole: &*hole_fns[k], taken: None }).collect();
    let mut code = vec![0u8; rows.iter().map(|row| row.stencil.code.len()).sum()];
    // SAFETY: `code` has room for every stencil's full length, which bounds the chain.
    let (len, _) = unsafe { quench_bjit::copy_patch(&rows, code.as_mut_ptr(), |_, _| 0) };
    code.truncate(len);
    Ok(code)
}

/// What the linker needs of the region: its span, the registers live at a PC, and the entry
/// loads.
pub(super) struct Layout<'a> {
    pub(super) header: u32,
    pub(super) backedge: u32,
    pub(super) homes: Homes,
    pub(super) webs: Webs,
    pub(super) region: &'a LoopRegion,
}

impl Layout<'_> {
    /// The pinned registers live at `pc`: the ones a stub there writes back.
    pub(super) fn live_pins(&self, pc: u32) -> Vec<Pin> {
        self.live_homes(&self.homes.pins, pc)
    }

    pub(super) fn live_floats(&self, pc: u32) -> Vec<Pin> {
        self.live_homes(&self.homes.floats, pc)
    }

    /// The pins of `homes` whose web is live at `pc`, each with the register it writes back to.
    fn live_homes(&self, homes: &[Pin], pc: u32) -> Vec<Pin> {
        let live = self.webs.live_at(self.region, pc);
        homes.iter().filter(|(_, web)| live.contains(&usize::from(*web))).map(|(pin, web)| (*pin, self.homes.orig[usize::from(*web)])).collect()
    }

    /// The registers a leave at `pc` writes back: the live pinned doubles, then the ints.
    fn spills(&self, pc: u32) -> Vec<(Place, Register)> {
        let doubles = self.live_floats(pc).into_iter().map(|(pin, register)| (Place::PinF(pin as u8), register));
        doubles.chain(self.live_pins(pc).into_iter().map(|(pin, register)| (Place::Pin(pin as u8), register))).collect()
    }

    /// The code that leaves with `word`: writes the live pinned registers back, and returns.
    pub(super) fn exit_code(&self, pc: u32, word: u64) -> Result<Vec<u8>, String> {
        leave_chain(&self.spills(pc), "leave_word", word)
    }

    /// The routine a row's exit stub jumps to: as `exit_code`, returning the PC on the stack.
    pub(super) fn routine(&self, pc: u32) -> Result<Vec<u8>, String> {
        leave_chain(&self.spills(pc), "leave_pop", 0)
    }
}

/// The entry rows: each pinned register live at the header (or, for a resume stub, at its PC; `entered` are the webs live there) loads its int32, each int slot is
/// checked (or, dead there, tagged), and each other Number register live there is checked, so a
/// loop whose values are no Numbers is never entered.
pub(super) fn entry_rows(layout: &Layout, entered: &[usize]) -> Result<Vec<(&'static Stencil, [u64; HOLES])>, String> {
    let region = layout.region;
    let guard = |kind, dst, offset: u64| {
        numeric::stencil(numeric::Row { kind, dst, a: Place::Mem, b: Place::Mem, speculate: true }).map(|stencil| {
            (stencil, padded_holes(if kind == Kind::TagInit { [offset, 0, 0] } else { [0, offset, 0] }))
        })
    };
    let mut rows = Vec::new();
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
        let number = region.number_registers().contains(&layout.homes.orig[usize::from(web)]);
        if number && layout.homes.place(web) == Place::Mem {
            rows.push(guard(Kind::Guard, Place::Mem, slot(web))?);
        }
    }
    Ok(rows)
}

/// One routine per set of live pinned registers; each exiting row's stub names its PC. The
/// exits are (row index, pc, routine index) with rows counted from the entry rows' `first`.
fn exit_routines(layout: &Layout, body: &[&Planned], first: usize) -> Result<(Vec<(usize, u32, usize)>, Vec<Vec<u8>>), String> {
    let mut routines: Vec<(Vec<Pin>, Vec<Pin>, Vec<u8>)> = Vec::new();
    let mut exits: Vec<(usize, u32, usize)> = Vec::new();
    for (k, row) in body.iter().enumerate() {
        let Some(pc) = row.exit_pc else { continue };
        let (ints, floats) = (layout.live_pins(pc), layout.live_floats(pc));
        let found = routines.iter().position(|(i, f, _)| *i == ints && *f == floats);
        let at = match found {
            Some(at) => at,
            None => {
                routines.push((ints, floats, layout.routine(pc)?));
                routines.len() - 1
            }
        };
        exits.push((first + k, pc, at));
    }
    Ok((exits, routines.into_iter().map(|(_, _, code)| code).collect()))
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

/// The hole values of the chain's rows, as the closures `copy_patch` reads them through.
type HoleFns<'a> = Vec<Box<dyn Fn(u32) -> u64 + 'a>>;

/// The leave stubs (the fall-through past the backedge, the failed entry with nothing to write
/// back, then each branch target outside the region) and each body row's branch target row.
fn branch_targets(layout: &Layout, body: &[&Planned], starts: &[usize], g: usize) -> Result<(Vec<(u64, Vec<u8>)>, Vec<Option<usize>>), String> {
    let (header, backedge) = (layout.header, layout.backedge);
    let entered = super::super::LaneExit::Entry.word();
    let mut leaves = vec![(u64::from(backedge) + 1, layout.exit_code(backedge + 1, u64::from(backedge) + 1)?), (entered, leave_chain(&[], "leave_word", entered)?)];
    let mut targets: Vec<Option<usize>> = Vec::with_capacity(body.len());
    for row in body {
        targets.push(match row.target {
            Some(pc) if (header..=backedge).contains(&pc) => Some(g + starts[(pc - header) as usize]),
            Some(pc) => Some(g + body.len() + leaf_index(layout, &mut leaves, pc)?),
            None => None,
        });
    }
    Ok((leaves, targets))
}

/// Lays the entry rows, rows, leave stubs and resume stubs out as one code buffer behind the
/// entry thunk, with the table of the resume stubs and the cache entries a miss may re-patch.
pub(super) fn link(layout: &Layout, planned: &[Planned]) -> Result<(Vec<u8>, ResumeTable, CacheSites), String> {
    // A row's own rows run before and after it at its PC: a branch to the PC lands on the first.
    let body: Vec<&Planned> = planned.iter().flat_map(|row| row.pre.iter().chain(std::iter::once(row)).chain(&row.post)).collect();
    let starts = row_starts(planned);
    let entry = entry_rows(layout, &layout.webs.entry_live(layout.region))?;
    let (g, n) = (entry.len(), body.len());
    let (leaves, targets) = branch_targets(layout, &body, &starts, g)?;
    let leave_stencils: Vec<Stencil> = leaves.into_iter().map(|(_, code)| leave_row(code)).collect();
    let jump = leave_row([&[JMP_REL32][..], &[0; JUMP_LEN - 1]].concat());
    let stubs = resume_stubs(layout, g, &starts)?;
    let mut holes: Vec<[u64; HOLES]> = entry.iter().map(|(_, holes)| *holes).collect();
    holes.extend(body.iter().map(|row| row.holes));
    holes.extend(std::iter::repeat_n([0; HOLES], leave_stencils.len()));
    holes.extend(stubs.iter().flat_map(Stub::holes));
    let hole_fns: HoleFns<'_> = holes.iter().map(|h| Box::new(move |i: u32| h[i as usize]) as Box<dyn Fn(u32) -> u64>).collect();
    let mut rows: Vec<Placed> = Vec::with_capacity(g + n + leave_stencils.len());
    rows.extend(entry.iter().enumerate().map(|(k, (stencil, _))| Placed { stencil, hole: &*hole_fns[k], taken: Some(g + n + 1) }));
    rows.extend(body.iter().enumerate().map(|(k, row)| Placed { stencil: row.stencil, hole: &*hole_fns[g + k], taken: targets[k] }));
    rows.extend(leave_stencils.iter().enumerate().map(|(k, stencil)| Placed { stencil, hole: &*hole_fns[g + n + k], taken: None }));
    let jumps = place_stubs(&mut rows, &stubs, &jump, &hole_fns, g + n + 1);
    let (exits, routines) = exit_routines(layout, &body, g)?;
    let (code, offsets, thunk) = place_rows(&rows, &exits, &routines, &jumps)?;
    Ok((code, ResumeTable::of(thunk, &stubs, &jumps, &offsets)?, CacheSites::collect(&body, g, &offsets)))
}

pub(super) fn leaf_index(layout: &Layout, leaves: &mut Vec<(u64, Vec<u8>)>, pc: u32) -> Result<usize, String> {
    let word = u64::from(pc);
    if let Some(found) = leaves.iter().position(|(candidate, _)| *candidate == word) {
        return Ok(found);
    }
    leaves.push((word, layout.exit_code(pc, word)?));
    Ok(leaves.len() - 1)
}

/// Copies the rows behind the thunk, then the exit routines and each row's stub, patching each
/// row's exit to its stub; the code is position independent, so the buffer can move to
/// executable memory.
/// Also the code offset of each row and of the resume thunk (`entry_resume`), which follows
/// the stubs; each resume stub's jump is patched to its row.
pub(super) fn place_rows(rows: &[Placed], exits: &[(usize, u32, usize)], routines: &[Vec<u8>], jumps: &[Jump]) -> Result<(Vec<u8>, Vec<usize>, usize), String> {
    let head = thunk(0)?.len();
    let resume = routine("entry_resume")?.code;
    let bound: usize = rows.iter().map(|row| row.stencil.code.len()).sum();
    let tail: usize = routines.iter().map(Vec::len).sum::<usize>() + exits.len() * EXIT_STUB_LEN + resume.len();
    let mut code = vec![0u8; head + bound + tail];
    let base = code.as_mut_ptr();
    // SAFETY: `base + head` has room for every stencil's full length, which bounds the rows.
    let (len, _) = unsafe { quench_bjit::copy_patch(rows, base.add(head), |_, _| base as usize) };
    let mut at = head + len;
    let mut routine_at = Vec::with_capacity(routines.len());
    for routine in routines {
        code[at..at + routine.len()].copy_from_slice(routine);
        routine_at.push(at);
        at += routine.len();
    }
    let mut addresses = vec![base as usize; rows.len()];
    for (row, pc, routine) in exits {
        addresses[*row] = base as usize + at;
        let stub = exit_stub(*pc, routine_at[*routine] as i64 - at as i64).map_err(|error| format!("exit stub: {error:?}"))?;
        code[at..at + EXIT_STUB_LEN].copy_from_slice(&stub);
        at += EXIT_STUB_LEN;
    }
    // SAFETY: as above; the stubs lie after the copied code inside `code`.
    let (_, starts) = unsafe { quench_bjit::copy_patch(rows, base.add(head), |row, _| addresses[row]) };
    for jump in jumps {
        let displacement = starts[jump.target] as i64 - (starts[jump.row] + JUMP_LEN) as i64;
        let operand = head + starts[jump.row] + 1;
        code[operand..operand + JUMP_LEN - 1].copy_from_slice(&(displacement as i32).to_le_bytes());
    }
    let thunk_bytes = thunk(head)?;
    code[..head].copy_from_slice(&thunk_bytes);
    code[at..at + resume.len()].copy_from_slice(&resume);
    let resume_at = at;
    at += resume.len();
    code.truncate(at);
    Ok((code, starts.iter().map(|start| head + start).collect(), resume_at))
}
