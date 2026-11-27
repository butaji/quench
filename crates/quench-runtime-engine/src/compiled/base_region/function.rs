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
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::call_plan::inline::Context;
use super::call_plan::{CallForm, CallShape, DirectFacts};
use super::callees::{chains, Callees};
use super::super::LaneHelpers;
use quench_runtime::bytecode::Function;
use quench_runtime::tier_surface::GlobalFact;
use quench_bjit::call_record::CallRecord;
use quench_bjit::{padded_holes, HOLES};

pub(crate) mod calls;
pub(crate) mod planning;
pub(crate) mod rows;
pub(crate) mod switch;

use calls::*;
use planning::*;
use rows::*;

/// A function's code: bytes entered at the lane body ABI, and what its rows are.
pub(crate) struct Body {
    pub(crate) code: Vec<u8>,
    /// Instructions that run as rows (a `Return` included), and those that leave the body.
    pub(crate) covered: usize,
    pub(crate) exits: usize,
    /// Whether it jumps backward: a loop pays an entry's cost over its iterations.
    pub(crate) loops: bool,
    /// The records its direct call rows read, which its code must not outlive.
    pub(crate) records: Vec<Box<CallRecord>>,
    /// The calls it runs as their callees' rows, by frame.
    pub(crate) frames: Vec<InlinedCall>,
    /// The Stable facts it folded, which its publication subscribes.
    pub(crate) stable: Box<[GlobalFact]>,
    /// The atoms its stores run in place for, held unnamed by method caches (a Stable fact): a
    /// cache naming one retires the body (`StableFacts::atoms`).
    pub(crate) atoms: Box<[Atom]>,
    /// The exact shapes of the holders whose values its folded reads hold (a Stable fact): a change
    /// of a value a prototype of one holds retires the body (`StableFacts::holders`).
    pub(crate) holders: Box<[u32]>,
    /// The prototype chains its folded reads, membership tests and adding stores read through (a
    /// Stable fact): a layout change of a prototype on one retires the body (`StableFacts::chains`).
    pub(crate) chains: Box<[quench_runtime::code_dependencies::PrototypeChain]>,
    /// What its `instanceof` fast arms hold of their constructors' `prototype`s (a Stable fact): a write of a `prototype`,
    /// a departure of a constructor's shape or a change of the `@@hasInstance` holder's values retires the body
    /// (`StableFacts::instanceofs`).
    pub(crate) instanceofs: Box<[InstanceofFact]>,
    /// The constructors those arms guard their operands against by identity, which a collection must not free
    /// while the body lives (`LaneMethod::constructors`).
    pub(crate) constructors: Box<[Value]>,
    /// Whether it reads no heap row, makes no step and proves no realm: a loop region that does not
    /// needs from its payload only the entry record's address (`Crossing::Bare`).
    pub(crate) bare: bool,
    /// The global layouts and slot types its rows hold (a Stable fact): a change retires the body
    /// (`StableFacts::layouts`, `StableFacts::slots`).
    pub(crate) layouts: Box<[quench_runtime::tier_surface::GlobalLayoutFact]>,
    pub(crate) slot_types: Box<[quench_runtime::code_dependencies::SlotDependency]>,
    /// Where a run that left a loop region at a PC re-enters it (`resume`); none for a body.
    pub(crate) resumes: ResumeTable,
    /// The registers its rows name past the function's own: its inlined callees' windows and the scratch of its
    /// `instanceof`/`in` jumps, whose frame slots its recovery room covers (`LaneRecoverySpace::reaching`).
    pub(crate) scratch: usize,
    /// Whether its inline-cache rows hold shape ids, which a shape compaction renumbers
    /// (`LaneMethod::names_shapes`).
    pub(crate) names_shapes: bool,
    /// The run-time addresses the code names by symbol (`SymbolSite`), which a linked image's loader fills; none
    /// for a body built for this process.
    pub(crate) symbols: Box<[SymbolSite]>,
    /// The code offset of each PC's first row, ascending by PC, for the PCs a control may enter (`link::Linked`).
    pub(crate) entries: Box<[(u32, usize)]>,
    /// The PCs with no stencil row of their own, which take the generic row (`steps::generic_row`), ascending.
    pub(crate) generic: Box<[u32]>,
    /// The parameter registers whose Number guards its proven-arguments entry skips (`RowEntries::numbers`).
    pub(crate) number_guards: u64,
    /// Whether the activation reads its `this` (`RowEntries::uses_this`), so a direct call writes the receiver for it.
    pub(crate) uses_this: bool,
}

