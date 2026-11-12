//! A whole function as one stitched body: every instruction is a row the base-region decode
//! covers (the numeric rows, the heap rows read through the heap's own tables: an array
//! word's record, a field's cache entry, the activation's captures, a helper step, a `Call`
//! the call planner chains, a call stitched in as its callee's rows, or a `Return` of a register)
//! or a leave row at its PC, where the interpreter resumes exactly. The rows are laid out as one
//! flat program (`stitch`), whose rows the webs, homes and exits are keyed by; they are the loop
//! regions' (`analyze`, `plan`, `link`); the body is entered and left as a lane body is
//! (`Ending::Body`), and polls the interrupt flag at every branch back (`Ending::poll`).
use super::*;
use super::call_plan::inline::Context;
use super::call_plan::{CallShape, DirectFacts};
use super::callees::{chains, Callees};
use super::super::LaneHelpers;
use crate::heap::GlobalFact;
use quench_bjit::call_record::CallRecord;
use quench_bjit::HOLES;

/// A function's code: bytes entered at the lane body ABI, and what its rows are.
pub(in crate::vm::compiled) struct Body {
    pub(in crate::vm::compiled) code: Vec<u8>,
    /// Instructions that run as rows (a `Return` included), and those that leave the body.
    pub(in crate::vm::compiled) covered: usize,
    pub(in crate::vm::compiled) exits: usize,
    /// Whether it jumps backward: a loop pays an entry's cost over its iterations.
    pub(in crate::vm::compiled) loops: bool,
    /// The records its direct call rows read, which its code must not outlive.
    pub(in crate::vm::compiled) records: Vec<Box<CallRecord>>,
    /// The calls it runs as their callees' rows, by frame.
    pub(in crate::vm::compiled) frames: Vec<InlinedCall>,
    /// The Stable facts it folded, which its publication subscribes.
    pub(in crate::vm::compiled) stable: Box<[GlobalFact]>,
}

/// What a body decides its call sites by: the callees they recorded and the stitch's budget.
struct Sites<'a> {
    callees: &'a Callees,
    facts: &'a [Option<DirectFacts>],
    context: Context,
    registers: u16,
}

/// The row of the instruction at `pc`: a `Return` of a register, a call the stitch inlines as its
/// callee's rows or the call planner's chain, else what the loop regions' decode covers (a leave
/// where nothing does).
fn cover(region: &LoopRegion, pc: u32, sites: &mut Sites, instruction: WideInstruction, helpers: LaneHelpers) -> What {
    let plain = instruction.result_flags() == 0;
    match instruction.op().class() {
        Class::Return if plain && instruction.register_a() < sites.registers => {
            What::Row(Decoded { a: Some(Source::Register(instruction.register_a())), ..Decoded::step(Step::Return, None) })
        }
        Class::Call if chains(instruction) => {
            let shape = CallShape::of(pc, instruction);
            match sites.callees.inlined_call(sites.facts, region, &mut sites.context, &shape) {
                Some(site) => What::Site(site),
                None => What::Row(Decoded::step(Step::Call(shape), Some(shape.result))),
            }
        }
        _ => What::Row(decode(region, Facts::Site, pc, instruction, helpers)),
    }
}

