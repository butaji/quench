//! A Number loop region built from BaseJIT stencils (`quench-bjit`) instead of Cranelift IR,
//! as the same stitched program of rows a function's body is
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

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::lane_unit::{InlinedCall, Origin};
use super::osr_loop::LoopRegion;
use quench_runtime::bytecode::{Atom, FieldBase, FieldLookup, OperandKind, Register, ResidualProgram, WideInstruction};
use quench_runtime::typed_ir::{Representation, Type};
use quench_runtime::value::Value;
use crate::operand_class::Site;
use quench_runtime::vm::in_presence::InPresence;
use oxc_ast::ast::{BinaryOperator, UnaryOperator};
use super::base_rows::{self, Heap, HeapRow};
use super::{ElementViews, NativeFieldLoad};
use quench_bjit::numeric::{self, Kind, Place};
use quench_bjit::{PINNED, PINNED_F, Row as Placed, Stencil};
use super::LaneHelpers;
use std::collections::BTreeMap;
use std::sync::Mutex;

pub(crate) mod adds;
pub(crate) mod call_args;
pub(crate) mod call_plan;
pub(crate) mod sites;
pub(crate) mod callees;
pub(crate) mod decode;
pub(crate) mod declared;
pub(crate) mod entry_facts;
pub(crate) mod exact;
pub(crate) mod fold;
pub(crate) mod function;
pub(crate) mod globals;
pub(crate) mod link;
pub(crate) mod locals;
pub(crate) use locals::env_owned;
pub(crate) mod numbers;
pub(crate) mod plan;
pub(crate) mod proofs;
pub(crate) mod repr;
pub(crate) mod residuals;
pub(crate) mod resume;
pub(crate) mod span;
pub(crate) mod steps;
pub(crate) mod throws;
pub(crate) mod stitch;
pub(crate) mod tile;
pub(crate) mod webs;
use sites::*;
#[cfg(test)]
#[path = "base_region/region_tests.rs"]
pub(crate) mod region_tests;
use decode::*;
use entry_facts::*;
use globals::*;
use link::*;
pub(crate) use link::row_entries;
pub(crate) use link::SymbolSite;
use plan::*;
use repr::*;
pub(crate) use resume::ResumeTable;
use resume::*;
use span::*;
use steps::*;
pub(crate) use steps::covers;
use stitch::*;
use webs::*;

/// What a row reads of the running program rather than of the registers: the names its
/// instructions' atoms are, and whether a store may run in place.
#[derive(Clone, Copy)]
pub(crate) struct Known<'a> {
    pub(crate) program: &'a ResidualProgram,
    /// The compiling VM's stitch-inlining thresholds (`tier_policy::Stitch`), a Static fact of the code.
    pub(crate) stitch: &'static super::tier_policy::Stitch,
    /// The names method caches hold now, where the VM and the program run specialized, so a
    /// store of an unnamed atom may run in place; `None` where none may. Code that stores holds
    /// each such atom unnamed as a Stable fact (`Body::atoms`), which a cache naming it retires.
    pub(crate) stores: Option<&'a quench_runtime::vm::method_cache::AtomBits>,
    /// The loads a cache site proves for a read (`Vm::native_field_read_descriptors`), where the VM and
    /// the program run specialized and the code may hold Stable facts: a read of a field its receiver's
    /// prototype chain holds folds to the value the holder holds now (`folded_read`), which a change of the
    /// holder's values or of a layout on the way retires the code over (`Body::holders`, `Body::chains`). `None`
    /// where no fact may be held.
    pub(crate) reads: Option<&'a dyn Fn(u16) -> Vec<NativeFieldLoad>>,
    /// The add a cache site recorded for an atom, as a define's (`false`) or an adding store's (`true`,
    /// whose proved prototype chain is then a Stable fact) transition (`Vm::native_field_add`): a
    /// `DefineField` or an adding `SetField` runs it as `staged::add`'s row. `None` where no fact may be held.
    pub(crate) adds: Option<&'a dyn Fn(u16, Atom, bool) -> Option<quench_runtime::staged::add_plan::AddSite>>,
    /// The address of the code's invalidation word (`code_dependencies::Invalidation`), which code that
    /// holds a Stable fact and makes a call reads before the rows that hold it.
    pub(crate) invalidation: Option<usize>,
    /// The program's constants as the compiling VM materialized them (a string or BigInt is a heap
    /// value there, which lives as long as the program), where it has them; none for a region.
    pub(crate) constants: &'a [Value],
    /// The compiling VM's table of `typeof` result strings (`Vm::typeof_names`), indexed as
    /// `bytecode::TYPEOF_NAMES`: a `typeof` string row names it, so a value's string is the entry its tag's
    /// index selects. Empty for a region, whose `typeof` is the helper step.
    pub(crate) typeof_names: &'a [Value],
    /// What `key in object` answered at the PC of an `in` whose key is a constant string (`Vm::membership_facts`), where
    /// the VM and the program run specialized and the code may hold Stable facts: the row answers the boolean while its
    /// object has the receiver layout (exact shape and prototype link) that answered, which a layout change of a
    /// prototype on its chain retires the code over (`Body::chains`). `None` where no fact may be held.
    pub(crate) presences: Option<&'a [Option<InPresence>]>,
    /// Whether a field cache site's primary entry holds a receiver (`Vm::field_cache_filled`), where the VM runs
    /// specialized: admits a field row's direct arm (`family_helpers::admits`). `None` where none may be held.
    pub(crate) fields: Option<&'a dyn Fn(u16) -> bool>,
    /// What each by-name global access takes (`Vm::global_table`, the b-jit's), where the code may hold
    /// Stable facts: a global object's slot under its layout (`globals`). `None` where none may.
    pub(crate) globals: Option<&'a quench_runtime::vm::global_facts::GlobalReads>,
    /// The prototypes the class-typed parameters entered with, as the compiling VM observed them (`VM::class_prototypes`):
    /// a named read of such a parameter folds to its prototype's own value (`declared::class_member_read`), and the entry
    /// checks each (`declared::class_prototypes`). Empty where none was observed.
    pub(crate) classes: &'a [function::ClassProto],
}

