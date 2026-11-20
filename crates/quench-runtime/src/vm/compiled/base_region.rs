//! A Number loop region built from BaseJIT stencils (`quench-bjit`) instead of Cranelift IR,
//! behind `QUENCH_DEV=bjit`, as the same stitched program of rows a function's body is
//! (`function`, `stitch`): the loop's span of its function, its small monomorphic calls inlined.
//! The registers the region touches most live in pinned machine registers through the whole chain
//! of stencils and the rest stay tagged `Value` words in the activation's register file; every exit
//! writes the pinned registers back first, so it leaves the lane's state canonical. An instruction
//! that is no row of the stencil library is lowered by `steps` (a helper step or a leave), so no
//! region is declined for its instructions.
//!
//! The result word is the exit: the PC a leave out of the region resumes at, a guard inside it
//! exits at its instruction (nothing of that instruction has run; inside an inlined callee, at the
//! callee's, `LaneExit::Inlined`), a backward jump exits at the jump when an interrupt is
//! requested so the interpreter's safepoint runs, and an entry guard that finds a register that is
//! no Number returns `LaneExit::Entry`.

use super::lane_unit::{InlinedCall, Origin};
use super::osr_loop::LoopRegion;
use crate::bytecode::{Atom, Class, FieldBase, FieldLookup, OperandKind, Register, ResidualProgram, WideInstruction};
use crate::code_dependencies::InstanceofFact;
use crate::typed_ir::{Representation, Type};
use crate::value::Value;
use crate::vm::dispatch_fast::operand_feedback::OperandKinds;
use crate::vm::in_presence::InPresence;
use oxc_ast::ast::{BinaryOperator, UnaryOperator};
use super::base_rows::{self, Heap, HeapRow};
use super::{ElementViews, NativeFieldLoad};
use quench_bjit::numeric::{self, Kind, Place};
use quench_bjit::{PINNED, PINNED_F, Row as Placed, Stencil};
use super::LaneHelpers;
use std::collections::BTreeMap;
use std::sync::Mutex;

mod adds;
mod call_args;
mod names;
mod call_plan;
mod sites;
pub(in crate::vm::compiled) mod callees;
mod decode;
pub(in crate::vm::compiled) mod declared;
mod entry_facts;
mod exact;
mod fold;
pub(in crate::vm::compiled) mod function;
pub(in crate::vm) use function::inlined_exit_key;
mod globals;
mod link;
mod locals;
pub(in crate::vm::compiled) use locals::env_owned;
mod numbers;
mod plan;
mod proofs;
mod repr;
mod residuals;
mod resume;
mod span;
mod steps;
mod throws;
mod stitch;
mod tile;
mod webs;
use sites::*;
#[cfg(test)]
#[path = "base_region/region_tests.rs"]
mod region_tests;
use decode::*;
use entry_facts::*;
use globals::*;
use link::*;
pub(in crate::vm::compiled) use link::row_entries;
pub(in crate::vm) use link::SymbolSite;
use plan::*;
use repr::*;
pub(in crate::vm) use resume::ResumeTable;
use resume::*;
use span::*;
use steps::*;
pub(in crate::vm::compiled) use steps::covers;
use stitch::*;
use webs::*;

