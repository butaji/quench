//! A whole function as one stitched body: every instruction is a row the base-region decode
//! covers (the numeric rows, and the heap rows read through the heap's own tables: an array
//! word's record, a field's cache entry, the activation's captures), a `Return` of a register,
//! or an exit row that leaves the body at its PC with the registers canonical, where the
//! interpreter resumes exactly. The rows, webs and homes are the loop regions' (`analyze`,
//! `plan`, `link`); the body is entered and left as a lane body is (`Ending::Body`), and polls
//! the interrupt flag at every branch back (`Ending::poll`).
use super::*;
use quench_bjit::HOLES;

/// A function's code: bytes entered at the lane body ABI, and what its rows are.
pub(in crate::vm::compiled) struct Body {
    pub(in crate::vm::compiled) code: Vec<u8>,
    /// Instructions that run as rows (a `Return` included), and those that leave the body.
    pub(in crate::vm::compiled) covered: usize,
    pub(in crate::vm::compiled) exits: usize,
    /// Whether it jumps backward: a loop pays an entry's cost over its iterations.
    pub(in crate::vm::compiled) loops: bool,
}

/// The row of the instruction at `pc`: what the loop regions' decode covers (a heap row from the
/// heap's tables), a `Return` of a register, or an exit.
fn cover(region: &LoopRegion, pc: u32, registers: u16, instruction: WideInstruction) -> Decoded {
    let plain = instruction.result_flags() == 0;
    let row = match instruction.op().class() {
        Class::Return if plain && instruction.register_a() < registers => Some(Decoded::ending(End::Return, Some(Source::Register(instruction.register_a())))),
        _ => numeric_row(region, instruction).unwrap_or_else(|| heap_row(region, Facts::Site, pc, instruction).filter(|_| plain)),
    };
    row.unwrap_or_else(|| Decoded::ending(End::Exit, None))
}

/// Which rows control reaches from PC 0; an error where one falls off the function's end.
fn reachable(region: &LoopRegion, decoded: &[Decoded]) -> Result<Vec<bool>, String> {
    let count = decoded.len();
    let (mut reached, mut pending) = (vec![false; count], vec![0]);
    while let Some(k) = pending.pop() {
        if std::mem::replace(&mut reached[k], true) {
            continue;
        }
        for next in decoded[k].successors(k, count, region) {
            match next {
                Next::Row(row) => pending.push(row),
                Next::Leave(_) => return Err("a row falls off the function".to_string()),
            }
        }
    }
    Ok(reached)
}

/// A row that runs no code: a row control never reaches.
fn skipped() -> Planned {
    static EMPTY: Stencil = Stencil { code: Vec::new(), relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0, raw_len: 0 };
    Planned::single(&EMPTY, [0; HOLES], None, None)
}

/// The stencil of a control row: it reads and writes no operand.
fn control(kind: Kind) -> Result<&'static Stencil, String> {
    numeric::stencil(numeric::Row { kind, dst: Place::Mem, a: Place::Mem, b: Place::Mem, speculate: true })
}

/// A row that leaves the function: a jump to its stub, which `link` places after the rows.
fn stopping(layout: &Layout, pc: u32, row: &Decoded, end: End) -> Result<Planned, String> {
    let returns = (end == End::Return).then(|| operand(&layout.homes, row.a).1);
    Ok(Planned { stop: Some(Stop { pc, returns }), ..Planned::single(control(Kind::Jump)?, [0; HOLES], None, None) })
}

/// A row that leaves at `pc` unless the frame slot at byte offset `slot` holds a Number.
fn number_guard(pc: u32, slot: u64) -> Result<Planned, String> {
    Ok(Planned { stop: Some(Stop { pc, returns: None }), ..Planned::single(control(Kind::Guard)?, [0, slot, 0], None, None) })
}

/// A row that leaves at `pc` when an interrupt is requested, and continues.
fn poll_row(pc: u32) -> Result<Planned, String> {
    let interrupt = std::mem::offset_of!(super::super::CallContext, interrupt) as u64;
    Ok(Planned::single(control(Kind::Poll)?, [interrupt, 0, 0], None, Some(pc)))
}