/// Which rows control reaches from row 0; an error where one falls off the program's end.
fn reachable(span: &Span, decoded: &[Decoded]) -> Result<Vec<bool>, String> {
    let count = decoded.len();
    let (mut reached, mut pending) = (vec![false; count], vec![0]);
    while let Some(k) = pending.pop() {
        if std::mem::replace(&mut reached[k], true) {
            continue;
        }
        for next in decoded[k].successors(k, count, span) {
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

/// A row that leaves at `pc` unless the frame slot at byte offset `slot` holds a Number.
fn number_guard(pc: u32, slot: u64) -> Result<Planned, String> {
    Ok(Planned { stop: Some(Stop { pc, returns: None }), ..Planned::single(control(Kind::Guard)?, [0, slot, 0], None, None) })
}

/// A row that leaves at `pc` when an interrupt is requested, and continues.
fn poll_row(pc: u32) -> Result<Planned, String> {
    let interrupt = std::mem::offset_of!(super::super::CallContext, interrupt) as u64;
    Ok(Planned::single(control(Kind::Poll)?, [interrupt, 0, 0], None, Some(pc)))
}

/// The rows that run before the row `pc`, at the same row: the proof a `ToNumeric` operand held in
/// a frame slot is a Number (a loop's entry guards prove its registers; a function's are not
/// proved), and the poll a conditional branch back takes (an unconditional one is its own
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

/// Whether the register a heap row reads its array, object or callee through is held in its frame
/// slot, where the row reads it: a pinned register holds a Number, never one of those, and its
/// slot is not kept current.
fn framed(homes: &Homes, row: &Decoded) -> bool {
    let through = match row.heap {
        Some(Heap::Store) => row.base,
        Some(Heap::Index | Heap::Array | Heap::Site | Heap::Identity) => row.a.and_then(register_of),
        _ => None,
    };
    through.into_iter().all(|register| homes.place(register) == Place::Mem)
}

/// Whether the row leaves the body: a leave step, or a heap row whose array is not in its frame slot.
fn leaves(layout: &Layout, row: &Decoded) -> bool {
    row.step == Some(Step::Leave) || !framed(&layout.homes, row)
}

/// The plan of row `pc`: nothing where control never reaches it, a leave row, or the row with the
/// rows before it its kind needs.
fn plan_row(layout: &Layout, reprs: &Reprs, reached: bool, pc: u32, row: &Decoded, helpers: LaneHelpers) -> Result<Planned, String> {
    if !reached {
        return Ok(skipped());
    }
    if leaves(layout, row) {
        return plan_step(&Decoded::step(Step::Leave, None), layout, pc, helpers);
    }
    Ok(Planned { pre: preludes(layout, row, pc)?, ..plan(row, layout, pc, is_general(row, reprs), helpers)? })
}

/// What a body is built for: its function and registers, the rows whose int32 speculation failed
/// (`widened`, by caller PC), the registers its entry held as ints and doubles (`seen`), and the
/// address of the invalidation word its method is published with (`Invalidation::address`), which
/// its code reads where a call may have changed a fact it folded.
pub(in crate::vm::compiled) struct BodySpec<'a> {
    pub(in crate::vm::compiled) function: u32,
    pub(in crate::vm::compiled) registers: u16,
    pub(in crate::vm::compiled) invalidation: usize,
    pub(in crate::vm::compiled) widened: &'a [usize],
    pub(in crate::vm::compiled) seen: [&'a [Register]; SEEN],
}

/// The body of the function `spec` names that `region` spans, over the callees its call sites
/// recorded, counted under the trace. An error where its first row leaves at once: such a body is
/// pure entry cost.
///
/// # Safety
/// Each of `callees` names its function's live entry cell (`Callees::facts`).
pub(in crate::vm::compiled) unsafe fn build_body(region: &LoopRegion, spec: &BodySpec, callees: &Callees, helpers: LaneHelpers) -> Result<Body, String> {
    // SAFETY: the caller's contract.
    let facts = unsafe { callees.facts() };
    let sites = Sites { callees, facts: &facts, context: Context::root(spec.function, spec.registers), registers: spec.registers };
    let built = assemble_body(region, spec, sites, helpers);
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

/// The rows whose int32 speculation failed: those of the caller's instructions at the `widened` PCs.
fn widened_rows(origins: &[Origin], widened: &[usize]) -> Vec<usize> {
    let widens = |origin: &Origin| matches!(origin, Origin::Caller(pc) if widened.contains(&(*pc as usize)));
    origins.iter().enumerate().filter(|(_, origin)| widens(origin)).map(|(k, _)| k).collect()
}

/// Every caller instruction as its part: its row, or its call's site.
fn parts(region: &LoopRegion, sites: &mut Sites, helpers: LaneHelpers) -> Vec<Part> {
    let instructions = region.instructions().iter().enumerate();
    instructions.map(|(pc, instruction)| Part { pc: pc as u32, what: cover(region, pc as u32, sites, *instruction, helpers) }).collect()
}

fn assemble_body(region: &LoopRegion, spec: &BodySpec, mut sites: Sites, helpers: LaneHelpers) -> Result<Body, String> {
    let mut stitched = stitch(region, parts(region, &mut sites, helpers), spec.registers, spec.invalidation);
    let span = Span::stitched(region, stitched.rows.len(), &stitched.live);
    let reached = reachable(&span, &stitched.rows)?;
    if stitched.rows[0].step == Some(Step::Leave) {
        return Err("first instruction leaves".to_string());
    }
    let unreached = |row: Decoded, reached: &bool| if *reached { row } else { Decoded::step(Step::Leave, None) };
    let rows: Vec<Decoded> = stitched.rows.into_iter().zip(&reached).map(|(row, reached)| unreached(row, reached)).collect();
    let (reprs, homes, webs, decoded) = analyze(&span, &rows, &widened_rows(&stitched.origins, spec.widened), spec.seen);
    let layout = Layout { span, homes, webs, ending: Ending::Body, callees: sites.facts, origins: &stitched.origins, copies: &stitched.copies };
    let mut planned = decoded.iter().enumerate().map(|(k, row)| plan_row(&layout, &reprs, reached[k], k as u32, row, helpers)).collect::<Result<Vec<_>, _>>()?;
    let code = link(&layout, &planned)?;
    let exits = decoded.iter().enumerate().filter(|(k, row)| reached[*k] && leaves(&layout, row)).count();
    let loops = decoded.iter().enumerate().any(|(k, row)| row.target.is_some_and(|target| target <= k as u32));
    let covered = reached.iter().filter(|reached| **reached).count() - exits;
    let records = planned.iter_mut().filter_map(|row| row.owned.take()).collect();
    stitched.facts.sort_unstable();
    stitched.facts.dedup();
    Ok(Body { code, covered, exits, loops, records, frames: stitched.frames, stable: stitched.facts.into() })
}
