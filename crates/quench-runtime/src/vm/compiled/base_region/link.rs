use super::*;
use quench_bjit::{call, HOLES};

/// `push imm32` and `jmp rel32`: a row's exit stub, which names its PC and the routine that
/// writes the live registers back.
pub(super) const PUSH_IMM32: u8 = 0x68;
pub(super) const JMP_REL32: u8 = 0xe9;
pub(super) const EXIT_STUB_LEN: usize = 2 * (1 + std::mem::size_of::<u32>());

/// A routine of the stencil build (`quench-bjit/stencils/entry.rs`) by name.
fn routine(name: &str) -> Result<Stencil, String> {
    quench_bjit::rustc::stencil(name).ok_or_else(|| format!("no stencil routine {name}"))
}

/// The entry thunk, its call to the first row (at `rows_at`) patched.
pub(super) fn thunk(rows_at: usize) -> Result<Vec<u8>, String> {
    let stencil = routine("entry_thunk")?;
    let mut code = stencil.code.clone();
    for reloc in &stencil.relocs {
        let at = reloc.offset as usize;
        let displacement = rows_at as i64 + reloc.addend - at as i64;
        code[at..at + std::mem::size_of::<i32>()].copy_from_slice(&(displacement as i32).to_le_bytes());
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
    pub(super) fn spills(&self, pc: u32) -> Vec<(Place, Register)> {
        let doubles = self.live_floats(pc).into_iter().map(|(pin, register)| (Place::PinF(pin as u8), register));
        doubles.chain(self.live_pins(pc).into_iter().map(|(pin, register)| (Place::Pin(pin as u8), register))).collect()
    }

    /// The routine a row's exit stub jumps to: it writes the live pinned registers back and returns the PC the stub pushed.
    pub(super) fn routine(&self, pc: u32) -> Result<Vec<u8>, String> {
        leave_chain(&self.spills(pc), "leave_pop", 0)
    }
}

/// The entry rows: each pinned register live at the header loads its int32, each int slot is
/// checked (or, dead there, tagged), and each other Number register live there is checked, so a
/// loop whose values are no Numbers is never entered.
pub(super) fn entry_rows(layout: &Layout) -> Result<Vec<(&'static Stencil, [u64; HOLES])>, String> {
    let region = layout.region;
    let guard = |kind, dst, offset: u64| {
        numeric::stencil(numeric::Row { kind, dst, a: Place::Mem, b: Place::Mem, speculate: true }).map(|stencil| {
            (stencil, if kind == Kind::TagInit { [offset, 0, 0] } else { [0, offset, 0] })
        })
    };
    let mut rows = Vec::new();
    let entered = layout.webs.entry_live(region);
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

/// A leave row of the cold area, behind the body: what it returns and what it writes back first.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Leave {
    /// Leaves at `pc`, the live pinned registers there written back first.
    Pc(u32),
    /// Returns this word; nothing is written back (the chain's spills already ran, or no row has).
    Word(u64),
    /// `LEAVE_STATUS`.
    Status,
}

/// The leave rows, each once, in the order they were asked for.
struct Leaves(Vec<(Leave, Stencil)>);

impl Leaves {
    /// The index of `leave` among the rows, which it joins if it is new.
    fn index(&mut self, layout: &Layout, leave: Leave) -> Result<usize, String> {
        if let Some(found) = self.0.iter().position(|(candidate, _)| *candidate == leave) {
            return Ok(found);
        }
        let stencil = match leave {
            Leave::Pc(pc) => leave_row(leave_chain(&layout.spills(pc), "leave_word", u64::from(pc))?),
            Leave::Word(word) => leave_row(leave_chain(&[], "leave_word", word)?),
            Leave::Status => routine(call::LEAVE_STATUS)?,
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

/// The target of each exit of each body row, and the routines the stubs enter. `leaf_base` is the
/// index of the first leave row among the placed rows.
fn exit_targets(layout: &Layout, body: &[&Planned], leaves: &mut Leaves, leaf_base: usize) -> Result<(Vec<Vec<Target>>, Vec<Vec<u8>>), String> {
    let mut routines = Vec::new();
    let mut targets = Vec::with_capacity(body.len());
    for row in body {
        let mut row_targets = Vec::with_capacity(row.exits.len());
        for exit in &row.exits {
            row_targets.push(match *exit {
                Exit::Spilling(pc) => Target::Stub { pc, routine: routine_index(layout, &mut routines, pc)? },
                Exit::At(pc) => Target::Row(leaf_base + leaves.index(layout, Leave::Word(u64::from(pc)))?),
                Exit::Status => Target::Row(leaf_base + leaves.index(layout, Leave::Status)?),
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

/// The row each body row branches to: a row of the region, or a leave row at a target outside it.
fn branch_targets(layout: &Layout, body: &[&Planned], starts: &[usize], leaves: &mut Leaves, entry: usize) -> Result<Vec<Option<usize>>, String> {
    let (header, backedge) = (layout.header, layout.backedge);
    body.iter()
        .map(|row| match row.target {
            Some(pc) if (header..=backedge).contains(&pc) => Ok(Some(entry + starts[(pc - header) as usize])),
            Some(pc) => Ok(Some(entry + body.len() + leaves.index(layout, Leave::Pc(pc))?)),
            None => Ok(None),
        })
        .collect()
}

/// Lays the entry rows, rows and leave stubs out as one code buffer behind the entry thunk. The
/// hot code is the entry rows and the body, which fall through to their successors; everything a
/// guard, a miss or a branch out of the region reaches (the leave rows, the exit routines and
/// each exiting row's stub) is placed after it (`place_rows`).
pub(super) fn link(layout: &Layout, planned: &[Planned]) -> Result<Vec<u8>, String> {
    // A row's own rows run before and after it at its PC: a branch to the PC lands on the first.
    let body: Vec<&Planned> = planned.iter().flat_map(|row| row.pre.iter().chain(std::iter::once(row)).chain(&row.post)).collect();
    let entry = entry_rows(layout)?;
    let (g, n) = (entry.len(), body.len());
    // The first leave rows: the fall-through past the backedge, then the failed entry (nothing to write back).
    let mut leaves = Leaves(Vec::new());
    leaves.index(layout, Leave::Pc(layout.backedge + 1))?;
    leaves.index(layout, Leave::Word(super::super::LaneExit::Entry.word()))?;
    let targets = branch_targets(layout, &body, &row_starts(planned), &mut leaves, g)?;
    let (body_exits, routines) = exit_targets(layout, &body, &mut leaves, g + n)?;
    let mut holes: Vec<[u64; HOLES]> = entry.iter().map(|(_, holes)| *holes).collect();
    holes.extend(body.iter().map(|row| row.holes));
    holes.extend(std::iter::repeat_n([0; HOLES], leaves.0.len()));
    let hole_fns: Vec<Box<dyn Fn(u32) -> u64 + '_>> =
        holes.iter().map(|h| Box::new(move |i: u32| h[i as usize]) as Box<dyn Fn(u32) -> u64>).collect();
    let mut rows: Vec<Placed> = Vec::with_capacity(g + n + leaves.0.len());
    rows.extend(entry.iter().enumerate().map(|(k, (stencil, _))| Placed { stencil, hole: &*hole_fns[k], taken: Some(g + n + 1) }));
    rows.extend(body.iter().enumerate().map(|(k, row)| Placed { stencil: row.stencil, hole: &*hole_fns[g + k], taken: targets[k] }));
    rows.extend(leaves.0.iter().enumerate().map(|(k, (_, stencil))| Placed { stencil, hole: &*hole_fns[g + n + k], taken: None }));
    let exits: Vec<Vec<Target>> = std::iter::repeat_with(Vec::new).take(g).chain(body_exits).chain(std::iter::repeat_with(Vec::new).take(leaves.0.len())).collect();
    place_rows(&rows, &exits, &routines)
}

/// Writes the exit stub of `pc` at `at`: it pushes the PC and jumps to the routine at `routine_at`.
fn write_stub(code: &mut [u8], at: usize, pc: u32, routine_at: usize) {
    let displacement = routine_at as i64 - (at + EXIT_STUB_LEN) as i64;
    let stub: Vec<u8> = [&[PUSH_IMM32][..], &pc.to_le_bytes(), &[JMP_REL32], &(displacement as i32).to_le_bytes()].concat();
    code[at..at + EXIT_STUB_LEN].copy_from_slice(&stub);
}

/// Copies the rows behind the thunk, then the exit routines and each stub, and patches each
/// row's exits to their targets, `exits` by row and exit kind (a leave row's start, or a stub);
/// the code is position independent, so the buffer can move to executable memory. The rows are
/// in layout order: the hot rows first, so nothing cold sits between them.
pub(super) fn place_rows(rows: &[Placed], exits: &[Vec<Target>], routines: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    let head = thunk(0)?.len();
    let bound: usize = rows.iter().map(|row| row.stencil.code.len()).sum();
    let stubs = exits.iter().flatten().filter(|target| matches!(target, Target::Stub { .. })).count();
    let tail: usize = routines.iter().map(Vec::len).sum::<usize>() + stubs * EXIT_STUB_LEN;
    let mut code = vec![0u8; head + bound + tail];
    let base = code.as_mut_ptr();
    // SAFETY: `base + head` has room for every stencil's full length, which bounds the rows.
    let (len, starts) = unsafe { quench_bjit::copy_patch(rows, base.add(head), |_, _| base as usize) };
    let mut at = head + len;
    let mut routine_at = Vec::with_capacity(routines.len());
    for routine in routines {
        code[at..at + routine.len()].copy_from_slice(routine);
        routine_at.push(at);
        at += routine.len();
    }
    let mut addresses: Vec<Vec<usize>> = Vec::with_capacity(exits.len());
    for row_exits in exits {
        let mut row_addresses = Vec::with_capacity(row_exits.len());
        for target in row_exits {
            row_addresses.push(match *target {
                Target::Row(row) => base as usize + head + starts[row],
                Target::Stub { pc, routine } => {
                    write_stub(&mut code, at, pc, routine_at[routine]);
                    at += EXIT_STUB_LEN;
                    base as usize + at - EXIT_STUB_LEN
                }
            });
        }
        addresses.push(row_addresses);
    }
    // SAFETY: as above; the stubs lie after the copied code inside `code`.
    unsafe { quench_bjit::copy_patch(rows, base.add(head), |row, kind| addresses[row].get(kind as usize).copied().unwrap_or(base as usize)) };
    let thunk_bytes = thunk(head)?;
    code[..head].copy_from_slice(&thunk_bytes);
    code.truncate(at);
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_bjit::Reloc;

    /// A row that is one `jmp rel32` through its first exit hole.
    fn exiting() -> Stencil {
        let exit = Reloc { offset: 1, pc_relative: true, namespace: 0, index: quench_bjit::EXIT, addend: -(std::mem::size_of::<i32>() as i64) };
        Stencil { code: vec![JMP_REL32, 0, 0, 0, 0], relocs: vec![exit], imm32: Vec::new(), next_jump: None, tail_len: 0, raw_len: 5 }
    }

    /// Where the row at `at` of `code` jumps to.
    fn jump_target(code: &[u8], at: usize) -> usize {
        let field = at + 1;
        let displacement = i32::from_le_bytes(code[field..field + std::mem::size_of::<i32>()].try_into().expect("a displacement"));
        (field as i64 + std::mem::size_of::<i32>() as i64 + i64::from(displacement)) as usize
    }

    /// A row's stub and a leave row lie after every hot row, the stub after the routines, and each
    /// exit reaches its own target.
    #[test]
    fn exits_land_in_the_cold_area_behind_the_hot_rows() {
        let (hot, leave) = (exiting(), leave_row(vec![quench_bjit::RET]));
        let hole = |_: u32| 0u64;
        let rows = [Placed { stencil: &hot, hole: &hole, taken: None }, Placed { stencil: &hot, hole: &hole, taken: None }, Placed { stencil: &leave, hole: &hole, taken: None }];
        let exits = [vec![Target::Stub { pc: 7, routine: 0 }], vec![Target::Row(2)], Vec::new()];
        let routines = [vec![quench_bjit::RET]];
        let code = place_rows(&rows, &exits, &routines).expect("a layout");
        let head = thunk(0).expect("a thunk").len();
        let (second, leave_at) = (head + hot.code.len(), head + 2 * hot.code.len());
        let stub = leave_at + leave.code.len() + routines[0].len();
        assert_eq!(jump_target(&code, head), stub, "the first row's exit is its stub");
        assert_eq!(jump_target(&code, second), leave_at, "the second row's exit is the leave row");
        assert_eq!(code.len(), stub + EXIT_STUB_LEN);
        assert_eq!(code[stub], PUSH_IMM32);
    }
}
