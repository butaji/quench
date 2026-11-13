//! A whole function as one stitched body: every instruction is a row the base-region decode
//! covers (the numeric rows, the heap rows read through the heap's own tables: an array
//! word's record, a field's cache entry, the activation's captures, a fast arm over the activation's
//! locals or a value's tag with the interpreter's step behind it, a helper step, a call of any form the
//! call planner chains, a `Return` of a register, or a `throw`: the catch's jump where a handler
//! of the function covers it, else the raising step), followed by the completion with its result where
//! the instruction carries the return flag, or a leave row at its PC, where the
//! interpreter resumes exactly (`steps` lists which instructions are those). The rows, webs and homes are the loop regions' (`analyze`,
//! `plan`, `link`); the body is entered and left as a lane body is (`Ending::Body`), and polls
//! the interrupt flag at every branch back (`Ending::poll`).
use super::*;
use super::call_plan::inline::Context;
use super::call_plan::{CallForm, CallShape, DirectFacts};
use super::callees::Callees;
use super::super::LaneHelpers;
use crate::bytecode::Function;
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
    /// The atoms its stores run in place for, held unnamed by method caches (a Stable fact): a
    /// cache naming one retires the body (`StableFacts::atoms`).
    pub(in crate::vm::compiled) atoms: Box<[Atom]>,
    /// The exact shapes of the holders whose values its folded reads hold (a Stable fact): a change
    /// of a value a prototype of one holds retires the body (`StableFacts::holders`).
    pub(in crate::vm::compiled) holders: Box<[u32]>,
    /// What its `instanceof` fast arms hold of their constructors' `prototype`s (a Stable fact): a write of a `prototype`,
    /// a departure of a constructor's shape or a change of the `@@hasInstance` holder's values retires the body
    /// (`StableFacts::instanceofs`).
    pub(in crate::vm::compiled) instanceofs: Box<[InstanceofFact]>,
    /// The constructors those arms guard their operands against by identity, which a collection must not free
    /// while the body lives (`LaneMethod::constructors`).
    pub(in crate::vm::compiled) constructors: Box<[Value]>,
    /// Whether it reads no heap row, makes no step and proves no realm: a loop region that does not
    /// needs from its payload only the entry record's address (`Crossing::Bare`).
    pub(in crate::vm::compiled) bare: bool,
    /// The global layouts and slot types its rows hold (a Stable fact): a change retires the body
    /// (`StableFacts::layouts`, `StableFacts::slots`).
    pub(in crate::vm::compiled) layouts: Box<[crate::heap::GlobalLayoutFact]>,
    pub(in crate::vm::compiled) slot_types: Box<[crate::code_dependencies::SlotDependency]>,
}

/// What a stitch decides its call sites by: the callees they recorded and the stitch's budget, the
/// program facts its rows read, and what it stitches (`Ending`), with, for a region, whether its
/// arrays' views are the entry record's (`Facts::Entry::dense`).
struct Sites<'a> {
    function: &'a Function,
    /// The index of `function` in its program.
    id: u32,
    callees: &'a Callees,
    facts: &'a [Option<DirectFacts>],
    context: Context,
    registers: u16,
    known: Known<'a>,
    ending: Ending,
    dense: bool,
}

/// The row of `instruction` at `pc`, which carries no result flag (`cover` strips the return flag
/// of a returning one, `returning`): a `Return` of a register, a `throw` a handler of the function
/// catches natively (`throws::caught`), a call the stitch inlines as the callee's one row or the
/// call planner's chain, else what the loop regions' decode covers (a leave where nothing does).
fn lowered(region: &LoopRegion, pc: u32, sites: &mut Sites, (instruction, returning): (WideInstruction, bool), helpers: LaneHelpers) -> Decoded {
    let plain = instruction.result_flags() == 0;
    if instruction.op().raises() && let Some(caught) = throws::caught(sites.function, (pc, sites.registers), instruction) {
        return caught;
    }
    if let Some(form) = CallForm::of(instruction) {
        let shape = CallShape { returning, ..CallShape::of(pc, instruction, form) };
        return sites.callees.inlined_call(sites.facts, region, &mut sites.context, &shape, sites.registers)
            .unwrap_or_else(|| Decoded::step(Step::Call(shape), Some(shape.result)));
    }
    match instruction.op().class() {
        Class::Return if plain && instruction.register_a() < sites.registers => {
            Decoded { a: Some(Source::Register(instruction.register_a())), ..Decoded::step(Step::Return, None) }
        }
        _ => locals::fast_row(sites.function, sites.id, instruction)
            .unwrap_or_else(|| decode(region, (Facts::Site, sites.known), pc, instruction, helpers)),
    }
}

