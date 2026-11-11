//! A Number loop region built from FastJIT stencils (`quench-fastjit`) instead of Cranelift IR,
//! behind `QUENCH_DEV=fastjit`. The registers the region touches most live in pinned machine
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
use super::fast_rows::{self, Heap, HeapRow};
use quench_fastjit::numeric::{self, Kind, Place};
use quench_fastjit::{PINNED, PINNED_F, Row as Placed, Stencil};
use std::collections::BTreeMap;
use std::sync::Mutex;

mod decode;
mod link;
mod plan;
mod repr;
use decode::*;
use link::*;
use plan::*;
use repr::*;

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
/// Per region (function, header): entry setups, their ns, native runs, their ns, and runs that left on a guard.
static RUNS: Mutex<BTreeMap<(u32, u32), [u64; 5]>> = Mutex::new(BTreeMap::new());
static REPORT: std::sync::Once = std::sync::Once::new();

/// Counts one entry setup (`entry`) or one native run of a region, for the report.
pub(crate) fn count_run(function: u32, header: u32, nanos: u64, run: Option<bool>) {
    let Ok(mut runs) = RUNS.lock() else { return };
    let row = runs.entry((function, header)).or_default();
    match run {
        None => { row[0] += 1; row[1] += nanos; }
        Some(exited) => { row[2] += 1; row[3] += nanos; row[4] += u64::from(exited); }
    }
}

extern "C" fn report() {
    if let Ok(runs) = RUNS.lock() {
        for ((function, header), [entries, entry_ns, runs, run_ns, exits]) in runs.iter() {
            eprintln!(
                "QUENCH_FASTJIT_RUN function={function} header={header} entries={entries} entry_ns_each={} runs={runs} run_ns_each={} exits={exits}",
                entry_ns / entries.max(&1), run_ns / runs.max(&1)
            );
        }
    }
    let (Ok(totals), Ok(rejects)) = (TOTALS.lock(), REJECTS.lock()) else { return };
    let ops = totals.ops.max(1);
    eprintln!(
        "QUENCH_FASTJIT regions={} rejected={} ops={} bytes={} bytes_per_op={:.1} build_ns={} build_ns_per_op={:.1} stencil_gen_ns={}",
        totals.regions, totals.rejected, totals.ops, totals.bytes, totals.bytes as f64 / ops as f64, totals.nanos,
        totals.nanos as f64 / ops as f64, numeric::generation_ns()
    );
    let mut ranked: Vec<_> = rejects.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (name, count) in ranked {
        eprintln!("QUENCH_FASTJIT_REJECT {name} {count}");
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
pub(super) fn build(region: &LoopRegion, widened: &[usize], seen: [&[Register]; SEEN]) -> Result<Vec<u8>, String> {
    let started = std::time::Instant::now();
    let built = assemble(region, widened, seen);
    let nanos = started.elapsed().as_nanos() as u64;
    note(|totals, rejects| match &built {
        Ok(code) => {
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

fn assemble(region: &LoopRegion, widened: &[usize], seen: [&[Register]; SEEN]) -> Result<Vec<u8>, String> {
    let (header, backedge) = (region.header, region.backedge);
    let count = (backedge - header) as usize + 1;
    let instructions = region.instructions().get(..count).ok_or("region instructions")?;
    let decoded = instructions.iter().enumerate().map(|(k, instruction)| decode(region, header + k as u32, *instruction)).collect::<Result<Vec<_>, _>>()?;
    let reprs = representations(region, &decoded, widened, seen);
    let homes = Homes::of(ranked(&decoded, &reprs, Repr::Int), ranked(&decoded, &reprs, Repr::Double));
    let layout = Layout { header, backedge, homes, region };
    let mut body = Vec::with_capacity(count);
    for (k, row) in decoded.iter().enumerate() {
        body.push(plan(row, &layout.homes, header + k as u32, is_general(row, &reprs))?);
    }
    link(&layout, &body)
}
