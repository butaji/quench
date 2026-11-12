//! A Number loop region built from BaseJIT stencils (`quench-bjit`) instead of Cranelift IR,
//! behind `QUENCH_DEV=bjit`. The registers the region touches most live in pinned machine
//! registers through the whole chain of stencils and the rest stay tagged `Value` words in the
//! activation's register file; every exit writes the pinned registers back first, so it leaves
//! the lane's state canonical. A region with an instruction that is no row of the stencil
//! library is declined (`build` names the instruction) and keeps its Cranelift code.
//!
//! The result word is the exit PC: a leave out of the region resumes there, a guard inside it
//! exits at its instruction (nothing of that instruction has run), a backward jump exits at the
//! jump when an interrupt is requested so the interpreter's safepoint runs, and an entry guard
//! that finds a register that is no Number returns `LaneExit::Entry`.

use super::osr_loop::LoopRegion;
use crate::bytecode::{Class, OperandKind, Register, WideInstruction};
use crate::value::Value;
use oxc_ast::ast::BinaryOperator;
use super::base_rows::{self, Heap, HeapRow};
use quench_bjit::numeric::{self, Kind, Place};
use quench_bjit::{PINNED, PINNED_F, Row as Placed, Stencil};
use std::collections::BTreeMap;
use std::sync::Mutex;

mod decode;
mod link;
mod plan;
mod repr;
mod tile;
mod webs;
use decode::*;
use link::*;
use plan::*;
use repr::*;
use webs::*;

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
}

static TOTALS: Mutex<Totals> = Mutex::new(Totals { regions: 0, ops: 0, bytes: 0, nanos: 0, rejected: 0 });
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
    let [entered, left, declined, compiled, bodies] = std::array::from_fn(|i| EVENTS[i].load(std::sync::atomic::Ordering::Relaxed));
    eprintln!("bjit-loops entered={entered} left={left} declined={declined} compiles={compiled} lane_body_compiles={bodies}");
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

/// The machine code of `region`, entered at the shared native entry ABI, or the instruction that
/// has no row. `ints` are the registers that held an int32 as the compile began; `widened` lists the PCs whose int32 speculation failed: they take general rows.
pub(super) fn build(region: &LoopRegion, dense: bool, widened: &[usize], seen: [&[Register]; SEEN]) -> Result<(Vec<u8>, bool), String> {
    let started = std::time::Instant::now();
    let built = assemble(region, dense, widened, seen);
    let nanos = started.elapsed().as_nanos() as u64;
    note(|totals, rejects| match &built {
        Ok((code, _)) => {
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

/// The code, and whether it reads no heap row: such a region needs from its payload only the entry
/// record's address (`Crossing::Bare`).
fn assemble(region: &LoopRegion, dense: bool, widened: &[usize], seen: [&[Register]; SEEN]) -> Result<(Vec<u8>, bool), String> {
    let (header, backedge) = (region.header, region.backedge);
    let count = (backedge - header) as usize + 1;
    let instructions = region.instructions().get(..count).ok_or("region instructions")?;
    let decoded = instructions.iter().enumerate().map(|(k, instruction)| decode(region, dense, header + k as u32, *instruction)).collect::<Result<Vec<_>, _>>()?;
    // A register with several webs may not have a web in an int slot, whose writes keep a
    // tagged word's high half; such a register is left whole.
    let mut merged: std::collections::BTreeSet<Register> = Default::default();
    let (reprs, homes, webs, decoded) = loop {
        let (webs, renamed) = Webs::split(region, &decoded, &merged);
        let reprs = representations(region, &renamed, widened, seen, &webs);
        let homes = Homes::of(&ranked(&renamed, &reprs, Repr::Int), &ranked(&renamed, &reprs, Repr::Double), webs.orig.clone(), &webs.clashes(region, &renamed));
        let split: Vec<Register> = homes.slots.iter().map(|web| webs.orig[usize::from(*web)]).filter(|register| webs.count(*register) > 1).collect();
        if split.is_empty() {
            break (reprs, homes, webs, renamed);
        }
        merged.extend(split);
    };
    let layout = Layout { header, backedge, homes, webs, region };
    let general: Vec<bool> = decoded.iter().map(|row| is_general(row, &reprs)).collect();
    let body = tile::plan_all(&layout, &decoded, &general)?;
    let bare = decoded.iter().all(|row| row.heap.is_none());
    Ok((link(&layout, &body)?, bare))
}

impl<H: crate::Host> crate::vm::Vm<H> {
    /// Whether the JavaScript function tiers compile function bodies: they run (`js_tier_runs`) and the process runs
    /// Cranelift function tiers at all (not `--b-jit`, whose loop sites are still admitted by `js_tier_runs`).
    pub(in crate::vm) fn js_function_compiles(&self) -> bool {
        self.js_tier_runs() && crate::jit_mode::function_tiers()
    }
}
