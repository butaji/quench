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

#[path = "aot_image/symbols.rs"]
pub(crate) mod symbols;
#[path = "aot_image/base.rs"]
pub(crate) mod base;
#[path = "aot_image/join.rs"]
pub(crate) mod join;
#[path = "aot_image/object.rs"]
pub(crate) mod object;
#[path = "aot_image/live.rs"]
pub(crate) mod live;
#[path = "aot_image/file.rs"]
pub(crate) mod file;

use crate::bytecode::ResidualProgram;
use crate::compile::aot_facts::AotFacts;
use super::jit_backend::relocatable_code::RelocatableCode;
pub(crate) use symbols::{HeapValueRecipe, ShapeRecipe, Symbol};

/// A program's AOT image: what all three linkers consume.
pub(crate) struct AotImage {
    /// The program the image is valid for (`ahead_images::fingerprint`) and the build it came from.
    pub(crate) fingerprint: u64,
    pub(crate) build_id: u64,
    /// By function id, ascending; every function of the program has one.
    pub(crate) functions: Vec<AotFunction>,
    /// The shapes the code folds, by `Symbol::Shape` key.
    pub(crate) shapes: Vec<ShapeRecipe>,
    /// The heap values the code folds, by `Symbol::HeapValue` key.
    pub(crate) heap_values: Vec<HeapValueRecipe>,
    /// Atom texts by `Symbol::Atom` key (atom numbers are per VM).
    pub(crate) atoms: Vec<Box<str>>,
    /// The `(function, pc)` labels of `Symbol::BasePc` keys.
    pub(crate) labels: Vec<(u32, u32)>,
}

/// One function of the image.
pub(crate) struct AotFunction {
    pub(crate) id: u32,
    /// The base body: every record lowered (a stencil row, or the generic semantic row).
    pub(crate) base: base::BaseImage,
    /// The lane body, where the facts gave it native records; its leaves enter `base`.
    pub(crate) lane: Option<LaneImage>,
}

/// A lane body compiled ahead: its code, and what its lowering proved about the entry ABI.
pub(crate) struct LaneImage {
    pub(crate) code: RelocatableCode,
    /// The fault-poll offset the lowering reported (`LaneRegion::fault_poll`).
    pub(crate) fault_poll: Option<u32>,
}

/// What a lowering of the image is for: its CPU baseline and target triple
/// (`jit_backend::NativeTarget`, never the build host's CPU unless asked for by name).
pub(crate) use super::jit_backend::NativeTarget as ImageTarget;

/// Lowers every function of `program` under `facts` for `target`.
pub(crate) fn lower(program: &ResidualProgram, facts: &AotFacts, target: &ImageTarget) -> Result<AotImage, String> {
    let mut symbols = symbols::SymbolTable::new(program);
    let functions = (0..program.function_count() as u32)
        .map(|id| lower_function(program, facts, target, id, &mut symbols))
        .collect::<Result<Vec<_>, String>>()?;
    let (shapes, heap_values, atoms, labels) = symbols.finish();
    Ok(AotImage {
        fingerprint: super::compiled::ahead_program_fingerprint(program),
        build_id: super::compiled::AHEAD_BUILD_ID,
        functions,
        shapes,
        heap_values,
        atoms,
        labels,
    })
}

/// One function: its base body always, its lane body where it lowers native records, the
/// lane's leaves bound to the base body's PC entries.
fn lower_function(
    program: &ResidualProgram,
    facts: &AotFacts,
    target: &ImageTarget,
    id: u32,
    symbols: &mut symbols::SymbolTable,
) -> Result<AotFunction, String> {
    let base = base::lower(program, facts, target, id, symbols)?;
    let lane = join::lower_lane(program, facts, target, id, &base, symbols)?;
    Ok(AotFunction { id, base, lane })
}

impl AotImage {
    /// Records the image leaves to the generic semantic row, over all functions: the
    /// shrink-only budget (`aot-exact-steps.txt`) the AOT gate requires to reach zero.
    pub(crate) fn generic_rows(&self) -> usize {
        self.functions.iter().map(|function| function.base.generic_rows()).sum()
    }
}