/// Whether the row is a fast arm of its instruction: a fast step, a constant written as it is, or the value a Stable
/// fact folded (`Decoded::folded`).
fn is_fast(row: &Decoded) -> bool {
    matches!(row.step, Some(Step::Fast(_))) || matches!(row.a, Some(Source::Value(_))) || row.folded.is_some()
}

/// Whether a row may be followed by the completion of the activation with its result: it falls
/// through to what follows it, and writes the register the completion reads.
fn completable(row: &Decoded) -> bool {
    row.dst.is_some() && row.target.is_none() && row.step.is_none_or(|step| matches!(step, Step::Helper | Step::Fast(_) | Step::Call(_)))
}

/// The row of the instruction at `pc`: what it lowers to (`lowered`), where an instruction that
/// carries the return flag (`return a + b`, `return o.p`, `return f(x)`) lowers as the one without it,
/// followed by the completion with its result (`Decoded::completes`); an instruction no row takes
/// leaves at its PC as it is, flag and all, for the interpreter.
fn cover(region: &LoopRegion, pc: u32, sites: &mut Sites, instruction: WideInstruction, helpers: LaneHelpers) -> Decoded {
    let (plain, returning) = instruction.split_return();
    let row = lowered(region, pc, sites, (plain, returning), helpers);
    if returning && !completable(&row) {
        return Decoded::step(Step::Leave, None);
    }
    if is_fast(&row) {
        note_fast(instruction.op());
    }
    Decoded { completes: returning, ..row }
}

/// Which rows control reaches from PC 0; an error where one falls off the function's end.
fn reachable(region: &LoopRegion, decoded: &[Decoded]) -> Result<Vec<bool>, String> {
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
    static EMPTY: Stencil = Stencil { code: Vec::new(), relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0, raw_len: 0, hot_len: 0 };
    Planned::single(&EMPTY, [0; HOLES], None, None)
}

