//! A whole function as one stitched body, or a loop's span of it as one stitched region: every
//! instruction is a row the base-region decode covers (the numeric rows, the heap rows read through
//! the heap's own tables: an array word's record, a field's cache entry, the activation's captures,
//! a fast arm over the activation's locals or a value's tag with the interpreter's step behind it, a
//! helper step, a call of any form the call planner chains, a call stitched in as its callee's rows,
//! a `Return` of a register, or a `throw`: the catch's jump where a handler of the function covers it,
//! else the raising step), followed by the completion with its result where the instruction carries
//! the return flag, or a leave row at its PC, where the interpreter resumes exactly (`steps` lists
//! which instructions are those).
//! The rows are laid out as one flat program (`stitch`), whose rows the webs, homes and exits are
//! keyed by (`analyze`, `plan`, `link`), for a region as for a body: the body is entered and left
//! as a lane body is (`Ending::Body`) and polls the interrupt flag at every branch back
//! (`Ending::poll`); the region reads the entry record its loop's entry made, leaves its loop by
//! a leave row at the PC it resumes the interpreter at, and its calls that complete no activation
//! are inlined or chained as a body's are (`Ending::Region`).
use super::*;
use super::call_plan::inline::Context;
use super::call_plan::{CallForm, CallShape, DirectFacts};
use super::callees::{chains, Callees};
use super::super::LaneHelpers;
use crate::bytecode::Function;
use crate::heap::GlobalFact;
use quench_bjit::call_record::CallRecord;
use quench_bjit::{padded_holes, HOLES};

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
    /// Where a run that left a loop region at a PC re-enters it (`resume`); none for a body.
    pub(in crate::vm::compiled) resumes: ResumeTable,
    /// The registers its rows name past the function's own: its inlined callees' windows and the scratch of its
    /// `instanceof`/`in` jumps, whose frame slots its recovery room covers (`LaneRecoverySpace::reaching`).
    pub(in crate::vm::compiled) scratch: usize,
}

/// What a stitch decides its call sites by: the callees they recorded and the stitch's budget, the
/// program facts its rows read, and what it stitches (`Ending`), with, for a region, whether its
/// arrays' views are the entry record's (`Facts::Entry::views`).
struct Sites<'a> {
    function: &'a Function,
    /// The index of `function` in its program.
    id: u32,
    callees: &'a Callees,
    facts: &'a [Option<DirectFacts>],
    context: Context,
    registers: u16,
    /// The first register the rows may write past the function's own (`inline_base`).
    scratch: Register,
    known: Known<'a>,
    ending: Ending,
    views: Views,
}

/// The row of `instruction` at `pc`, which carries no result flag (`cover` strips the return flag
/// of a returning one, `returning`): a `Return` of a register, a `throw` a handler of the function
/// catches natively (`throws::caught`), a call of a form no stitch plans (`Sites::plans`: the
/// construct and stepped forms, the call planner's chain), else what the loop regions' decode covers
/// (a leave where nothing does).
fn lowered(region: &LoopRegion, pc: u32, sites: &Sites, (instruction, returning): (WideInstruction, bool), (facts, helpers): (Facts, LaneHelpers)) -> Decoded {
    let plain = instruction.result_flags() == 0;
    if instruction.op().raises() && let Some(caught) = throws::caught(sites.function, (pc, sites.registers), instruction) {
        return caught;
    }
    if let Some(form) = CallForm::of(instruction) {
        let shape = CallShape { returning, ..CallShape::of(pc, instruction, form) };
        return Decoded::step(Step::Call(shape), Some(shape.result));
    }
    match instruction.op().class() {
        Class::Return if plain && instruction.register_a() < sites.registers => returned(instruction.register_a()),
        _ => locals::fast_row(sites.function, sites.id, instruction, sites.ending)
            .unwrap_or_else(|| decode(region, (facts, sites.known), pc, instruction, helpers)),
    }
}

/// Whether the row is a fast arm of its instruction: a fast step, a constant written as it is, the value a Stable
/// fact folded (`Decoded::folded`), or a global lexical binding's word read or written in place.
#[cfg(test)]
fn is_fast(row: &Decoded) -> bool {
    matches!(row.step, Some(Step::Fast(_) | Step::Guard(_))) || matches!(row.a, Some(Source::Value(_))) || row.folded.is_some() || row.lexical.is_some()
}