/// What a stitch decides its call sites by: the callees they recorded and the stitch's budget, the
/// program facts its rows read, and what it stitches (`Ending`), with, for a region, whether its
/// arrays' views are the entry record's (`Facts::Entry::views`).
struct Sites<'a> {
    function: &'a Function,
    /// The index of `function` in its program.
    id: u32,
    callees: &'a Callees,
    /// By PC, every callee the call site recorded, the first the one it may inline (`Callees::targets`).
    targets: &'a [Vec<DirectFacts>],
    context: Context,
    registers: u16,
    /// The first register the rows may write past the function's own (`inline_base`).
    scratch: Register,
    known: Known<'a>,
    ending: Ending,
    views: Views,
}

/// A row that runs no code: a row control never reaches, or one a tile covers.
pub(crate) fn skipped() -> Planned {
    static EMPTY: Stencil = Stencil { code: Vec::new(), relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0, raw_len: 0, hot_len: 0 };
    Planned::single(&EMPTY, [0; HOLES], None, None)
}

/// What a body is built for: its function and registers, the rows whose int32 speculation failed
/// (`widened`, by caller PC), the types its entry held by register (`seen`, its declared Numbers joining as Number types), and what
/// the program holds Stable (`Known`), whose invalidation word its code reads where a call may
/// have changed a fact it folded, and the realm its inlined callees belong to (`RealmProof`).
pub(crate) struct BodySpec<'a> {
    pub(crate) function: u32,
    pub(crate) registers: u16,
    pub(crate) known: Known<'a>,
    /// The realm the body proves active at its entry; none where it folds nothing realm-bound (an image's body).
    pub(crate) realm: Option<RealmProof>,
    pub(crate) widened: &'a [usize],
    /// The type each entry register held when the compile observed it, by register (a register past the slice
    /// is no observation: `Type::ANY`).
    pub(crate) seen: &'a [Type],
    /// What each of the spanned instructions' sites saw of its operands (`OperandKinds`), from the
    /// header: which variant of row an arithmetic or comparison site takes (`variant_of`) and whether it is
    /// widened. Empty where nothing was recorded.
    pub(crate) kinds: &'a [OperandKinds],
    /// The typed array kind each spanned instruction's interpreter record observed, from the header
    /// (`Observation::typed_rows`); empty where nothing was recorded.
    pub(crate) typed: &'a [Option<quench_runtime::tier_surface::TypedArrayKind>],
}

/// A class-typed parameter's prototype as the entry observed it: the parameter's register, the receiver's exact shape
/// and prototype link (the receiver check of `PrototypeProof::Linked`, `entry::shape_link_matches`), the prototype's
/// exact shape (`holder`, which the body subscribes to, so a change of its values retires the body), and the prototype's
/// own data values of the named reads the body makes of the parameter (`members`, by atom, as bits).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClassProto {
    pub(crate) register: quench_runtime::bytecode::Register,
    pub(crate) shape: u32,
    pub(crate) link: u32,
    pub(crate) holder: u32,
    pub(crate) members: Vec<(quench_runtime::bytecode::Atom, u64)>,
}

/// A loop-invariant receiver and the exact shape its field accesses saw at the region's entry: the entry checks the shape
/// once (`Heap::Shape`), and every access of the register the proofs carry takes its known-shape row (`proofs::prove`).
/// `flat`: every access saw the shape in flat form too, so the entry checks the form (`Heap::ShapeFlat`) and the accesses
/// take the flat rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShapeProto {
    pub(crate) register: quench_runtime::bytecode::Register,
    pub(crate) shape: u32,
    pub(crate) flat: bool,
}

/// The body of the function `spec` names that `region` spans, over the callees its call sites
/// recorded, counted under the trace. An error where its first row leaves at once: such a body is
/// pure entry cost.
///
/// # Safety
/// Each of `callees` names its function's live entry cell (`Callees::targets`).
pub(crate) unsafe fn build_body(region: &LoopRegion, spec: &BodySpec, callees: &Callees, helpers: LaneHelpers) -> Result<Body, String> {
    // SAFETY: the caller's contract.
    unsafe { build(region, spec, (Ending::Body, Views::Frame), callees, helpers) }
}