/// A row that leaves at `pc` unless the frame slot at byte offset `slot` holds a Number.
fn number_guard(pc: u32, slot: u64) -> Result<Planned, String> {
    Ok(Planned { stop: Some(Stop { pc, returns: None }), ..Planned::single(control(Kind::Guard)?, [0, slot, 0], None, None) })
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

/// The rows that run before the row `pc`, at the same row: the check that a Stable fact the row
/// holds still holds, the proof a `ToNumeric` operand held in a frame slot is a Number (a loop's
/// entry guards prove the registers it names itself, a function's registers and those of an inlined
/// callee are not proved), and the poll a conditional branch back takes (an unconditional one is its
/// own `JumpWatch`), so no loop a function runs goes unwatched; a region's own jumps count and poll
/// (`JumpPoll`).
fn preludes(layout: &Layout, row: &Decoded, pc: u32) -> Result<Vec<Planned>, String> {
    let (place, slot) = operand(&layout.homes, row.a);
    let body = layout.ending == Ending::Body;
    let mut before = Vec::new();
    if let Some(address) = layout.invalidation.filter(|_| row.holds_stable()) {
        before.push(live_guard(pc, address)?);
    }
    if row.checked && place == Place::Mem && (body || layout.inlined(pc)) {
        before.push(number_guard(pc, slot)?);
    }
    if body && row.kind != Kind::Jump && row.target.is_some_and(|target| target <= pc) {
        before.push(poll_row(pc)?);
    }
    Ok(before)
}

/// Whether the register a heap row reads its array, object or callee through is held in its frame
/// slot, where the row reads it: a pinned register holds a Number, never one of those, and its
/// slot is not kept current.
fn framed(homes: &Homes, row: &Decoded) -> bool {
    let through = match row.heap {
        Some(Heap::Store | Heap::SiteStore) => row.base,
        Some(Heap::Index | Heap::Array | Heap::Site | Heap::SiteChain | Heap::Length | Heap::Held | Heap::Identity) => row.a.and_then(register_of),
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
    // The preludes come before the row's own run (a chain's spills): their leaves write the pinned registers back themselves.
    let mut planned = plan(row, layout, pc, is_general(row, reprs), helpers)?.after(preludes(layout, row, pc)?);
    if row.completes {
        planned = completing(planned, layout, pc, row.dst)?;
    }
    Ok(planned)
}

/// What a body is built for: its function and registers, the rows whose int32 speculation failed
/// (`widened`, by caller PC), the registers its entry held as ints and doubles (`seen`), and what
/// the program holds Stable (`Known`), whose invalidation word its code reads where a call may
/// have changed a fact it folded, and the realm its inlined callees belong to (`RealmProof`).
pub(in crate::vm::compiled) struct BodySpec<'a> {
    pub(in crate::vm::compiled) function: u32,
    pub(in crate::vm::compiled) registers: u16,
    pub(in crate::vm::compiled) known: Known<'a>,
    pub(in crate::vm::compiled) realm: RealmProof,
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
    let sites = Sites { function: known.program.function(function as usize), id: function, callees, facts: &facts, context: Context::root(function, registers), registers, known };
    let built = assemble_body(region, widened, seen, sites, helpers);
    note(|totals, rejects| match &built {
        Ok(body) => {
            totals.bodies += 1;
            totals.covered += body.covered as u64;
            totals.exits += body.exits as u64;
        }
        (Ok(body), Ending::Region) => {
            totals.regions += 1;
            totals.ops += span;
            totals.bytes += body.code.len() as u64;
            totals.nanos += nanos;
        }
        (Err(name), Ending::Body) => *rejects.entry(format!("body: {name}")).or_default() += 1,
        (Err(name), Ending::Region) => {
            totals.rejected += 1;
            *rejects.entry(name.clone()).or_default() += 1;
        }
    });
}

/// The stitch of `region` (`ending`: what it is, and for a region whether its views are `dense`).
///
/// # Safety
/// As `build_body`.
unsafe fn build(region: &LoopRegion, spec: &BodySpec, (ending, dense): (Ending, bool), callees: &Callees, helpers: LaneHelpers) -> Result<Body, String> {
    let started = std::time::Instant::now();
    // SAFETY: the caller's contract.
    let facts = unsafe { callees.facts() };
    let sites = Sites { callees, facts: &facts, context: Context::root(spec.function, spec.registers), registers: spec.registers, known: spec.known, ending, dense };
    let built = assemble(region, spec, sites, helpers);
    tally(&built, ending, (u64::from(region.backedge - region.header) + 1, started.elapsed().as_nanos() as u64));
    built
}

/// Whether the row runs a call or a helper step, which may run guest code that changes a Stable fact.
fn makes_calls(row: &Decoded) -> bool {
    matches!(row.step, Some(Step::Call(_))) || row.step.is_some_and(Step::runs_helper)
}

/// The rows whose int32 speculation failed: those of the caller's instructions at the `widened` PCs,
/// and of the inlined callees' at the `widened` keys (`inlined_exit_key`).
fn widened_rows(origins: &[Origin], frames: &[InlinedCall], widened: &[usize]) -> Vec<usize> {
    let widens = |origin: &Origin| match origin {
        Origin::Caller(pc) => widened.contains(&(*pc as usize)),
        Origin::Inlined { frame, pc } => widened.contains(&inlined_exit_key(frames[*frame].call, *pc)),
    };
    origins.iter().enumerate().filter(|(_, origin)| widens(origin)).map(|(k, _)| k).collect()
}

/// Every caller instruction as its parts: its row, or its call's site, and a returning call's
/// completion. The calls are planned first: which a site takes decides what the rest decode by
/// (`decode_facts`).
fn parts(region: &LoopRegion, sites: &mut Sites, helpers: LaneHelpers) -> Vec<Part> {
    let at = |k: usize| region.header + k as u32;
    let planned: Vec<(usize, WideInstruction)> = region.instructions().iter().copied().enumerate().filter(|(_, instruction)| sites.plans(*instruction, helpers)).collect();
    let mut calls = BTreeMap::new();
    for (k, instruction) in planned {
        calls.insert(at(k), call(region, at(k), sites, instruction));
    }
    let facts = decode_facts(region, sites, &calls);
    let mut parts: Vec<Part> = region.instructions().iter().enumerate().flat_map(|(k, instruction)| {
        let pc = at(k);
        let whats = calls.remove(&pc).unwrap_or_else(|| vec![cover(region, pc, sites, *instruction, (facts, helpers))]);
        whats.into_iter().map(move |what| Part { pc, what })
    }).collect();
    if sites.ending == Ending::Region {
        let mut rows: Vec<&mut Decoded> = parts.iter_mut().filter_map(|part| if let What::Row(row) = &mut part.what { Some(row) } else { None }).collect();
        settle(&mut rows);
    }
    parts
}

fn assemble(region: &LoopRegion, spec: &BodySpec, mut sites: Sites, helpers: LaneHelpers) -> Result<Body, String> {
    let (ending, callees) = (sites.ending, sites.facts);
    let mut stitched = stitch(region, parts(region, &mut sites, helpers), spec.registers, spec.known.invalidation, ending);
    let span = Span::of(region, stitched.rows.len(), &stitched.live);
    let reached = reachable(&span, &stitched.rows)?;
    if ending == Ending::Body && stitched.rows[0].step == Some(Step::Leave) {
        return Err("first instruction leaves".to_string());
    }
    let unreached = |row: Decoded, reached: &bool| if *reached { row } else { Decoded::step(Step::Leave, None) };
    let rows: Vec<Decoded> = stitched.rows.into_iter().zip(&reached).map(|(row, reached)| unreached(row, reached)).collect();
    let (reprs, homes, webs, decoded) = analyze(&span, &rows, &widened_rows(&stitched.origins, &stitched.frames, spec.widened), spec.seen);
    let invalidation = spec.known.invalidation.filter(|_| decoded.iter().any(Decoded::holds_stable) && decoded.iter().any(Decoded::makes_calls));
    let realm = (!stitched.frames.is_empty()).then_some(spec.realm);
    let layout = Layout { span, homes, webs, ending, callees, origins: &stitched.origins, copies: &stitched.copies, invalidation, realm };
    let mut planned = decoded.iter().enumerate().map(|(k, row)| plan_row(&layout, &reprs, reached[k], k as u32, row, helpers)).collect::<Result<Vec<_>, _>>()?;
    let code = link(&layout, &planned)?;
    let exits = decoded.iter().enumerate().filter(|(k, row)| reached[*k] && leaves(&layout, row)).count();
    let loops = decoded.iter().enumerate().any(|(k, row)| row.target.is_some_and(|target| target <= k as u32));
    let covered = reached.iter().filter(|reached| **reached).count() - exits;
    let bare = realm.is_none() && decoded.iter().all(|row| row.heap.is_none() && !row.makes_calls());
    let records = planned.iter_mut().filter_map(|row| row.owned.take()).collect();
    Ok(Body { code, covered, exits, loops, records, atoms: held_atoms(&decoded), holders: held_holders(&decoded), instanceofs: held_instanceofs(&decoded), constructors: held_constructors(&decoded) })
}