/// Whether a row may be followed by the completion of the activation with its result: it falls
/// through to what follows it, and writes the register the completion reads.
fn completable(row: &Decoded) -> bool {
    row.dst.is_some() && row.target.is_none() && row.step.is_none_or(|step| matches!(step, Step::Helper | Step::Fast(_) | Step::Call(_)))
}

impl Sites<'_> {
    /// Whether the stitch plans `instruction` as a call: a body's chain makes every plain call
    /// `callees::chains` admits (a returning one with its completion); a region makes the plain calls
    /// it can run in place (`calls_in_place`), which complete no activation. The other forms are rows
    /// of their own (`lowered`).
    fn plans(&self, instruction: WideInstruction, helpers: LaneHelpers) -> bool {
        match self.ending {
            Ending::Body => chains(instruction),
            Ending::Region => calls_in_place(instruction, helpers),
        }
    }
}

/// The row that completes the activation with the register `register` holds.
fn returned(register: Register) -> Decoded {
    Decoded { a: Some(Source::Register(register)), ..Decoded::step(Step::Return, None) }
}

/// A call: the stitch's site where its callee inlines, else the call planner's chain; and where
/// it returns its result (`return f(x)`), the completion with it. A tail jump that reuses the
/// caller's frame for the callee's window is not made: the lane stack holds one activation per
/// native frame, and a leave rebuilds them (`lane_materialize_frames`), which a frame the callee
/// took over would leave inexact.
fn call(region: &LoopRegion, pc: u32, sites: &mut Sites, instruction: WideInstruction) -> Vec<What> {
    let (plain, returning) = instruction.split_return();
    let shape = CallShape { returning, ..CallShape::of(pc, plain, CallForm::Plain) };
    let first = match math_call(region, sites, &shape) {
        Some(rows) => rows.into_iter().map(What::Row).collect(),
        None => match sites.callees.inlined_call(sites.facts, region, &mut sites.context, &shape) {
            Some(site) => vec![What::Site(site)],
            None => vec![What::Row(Decoded::step(Step::Call(shape), Some(shape.result)))],
        },
    };
    let completion = shape.returning.then(|| What::Row(returned(shape.result)));
    first.into_iter().chain(completion).collect()
}

/// A call of a native Math function a row computes (`Math.sqrt(x)`): its callee
/// register was defined by a field read of a Stable global's member that was that native when the
/// code was built (`GlobalRead::NativeMember`), and the call passes one argument. The rows: the
/// callee's identity guarded (any other callee leaves at the call, for the interpreter to call it),
/// then the staged Math operation of the argument into the result, which leaves where it is no
/// Number. The holder's exact shape is held, so the identity is not recycled while the code runs.
/// The interpreter's call protocol, the native's dispatch and its argument copy fold away: partial
/// evaluation of the call on the callee the site proves.
fn math_call(region: &LoopRegion, sites: &Sites, shape: &CallShape) -> Option<Vec<Decoded>> {
    use crate::vm::global_facts::GlobalRead;
    use crate::vm::Native;
    (shape.form == CallForm::Plain && shape.argc == 1).then_some(())?;
    let local = (shape.call_pc - region.header) as usize;
    let defined = (0..local).rev().find(|at| region.instructions()[*at].written_registers().any(|register| register == shape.callee))?;
    let GlobalRead::NativeMember { value, native, holder } = *sites.known.globals?.get(&(region.header as usize + defined))? else { return None };
    let kind = match native {
        Native::MathSqrt => Kind::Sqrt,
        _ => return None,
    };
    let guard = Decoded { hole: value.to_bits(), holder: Some(holder), ..Decoded::heap(Heap::Identity, None, Some(Source::Register(shape.callee)), None) };
    let operation = Decoded::number(kind, Some(shape.result), Some(Source::Register(shape.window)), None, None);
    Some(vec![guard, operation])
}

/// The rows of an instruction a body makes in the scratch registers past the function's own (`scratch`): the comparison and the jump on it
/// of an `instanceof` or `in` jump (`sites::compare_jump`). A region reads its entry record, and leaves it to `cover`.
fn expanded(sites: &Sites, pc: u32, instruction: WideInstruction) -> Option<Vec<What>> {
    (sites.ending == Ending::Body && sites.scratch < crate::bytecode::REGISTER_MASK).then_some(())?;
    let rows = compare_jump(sites.known, pc, instruction, sites.scratch)?;
    #[cfg(test)]
    note_fast(instruction.op() as u16);
    Some(rows.into_iter().map(What::Row).collect())
}

