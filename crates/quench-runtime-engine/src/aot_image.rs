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
//! constructor shapes and number representations, each a dependency whose retirement sets the
//! image's invalidation word; frozen bindings, which the closed world fixes
//! (`AotFacts::dominated_root_store`). Heap values and shapes the code folds are symbols
//! (`Symbol::HeapValue`, `Symbol::Shape`) the loader resolves from recipes, so no address of the
//! compiling process reaches the image.

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
#[path = "aot_image/page.rs"]
mod page;
#[path = "aot_image/demand.rs"]
pub mod demand;
#[cfg(feature = "jit")]
#[path = "aot_image/class_recipe.rs"]
pub(crate) mod class_recipe;

use quench_runtime::bytecode::ResidualProgram;
use quench_runtime::compile::aot_facts::AotFacts;
#[cfg(feature = "jit")]
pub(crate) use symbols::HeapValueRecipe;
pub(crate) use symbols::Symbol;

pub(crate) use quench_runtime::vm::engine_types::AotImage;

pub(crate) use quench_runtime::vm::engine_types::CallSite;

/// The callee word of a call record bound to no closure: the deleted marker, which no callee register
/// holds at a call, so a checked direct row refuses it before it reads anything else of the record.
pub(crate) fn unbound_callee() -> u64 {
    quench_runtime::Value::DELETED.to_bits()
}

/// The callee record a site names until a binding (`BoundCalls::bind`): no callee (`unbound_callee`), every other word
/// zero. Leaked once for the process, so a row reads a live record before and after any binding.
pub(crate) fn unbound_callee_record() -> u64 {
    static UNBOUND: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *UNBOUND.get_or_init(|| {
        let record: &'static quench_bjit::call_record::CalleeRecord =
            Box::leak(Box::new(quench_bjit::call_record::CalleeRecord { callee: unbound_callee(), ..Default::default() }));
        std::ptr::from_ref(record).addr()
    }) as u64
}

pub(crate) use quench_runtime::vm::engine_types::AotFunction;

pub(crate) use quench_runtime::vm::engine_types::LaneImage;

/// What a lowering of the image is for: its CPU baseline and target triple
/// (`jit_backend::NativeTarget`, never the build host's CPU unless asked for by name).
pub(crate) use quench_runtime::vm::jit_backend::NativeTarget as ImageTarget;

/// Lowers every function of `program` under `facts` for `target`.
#[cfg(feature = "jit")]
pub(crate) fn lower(program: &ResidualProgram, facts: &AotFacts, target: &ImageTarget) -> Result<AotImage, String> {
    // Every function lowers as a VM that loads the image runs it (`lowered_function`), so the image's PCs are its.
    for index in 0..program.function_count() { program.lowered_function(index); }
    let mut symbols = symbols::SymbolTable::new(program, std::sync::Arc::new(quench_runtime::compile::call_plan::Selectors::of(program, facts)));
    let graph = quench_runtime::compile::call_graph::CallGraph::of(program, facts);
    let mut sites = std::collections::BTreeMap::new();
    let functions = (0..program.function_count() as u32)
        .map(|id| lower_function(program, (facts, &graph), target, id, (&mut symbols, &mut sites)))
        .collect::<Result<Vec<_>, String>>()?;
    if quench_runtime::trace::Channel::AotLanes.on() {
        let lanes = functions.iter().filter(|function| function.lane.is_some()).count();
        let leaves: usize = functions.iter().map(|function| function.base.generic_rows()).sum();
        eprintln!("aot-lanes: {} {lanes} of {} functions have a lane body; base bodies leave at {leaves} records", program.source_name, functions.len());
        // The opcodes those records stand for, most first: what staging drives the leaves to zero.
        let mut by_op = std::collections::BTreeMap::<&str, usize>::new();
        for function in &functions {
            let code: Vec<_> = program.function(function.id as usize).instructions().collect();
            for &pc in function.base.generic.iter() {
                if let Some(instruction) = code.get(pc as usize) { *by_op.entry(quench_runtime::bytecode::Op::NAMES[instruction.op() as usize]).or_default() += 1; }
            }
        }
        let mut by_op: Vec<_> = by_op.into_iter().collect();
        by_op.sort_by(|a, b| b.1.cmp(&a.1));
        eprintln!("aot-lanes: {} leaves by opcode: {by_op:?}", program.source_name);
    }
    let mut image = assemble(program, functions, symbols.finish(), &sites)?;
    image.regexps = regexps(program);
    if quench_runtime::trace::Channel::AotLanes.on() {
        eprintln!("aot-lanes: {} demand: {}", program.source_name, demand::of(&image, program));
    }
    Ok(image)
}