/// The code of the loop region `region` spans of the function `spec` names, over its entry's
/// element `views` (the entry record's, unless a call row it keeps changes what the entry recorded) and the callees its call sites recorded: the loop's
/// calls inlined or chained as a body's are, the rest of its rows its own.
///
/// # Safety
/// Each of `callees` names its function's live entry cell (`Callees::targets`).
pub(crate) unsafe fn build_region(region: &LoopRegion, spec: &BodySpec, views: ElementViews, callees: &Callees, helpers: LaneHelpers) -> Result<Body, String> {
    // SAFETY: the caller's contract.
    unsafe { build(region, spec, (Ending::Region, Views::of(views, region)), callees, helpers) }
}

/// What a built stitch adds to the trace's totals: a body's rows and exits, or a region's span and
/// bytes.
fn tally(built: &Result<Body, String>, ending: Ending, span: u64) {
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
    // SAFETY: the caller's contract.
    let targets = unsafe { callees.targets() };
    let function = spec.known.program.function(spec.function as usize);
    // The prototypes the entry may check and the rows may fold on: none where the function's rows reshape an object.
    let classes = super::declared::class_prototypes(function, spec.known.classes);
    let known = Known { classes: &classes, ..spec.known };
    let sites = Sites { function, id: spec.function, callees, targets: &targets, context: Context::root(spec.function, inline_base(function, spec.registers), spec.known.stitch), registers: spec.registers, scratch: inline_base(function, spec.registers), known, ending, views };
    let built = assemble(region, spec, sites, helpers).map(|body| with_native_holders(body, callees));
    tally(&built, ending, u64::from(region.backedge - region.header) + 1);
    built
}

/// The first register past `function`'s own (`registers`) that the code's rows may write: an inlined
/// callee's window and a body's scratch (`expanded`). A region and a body alike run over an activation
/// that may keep its environment window (its header words, the environment among them, and its
/// locals) right past its registers, in a lane activation's value stack or a VM frame's register file
/// (`context_locals_in_window_in_lane`); the rows' registers start past it, so they never overlap it
/// whatever the activation, and the run is admitted only where the storage reaches them
/// (`dispatch_fast::frame_holds_window_in_lane`, `body_admits_in_lane`). A body's scratch written at
/// `registers` overwrote the environment word (JetStream earley-boyer's "invalid local environment").
fn inline_base(function: &quench_runtime::bytecode::Function, registers: u16) -> u16 {
    registers.saturating_add(crate::dispatch_fast::WINDOW_LOCALS).saturating_add(function.locals)
}

/// `body` with the holders its builtin calls' guards depend on (`Callees::native_holders`) among its own.
fn with_native_holders(mut body: Body, callees: &Callees) -> Body {
    body.holders = body.holders.iter().copied().chain(callees.native_holders()).collect();
    body
}

/// The bit of a widened key that says the instruction is an inlined callee's (`inlined_exit_key`).
const INLINED_KEY: usize = 1 << (usize::BITS - 1);

/// The key a leave inside an inlined callee is counted and widened by: the callee's instruction `pc`
/// of the frame whose call is at the caller's PC `call`.
pub(crate) fn inlined_exit_key(call: u32, pc: u32) -> usize {
    INLINED_KEY | (call as usize) << u32::BITS | pc as usize
}

