//! A whole function as one stitched body: every instruction is a row the base-region decode
//! covers, a `Return` of a register, or an exit row that leaves the body at its PC with the
//! registers canonical, where the interpreter resumes exactly. The rows, webs and homes are the
//! loop regions' (`analyze`, `plan`, `link`); the body is entered and left as a lane body is
//! (`Ending::Body`).
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

/// The row of the instruction at `pc`: what the loop regions' decode covers, a `Return` of a
/// register, or an exit. `ToNumeric` is no copy here (a loop's entry guards prove the Number it
/// copies; a function's registers are not proved), and a conditional jump back has no poll, so
/// both leave.
fn cover(region: &LoopRegion, pc: u32, registers: u16, instruction: WideInstruction) -> Decoded {
    let forward = |row: &Decoded| row.kind == Kind::Jump || row.target.is_none_or(|target| target > pc);
    let row = match instruction.op().class() {
        Class::Return if instruction.result_flags() == 0 && instruction.register_a() < registers => {
            Some(Decoded::ending(End::Return, Some(Source::Register(instruction.register_a()))))
        }
        Class::ToNumeric => None,
        _ => numeric_row(region, instruction).flatten().filter(forward),
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
        let (inside, outside) = decoded[k].successors(k, count, region);
        if !outside.is_empty() {
            return Err("a row falls off the function".to_string());
        }
        pending.extend(inside.into_iter().filter(|next| !reached[*next]));
    }
    Ok(reached)
}

/// A row that runs no code: a row control never reaches.
fn skipped() -> Planned {
    static EMPTY: Stencil = Stencil { code: Vec::new(), relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0, raw_len: 0 };
    Planned::single(&EMPTY, [0; HOLES], None, None)
}

/// A row that leaves the function: a jump to its stub, which `link` places after the rows.
fn stopping(layout: &Layout, pc: u32, row: &Decoded, end: End) -> Result<Planned, String> {
    let jump = numeric::stencil(numeric::Row { kind: Kind::Jump, dst: Place::Mem, a: Place::Mem, b: Place::Mem, speculate: true })?;
    let returns = (end == End::Return).then(|| operand(&layout.homes, row.a).1);
    Ok(Planned { stop: Some(Stop { pc, returns }), ..Planned::single(jump, [0; HOLES], None, None) })
}

fn plan_row(layout: &Layout, reprs: &Reprs, reached: bool, pc: u32, row: &Decoded) -> Result<Planned, String> {
    match (reached, row.end) {
        (false, _) => Ok(skipped()),
        (true, None) => plan(row, &layout.homes, pc, is_general(row, reprs), Kind::JumpWatch),
        (true, Some(end)) => stopping(layout, pc, row, end),
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
    let exits = decoded.iter().zip(&reached).filter(|(row, reached)| **reached && row.end == Some(End::Exit)).count();
    let loops = decoded.iter().enumerate().any(|(k, row)| row.target.is_some_and(|target| target <= k as u32));
    let covered = reached.iter().filter(|reached| **reached).count() - exits;
    Ok(Body { code: link(&layout, &planned)?, covered, exits, loops })
}
