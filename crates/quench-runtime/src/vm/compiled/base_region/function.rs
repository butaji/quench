//! A whole function as one stitched body: every instruction is a row the base-region decode
//! covers (the numeric rows, the heap rows read through the heap's own tables: an array
//! word's record, a field's cache entry, the activation's captures, a helper step, a `Call`
//! the call planner chains, or a `Return` of a register) or a leave row at its PC, where the
//! interpreter resumes exactly. The rows, webs and homes are the loop regions' (`analyze`,
//! `plan`, `link`); the body is entered and left as a lane body is (`Ending::Body`), and polls
//! the interrupt flag at every branch back (`Ending::poll`).
use super::*;
use super::call_plan::inline::Context;
use super::call_plan::{CallShape, DirectFacts};
use super::callees::{chains, Callees};
use super::super::LaneHelpers;
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
    /// The atoms its stores run in place for, held unnamed by method caches (a Stable fact): a
    /// cache naming one retires the body (`StableFacts::atoms`).
    pub(in crate::vm::compiled) atoms: Box<[Atom]>,
    /// The exact shapes of the holders whose values its folded reads hold (a Stable fact): a change
    /// of a value a prototype of one holds retires the body (`StableFacts::holders`).
    pub(in crate::vm::compiled) holders: Box<[u32]>,
}

/// What a body decides its call sites by: the callees they recorded and the stitch's budget, and
/// the program facts its rows read.
struct Sites<'a> {
    callees: &'a Callees,
    facts: &'a [Option<DirectFacts>],
    context: Context,
    registers: u16,
    known: Known<'a>,
}

/// The row of the instruction at `pc`: a `Return` of a register, a call the stitch inlines as the
/// callee's one row or the call planner's chain, else what the loop regions' decode covers (a
/// leave where nothing does).
fn cover(region: &LoopRegion, pc: u32, sites: &mut Sites, instruction: WideInstruction, helpers: LaneHelpers) -> Decoded {
    let plain = instruction.result_flags() == 0;
    match instruction.op().class() {
        Class::Return if plain && instruction.register_a() < sites.registers => {
            Decoded { a: Some(Source::Register(instruction.register_a())), ..Decoded::step(Step::Return, None) }
        }
        Class::Call if chains(instruction) => {
            let shape = CallShape::of(pc, instruction);
            sites.callees.inlined_call(sites.facts, region, &mut sites.context, &shape, sites.registers)
                .unwrap_or_else(|| Decoded::step(Step::Call(shape), Some(shape.result)))
        }
        _ => decode(region, (Facts::Site, sites.known), pc, instruction, helpers),
    }
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
    static EMPTY: Stencil = Stencil { code: Vec::new(), relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0, raw_len: 0, hot_len: 0 };
    Planned::single(&EMPTY, [0; HOLES], None, None)
}

/// A row that leaves at `pc` unless the frame slot at byte offset `slot` holds a Number.
fn number_guard(pc: u32, slot: u64) -> Result<Planned, String> {
    Ok(Planned { stop: Some(Stop { pc, returns: None }), ..Planned::single(control(Kind::Guard)?, [0, slot, 0], None, None) })
}