/// What a row reads of the running program rather than of the registers: the names its
/// instructions' atoms are, and whether a store may run in place.
#[derive(Clone, Copy)]
pub(in crate::vm::compiled) struct Known<'a> {
    pub(in crate::vm::compiled) program: &'a ResidualProgram,
    /// The compiling VM's stitch-inlining thresholds (`tier_policy::Stitch`), a Static fact of the code.
    pub(in crate::vm::compiled) stitch: &'static super::tier_policy::Stitch,
    /// The names method caches hold now, where the VM and the program run specialized, so a
    /// store of an unnamed atom may run in place; `None` where none may. Code that stores holds
    /// each such atom unnamed as a Stable fact (`Body::atoms`), which a cache naming it retires.
    pub(in crate::vm::compiled) stores: Option<&'a crate::vm::method_cache::AtomBits>,
    /// The loads a cache site proves for a read (`Vm::native_field_read_descriptors`), where the VM and
    /// the program run specialized and the code may hold Stable facts: a read of a field its receiver's
    /// prototype chain holds folds to the value the holder holds now (`folded_read`), which a change of the
    /// holder's values or of a layout on the way retires the code over (`Body::holders`, `Body::chains`). `None`
    /// where no fact may be held.
    pub(in crate::vm::compiled) reads: Option<&'a dyn Fn(u16) -> Vec<NativeFieldLoad>>,
    /// The add a cache site recorded for an atom, as a define's (`false`) or an adding store's (`true`,
    /// whose proved prototype chain is then a Stable fact) transition (`Vm::native_field_add`): a
    /// `DefineField` or an adding `SetField` runs it as `staged::add`'s row. `None` where no fact may be held.
    pub(in crate::vm::compiled) adds: Option<&'a dyn Fn(u16, Atom, bool) -> Option<crate::staged::add_plan::AddSite>>,
    /// The naming a cache site recorded for a `SetFunctionName`'s atom, with the word of the name's string
    /// (`Vm::native_function_name`): the instruction runs it as `staged::naming`'s row. `None` where no fact may be held.
    pub(in crate::vm::compiled) names: Option<&'a dyn Fn(u16, Atom) -> Option<(crate::staged::add_plan::Transition, u64)>>,
    /// The address of the code's invalidation word (`code_dependencies::Invalidation`), which code that
    /// holds a Stable fact and makes a call reads before the rows that hold it.
    pub(in crate::vm::compiled) invalidation: Option<usize>,
    /// The program's constants as the compiling VM materialized them (a string or BigInt is a heap
    /// value there, which lives as long as the program), where it has them; none for a region.
    pub(in crate::vm::compiled) constants: &'a [Value],
    /// The compiling VM's table of `typeof` result strings (`Vm::typeof_names`), indexed as
    /// `bytecode::TYPEOF_NAMES`: a `typeof` string row names it, so a value's string is the entry its tag's
    /// index selects. Empty for a region, whose `typeof` is the helper step.
    pub(in crate::vm::compiled) typeof_names: &'a [Value],
    /// The constructor the `instanceof` at a PC names and what the VM proved of it (`Vm::instanceof_fact`), where the VM and
    /// the program run specialized and the code may hold Stable facts: the row stands on the constructor's identity (its
    /// register is guarded against it) and walks the value's chain for its `prototype` link, which a write of that
    /// slot, a departure of its shape or a change of the `@@hasInstance` holder's values retires the code over
    /// (`Body::instanceofs`). `None` where no fact may be held.
    pub(in crate::vm::compiled) instanceofs: Option<&'a dyn Fn(usize) -> Option<(Value, InstanceofFact)>>,
    /// What `key in object` answered at the PC of an `in` whose key is a constant string (`Vm::membership_facts`), where
    /// the VM and the program run specialized and the code may hold Stable facts: the row answers the boolean while its
    /// object has the receiver layout (exact shape and prototype link) that answered, which a layout change of a
    /// prototype on its chain retires the code over (`Body::chains`). `None` where no fact may be held.
    pub(in crate::vm::compiled) presences: Option<&'a [Option<InPresence>]>,
    /// Whether a field cache site's primary entry holds a receiver (`Vm::field_cache_filled`), where the VM runs
    /// specialized: admits a field row's direct arm (`family_helpers::admits`). `None` where none may be held.
    pub(in crate::vm::compiled) fields: Option<&'a dyn Fn(u16) -> bool>,
    /// What each by-name global access takes (`Vm::global_table`, the b-jit's), where the code may hold
    /// Stable facts: a global object's slot under its layout (`globals`). `None` where none may.
    pub(in crate::vm::compiled) globals: Option<&'a crate::vm::global_facts::GlobalReads>,
    /// The prototypes the class-typed parameters entered with, as the compiling VM observed them (`VM::class_prototypes`):
    /// a named read of such a parameter folds to its prototype's own value (`declared::class_member_read`), and the entry
    /// checks each (`declared::class_prototypes`). Empty where none was observed.
    pub(in crate::vm::compiled) classes: &'a [function::ClassProto],
    /// The exact shapes the region's field accesses of each loop-invariant receiver saw at entry (`osr_compile::loop_shapes`):
    /// the entry checks each (`Heap::Shape`). Empty for a body, whose parameters are `classes`.
    pub(in crate::vm::compiled) shapes: &'a [function::ShapeProto],
}

/// The realm a body's inlined callees were found in: where the active realm's global object sits in
/// the VM (`Vm::realm_globals_offset`) and the global object itself. A body that inlines code runs
/// it under its own realm's globals, so its entry proves this realm active (`Heap::Realm`).
#[derive(Clone, Copy, Default)]
pub(in crate::vm::compiled) struct RealmProof {
    pub(in crate::vm::compiled) offset: u64,
    pub(in crate::vm::compiled) global: u64,
}

impl Known<'_> {
    /// Whether a store of `atom` may run in place: no method cache names it.
    fn stores_in_place(&self, atom: Atom) -> bool {
        self.stores.is_some_and(|names| !names.contains(atom as usize))
    }

    /// Whether `atom` names `length`.
    fn names_length(&self, atom: Atom) -> bool {
        super::names_length(self.program, atom)
    }
}

/// Bytes of a register slot in the activation's register file: the shared word (`call_record::REGISTER_BYTES`,
/// which the call rows read the same slots by; a test pins it to `size_of::<Value>()`).
const SLOT: u64 = quench_bjit::call_record::REGISTER_BYTES as u64;

