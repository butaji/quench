//! A Number loop region built from BaseJIT stencils (`quench-bjit`) instead of Cranelift IR,
//! behind `QUENCH_DEV=bjit`. The registers the region touches most live in pinned machine
//! registers through the whole chain of stencils and the rest stay tagged `Value` words in the
//! activation's register file; every exit writes the pinned registers back first, so it leaves
//! the lane's state canonical. An instruction that is no row of the stencil library is lowered
//! by `steps` (a helper step or a leave), so no region is declined for its instructions.
//!
//! The result word is the exit PC: a leave out of the region resumes there, a guard inside it
//! exits at its instruction (nothing of that instruction has run), a backward jump exits at the
//! jump when an interrupt is requested so the interpreter's safepoint runs, and an entry guard
//! that finds a register that is no Number returns `LaneExit::Entry`.

use super::osr_loop::LoopRegion;
use crate::bytecode::{Atom, Class, FieldBase, FieldLookup, OperandKind, Register, ResidualProgram, WideInstruction};
use crate::value::Value;
use oxc_ast::ast::{BinaryOperator, UnaryOperator};
use super::base_rows::{self, Heap, HeapRow};
use super::NativeFieldLoad;
use quench_bjit::numeric::{self, Kind, Place};
use quench_bjit::{PINNED, PINNED_F, Row as Placed, Stencil};
use super::LaneHelpers;
use std::collections::BTreeMap;
use std::sync::Mutex;

mod call_plan;
pub(in crate::vm::compiled) mod callees;
mod decode;
pub(in crate::vm::compiled) mod function;
mod link;
mod plan;
mod repr;
mod steps;
mod throws;
mod webs;
use decode::*;
use link::*;
use plan::*;
use repr::*;
use steps::*;
pub(in crate::vm::compiled) use steps::covers;
use webs::*;

/// What a row reads of the running program rather than of the registers: the names its
/// instructions' atoms are, and whether a store may run in place.
#[derive(Clone, Copy)]
pub(in crate::vm::compiled) struct Known<'a> {
    pub(in crate::vm::compiled) program: &'a ResidualProgram,
    /// The names method caches hold now, where the VM and the program run specialized, so a
    /// store of an unnamed atom may run in place; `None` where none may. Code that stores holds
    /// each such atom unnamed as a Stable fact (`Body::atoms`), which a cache naming it retires.
    pub(in crate::vm::compiled) stores: Option<&'a crate::vm::method_cache::AtomBits>,
    /// The loads a cache site proves for a read (`Vm::native_field_read_descriptors`), where the VM and
    /// the program run specialized and the code may hold Stable facts: a read of a field its receiver's
    /// prototype holds folds to the value the holder holds now (`folded_read`), which a change of the
    /// holder's values retires the code over (`Body::holders`). `None` where no fact may be held.
    pub(in crate::vm::compiled) reads: Option<&'a dyn Fn(u16) -> Vec<NativeFieldLoad>>,
    /// The address of the code's invalidation word (`code_dependencies::Invalidation`), which a body that
    /// holds a Stable fact and makes a call reads before the rows that hold it. `None` for a region.
    pub(in crate::vm::compiled) invalidation: Option<usize>,
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

/// Bytes of a register slot in the activation's register file.
const SLOT: u64 = std::mem::size_of::<Value>() as u64;

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
    /// A compilation of a loop region started (`lane_loop_compile`).
    Compiled,
    /// A lane body compile started (a Cranelift function tier; none under `--b-jit`).
    BodyCompile,
}