/// A row that leaves at `pc` unless the register's frame slot holds exactly the function `word`
/// (`Heap::Identity`): the proof that a call the row replaces calls the function it was stitched for.
fn identity_guard(homes: &Homes, pc: u32, register: Register, word: u64) -> Result<Planned, String> {
    let slot = u64::from(homes.orig[usize::from(register)]) * SLOT;
    let row = HeapRow { heap: Heap::Identity, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    Ok(Planned::single(base_rows::stencil(row)?, [0, slot, word], None, Some(pc)))
}

/// A row that leaves at `pc` once the invalidation word at `address` is set (`Heap::Live`): a Stable
/// fact the code holds changed under a call, so the interpreter makes the instruction.
fn live_guard(pc: u32, address: usize) -> Result<Planned, String> {
    let row = HeapRow { heap: Heap::Live, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    Ok(Planned::single(base_rows::stencil(row)?, [0, address as u64, 0], None, Some(pc)))
}

/// A row that leaves at `pc` when an interrupt is requested, and continues.
fn poll_row(pc: u32) -> Result<Planned, String> {
    let interrupt = std::mem::offset_of!(super::super::CallContext, interrupt) as u64;
    Ok(Planned::single(control(Kind::Poll)?, [interrupt, 0, 0], None, Some(pc)))
}

/// The rows that run before the row at `pc`, at the same PC: the check that a Stable fact the row
/// holds still holds, the proof that an inlined call's callee
/// is the one stitched, the proof a `ToNumeric` operand
/// held in a frame slot is a Number (a loop's entry guards prove its registers; a function's are
/// not proved), and the poll a conditional branch back takes (an unconditional one is its own
/// `JumpWatch`), so no loop a function runs goes unwatched.
fn preludes(layout: &Layout, row: &Decoded, pc: u32) -> Result<Vec<Planned>, String> {
    let (place, slot) = operand(&layout.homes, row.a);
    let mut before = Vec::new();
    if let Some(address) = layout.invalidation.filter(|_| row.holds_stable()) {
        before.push(live_guard(pc, address)?);
    }
    if let Some((register, word)) = row.identity {
        before.push(identity_guard(&layout.homes, pc, register, word)?);
    }
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
        Some(Heap::Store | Heap::SiteStore) => row.base,
        Some(Heap::Index | Heap::Array | Heap::Site | Heap::SiteChain | Heap::Length | Heap::Held) => row.a.and_then(register_of),
        _ => None,
    };
    through.into_iter().chain(row.identity.map(|(register, _)| register)).all(|register| homes.place(register) == Place::Mem)
}

/// Whether the row leaves the body: a leave step, or a heap row whose array is not in its frame slot.
fn leaves(layout: &Layout, row: &Decoded) -> bool {
    row.step == Some(Step::Leave) || !framed(&layout.homes, row)
}

/// The plan of a row at `pc`: nothing where control never reaches it, a leave row, or the row
/// with the rows before it its kind needs.
fn plan_row(layout: &Layout, reprs: &Reprs, reached: bool, pc: u32, row: &Decoded, helpers: LaneHelpers) -> Result<Planned, String> {
    if !reached {
        return Ok(skipped());
    }
    if leaves(layout, row) {
        return plan_step(&Decoded::step(Step::Leave, None), layout, pc, helpers);
    }
    Ok(Planned { pre: preludes(layout, row, pc)?, ..plan(row, layout, pc, is_general(row, reprs), helpers)? })
}

/// The body of the function `function` that `region` spans, over the registers observed at its
/// entry (`seen`: the ints and the doubles; `widened`: the PCs whose int32 speculation failed),
/// and the callees its call sites recorded, counted under the trace. An error where its first
/// instruction leaves at once: such a body is pure entry cost.
///
/// # Safety
/// Each of `callees` names its function's live entry cell (`Callees::facts`).
pub(in crate::vm::compiled) unsafe fn build_body(region: &LoopRegion, (function, registers): (u32, u16), known: Known, widened: &[usize], seen: [&[Register]; SEEN], callees: &Callees, helpers: LaneHelpers) -> Result<Body, String> {
    // SAFETY: the caller's contract.
    let facts = unsafe { callees.facts() };
    let sites = Sites { callees, facts: &facts, context: Context::root(function, registers), registers, known };
    let built = assemble_body(region, widened, seen, sites, helpers);
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

/// Whether the row runs a call or a helper step, which may run guest code that changes a Stable fact.
fn makes_calls(row: &Decoded) -> bool {
    matches!(row.step, Some(Step::Call(_) | Step::Helper))
}

fn assemble_body(region: &LoopRegion, widened: &[usize], seen: [&[Register]; SEEN], mut sites: Sites, helpers: LaneHelpers) -> Result<Body, String> {
    let covers: Vec<Decoded> = region.instructions().iter().enumerate().map(|(pc, instruction)| cover(region, pc as u32, &mut sites, *instruction, helpers)).collect();
    let reached = reachable(region, &covers)?;
    if covers[0].step == Some(Step::Leave) {
        return Err("first instruction leaves".to_string());
    }
    let unreached = |k: usize| !reached[k];
    let decoded: Vec<Decoded> = covers.into_iter().enumerate().map(|(k, row)| if unreached(k) { Decoded::step(Step::Leave, None) } else { row }).collect();
    let (reprs, homes, webs, decoded) = analyze(region, &decoded, widened, seen);
    let invalidation = sites.known.invalidation.filter(|_| decoded.iter().any(Decoded::holds_stable) && decoded.iter().any(makes_calls));
    let layout = Layout { header: region.header, backedge: region.backedge, homes, webs, region, ending: Ending::Body, callees: sites.facts, invalidation };
    let mut planned = decoded.iter().enumerate().map(|(k, row)| plan_row(&layout, &reprs, reached[k], k as u32, row, helpers)).collect::<Result<Vec<_>, _>>()?;
    let code = link(&layout, &planned)?;
    let exits = decoded.iter().enumerate().filter(|(k, row)| reached[*k] && leaves(&layout, row)).count();
    let loops = decoded.iter().enumerate().any(|(k, row)| row.target.is_some_and(|target| target <= k as u32));
    let covered = reached.iter().filter(|reached| **reached).count() - exits;
    let records = planned.iter_mut().filter_map(|row| row.owned.take()).collect();
    Ok(Body { code, covered, exits, loops, records, atoms: held_atoms(&decoded), holders: held_holders(&decoded) })
}