/// The registers the rows name above the function's own `registers`: the scratch the rows of `expanded` write, and inlined windows, whose
/// frame slots the body's recovery room covers (`LaneRecoverySpace::reaching`).
fn scratch(rows: &[Decoded], registers: u16) -> usize {
    let reach = rows.iter().flat_map(Decoded::registers).map(|register| usize::from(register) + 1).max().unwrap_or(0);
    reach.saturating_sub(usize::from(registers))
}

/// The facts the rows of the instructions that are no planned call decode by: a body's read the heap
/// by its tables; a region's read its entry record, but for what a call row it keeps may change
/// (`Facts::Entry::calls`: a call stitched in changes nothing), and what its stores stale.
fn decode_facts(region: &LoopRegion, sites: &Sites, calls: &[Option<Vec<What>>]) -> Facts {
    if sites.ending == Ending::Body {
        return Facts::Site;
    }
    let kept = calls.iter().flatten().flatten().any(|what| matches!(what, What::Row(row) if matches!(row.step, Some(Step::Call(_)))));
    let stores = region.instructions().iter().any(|instruction| stores_field(sites.known, *instruction) || stores_name(*instruction));
    Facts::Entry { views: if kept { Views::Frame } else { sites.views }, writes: stores || kept }
}

/// The row of the instruction at `pc` that is no planned call: in a body, what it lowers to
/// (`lowered`), where an instruction that carries the return flag (`return a + b`, `return o.p`)
/// lowers as the one without it, followed by the completion with its result
/// (`Decoded::completes`); in a region what the loop regions' decode covers. An instruction no row
/// takes leaves at its PC as it is, flag and all, for the interpreter.
fn cover(region: &LoopRegion, pc: u32, sites: &Sites, instruction: WideInstruction, (facts, helpers): (Facts, LaneHelpers)) -> What {
    if sites.ending == Ending::Region {
        return What::Row(decode(region, (facts, sites.known), pc, instruction, helpers));
    }
    let (plain, returning) = instruction.split_return();
    let row = lowered(region, pc, sites, (plain, returning), (facts, helpers));
    if returning && !completable(&row) {
        return What::Row(Decoded::step(Step::Leave, None));
    }
    #[cfg(test)]
    if is_fast(&row) {
        note_fast(instruction.op() as u16);
    }
    What::Row(Decoded { completes: returning, ..row })
}

/// Which rows control reaches from row 0; an error where one falls off the program's end.
fn reachable(span: &Span, decoded: &[Decoded]) -> Result<Bits, String> {
    let count = decoded.len();
    let (mut reached, mut pending) = (Bits::default(), vec![0]);
    while let Some(k) = pending.pop() {
        if reached.contains(k) {
            continue;
        }
        reached.insert(k);
        for next in decoded[k].successors(k, count, span) {
            match next {
                Next::Row(row) => pending.push(row),
                Next::Leave(_) => return Err("a row falls off the function".to_string()),
            }
        }
    }
    Ok(reached)
}

/// A row that runs no code: a row control never reaches, or one a tile covers.
pub(super) fn skipped() -> Planned {
    static EMPTY: Stencil = Stencil { code: Vec::new(), relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0, raw_len: 0, hot_len: 0 };
    Planned::single(&EMPTY, [0; HOLES], None, None)
}

/// A row that leaves at `pc` unless the frame slot at byte offset `slot` holds a Number.
fn number_guard(pc: u32, slot: u64) -> Result<Planned, String> {
    Ok(Planned { stop: Some(Stop { pc, returns: None }), ..Planned::single(control(Kind::Guard)?, padded_holes([0, slot]), None, None) })
}