/// The direct-cell call sites the stitcher inlined (`call_plan::plan_call`), counted whether or not
/// the trace channel is on and reported with the totals.
static INLINED_SITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn note_inlined() {
    INLINED_SITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

fn inlined_sites() -> u64 {
    INLINED_SITES.load(std::sync::atomic::Ordering::Relaxed)
}

static EVENTS: [std::sync::atomic::AtomicU64; 5] = [const { std::sync::atomic::AtomicU64::new(0) }; 5];

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

/// Counts `event` where the trace channel is on.
pub(in crate::vm) fn count(event: Event) {
    if !crate::trace::Channel::JsJit.on() {
        return;
    }
    REPORT.call_once(|| {
        // SAFETY: `report` is an `extern "C" fn()` that touches only statics.
        unsafe { libc::atexit(report) };
    });
    EVENTS[event as usize].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

extern "C" fn report() {
    let (Ok(totals), Ok(rejects)) = (TOTALS.lock(), REJECTS.lock()) else { return };
    let ops = totals.ops.max(1);
    eprintln!(
        "bjit regions={} rejected={} ops={} bytes={} bytes_per_op={:.1} build_ns={} build_ns_per_op={:.1}",
        totals.regions, totals.rejected, totals.ops, totals.bytes, totals.bytes as f64 / ops as f64, totals.nanos,
        totals.nanos as f64 / ops as f64
    );
    let rows = (totals.covered + totals.exits).max(1);
    eprintln!("bjit-bodies bodies={} covered={} exits={} covered_share={:.1}% direct_calls={} helper_calls={}", totals.bodies, totals.covered, totals.exits, 100.0 * totals.covered as f64 / rows as f64, totals.direct_calls, totals.helper_calls);
    let [entered, left, declined, compiled, bodies] = std::array::from_fn(|i| EVENTS[i].load(std::sync::atomic::Ordering::Relaxed));
    eprintln!("bjit-loops entered={entered} left={left} declined={declined} compiles={compiled} lane_body_compiles={bodies} inlined_sites={}", inlined_sites());
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
    if !crate::trace::Channel::JsJit.on() {
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

/// The machine code of `region`, entered at the shared native entry ABI, or the stencil it lacks.
/// `seen` are the registers that held an int32 and a double as the compile began; `widened` lists the PCs whose int32 speculation failed: they take general rows.
pub(super) fn build(region: &LoopRegion, dense: bool, known: Known, widened: &[usize], seen: [&[Register]; SEEN], helpers: LaneHelpers) -> Result<Built, String> {
    let started = std::time::Instant::now();
    let built = assemble(region, (dense, known), widened, seen, helpers);
    let nanos = started.elapsed().as_nanos() as u64;
    note(|totals, rejects| match &built {
        Ok(Built { code, .. }) => {
            totals.regions += 1;
            totals.ops += u64::from(region.backedge - region.header) + 1;
            totals.bytes += code.len() as u64;
            totals.nanos += nanos;
        }
        Err(name) => {
            totals.rejected += 1;
            *rejects.entry(name.clone()).or_default() += 1;
        }
    });
    built
}

/// The webs of `decoded`, their representations and homes, and the rows renamed to webs.
fn analyze(region: &LoopRegion, decoded: &[Decoded], widened: &[usize], seen: [&[Register]; SEEN]) -> (Reprs, Homes, Webs, Vec<Decoded>) {
    // A register with several webs may not have a web in an int slot, whose writes keep a
    // tagged word's high half; such a register is left whole.
    let mut merged = Bits::default();
    loop {
        let (webs, renamed) = Webs::split(region, decoded, &merged);
        let reprs = representations(region, &renamed, widened, seen, &webs);
        let homes = Homes::of(&ranked(&renamed, &reprs, Repr::Int), &ranked(&renamed, &reprs, Repr::Double), webs.orig.clone(), &webs.clashes(region, &renamed));
        let split: Vec<Register> = homes.slots.iter().map(|web| webs.orig[usize::from(*web)]).filter(|register| webs.count(*register) > 1).collect();
        if split.is_empty() {
            return (reprs, homes, webs, renamed);
        }
        for register in split {
            merged.insert(usize::from(register));
        }
    }
}

/// A loop region's code, whether it reads no heap row and makes no step (such a region needs from
/// its payload only the entry record's address, `Crossing::Bare`), and the atoms its stores hold
/// unnamed by method caches (a Stable fact: a cache naming one retires it, `OsrFacts::atoms`).
pub(in crate::vm::compiled) struct Built {
    pub(in crate::vm::compiled) code: Vec<u8>,
    pub(in crate::vm::compiled) bare: bool,
    pub(in crate::vm::compiled) atoms: Box<[Atom]>,
}

fn assemble(region: &LoopRegion, (dense, known): (bool, Known), widened: &[usize], seen: [&[Register]; SEEN], helpers: LaneHelpers) -> Result<Built, String> {
    let (header, backedge) = (region.header, region.backedge);
    let count = (backedge - header) as usize + 1;
    let instructions = region.instructions().get(..count).ok_or("region instructions")?;
    let writes = instructions.iter().any(|instruction| stores_field(known, *instruction));
    let facts = (Facts::Entry { dense, writes }, known);
    let mut decoded: Vec<Decoded> = instructions.iter().enumerate().map(|(k, instruction)| decode(region, facts, header + k as u32, *instruction, helpers)).collect();
    settle(&mut decoded);
    let (reprs, homes, webs, decoded) = analyze(region, &decoded, widened, seen);
    let layout = Layout { header, backedge, homes, webs, region, ending: Ending::Region, callees: &[], invalidation: None };
    let body = decoded.iter().enumerate().map(|(k, row)| plan(row, &layout, header + k as u32, is_general(row, &reprs), helpers)).collect::<Result<Vec<_>, _>>()?;
    let bare = decoded.iter().all(|row| row.heap.is_none() && !row.step.is_some_and(Step::runs_helper));
    Ok(Built { code: link(&layout, &body)?, bare, atoms: held_atoms(&decoded) })
}

impl<H: crate::Host> crate::vm::Vm<H> {
    /// Whether the JavaScript function tiers compile function bodies: they run (`js_tier_runs`) and the process runs
    /// Cranelift function tiers at all (not `--b-jit`, whose loop sites are still admitted by `js_tier_runs`).
    pub(in crate::vm) fn js_function_compiles(&self) -> bool {
        self.js_tier_runs() && crate::jit_mode::function_tiers()
    }
}
