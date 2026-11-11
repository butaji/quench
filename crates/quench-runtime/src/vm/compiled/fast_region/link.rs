use super::*;
use quench_fastjit::HOLES;

/// `mov r11d, reg32` for the pinned registers rdx, rcx, r8 and r9: the raw int32 to tag.
pub(super) const PIN_TO_R11: [[u8; 3]; PINNED] = [[0x41, 0x89, 0xd3], [0x41, 0x89, 0xcb], [0x45, 0x89, 0xc3], [0x45, 0x89, 0xcb]];
/// `movabs r10, imm64` (the int tag), `or r11, r10`, and `mov [rdi+disp32], r11`.
pub(super) const MOVABS_R10: [u8; 2] = [0x49, 0xba];
pub(super) const OR_R11_R10: [u8; 3] = [0x4d, 0x09, 0xd3];
pub(super) const STORE_R11: [u8; 3] = [0x4c, 0x89, 0x9f];
/// The int tag a pinned int32 takes back into a register slot.
pub(super) const INT_TAG: u64 = 0x7ffd_0000_0000_0000;
/// The callee-saved registers the thunk preserves (stencils of the tail convention save none):
/// push rbp, rbx, r12-r15 and r8 (the result pointer), in order.
pub(super) const PROLOGUE: [u8; 12] = [0x55, 0x53, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x41, 0x50];
/// `mov rsi, rdi; mov rdi, rcx`: the entry ABI's context and registers to the stencil convention.
pub(super) const MOVE_ARGUMENTS: [u8; 6] = [0x48, 0x89, 0xfe, 0x48, 0x89, 0xcf];
pub(super) const CALL_OPCODE: u8 = 0xe8;
/// `pop r8; mov [r8], rax; pop r15..r12, rbx, rbp; xor eax, eax; ret`.
pub(super) const EPILOGUE: [u8; 18] =
    [0x41, 0x58, 0x49, 0x89, 0x00, 0x41, 0x5f, 0x41, 0x5e, 0x41, 0x5d, 0x41, 0x5c, 0x5b, 0x5d, 0x31, 0xc0, 0xc3];
pub(super) const MOV_EAX: u8 = 0xb8;
pub(super) const MOVABS_RAX: [u8; 2] = [0x48, 0xb8];

/// The entry thunk: saves the callee-saved registers, calls the first row at `rows_at`, stores
/// the word the chain returned through the result pointer and returns status 0.
pub(super) fn thunk(rows_at: usize) -> Vec<u8> {
    let mut code = PROLOGUE.to_vec();
    code.extend(MOVE_ARGUMENTS);
    code.push(CALL_OPCODE);
    let call_end = code.len() + std::mem::size_of::<i32>();
    code.extend((rows_at as i64 - call_end as i64).to_le_bytes()[..std::mem::size_of::<i32>()].iter());
    code.extend(EPILOGUE);
    code
}

/// The code that tags the pinned registers back into the register file.
pub(super) fn tag_back(pins: &[Pin]) -> Vec<u8> {
    let mut code = Vec::new();
    if !pins.is_empty() {
        code.extend(MOVABS_R10);
        code.extend(INT_TAG.to_le_bytes());
    }
    for (pin, register) in pins {
        code.extend(PIN_TO_R11[*pin]);
        code.extend(OR_R11_R10);
        code.extend(STORE_R11);
        code.extend((u32::from(*register) * SLOT as u32).to_le_bytes());
    }
    code
}

/// A stub that tags the pinned registers back into the register file and returns `word`.
pub(super) fn stub(pins: &[Pin], word: u64) -> Vec<u8> {
    let mut code = tag_back(pins);
    match u32::try_from(word) {
        Ok(small) => {
            code.push(MOV_EAX);
            code.extend(small.to_le_bytes());
        }
        Err(_) => {
            code.extend(MOVABS_RAX);
            code.extend(word.to_le_bytes());
        }
    }
    code.push(quench_fastjit::RET);
    code
}