/// A row that leaves at `pc` once the invalidation word at `address` is set (`Heap::Live`): a Stable
/// fact the code holds changed under a call, so the interpreter makes the instruction.
fn live_guard(pc: u32, address: usize) -> Result<Planned, String> {
    let row = HeapRow { heap: Heap::Live, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    Ok(Planned::single(base_rows::stencil(row)?, padded_holes([0, address as u64]), None, Some(pc)))
}

/// A row that leaves at `pc` when an interrupt is requested, and continues.
fn poll_row(pc: u32) -> Result<Planned, String> {
    let interrupt = std::mem::offset_of!(super::super::CallContext, interrupt) as u64;
    Ok(Planned::single(control(Kind::Poll)?, padded_holes([interrupt]), None, Some(pc)))
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
        Some(Heap::Store | Heap::TypedStore(_) | Heap::SiteStore) => row.base,
        Some(Heap::Index | Heap::TypedIndex(_) | Heap::TypedLength | Heap::Array | Heap::Site | Heap::SiteChain | Heap::Length | Heap::Held | Heap::Identity) => row.a.and_then(register_of),
        _ => None,
    };
    through.into_iter().all(|register| homes.place(register) == Place::Mem)
}

/// Whether the row leaves the body: a leave step, or a heap row whose array is not in its frame slot.
fn leaves(layout: &Layout, row: &Decoded) -> bool {
    row.step == Some(Step::Leave) || !framed(&layout.homes, row)
}

/// Whether `row` interprets its instruction rather than running its handler's residue: the general
/// step a lane helper makes from the instruction's PC (re-decoding it and dispatching on its form,
/// hundreds of instructions where the interpreter's quickened handler spends tens), or a call (the
/// region call protocol costs more than the interpreter's own call today). The design is a ladder, b-jit at most the interpreter's instructions on every
/// operation (b-jit is the interpreter with dispatch and decode folded), so a loop region holding
/// one on its loop is not built and the interpreter runs the loop: interpreting inside a region is
/// slower than the interpreter by construction. A fast arm's slow arm (`Step::Fast`), a throw and a
/// leave are off the loop's steady path. A call leaves this list once its protocol runs in fewer
/// instructions than the interpreter's call: the tier-ladder gate (`quench-gate --only ir-budget`,
/// call_direct, call_closure, k03_call_add) says when.
fn interprets(row: &Decoded) -> bool {
    match row.step {
        Some(Step::Helper | Step::CallOut) => true,
        Some(Step::Call(_)) => !region_call_rows(),
        _ => false,
    }
}

/// Whether loop regions keep their call rows (`interprets` refuses them): never in a build, but a
/// test of the region call protocol itself (a callee's throw, a callee changing what the entry
/// recorded) runs under `with_region_call_rows`, so the protocol stays exact for the day calls
/// leave `interprets`.
#[cfg(not(test))]
fn region_call_rows() -> bool {
    false
}

#[cfg(test)]
fn region_call_rows() -> bool {
    REGION_CALL_ROWS.with(std::cell::Cell::get)
}