/// The rows that run before the row at `pc`, at the same PC: the proof a `ToNumeric` operand
/// held in a frame slot is a Number (a loop's entry guards prove its registers; a function's are
/// not proved), and the poll a conditional branch back takes (an unconditional one is its own
/// `JumpWatch`), so no loop a function runs goes unwatched.
fn preludes(layout: &Layout, row: &Decoded, pc: u32) -> Result<Vec<Planned>, String> {
    let (place, slot) = operand(&layout.homes, row.a);
    let mut before = Vec::new();
    if row.checked && place == Place::Mem {
        before.push(number_guard(pc, slot)?);
    }
    if row.kind != Kind::Jump && row.target.is_some_and(|target| target <= pc) {
        before.push(poll_row(pc)?);
    }
    Ok(before)
}

/// Whether the register a heap row reads its array or object through is held in its frame slot,
/// where the row reads it: a pinned register holds a Number, never one of those, and its slot is
/// not kept current.
fn framed(homes: &Homes, row: &Decoded) -> bool {
    let through = match row.heap {
        Some(Heap::Store) => row.base,
        Some(Heap::Index | Heap::Array | Heap::Site) => row.a.and_then(register_of),
        _ => None,
    };
    through.is_none_or(|register| homes.place(register) == Place::Mem)
}

fn plan_row(layout: &Layout, reprs: &Reprs, reached: bool, pc: u32, row: &Decoded) -> Result<Planned, String> {
    if !reached {
        return Ok(skipped());
    }
    match row.end {
        Some(end) => stopping(layout, pc, row, end),
        None if !framed(&layout.homes, row) => stopping(layout, pc, row, End::Exit),
        None => Ok(Planned { pre: preludes(layout, row, pc)?, ..plan(row, &layout.homes, pc, is_general(row, reprs), layout.ending)? }),
    }
}

/// The body of the function `region` spans, over the registers observed at its entry (`seen`:
/// the ints and the doubles; `widened`: the PCs whose int32 speculation failed), counted under
/// the trace. An error where its first instruction leaves at once: such a body is pure entry cost.
pub(in crate::vm::compiled) fn build_body(region: &LoopRegion, registers: u16, widened: &[usize], seen: [&[Register]; SEEN]) -> Result<Body, String> {
    let built = assemble_body(region, registers, widened, seen);
    note(|totals, rejects| match &built {
        Ok(body) => {
            totals.bodies += 1;
            totals.covered += body.covered as u64;
            totals.exits += body.exits as u64;
        }
        Err(name) => *rejects.entry(format!("body: {name}")).or_default() += 1,
    });
    built
}

fn assemble_body(region: &LoopRegion, registers: u16, widened: &[usize], seen: [&[Register]; SEEN]) -> Result<Body, String> {
    let covers: Vec<Decoded> = region.instructions().iter().enumerate().map(|(pc, instruction)| cover(region, pc as u32, registers, *instruction)).collect();
    let reached = reachable(region, &covers)?;
    if covers[0].end == Some(End::Exit) {
        return Err("first instruction leaves".to_string());
    }
    let unreached = |k: usize| !reached[k];
    let decoded: Vec<Decoded> = covers.into_iter().enumerate().map(|(k, row)| if unreached(k) { Decoded::ending(End::Exit, None) } else { row }).collect();
    let (reprs, homes, webs, decoded) = analyze(region, &decoded, widened, seen);
    let layout = Layout { header: region.header, backedge: region.backedge, homes, webs, region, ending: Ending::Body };
    let planned = decoded.iter().enumerate().map(|(k, row)| plan_row(&layout, &reprs, reached[k], k as u32, row)).collect::<Result<Vec<_>, _>>()?;
    let exits = planned.iter().filter(|row| row.stop.is_some_and(|stop| stop.returns.is_none())).count();
    let loops = decoded.iter().enumerate().any(|(k, row)| row.target.is_some_and(|target| target <= k as u32));
    let covered = reached.iter().filter(|reached| **reached).count() - exits;
    Ok(Body { code: link(&layout, &planned)?, covered, exits, loops })
}