fn assemble(region: &LoopRegion, spec: &BodySpec, mut sites: Sites, helpers: LaneHelpers) -> Result<Body, String> {
    let (ending, callees, function) = (sites.ending, sites.targets, sites.function);
    // The class-typed receivers the entry checks, as `build` limited them (`Known::classes`): the same list the rows fold on.
    let prototypes = sites.known.classes.to_vec();
    // The loop-invariant receivers whose entry shape is checked (`Known::shapes`, already limited to those the loop never writes).
    let shapes = sites.known.shapes.to_vec();
    let stitched = stitch(region, parts(region, &mut sites, helpers), spec.registers, spec.known.invalidation, ending);
    let scratch = scratch(&stitched.rows, spec.registers);
    let span = Span::of(region, stitched.rows.len(), &stitched.live);
    let reached = reachable(&span, &stitched.rows)?;
    // An image body covers every function of its program: one whose first record leaves runs it in the lane's interpreter.
    if ending == Ending::Body && stitched.rows[0].step == Some(Step::Leave) && helpers != LaneHelpers::Ahead {
        return Err("first instruction leaves".to_string());
    }
    let mut rows: Vec<Decoded> = stitched.rows.into_iter().enumerate().map(|(k, row)| if !reached.contains(k) { Decoded::step(Step::Leave, None) } else { row }).collect();
    super::exact::promote(function, &spec.known.program.constants, &stitched.origins, &mut rows);
    let kinds = row_kinds(region, &stitched.origins, spec.kinds);
    typed_rows(region, &stitched.origins, spec.typed, &mut rows);
    let seen = super::declared::seen(function, ending, spec.seen);
    let (reprs, homes, webs, mut decoded) = analyze(&span, &rows, (widened_rows(&stitched.origins, &stitched.frames, spec.widened).as_slice(), kinds.as_slice()), &seen);
    feedback_admits(&mut decoded, &kinds, spec.known.fields);
    let shaped: Vec<Register> = prototypes.iter().map(|proto| proto.register).collect();
    // The entry seeds the direct objects it checks: the declared ones, and each shape-checked receiver (`Heap::Shape`).
    let objects: Vec<Register> = super::declared::exact_objects(function, &prototypes).into_iter().chain(shapes.iter().map(|proto| proto.register)).collect();
    let loops: Vec<Register> = shapes.iter().map(|proto| proto.register).collect();
    let flat: Vec<Register> = shapes.iter().filter(|proto| proto.flat).map(|proto| proto.register).collect();
    // The slot-type proofs a row keeps (`Decoded::typed`) are settled before the receiver proofs, which an object read's
    // destination joins (`proofs::typed_object`).
    super::repr::keep_typed_reads(&homes, &mut decoded);
    proofs::prove(&span, &mut decoded, &reached, &objects, &shaped, &loops, &flat);
    // The run's stores word is checked at the entry where a store is admitted on it (`Heap::Stores`); a store that re-reads
    // the word itself (`Decoded::recheck`) needs no entry test, and the stores after it are admitted by that test.
    let stores = decoded.iter().any(|row| row.admitted && !row.recheck);
    if ending == Ending::Region && let Some(generic) = decoded.iter().enumerate().find(|(k, row)| reached.contains(*k) && interprets(row)) {
        return Err(format!("interprets in the loop: row {} {:?}", generic.0, generic.1.step));
    }
    let invalidation = spec.known.invalidation.filter(|_| decoded.iter().any(Decoded::holds_stable) && decoded.iter().any(Decoded::makes_calls));
    // A body proves the realm it was compiled in at its entry, whatever it folds: a program shared by
    // realms enters it only in that one, so a realm switch need not retire it (`LaneMethod::realm_bound`).
    let realm = (ending == Ending::Body || !stitched.frames.is_empty()).then_some(spec.realm).flatten().or_else(|| held_realm(&decoded));
    let relies: Vec<bool> = decoded.iter().map(|row| row.relies).collect();
    let (guards, numbers) = (super::declared::entry_guards(function), super::declared::entry_numbers(function));
    // A declared Number's rows rely on its entry guard, and an inlined frame's copies run at a leave: those guards stay.
    let kept: Vec<Register> = numbers.iter().copied().chain(stitched.copies.iter().flatten().flat_map(|&(param, arg)| [param, arg])).collect();
    let entry_reads = numbers::entry_reads(&span, &decoded, &homes, &kinds, &kept);
    let number_held = numbers::proven_operands(&span, &decoded, &reached, &homes, &webs, &entry_reads, &kinds, ending, &stitched.entries);
    let ints = numbers::proven_int_operands(&span, &decoded, &reached, &homes, &webs, ending, &stitched.entries);
    for (row, (int_a, int_b)) in decoded.iter_mut().zip(ints) {
        (row.int_a, row.int_b) = (int_a, int_b);
    }
    let mut identities: Vec<(u16, u64)> = sites.callees.captured_identities(region.header as usize..=region.backedge as usize).collect();
    identities.sort_unstable();
    identities.dedup();
    // The entry guards hold the pinned int32 and double webs live at the header (`entry_rows`, which every entry runs), so
    // the argument proof starts from them as well as from the declared Numbers.
    let (pinned_ints, pinned_doubles) = super::call_args::entry_pins(&homes, &webs, &span);
    let seeded: Vec<Register> = numbers.iter().copied().chain(pinned_doubles).collect();
    let proven_args = super::call_args::ProvenArgs::new(function, &spec.known.program.constants, seeded, pinned_ints);
    let chain_holders = std::cell::RefCell::new(Vec::new());
    let builtins = sites.callees.builtins();
    let layout = Layout { id: spec.function, span, homes, webs, ending, callees, origins: &stitched.origins, copies: &stitched.copies, entries: &stitched.entries, invalidation, realm, relies: &relies, number_held: &number_held, entry_reads: &entry_reads, proven_args: &proven_args, guards, numbers, prototypes, shapes, stores, identities, reads: sites.known.reads, holders: &chain_holders, builtins: &builtins };
    let singles = decoded.iter().enumerate().map(|(k, row)| plan_row(&layout, (&reprs, kinds[k].as_ref()), &decoded, reached.contains(k), k as u32, row, helpers)).collect::<Result<Vec<_>, _>>()?;
    if helpers == LaneHelpers::Ahead && quench_runtime::trace::Channel::AotLanes.on() {
        report_steps(&layout, spec.known.program, &singles);
    }
    let mut planned = tile::tiled(&layout, &decoded, singles);
    // An image body names the process statics its rows take by symbol: their addresses are this process's.
    if helpers == LaneHelpers::Ahead {
        planned.iter_mut().try_for_each(name_process_statics)?;
    }
    let linked = link(&layout, &planned)?;
    residuals::report_rows(&layout, region.header, &decoded, &planned, linked.code.len());
    let generic = generic_records(&layout, &decoded, &reached);
    let exits = decoded.iter().enumerate().filter(|(k, row)| reached.contains(*k) && leaves(&layout, row)).count();
    let loops = decoded.iter().enumerate().any(|(k, row)| row.target.is_some_and(|target| target <= k as u32));
    let covered = reached.count() - exits;
    // A bare run binds no window header (`stencil_run::run_bare`), so a body that reads the captures word
    // (`CONTEXT_BODY`: a capture read of any row, or an entry identity test of a captured callee) is no bare one.
    let reads_captures = !layout.identities.is_empty() || !region.captures.is_empty() || !region.stored_captures.is_empty();
    let bare = realm.is_none() && !reads_captures
        && decoded.iter().all(|row| row.heap.is_none() && !row.makes_calls() && !row.step.is_some_and(Step::reads_payload));
    let records = planned.iter_mut().flat_map(|row| std::mem::take(&mut row.owned)).collect();
    let atoms = held_atoms(&decoded);
    // A class-typed parameter's prototype shape joins the holders the body subscribes to, so a value change there retires it.
    let holders: Box<[u32]> = held_holders(&decoded).iter().copied().chain(layout.prototypes.iter().map(|proto| proto.holder)).chain(layout.holders.borrow().iter().copied())
        .collect::<std::collections::BTreeSet<u32>>().into_iter().collect();
    let names_shapes = decoded.iter().any(|row| row.ic.is_some());
    Ok(Body { code: linked.code, covered, exits, loops, records, frames: stitched.frames, stable: distinct(stitched.facts.into_iter().chain(held_stores(spec.known, region)).chain(sites.callees.held_facts())), atoms, holders,
        chains: held_chains(&decoded), instanceofs: held_instanceofs(&decoded), constructors: held_constructors(&decoded), bare,
        layouts: held_layouts(&decoded), slot_types: held_slot_types(&decoded), resumes: linked.resumes, scratch, names_shapes, number_guards: linked.number_guards,
        uses_this: quench_runtime::bytecode::activation_reads_this(spec.known.program, function),
        symbols: linked.symbols.into(), entries: linked.entries.into(), generic })
}