#[cfg(test)]
thread_local! {
    static REGION_CALL_ROWS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `body` with the calling thread's loop regions keeping their call rows.
#[cfg(test)]
pub(in crate::vm) fn with_region_call_rows<R>(body: impl FnOnce() -> R) -> R {
    let previous = REGION_CALL_ROWS.with(|rows| rows.replace(true));
    let result = body();
    REGION_CALL_ROWS.with(|rows| rows.set(previous));
    result
}

/// The plan of row `pc`: nothing where control never reaches it, a leave row, or the row with the
/// rows before it its kind needs.
fn plan_row(layout: &Layout, (reprs, kinds): (&Reprs, Option<&OperandKinds>), reached: bool, pc: u32, row: &Decoded, helpers: LaneHelpers) -> Result<Planned, String> {
    if !reached {
        return Ok(skipped());
    }
    if leaves(layout, row) {
        return plan_step(&Decoded::step(Step::Leave, None), layout, pc, helpers);
    }
    // The preludes come before the row's own run (a chain's spills): their leaves write the pinned registers back themselves.
    let mut planned = plan(row, layout, pc, (is_general(row, reprs), variant_of(row.kind, kinds)), helpers)?.after(preludes(layout, row, pc)?);
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
    /// What each of the spanned instructions' sites saw of its operands (`OperandKinds`), from the
    /// header: which variant of row an arithmetic or comparison site takes (`variant_of`) and whether it is
    /// widened. Empty where nothing was recorded.
    pub(in crate::vm::compiled) kinds: &'a [OperandKinds],
}

/// The body of the function `spec` names that `region` spans, over the callees its call sites
/// recorded, counted under the trace. An error where its first row leaves at once: such a body is
/// pure entry cost.
///
/// # Safety
/// Each of `callees` names its function's live entry cell (`Callees::facts`).
pub(in crate::vm::compiled) unsafe fn build_body(region: &LoopRegion, spec: &BodySpec, callees: &Callees, helpers: LaneHelpers) -> Result<Body, String> {
    // SAFETY: the caller's contract.
    unsafe { build(region, spec, (Ending::Body, Views::Frame), callees, helpers) }
}

/// The code of the loop region `region` spans of the function `spec` names, over its entry's
/// element `views` (the entry record's, unless a call row it keeps changes what the entry recorded) and the callees its call sites recorded: the loop's
/// calls inlined or chained as a body's are, the rest of its rows its own.
///
/// # Safety
/// Each of `callees` names its function's live entry cell (`Callees::facts`).
pub(in crate::vm::compiled) unsafe fn build_region(region: &LoopRegion, spec: &BodySpec, views: ElementViews, callees: &Callees, helpers: LaneHelpers) -> Result<Body, String> {
    // SAFETY: the caller's contract.
    unsafe { build(region, spec, (Ending::Region, views.into()), callees, helpers) }
}

/// What a built stitch adds to the trace's totals: a body's rows and exits, or a region's span and
/// bytes with the time it took.
fn tally(built: &Result<Body, String>, ending: Ending, (span, nanos): (u64, u64)) {
    note(|totals, rejects| match (built, ending) {
        (Ok(body), Ending::Body) => {
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

/// The stitch of `region` (`ending`: what it is, and for a region how its arrays are viewed).
///
/// # Safety
/// As `build_body`.
unsafe fn build(region: &LoopRegion, spec: &BodySpec, (ending, views): (Ending, Views), callees: &Callees, helpers: LaneHelpers) -> Result<Body, String> {
    let started = std::time::Instant::now();
    // SAFETY: the caller's contract.
    let facts = unsafe { callees.facts() };
    let function = spec.known.program.function(spec.function as usize);
    let sites = Sites { function, id: spec.function, callees, facts: &facts, context: Context::root(spec.function, inline_base(function, spec.registers), spec.known.stitch), registers: spec.registers, scratch: inline_base(function, spec.registers), known: spec.known, ending, views };
    let built = assemble(region, spec, sites, helpers);
    tally(&built, ending, (u64::from(region.backedge - region.header) + 1, started.elapsed().as_nanos() as u64));
    built
}

/// The first register past `function`'s own (`registers`) that the code's rows may write: an inlined
/// callee's window and a body's scratch (`expanded`). A region and a body alike run over an activation
/// that may keep its environment window (its header words, the environment among them, and its
/// locals) right past its registers, in a lane activation's value stack or a VM frame's register file
/// (`lane_context_locals_in_window`); the rows' registers start past it, so they never overlap it
/// whatever the activation, and the run is admitted only where the storage reaches them
/// (`dispatch_fast::lane_frame_holds_window`, `lane_body_admits`). A body's scratch written at
/// `registers` overwrote the environment word (JetStream earley-boyer's "invalid local environment").
fn inline_base(function: &crate::bytecode::Function, registers: u16) -> u16 {
    registers.saturating_add(crate::vm::dispatch_fast::WINDOW_LOCALS).saturating_add(function.locals)
}

/// The bit of a widened key that says the instruction is an inlined callee's (`inlined_exit_key`).
const INLINED_KEY: usize = 1 << (usize::BITS - 1);

/// The key a leave inside an inlined callee is counted and widened by: the callee's instruction `pc`
/// of the frame whose call is at the caller's PC `call`.
pub(in crate::vm) fn inlined_exit_key(call: u32, pc: u32) -> usize {
    INLINED_KEY | (call as usize) << u32::BITS | pc as usize
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

/// What each row's site saw of its operands: the caller's instructions' (`kinds`, from the region's
/// header), none for the rows of an inlined callee.
fn row_kinds(region: &LoopRegion, origins: &[Origin], kinds: &[OperandKinds]) -> Vec<Option<OperandKinds>> {
    origins.iter().map(|origin| match origin {
        Origin::Caller(pc) => kinds.get((*pc - region.header) as usize).copied(),
        Origin::Inlined { .. } => None,
    }).collect()
}

/// Every caller instruction as its parts: its row, or its call's site, and a returning call's
/// completion. The calls are planned first: which a site takes decides what the rest decode by
/// (`decode_facts`).
fn parts(region: &LoopRegion, sites: &mut Sites, helpers: LaneHelpers) -> Vec<Part> {
    let at = |k: usize| region.header + k as u32;
    let planned: Vec<(usize, WideInstruction)> = region.instructions().iter().copied().enumerate().filter(|(_, instruction)| sites.plans(*instruction, helpers)).collect();
    let mut calls: Vec<Option<Vec<What>>> = region.instructions().iter().map(|_| None).collect();
    for (k, instruction) in planned {
        calls[k] = Some(call(region, at(k), sites, instruction));
    }
    let facts = decode_facts(region, sites, &calls);
    let mut parts: Vec<Part> = region.instructions().iter().enumerate().flat_map(|(k, instruction)| {
        let pc = at(k);
        let whats = calls[k].take().or_else(|| expanded(sites, pc, *instruction)).unwrap_or_else(|| vec![cover(region, pc, sites, *instruction, (facts, helpers))]);
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
    let stitched = stitch(region, parts(region, &mut sites, helpers), spec.registers, spec.known.invalidation, ending);
    let scratch = scratch(&stitched.rows, spec.registers);
    let span = Span::of(region, stitched.rows.len(), &stitched.live);
    let reached = reachable(&span, &stitched.rows)?;
    if ending == Ending::Body && stitched.rows[0].step == Some(Step::Leave) {
        return Err("first instruction leaves".to_string());
    }
    let rows: Vec<Decoded> = stitched.rows.into_iter().enumerate().map(|(k, row)| if reached.contains(k) { row } else { Decoded::step(Step::Leave, None) }).collect();
    let kinds = row_kinds(region, &stitched.origins, spec.kinds);
    let (reprs, homes, webs, decoded) = analyze(&span, &rows, (widened_rows(&stitched.origins, &stitched.frames, spec.widened).as_slice(), kinds.as_slice()), spec.seen);
    if ending == Ending::Region && let Some(generic) = decoded.iter().enumerate().find(|(k, row)| reached.contains(*k) && interprets(row)) {
        return Err(format!("interprets in the loop: row {} {:?}", generic.0, generic.1.step));
    }
    let invalidation = spec.known.invalidation.filter(|_| decoded.iter().any(Decoded::holds_stable) && decoded.iter().any(Decoded::makes_calls));
    let realm = (!stitched.frames.is_empty()).then_some(spec.realm).or_else(|| held_realm(&decoded));
    let layout = Layout { span, homes, webs, ending, callees, origins: &stitched.origins, copies: &stitched.copies, entries: &stitched.entries, invalidation, realm };
    let singles = decoded.iter().enumerate().map(|(k, row)| plan_row(&layout, (&reprs, kinds[k].as_ref()), reached.contains(k), k as u32, row, helpers)).collect::<Result<Vec<_>, _>>()?;
    let mut planned = tile::tiled(&layout, &decoded, singles);
    let (code, resumes) = link(&layout, &planned)?;
    residuals::report_rows(&layout, region.header, &decoded, &planned, code.len());
    let exits = decoded.iter().enumerate().filter(|(k, row)| reached.contains(*k) && leaves(&layout, row)).count();
    let loops = decoded.iter().enumerate().any(|(k, row)| row.target.is_some_and(|target| target <= k as u32));
    let covered = reached.count() - exits;
    let bare = realm.is_none() && decoded.iter().all(|row| row.heap.is_none() && !row.makes_calls() && !row.step.is_some_and(Step::reads_payload));
    let records = planned.iter_mut().filter_map(|row| row.owned.take()).collect();
    let (atoms, holders) = (held_atoms(&decoded), held_holders(&decoded));
    Ok(Body { code, covered, exits, loops, records, frames: stitched.frames, stable: distinct(stitched.facts.into_iter().chain(held_stores(spec.known, region))), atoms, holders,
        instanceofs: held_instanceofs(&decoded), constructors: held_constructors(&decoded), bare,
        layouts: held_layouts(&decoded), slot_types: held_slot_types(&decoded), resumes, scratch })
}