/// A pinned register and the register of the activation it holds.
type Pin = (usize, Register);

#[derive(Default)]
struct Totals {
    regions: u64,
    ops: u64,
    bytes: u64,
    nanos: u64,
    rejected: u64,
    /// Whole-function bodies built (`function::build_body`), and their instructions that run as
    /// rows and that leave the body.
    bodies: u64,
    covered: u64,
    exits: u64,
    /// The call sites of bodies that call a callee's entry cell directly, and those that take the
    /// helper row (`call_plan::Choice`).
    direct_calls: u64,
    helper_calls: u64,
}

static TOTALS: Mutex<Totals> = Mutex::new(Totals { regions: 0, ops: 0, bytes: 0, nanos: 0, rejected: 0, bodies: 0, covered: 0, exits: 0, direct_calls: 0, helper_calls: 0 });
static REJECTS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
static LEAVES: Mutex<BTreeMap<(u32, u32), (crate::bytecode::Op, u64)>> = Mutex::new(BTreeMap::new());
static REPORT: std::sync::Once = std::sync::Once::new();

/// What a loop site did in the lane, counted under `QUENCH_TRACE=js-jit` (printed with the totals): the
/// cost of the tier machinery around the regions, which `--b-jit` pays whether or not a region exists.
#[derive(Clone, Copy)]
pub(in crate::vm) enum Event {
    /// A loop site's run entered published code.
    Entered,
    /// A run left its code at an exit (the interpreter resumed mid-loop).
    Left,
    /// A run did not start code: the site is waiting for code, or the entry declined it.
    Declined,
    /// A compilation of a loop region started (`loop_compile_in_lane`).
    Compiled,
    /// A lane body compile started (a Cranelift function tier; none under `--b-jit`).
    BodyCompile,
    /// Of those, the ones its recurrence started (`LaneView::recurrent`), not a warm-up count.
    BodyRecurrent,
    /// A body plan built its IR (`Lowered::of`), whatever the attempt came to.
    BodyPlanned,
    /// What an attempt came to (`LaneBodyOutcome`): published, still compiling, refused for
    /// its cost (unpaid), for its length (short), declined, outrun by a lifecycle edge (stale),
    /// deferred, or handed to a b-jit body (base).
    BodyPublished,
    BodyCompiling,
    BodyUnpaid,
    BodyShort,
    BodyDeclined,
    BodyStale,
    BodyDeferred,
    BodyBase,
}

/// How many `Event`s there are.
const EVENT_COUNT: usize = Event::BodyBase as usize + 1;

#[cfg(test)]
thread_local! {
    /// The rows planned as a fast arm (`Step::Fast`, or a constant a row writes as it is), by the opcode
    /// they stand for, counted whether or not the trace channel is on: a test asks that an operation's
    /// fast row exists in the bodies it compiled. Per thread, as bodies compile on the thread that runs
    /// them, so a test counts its own rows and no test running beside it answers for it.
    static FAST_ROWS: [std::cell::Cell<u64>; crate::bytecode::Op::COUNT] = [const { std::cell::Cell::new(0) }; crate::bytecode::Op::COUNT];
}

/// The fast rows planned for `op` on this thread.
#[cfg(test)]
pub(in crate::vm) fn fast_rows(op: crate::bytecode::Op) -> u64 {
    FAST_ROWS.with(|rows| rows[op as usize].get())
}

#[cfg(test)]
fn note_fast(op: crate::bytecode::Op) {
    FAST_ROWS.with(|rows| rows[op as usize].update(|count| count + 1));
}

/// The direct-cell call sites the stitcher inlined (`call_plan::plan_call`), counted whether or not
/// the trace channel is on and reported with the totals.
static INLINED_SITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn note_inlined() {
    INLINED_SITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub(in crate::vm::compiled) fn inlined_sites() -> u64 {
    INLINED_SITES.load(std::sync::atomic::Ordering::Relaxed)
}

static EVENTS: [std::sync::atomic::AtomicU64; EVENT_COUNT] = [const { std::sync::atomic::AtomicU64::new(0) }; EVENT_COUNT];

/// The most frequent leave sites a run reports.
const TOP_LEAVES: usize = 40;

/// Counts a region run leaving at `pc` of `function` (an instruction of operation `op`), where the trace channel is on.
pub(in crate::vm) fn leave(function: u32, pc: u32, op: crate::bytecode::Op) {
    if !crate::trace::Channel::BjitLeave.on() {
        return;
    }
    REPORT.call_once(|| {
        // SAFETY: `report` is an `extern "C" fn()` that touches only statics.
        unsafe { libc::atexit(report) };
    });
    if let Ok(mut leaves) = LEAVES.lock() {
        leaves.entry((function, pc)).or_insert((op, 0)).1 += 1;
    }
}

/// Whether regions and events are counted: a report that reads them is on (`report`, `census`).
fn counted() -> bool {
    static COUNTED: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| crate::trace::Channel::JsJit.on() || crate::trace::Channel::Census.on());
    *COUNTED
}