/// Names by symbol each hole of `row` and its parts that holds a process static a row takes: the narrow
/// decode's upper-word table (`NarrowLayout::upper_words`: field and element reads) and the attention byte
/// (`execution_interrupt::attention_address`: polls). An image's loader fills them, as the compiling
/// process's addresses mean nothing where the image runs.
fn name_process_statics(row: &mut Planned) -> Result<(), String> {
    use crate::aot_image::Symbol;
    let statics = [(quench_runtime::value::NarrowLayout::upper_words() as u64, Symbol::NarrowUpperWords), (quench_runtime::execution_interrupt::attention_address(), Symbol::Attention)];
    row.pre.iter_mut().chain(row.post.iter_mut()).try_for_each(name_process_statics)?;
    for hole in 0..row.holes.len() {
        let Some(&(_, symbol)) = statics.iter().find(|(address, _)| row.holes[hole] == *address) else { continue };
        if row.symbolic.iter().flatten().any(|(named, _)| usize::from(*named) == hole) { continue; }
        let free = row.symbolic.iter_mut().find(|slot| slot.is_none()).ok_or("process static: no symbolic hole left")?;
        *free = Some((hole as u8, symbol));
    }
    Ok(())
}

/// An element row no loop entry proves (`Heap::Index`, `Heap::Store` with no entry hole) whose interpreter
/// record observed a native typed kind becomes that kind's typed row, which proves the array's view for
/// the kind on each access (`TYPED_INDEX_ROW`, `TYPED_STORE_ROW` without entry) instead of dispatching on it.
fn typed_rows(region: &LoopRegion, origins: &[super::super::lane_unit::Origin], typed: &[Option<quench_runtime::tier_surface::TypedArrayKind>], rows: &mut [Decoded]) {
    for (row, origin) in rows.iter_mut().zip(origins) {
        let super::super::lane_unit::Origin::Caller(pc) = origin else { continue };
        let Some(Some(kind)) = typed.get((*pc - region.header) as usize) else { continue };
        let Some(native) = quench_runtime::staged::typed_elements::NativeKind::new(*kind) else { continue };
        if row.hole != 0 { continue; }
        row.heap = match row.heap {
            // An integer kind's read delivers a double (`TYPED_INDEX_ROW`'s Float destination), which an index or
            // bitwise use would convert back: only a float kind's element (its type is the Number) takes the typed row.
            Some(Heap::Index) if native.element_type() == quench_runtime::typed_ir::Type::NUMBER => Some(Heap::TypedIndex(*kind as u8)),
            Some(Heap::Store) => Some(Heap::TypedStore(*kind as u8)),
            other => other,
        };
    }
}