/// The image of `program`: its `functions`, with what their bodies name (`symbols`), each named call
/// record resolved to the call site a base body planned (`sites`). The one assembly every lowering ends in.
fn assemble(program: &ResidualProgram, functions: Vec<AotFunction>, symbols: symbols::Finished, sites: &std::collections::BTreeMap<(u32, u32, u32), CallSite>)
    -> Result<AotImage, String> {
    let symbols::Finished { shapes, heap_values, atoms, labels, calls } = symbols;
    let calls = calls.into_iter().map(|site| sites.get(&site).copied().ok_or("a body names a call record no base body planned"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AotImage {
        fingerprint: crate::compiled::ahead_program_fingerprint(program),
        build_id: crate::compiled::AHEAD_BUILD_ID,
        world: quench_runtime::compile::aot_facts::world(program),
        functions,
        shapes,
        heap_values,
        atoms,
        labels,
        calls,
        wasm: None,
        regexps: Vec::new(),
    })
}

/// The native searches of `program`'s RegExp literals (`AotImage::regexps`): each literal's programs, compiled as the
/// regexp tier compiles a hot one (`regexp_native::ahead_code`), which a VM that loads the image adopts at their first
/// search (`live::register_regexps`). A program whose code would name a live address has none (`Regex::ahead_plans`).
#[cfg(not(feature = "jit"))]
fn regexps(_: &ResidualProgram) -> Vec<quench_runtime::vm::engine_types::AotRegexp> { Vec::new() }
#[cfg(feature = "jit")]
fn regexps(program: &ResidualProgram) -> Vec<quench_runtime::vm::engine_types::AotRegexp> {
    let mut keys = std::collections::BTreeSet::new();
    quench_runtime::vm::Vm::regexp_literal_matchers(program).iter().flat_map(|matcher| matcher.ahead_plans())
        .filter(|(key, _)| keys.insert(key.clone()))
        .filter_map(|(key, plan)| crate::regexp_native::ahead_code(&plan).map(|code| quench_runtime::vm::engine_types::AotRegexp { key, code: code.into() }))
        .collect()
}

/// The image of Wasm program `id`, every function planned from the module's binding types `bindings`: each
/// function's stitched rows (`compiled::wasm_image`, `AotImage::wasm`), which the instance's Wasm tier links
/// in place of stitching (`compiled::wasm_image::linked`); its base body is empty, every record generic.
pub(crate) fn lower_wasm(vm: &quench_runtime::vm::Vm, id: quench_runtime::vm::ProgramId, program: &ResidualProgram,
    bindings: &[quench_runtime::WasmBindingType]) -> Result<AotImage, String> {
    let mut symbols = symbols::SymbolTable::new(program, Default::default());
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    let wasm = {
        use quench_runtime::vm::jit_backend::relocatable_code::{PatchKind, RelocatableCode, Relocation};
        let (memory_slot, rows) = crate::compiled::wasm_image::rows(vm, id, program, bindings);
        if quench_runtime::trace::Channel::AotLanes.on() {
            let (stitched, named) = (rows.iter().flatten().count(), rows.iter().flatten().map(|rows| rows.symbols.len()).sum::<usize>());
            eprintln!("aot-lanes: {} wasm: {stitched} of {} functions stitch, naming {named} symbols", program.source_name, rows.len());
        }
        let functions = rows.into_iter().map(|rows| rows.map(|rows| {
            let relocations = rows.symbols.iter().map(|&(offset, symbol)| {
                let (namespace, key) = symbols.intern(symbol);
                Relocation { offset, kind: PatchKind::Abs8, namespace, key, addend: 0 }
            }).collect();
            RelocatableCode { code: rows.code.into(), relocations }
        })).collect();
        Some(quench_runtime::vm::engine_types::WasmRows { memory_slot, functions })
    };
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    let wasm = { let _ = (vm, id, bindings); None };
    // The interpreter runs every record of a function the instance's tier did not link rows for.
    let functions = (0..program.function_count() as u32).map(|function| {
        let records = program.function(function as usize).instructions().count() as u32;
        let code = quench_runtime::vm::jit_backend::relocatable_code::RelocatableCode { code: Box::default(), relocations: Box::default() };
        let base = base::BaseImage { code, pc_offsets: vec![base::NO_ENTRY; records as usize].into(), generic: (0..records).collect(), scratch: 0 };
        AotFunction { id: function, base, lane: None }
    }).collect();
    let mut image = assemble(program, functions, symbols.finish(), &std::collections::BTreeMap::new())?;
    image.wasm = wasm;
    if quench_runtime::trace::Channel::AotLanes.on() {
        eprintln!("aot-lanes: {} demand: {}", program.source_name, demand::of(&image, program));
    }
    Ok(image)
}

/// One function: its base body always, its lane body where it lowers native records, the
/// lane's leaves bound to the base body's PC entries; its base body's proven calls join `sites`.
#[cfg(feature = "jit")]
fn lower_function(
    program: &ResidualProgram,
    (facts, graph): (&AotFacts, &quench_runtime::compile::call_graph::CallGraph),
    target: &ImageTarget,
    id: u32,
    (symbols, sites): (&mut symbols::SymbolTable, &mut std::collections::BTreeMap<(u32, u32, u32), CallSite>),
) -> Result<AotFunction, String> {
    let (mut base, calls) = base::lower(program, facts, target, id, symbols)?;
    let (mut lane, lane_calls) = join::lower_lane(program, (facts, graph), target, id, (&base, &calls), symbols)?;
    // A linked body's absolute words become the page references Mach-O can link in text.
    crate::aot_image::page::rewrite(&mut base.code)?;
    if let Some(lane) = &mut lane {
        crate::aot_image::page::rewrite(&mut lane.code)?;
    }
    sites.extend(calls.into_iter().chain(lane_calls).map(|call| ((call.function, call.pc, call.callee), call)));
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

/// An execution-only build carries no lowering; the compiler that builds images enables `jit`.
#[cfg(not(feature = "jit"))]
pub(crate) fn lower(_: &ResidualProgram, _: &AotFacts, _: &ImageTarget) -> Result<AotImage, String> {
    Err("ahead image lowering requires the jit feature".into())
}
