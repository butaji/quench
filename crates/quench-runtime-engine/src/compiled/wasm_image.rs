//! A Wasm program's functions for its AOT image: each function's stitched rows (`wasm_stitch`, the b-jit's
//! Wasm projection), planned from the module's declared binding types (`Analysis::declared`) rather than an
//! instance's cells. A run-time address the rows name is a symbol: a callee's entry cell is
//! `Symbol::EntryCell`. A row that would name anything else this process holds (a callout, an indirect
//! site's canonical type, a GC descriptor) refuses its function, which then has no image body.
//!
//! Owner: base tier (Wasm rows), image driver (`aot_image::lower`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::jit_backend::relocatable_code::PatchKind;
use super::*;
use crate::aot_image::Symbol;

/// The base of the values an image's World gives for its symbols; symbol `n` is `SYMBOL_WORDS + n`.
/// No hole of a planned row holds one otherwise: register slots, packed slot pairs and lengths stay
/// far below it, and a constant inside the range refuses its function (`constant`).
const SYMBOL_WORDS: u64 = 0xC0DE_5EED_0000_0000;
/// The values past `SYMBOL_WORDS` reserved for symbols.
const SYMBOL_RANGE: u64 = 1 << 32;
/// The World value of what an image cannot name: a row that names it refuses its function.
const UNNAMED: u64 = SYMBOL_WORDS + SYMBOL_RANGE - 1;

/// One function's image rows: its code and, by code offset, the symbol of each absolute word in it.
pub(crate) struct WasmImageRows {
    pub(crate) code: Vec<u8>,
    pub(crate) symbols: Vec<(usize, Symbol)>,
}

/// The memory slot the rows' entry reads, and the rows of every function of Wasm program `id`, under the
/// binding types `declared`: `None` for a
/// function with no plan or no rows. The callable set shrinks until every callable function stitches, as
/// the b-jit's does (`compile_program_with_diagnostics_and_root_mode`).
pub(crate) fn rows(vm: &Vm, id: ProgramId, code: &ResidualProgram, declared: &[quench_runtime::WasmBindingType])
    -> (Option<u32>, Vec<Option<WasmImageRows>>) {
    let count = code.function_count();
    let analysis = Analysis {
        // A memory an instance may share counts as growing under every function (`wasm_growth`).
        grows_memory: wasm_growth::growing_functions(code, count, true),
        declared: Some(declared),
        ..Analysis::new(vm, id, code, Value::UNDEFINED)
    };
    let (full_plans, memory_slot) = analysis.settle_callable(&quench_runtime::bitset::BitSet::from_fn(count, |_| true));
    let plans: Vec<Option<Plan>> = full_plans.iter().map(|plan| plan.as_ref().map(Plan::interface)).collect();
    let mut callable: Vec<bool> = plans.iter().map(Option::is_some).collect();
    loop {
        let stitched: Vec<Option<WasmImageRows>> = (0..count)
            .map(|index| callable[index].then(|| function_rows(&analysis, index, (&plans, &callable), memory_slot, full_plans[index].as_ref()?)).flatten())
            .collect();
        let kept: Vec<bool> = stitched.iter().map(Option::is_some).collect();
        if kept == callable { break (memory_slot, stitched); }
        callable = kept;
    }
}

/// Function `index`'s rows, its World (`Analysis::with_world`) naming only image symbols.
fn function_rows(analysis: &Analysis<'_>, index: usize, (plans, callable): (&[Option<Plan>], &[bool]), memory_slot: Option<u32>, full_plan: &Plan)
    -> Option<WasmImageRows> {
    let function = analysis.code.function_get(index)?;
    let symbols = std::cell::RefCell::new(Vec::<Symbol>::new());
    let name = |symbol: Symbol| -> u64 {
        let mut symbols = symbols.borrow_mut();
        let at = symbols.iter().position(|known| *known == symbol).unwrap_or_else(|| { symbols.push(symbol); symbols.len() - 1 });
        SYMBOL_WORDS + at as u64
    };
    let symbolic = |value: u64| value.wrapping_sub(SYMBOL_WORDS) < SYMBOL_RANGE;
    let callee = |callee: usize| callable.get(callee).copied()?.then(|| name(Symbol::EntryCell(callee as u32)));
    let naming = wasm_world::Naming {
        constant: &|bits| !symbolic(bits),
        callee: &callee,
        indirect: &|_| None,
        // An instance's memory may move when it grows: the image takes no reservation as given.
        memory_stable: false,
        callouts: wasm_world::Callouts::all(UNNAMED),
    };
    let returns = plans.get(index)?.as_ref()?.result.is_some();
    let (code, named) = analysis.with_world(function, full_plan, memory_slot, naming, |world| {
        wasm_stitch::stitch_naming(function, usize::from(function.params), returns, world, &symbolic)
    })?;
    let symbols = symbols.into_inner();
    let named = named.into_iter().map(|(offset, value)| symbols.get(usize::try_from(value - SYMBOL_WORDS).ok()?).map(|symbol| (offset, *symbol)))
        .collect::<Option<Vec<_>>>()?;
    Some(WasmImageRows { code, symbols: named })
}

/// Function `index`'s rows from the image of program `analysis.program`, linked for this instance's compile:
/// each callee's entry cell (`cells`) written where the rows name it. `None` where the image has no rows for
/// it, was planned for another memory slot, or names a callee that is not `callable` here (the live plan may
/// keep fewer functions than the module's declarations did), and the function then stitches as usual.
pub(crate) fn linked(analysis: &Analysis<'_>, index: usize, memory_slot: Option<u32>, (cells, callable): (&[std::cell::Cell<usize>], &[bool]))
    -> Option<Vec<u8>> {
    let images = analysis.vm.programs.ahead_images(analysis.program, analysis.code)?;
    let image = &images.image;
    let wasm = image.wasm.as_ref().filter(|wasm| wasm.memory_slot == memory_slot)?;
    let rows = wasm.functions.get(index)?.as_ref()?;
    let mut code = rows.code.to_vec();
    for relocation in rows.relocations.iter() {
        let Some(Symbol::EntryCell(callee)) = crate::aot_image::symbols::SymbolTable::decode(image, relocation.namespace, relocation.key) else { return None };
        if relocation.kind != PatchKind::Abs8 || !callable.get(callee as usize).copied().unwrap_or(false) {
            return None;
        }
        let word = (cells.get(callee as usize)?.as_ptr() as i64).wrapping_add(relocation.addend) as u64;
        code.get_mut(relocation.offset..relocation.offset + 8)?.copy_from_slice(&word.to_le_bytes());
    }
    if quench_runtime::trace::Channel::AotLanes.on() {
        eprintln!("aot-lanes: wasm function {index}: image rows");
    }
    Some(code)
}
