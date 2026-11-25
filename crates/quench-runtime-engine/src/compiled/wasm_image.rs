//! A Wasm program's functions for its AOT image: each function's stitched rows (`wasm_stitch`, the b-jit's
//! Wasm projection), planned from the module's declared binding types (`Analysis::declared`) rather than an
//! instance's cells. A run-time address the rows name is a symbol: a callee's entry cell is
//! `Symbol::EntryCell`. A row that would name anything else this process holds (a callout, an indirect
//! site's canonical type, a GC descriptor) refuses its function, which then has no image body.
//!
//! Owner: base tier (Wasm rows), image driver (`aot_image::lower`).

#[allow(unused_imports)]
use crate::ext_traits::*;
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

/// The rows of every function of Wasm program `id`, under the binding types `declared`: `None` for a
/// function with no plan or no rows. The callable set shrinks until every callable function stitches, as
/// the b-jit's does (`compile_program_with_diagnostics_and_root_mode`).
pub(crate) fn rows(vm: &Vm, id: ProgramId, code: &ResidualProgram, declared: &[quench_runtime::WasmBindingType]) -> Vec<Option<WasmImageRows>> {
    let count = code.function_count();
    let analysis = Analysis {
        vm, program: id, code, env: Value::UNDEFINED, callouts: true, experimental_root_compilation: false, root_only: false,
        compile_rejections: None, unrolls: false,
        // A memory an instance may share counts as growing under every function (`wasm_growth`).
        grows_memory: wasm_growth::growing_functions(code, count, true), declared: Some(declared),
    };
    let (full_plans, memory_slot) = analysis.settle_callable(&quench_runtime::bitset::BitSet::from_fn(count, |_| true));
    let plans: Vec<Option<Plan>> = full_plans.iter().map(|plan| plan.as_ref().map(Plan::interface)).collect();
    let mut callable: Vec<bool> = plans.iter().map(Option::is_some).collect();
    loop {
        let stitched: Vec<Option<WasmImageRows>> = (0..count)
            .map(|index| callable[index].then(|| function_rows(&analysis, index, (&plans, &callable), memory_slot, full_plans[index].as_ref()?)).flatten())
            .collect();
        let kept: Vec<bool> = stitched.iter().map(Option::is_some).collect();
        if kept == callable { break stitched; }
        callable = kept;
    }
}

/// Function `index`'s rows, its World naming only image symbols.
fn function_rows(analysis: &Analysis<'_>, index: usize, (plans, callable): (&[Option<Plan>], &[bool]), memory_slot: Option<u32>, full_plan: &Plan)
    -> Option<WasmImageRows> {
    let (vm, program, code) = (analysis.vm, analysis.program, analysis.code);
    let function = code.function_get(index)?;
    let symbols = std::cell::RefCell::new(Vec::<Symbol>::new());
    let name = |symbol: Symbol| -> u64 {
        let mut symbols = symbols.borrow_mut();
        let at = symbols.iter().position(|known| *known == symbol).unwrap_or_else(|| { symbols.push(symbol); symbols.len() - 1 });
        SYMBOL_WORDS + at as u64
    };
    let symbolic = |value: u64| value.wrapping_sub(SYMBOL_WORDS) < SYMBOL_RANGE;
    let constant = |at: usize| {
        let value = vm.programs.constant(program, at)?;
        if vm.heap_reads().v128(value).is_some() { return None; }
        let bits = vm.decode_bits64(value).unwrap_or(u64::from(value.wasm_bits32()));
        (!symbolic(bits)).then_some(bits)
    };
    let callee = |callee: usize| callable.get(callee).copied()?.then(|| name(Symbol::EntryCell(callee as u32)));
    let global = |pc: usize| -> Option<(u32, quench_runtime::staged::wasm_instance::Storage)> {
        if !full_plan.reachable(pc) { return None; }
        let Slot::Binding(slot) = full_plan.operand(pc) else { return None };
        Some((slot, quench_runtime::staged::wasm_instance::Storage::of(analysis.global_type(slot)?)))
    };
    let vector_constant = |at: usize| vm.heap_reads().v128(vm.programs.constant(program, at)?);
    let vector_move = |pc: usize| full_plan.reachable(pc) && full_plan.operand(pc) == Slot::Vector;
    let table_bindings = |pc: usize| -> Option<(Option<u32>, Option<u32>)> {
        if !full_plan.reachable(pc) { return None; }
        let first = match full_plan.operand(pc) { Slot::Binding(slot) => Some(slot), _ => None };
        let second = match full_plan.seconds[pc] { Some(Slot::Binding(slot)) => Some(slot), _ => None };
        Some((first, second))
    };
    let references = |pc: usize| -> Vec<u16> { full_plan.references.get(pc).cloned().unwrap_or_default() };
    let gc_reads = |pc: usize| -> Vec<(u16, u64)> {
        full_plan.gc_reads.get(pc).map_or_else(Vec::new, |reads| reads.iter().map(|(register, kind)| (*register, *kind as u64)).collect())
    };
    let world = wasm_stitch::World {
        constant: &constant,
        memory_slot,
        // An instance's memory may move when it grows: the image takes no reservation as given.
        memory_stable: false,
        callee: &callee,
        grows_memory: &analysis.grows_memory,
        vm_call: UNNAMED,
        global: &global,
        globals_offset: CONTEXT_GLOBALS as u64,
        indirect: &|_| None,
        table_callout: UNNAMED,
        gc_callout: UNNAMED,
        allocate_callout: UNNAMED,
        references: &references,
        gc_reads: &gc_reads,
        table_bindings: &table_bindings,
        registers: function.registers,
        vector_constant: &vector_constant,
        vector_move: &vector_move,
    };
    let returns = plans.get(index)?.as_ref()?.result.is_some();
    let (code, named) = wasm_stitch::stitch_naming(function, usize::from(function.params), returns, &world, &symbolic)?;
    let symbols = symbols.into_inner();
    let named = named.into_iter().map(|(offset, value)| symbols.get(usize::try_from(value - SYMBOL_WORDS).ok()?).map(|symbol| (offset, *symbol)))
        .collect::<Option<Vec<_>>>()?;
    Some(WasmImageRows { code, symbols: named })
}
