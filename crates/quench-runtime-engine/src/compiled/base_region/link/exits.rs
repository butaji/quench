//! The leave and exit machinery of a linked region: the leave rows (`Leaves`), where each exit of a row goes
//! (`exit_targets`), and the exit stubs and their routines written behind the rows (`exit_addresses`).

use super::*;
use quench_bjit::{EXIT_STUB_LEN, exit_stub};

/// A leave row of the cold area, behind the body: what it returns and what it writes back first.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Leave {
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
    pub(super) fn leave(self) -> Leave {
        self.returns.map_or(Leave::Pc(self.pc), |slot| Leave::Return { pc: self.pc, slot })
    }
}

/// The leave rows, each once, in the order they were asked for.
pub(super) struct Leaves(pub(super) Vec<(Leave, Stencil)>);

impl Leaves {
    /// The leave rows every link starts with, and the index of the failed entry's (nothing to
    /// write back).
    pub(super) fn open(layout: &Layout) -> Result<(Self, usize), String> {
        let mut leaves = Self(Vec::new());
        let failed = leaves.index(layout, Leave::Word(crate::compiled::LaneExit::Entry.word()))?;
        Ok((leaves, failed))
    }

    /// The index of `leave` among the rows, which it joins if it is new.
    pub(super) fn index(&mut self, layout: &Layout, leave: Leave) -> Result<usize, String> {
        if let Some(found) = self.0.iter().position(|(candidate, _)| *candidate == leave) {
            return Ok(found);
        }
        let ending = layout.ending;
        let stencil = match leave {
            Leave::Pc(pc) => leave_row(leave_chain(&layout.spills(pc), layout.copies(pc), ending.tails().0, ending.holes(layout.exit_word(pc)))?),
            Leave::Word(word) => leave_row(leave_chain(&[], &[], ending.tails().0, ending.holes(word))?),
            Leave::Status => routine(call::LEAVE_STATUS)?.clone(),
            Leave::Return { pc, slot } => {
                let status = u64::from(crate::compiled::LANE_BODY_RETURNED);
                leave_row(leave_chain(&layout.spills(pc), &[], "leave_returned", padded_holes([slot, status, u64::from(layout.record(pc))]))?)
            }
        };
        self.0.push((leave, stencil));
        Ok(self.0.len() - 1)
    }
}

/// Where an exit hole of a placed row goes: a stub in the cold area behind the leave rows, or a
/// leave row.
pub(in crate::compiled) enum Target {
    Stub { pc: u32, routine: usize },
    Row(usize),
}

/// The index of the routine of the pinned registers live at `pc`, one per set of them.
fn routine_index(layout: &Layout, routines: &mut Vec<(Vec<Pin>, Vec<Pin>, Vec<u8>)>, pc: u32) -> Result<usize, String> {
    let (ints, floats) = layout.live_pinned(pc);
    if let Some(found) = routines.iter().position(|(i, f, _)| *i == ints && *f == floats) {
        return Ok(found);
    }
    let code = layout.routine(&spill_list(&ints, &floats))?;
    routines.push((ints, floats, code));
    Ok(routines.len() - 1)
}

/// The target of each exit of each body row, and the routines the stubs enter. `entry` is the number
/// of entry rows, which precede the body, and `leaf_base` the index of the first leave row among the
/// placed rows.
pub(super) fn exit_targets(layout: &Layout, body: &[&Planned], (entry, leaf_base): (usize, usize), leaves: &mut Leaves) -> Result<(Vec<Vec<Target>>, Vec<Vec<u8>>), String> {
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

/// Writes the exit stub of `pc` at `at`: it pushes the PC and jumps to the routine at `routine_at`.
fn write_stub(code: &mut [u8], at: usize, pc: u32, routine_at: usize) -> Result<(), String> {
    let stub = exit_stub(pc, routine_at as i64 - at as i64).map_err(|error| format!("exit stub: {error:?}"))?;
    code[at..at + EXIT_STUB_LEN].copy_from_slice(&stub);
    Ok(())
}

/// Copies each of `routines` to `at`, returning where each starts and the end.
pub(super) fn append_routines(code: &mut [u8], mut at: usize, routines: &[Vec<u8>]) -> (Vec<usize>, usize) {
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
pub(super) fn exit_addresses(code: &mut [u8], (rows_at, starts): (usize, &[usize]), mut at: usize, (exits, routine_at): (&[Vec<Target>], &[usize])) -> Result<(Vec<Vec<usize>>, usize), String> {
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
