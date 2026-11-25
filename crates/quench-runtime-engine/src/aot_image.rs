//! The one AOT image: a program lowered ahead of time, every function as a BaseJIT body
//! (`base`, stencils derived from the staged rows, covering every record) and, where the
//! closed-world facts make it better, a lane body (`lane_region::lower_body` on
//! `LaneHelpers::Ahead`) whose leaves continue in the base body at the same PC on the same
//! Frame (`join`). Nothing runs one record at a time through a dispatch loop.
//!
//! Every run-time address in the image is a `Symbol`, so one image has three linkers that differ
//! only in where symbols resolve: a relocatable object for the system linker (`object`, the
//! `quench compile` executable), this process's memory (`live`, eager publication and tests),
//! and a file a VM loads at start (`file`). The image replaces the call-threaded graph
//! (`native_graph::lower_body`), the per-PC semantic fallback (`ahead_entry`'s loop, `eager`'s
//! fallback table) and `native_stencil.rs`, which this image replaces.
//!
//! Facts are the program's alone (`compile::aot_facts`): whole-program call targets,
//! constructor shapes, number representations and frozen globals, each a dependency whose
//! retirement sets the image's invalidation word. Heap values and shapes the code folds are
//! symbols (`Symbol::HeapValue`, `Symbol::Shape`) the loader resolves from recipes, so no
//! address of the compiling process reaches the image.

#[allow(unused_imports)]
use crate::ext_traits::*;
#[path = "aot_image/symbols.rs"]
pub(crate) mod symbols;
#[path = "aot_image/base.rs"]
pub(crate) mod base;
#[path = "aot_image/join.rs"]
pub(crate) mod join;
#[path = "aot_image/object.rs"]
pub(crate) mod object;
#[path = "aot_image/bound.rs"]
pub(crate) mod bound;
#[path = "aot_image/live.rs"]
pub(crate) mod live;
#[path = "aot_image/file.rs"]
pub(crate) mod file;

use quench_runtime::bytecode::ResidualProgram;
use quench_runtime::compile::aot_facts::AotFacts;
pub(crate) use symbols::{HeapValueRecipe, Symbol};

pub(crate) use quench_runtime::vm::engine_types::AotImage;

pub(crate) use quench_runtime::vm::engine_types::CallSite;

/// The callee word of a call record bound to no closure: the deleted marker, which no callee register
/// holds at a call, so a checked direct row refuses it before it reads anything else of the record.
pub(crate) fn unbound_callee() -> u64 {
    quench_runtime::Value::DELETED.to_bits()
}

pub(crate) use quench_runtime::vm::engine_types::AotFunction;

pub(crate) use quench_runtime::vm::engine_types::LaneImage;

/// What a lowering of the image is for: its CPU baseline and target triple
/// (`jit_backend::NativeTarget`, never the build host's CPU unless asked for by name).
pub(crate) use quench_runtime::vm::jit_backend::NativeTarget as ImageTarget;

/// Lowers every function of `program` under `facts` for `target`.
pub(crate) fn lower(program: &ResidualProgram, facts: &AotFacts, target: &ImageTarget) -> Result<AotImage, String> {
    // Every function lowers as a VM that loads the image runs it (`lowered_function`), so the image's PCs are its.
    for index in 0..program.function_count() { program.lowered_function(index); }
    let mut symbols = symbols::SymbolTable::new(program);
    let frozen = crate::compiled::ClosedFrozen::of(program);
    let mut sites = std::collections::BTreeMap::new();
    let functions = (0..program.function_count() as u32)
        .map(|id| lower_function(program, (facts, &frozen), target, id, (&mut symbols, &mut sites)))
        .collect::<Result<Vec<_>, String>>()?;
    if quench_runtime::trace::Channel::AotLanes.on() {
        let lanes = functions.iter().filter(|function| function.lane.is_some()).count();
        let leaves: usize = functions.iter().map(|function| function.base.generic_rows()).sum();
        eprintln!("aot-lanes: {} {lanes} of {} functions have a lane body; base bodies leave at {leaves} records", program.source_name, functions.len());
    }
    let symbols::Finished { shapes, heap_values, atoms, labels, calls } = symbols.finish();
    let calls = calls.into_iter().map(|site| sites.get(&site).copied().ok_or("a body names a call record no base body planned"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AotImage {
        fingerprint: crate::compiled::ahead_program_fingerprint(program),
        build_id: crate::compiled::AHEAD_BUILD_ID,
        closed: quench_runtime::compile::aot_facts::closed_world(program),
        functions,
        shapes,
        heap_values,
        atoms,
        labels,
        calls,
    })
}

/// One function: its base body always, its lane body where it lowers native records, the
/// lane's leaves bound to the base body's PC entries; its base body's proven calls join `sites`.
fn lower_function(
    program: &ResidualProgram,
    (facts, frozen): (&AotFacts, &crate::compiled::ClosedFrozen),
    target: &ImageTarget,
    id: u32,
    (symbols, sites): (&mut symbols::SymbolTable, &mut std::collections::BTreeMap<(u32, u32), CallSite>),
) -> Result<AotFunction, String> {
    let (base, calls) = base::lower(program, facts, target, id, symbols)?;
    let lane = join::lower_lane(program, (facts, frozen), target, id, (&base, &calls), symbols)?;
    sites.extend(calls.into_iter().map(|call| ((call.function, call.pc), call)));
    Ok(AotFunction { id, base, lane })
}

#[quench_macros::ext(AotImageOps1)]
impl AotImage {
    /// The records its base bodies leave at (`Step::Leave`), over all functions: what an image
    /// with no interpreter behind it cannot run, the shrink-only budget (`aot-exact-steps.txt`)
    /// the AOT gate requires to reach zero.
    pub(crate) fn generic_rows(&self) -> usize {
        self.functions.iter().map(|function| function.base.generic_rows()).sum()
    }
}