/// `pop rax; ret`: the shared exit routine returns the PC its stub pushed.
pub(super) const POP_RET: [u8; 2] = [0x58, quench_fastjit::RET];
/// `push imm32` and `jmp rel32`: a row's exit stub, which names its PC and the routine that
/// writes the live registers back.
pub(super) const PUSH_IMM32: u8 = 0x68;
pub(super) const JMP_REL32: u8 = 0xe9;
pub(super) const EXIT_STUB_LEN: usize = 2 * (1 + std::mem::size_of::<u32>());

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

    /// The code that leaves with `word`: spills the live pinned doubles (canonical words), tags
    /// the live pinned int32s back, and returns.
    pub(super) fn exit_code(&self, pc: u32, word: u64) -> Result<Vec<u8>, String> {
        self.chain(pc, stub(&self.live_pins(pc), word))
    }

    /// The routine a row's exit stub jumps to: as `exit_code`, returning the PC on the stack.
    pub(super) fn routine(&self, pc: u32) -> Result<Vec<u8>, String> {
        let mut tail = tag_back(&self.live_pins(pc));
        tail.extend(POP_RET);
        self.chain(pc, tail)
    }

    pub(super) fn chain(&self, pc: u32, ending: Vec<u8>) -> Result<Vec<u8>, String> {
        let tail = leave_row(ending);
        let spills = self.live_floats(pc);
        let mut chain = Vec::new();
        for (pin, register) in &spills {
            let row = numeric::Row { kind: Kind::Spill, dst: Place::Mem, a: Place::PinF(*pin as u8), b: Place::Mem, speculate: false };
            chain.push((numeric::stencil(row)?, [u64::from(*register) * SLOT, 0, 0]));
        }
        let holes: Vec<_> = chain.iter().map(|(_, holes)| *holes).chain([[0; HOLES]]).collect();
        let hole_fns: Vec<Box<dyn Fn(u32) -> u64 + '_>> = holes.iter().map(|h| Box::new(move |i: u32| h[i as usize]) as Box<dyn Fn(u32) -> u64>).collect();
        let rows: Vec<Placed> = chain
            .iter()
            .map(|(stencil, _)| *stencil)
            .chain([&tail])
            .enumerate()
            .map(|(k, stencil)| Placed { stencil, hole: &*hole_fns[k], taken: None })
            .collect();
        let mut code = vec![0u8; rows.iter().map(|row| row.stencil.code.len()).sum()];
        // SAFETY: `code` has room for every stencil's full length, which bounds the chain.
        let (len, _) = unsafe { quench_fastjit::copy_patch(&rows, code.as_mut_ptr(), |_, _| 0) };
        code.truncate(len);
        Ok(code)
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

/// Lays the entry rows, rows and leave stubs out as one code buffer behind the entry thunk.
pub(super) fn link(layout: &Layout, planned: &[Planned]) -> Result<Vec<u8>, String> {
    let (header, backedge) = (layout.header, layout.backedge);
    // A row's own rows run before and after it at its PC: a branch to the PC lands on the first.
    let body: Vec<&Planned> = planned.iter().flat_map(|row| row.pre.iter().chain(std::iter::once(row)).chain(&row.post)).collect();
    let starts = row_starts(planned);
    let entry = entry_rows(layout)?;
    // Leave stubs: the fall-through past the backedge, the failed entry (nothing to write
    // back), then each branch target outside the region.
    let mut leaves: Vec<(u64, Vec<u8>)> = vec![
        (u64::from(backedge) + 1, layout.exit_code(backedge + 1, u64::from(backedge) + 1)?),
        (super::super::LaneExit::Entry.word(), stub(&[], super::super::LaneExit::Entry.word())),
    ];
    let (g, n) = (entry.len(), body.len());
    let mut targets: Vec<Option<usize>> = Vec::with_capacity(n);
    for row in &body {
        targets.push(match row.target {
            Some(pc) if (header..=backedge).contains(&pc) => Some(g + starts[(pc - header) as usize]),
            Some(pc) => Some(g + n + leaf_index(layout, &mut leaves, pc)?),
            None => None,
        });
    }
    let leave_stencils: Vec<Stencil> = leaves.into_iter().map(|(_, code)| leave_row(code)).collect();
    let mut holes: Vec<[u64; HOLES]> = entry.iter().map(|(_, holes)| *holes).collect();
    holes.extend(body.iter().map(|row| row.holes));
    holes.extend(std::iter::repeat_n([0; HOLES], leave_stencils.len()));
    let hole_fns: Vec<Box<dyn Fn(u32) -> u64 + '_>> =
        holes.iter().map(|h| Box::new(move |i: u32| h[i as usize]) as Box<dyn Fn(u32) -> u64>).collect();
    let mut rows: Vec<Placed> = Vec::with_capacity(g + n + leave_stencils.len());
    rows.extend(entry.iter().enumerate().map(|(k, (stencil, _))| Placed { stencil, hole: &*hole_fns[k], taken: Some(g + n + 1) }));
    rows.extend(body.iter().enumerate().map(|(k, row)| Placed { stencil: row.stencil, hole: &*hole_fns[g + k], taken: targets[k] }));
    rows.extend(leave_stencils.iter().enumerate().map(|(k, stencil)| Placed { stencil, hole: &*hole_fns[g + n + k], taken: None }));
    let (exits, routines) = exit_routines(layout, &body, g)?;
    place_rows(&rows, &exits, &routines)
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
pub(super) fn place_rows(rows: &[Placed], exits: &[(usize, u32, usize)], routines: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    let head = thunk(0).len();
    let bound: usize = rows.iter().map(|row| row.stencil.code.len()).sum();
    let tail: usize = routines.iter().map(Vec::len).sum::<usize>() + exits.len() * EXIT_STUB_LEN;
    let mut code = vec![0u8; head + bound + tail];
    let base = code.as_mut_ptr();
    // SAFETY: `base + head` has room for every stencil's full length, which bounds the rows.
    let (len, _) = unsafe { quench_fastjit::copy_patch(rows, base.add(head), |_, _| base as usize) };
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
        let displacement = routine_at[*routine] as i64 - (at + EXIT_STUB_LEN) as i64;
        let stub: Vec<u8> = [&[PUSH_IMM32][..], &pc.to_le_bytes(), &[JMP_REL32], &(displacement as i32).to_le_bytes()].concat();
        code[at..at + EXIT_STUB_LEN].copy_from_slice(&stub);
        at += EXIT_STUB_LEN;
    }
    // SAFETY: as above; the stubs lie after the copied code inside `code`.
    unsafe { quench_fastjit::copy_patch(rows, base.add(head), |row, _| addresses[row]) };
    let thunk_bytes = thunk(head);
    code[..head].copy_from_slice(&thunk_bytes);
    code.truncate(at);
    Ok(code)
}