/// Admits a packed helper row only where its site's feedback admits its direct arm (`family_helpers::admits`):
/// a binary site met Number operands only, an element site met a dense or typed element. Another site would
/// run the general step on every execution. An inlined row has no site of its own, and is not admitted.
fn feedback_admits(decoded: &mut [Decoded], kinds: &[Option<OperandKinds>], fields: Option<&dyn Fn(u16) -> bool>) {
    for (row, site) in decoded.iter_mut().zip(kinds) {
        if let Some(words) = row.packed
            && !crate::dispatch_fast::family_helpers::admits(words, *site, fields)
        {
            row.packed = None;
        }
    }
}

/// Prints each record of an image body whose rows call a general step helper (`Step`, `VmStep`), with its
/// opcode and step kind: what an image needs the interpreter's semantics for (the aot-lanes trace).
fn report_steps(layout: &Layout, program: &ResidualProgram, singles: &[Planned]) {
    fn steps(row: &Planned) -> Option<&'static str> {
        let named = row.symbolic.iter().flatten().find_map(|(_, symbol)| match symbol {
            crate::aot_image::Symbol::Helper(helper) => crate::compiled::LaneHelper::name(*helper as usize).filter(|name| matches!(*name, "Step" | "VmStep" | "StepPacked")),
            _ => None,
        });
        named.or_else(|| row.pre.iter().chain(row.post.iter()).find_map(steps))
    }
    let code: Vec<_> = program.function(layout.id as usize).instructions().collect();
    for (k, helper) in singles.iter().enumerate().filter_map(|(k, row)| steps(row).map(|helper| (k, helper))) {
        match layout.origins.get(k) {
            Some(crate::compiled::lane_unit::Origin::Caller(pc)) => {
                let name = code.get(*pc as usize).map_or("?", |instruction| quench_runtime::bytecode::Op::NAMES[instruction.op() as usize]);
                eprintln!("aot-lanes: step record {} pc {pc} {name} {helper}", layout.id);
            }
            _ => eprintln!("aot-lanes: step record {} row {k} inlined {helper}", layout.id),
        }
    }
}