/// The realm a body's inlined callees were found in: where the active realm's global object sits in
/// the VM (`Vm::realm_globals_offset`) and the global object itself. A body that inlines code runs
/// it under its own realm's globals, so its entry proves this realm active (`Heap::Realm`).
#[derive(Clone, Copy, Default)]
pub(crate) struct RealmProof {
    pub(crate) offset: u64,
    pub(crate) global: u64,
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

static TOTALS: Mutex<Totals> = Mutex::new(Totals { regions: 0, ops: 0, bytes: 0, rejected: 0, bodies: 0, covered: 0, exits: 0, direct_calls: 0, helper_calls: 0 });
static REJECTS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
static REPORT: std::sync::Once = std::sync::Once::new();


#[cfg(test)]
thread_local! {
    /// The rows planned as a fast arm (`Step::Fast`, or a constant a row writes as it is), by the opcode
    /// they stand for, counted whether or not the trace channel is on: a test asks that an operation's
    /// fast row exists in the bodies it compiled. Per thread, as bodies compile on the thread that runs
    /// them, so a test counts its own rows and no test running beside it answers for it.
    static FAST_ROWS: [std::cell::Cell<u64>; quench_runtime::bytecode::Op::COUNT] = [const { std::cell::Cell::new(0) }; quench_runtime::bytecode::Op::COUNT];
}

/// The fast rows planned for `op` on this thread.
#[cfg(test)]
pub(crate) fn fast_rows(op: quench_runtime::bytecode::Op) -> u64 {
    FAST_ROWS.with(|rows| rows[op as usize].get())
}

#[cfg(test)]
fn note_fast(op: quench_runtime::bytecode::Op) {
    FAST_ROWS.with(|rows| rows[op as usize].update(|count| count + 1));
}

/// The direct-cell call sites the stitcher inlined (`call_plan::plan_call`), counted whether or not
/// the trace channel is on and reported with the totals.
static INLINED_SITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn note_inlined() {
    INLINED_SITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn inlined_sites() -> u64 {
    INLINED_SITES.load(std::sync::atomic::Ordering::Relaxed)
}


/// Whether regions and events are counted: a report that reads them is on (`report`, `census`).
fn counted() -> bool {
    static COUNTED: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| quench_runtime::trace::Channel::JsJit.on() || quench_runtime::trace::Channel::Census.on());
    *COUNTED
}

extern "C" fn report() {
    if !quench_runtime::trace::Channel::JsJit.on() {
        return;
    }
    let (Ok(totals), Ok(rejects)) = (TOTALS.lock(), REJECTS.lock()) else { return };
    let ops = totals.ops.max(1);
    eprintln!(
        "bjit regions={} rejected={} ops={} bytes={} bytes_per_op={:.1}",
        totals.regions, totals.rejected, totals.ops, totals.bytes, totals.bytes as f64 / ops as f64
    );
    let rows = (totals.covered + totals.exits).max(1);
    eprintln!("bjit-bodies bodies={} covered={} exits={} covered_share={:.1}% direct_calls={} helper_calls={}", totals.bodies, totals.covered, totals.exits, 100.0 * totals.covered as f64 / rows as f64, totals.direct_calls, totals.helper_calls);
    eprintln!("bjit-inlined sites={}", inlined_sites());
    eprintln!("{}", crate::dispatch_fast::lane_body_stats::unlowered_report());
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
fn analyze(span: &Span, decoded: &[Decoded], sites: &[Option<Site>], seen: &[Type]) -> (Reprs, Homes, Webs, Vec<Decoded>) {
    // A register with several webs may not have a web in an int slot, whose writes keep a
    // tagged word's high half; such a register is left whole.
    let mut merged = Bits::default();
    loop {
        let (webs, renamed) = Webs::split(span, decoded, &merged);
        let reprs = representations(span, &renamed, sites, seen, &webs);
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