/// What regions did, for the tier census: regions built, the instructions they cover, and runs that
/// entered published code. All zero unless a report is on.
pub(in crate::vm) fn census() -> (u64, u64, u64) {
    let regions = TOTALS.lock().map_or((0, 0), |totals| (totals.regions, totals.ops));
    (regions.0, regions.1, EVENTS[Event::Entered as usize].load(std::sync::atomic::Ordering::Relaxed))
}

/// Counts `event` where a report is on.
pub(in crate::vm) fn count(event: Event) {
    if !counted() {
        return;
    }
    REPORT.call_once(|| {
        // SAFETY: `report` is an `extern "C" fn()` that touches only statics.
        unsafe { libc::atexit(report) };
    });
    EVENTS[event as usize].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

extern "C" fn report() {
    if !crate::trace::Channel::JsJit.on() {
        return;
    }
    let (Ok(totals), Ok(rejects)) = (TOTALS.lock(), REJECTS.lock()) else { return };
    let ops = totals.ops.max(1);
    eprintln!(
        "bjit regions={} rejected={} ops={} bytes={} bytes_per_op={:.1} build_ns={} build_ns_per_op={:.1}",
        totals.regions, totals.rejected, totals.ops, totals.bytes, totals.bytes as f64 / ops as f64, totals.nanos,
        totals.nanos as f64 / ops as f64
    );
    let rows = (totals.covered + totals.exits).max(1);
    eprintln!("bjit-bodies bodies={} covered={} exits={} covered_share={:.1}% direct_calls={} helper_calls={}", totals.bodies, totals.covered, totals.exits, 100.0 * totals.covered as f64 / rows as f64, totals.direct_calls, totals.helper_calls);
    let [entered, left, declined, compiled, bodies, recurrent, planned, published, compiling, unpaid, short, body_declined, stale, deferred, base] =
        std::array::from_fn::<u64, EVENT_COUNT, _>(|i| EVENTS[i].load(std::sync::atomic::Ordering::Relaxed));
    eprintln!("bjit-loops entered={entered} left={left} declined={declined} compiles={compiled} lane_body_compiles={bodies} inlined_sites={}", inlined_sites());
    eprintln!("lane-body-attempts started={bodies} recurrent={recurrent} planned={planned} published={published} compiling={compiling} unpaid={unpaid} short={short} declined={body_declined} stale={stale} deferred={deferred} base={base}");
    if let Ok(leaves) = LEAVES.lock() {
        let mut ranked: Vec<_> = leaves.iter().collect();
        ranked.sort_by(|a, b| (b.1).1.cmp(&(a.1).1).then(a.0.cmp(b.0)));
        for ((function, pc), (op, count)) in ranked.into_iter().take(TOP_LEAVES) {
            eprintln!("bjit-leave {function}:{pc} {op:?} {count}");
        }
    }
    let mut ranked: Vec<_> = rejects.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (name, count) in ranked {
        eprintln!("bjit-reject {name} {count}");
    }
}

fn note(update: impl FnOnce(&mut Totals, &mut BTreeMap<String, u64>)) {
    if !counted() {
        return;
    }
    REPORT.call_once(|| {
        // SAFETY: `report` is an `extern "C" fn()` that touches only statics.
        unsafe { libc::atexit(report) };
    });
    if let (Ok(mut totals), Ok(mut rejects)) = (TOTALS.lock(), REJECTS.lock()) {
        update(&mut totals, &mut rejects);
    }
}

/// The webs of `decoded`, their representations and homes, and the rows renamed to webs.
fn analyze(span: &Span, decoded: &[Decoded], widened: (&[usize], &[Option<OperandKinds>]), seen: &[Type]) -> (Reprs, Homes, Webs, Vec<Decoded>) {
    // A register with several webs may not have a web in an int slot, whose writes keep a
    // tagged word's high half; such a register is left whole.
    let mut merged = Bits::default();
    loop {
        let (webs, renamed) = Webs::split(span, decoded, &merged);
        let reprs = representations(span, &renamed, widened, seen, &webs);
        let homes = Homes::of(&ranked(&renamed, &reprs, Some(Representation::Int32)), &ranked(&renamed, &reprs, Some(Representation::Float64)), webs.orig.clone(), &webs.clashes(span, &renamed));
        let split: Vec<Register> = homes.slots.iter().map(|web| webs.orig[usize::from(*web)]).filter(|register| webs.count(*register) > 1).collect();
        if split.is_empty() {
            return (reprs, homes, webs, renamed);
        }
        for register in split {
            merged.insert(usize::from(register));
        }
    }
}
