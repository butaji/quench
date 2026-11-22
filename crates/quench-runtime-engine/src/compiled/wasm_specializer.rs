//! The Wasm tiers as specializers of the interpreter onto one program (`vm::specializer`), as
//! the JS tiers are (`RegionSpecializer`). The u-jit's Cranelift body and entry
//! (`Analysis::build_function`, `build_entry`) and the b-jit's stitched rows (`stitched_entry`,
//! which runs `wasm_stitch::stitch`) are the two publications. The callers keep their drivers,
//! their order and their fallbacks: a failed specialization is an `Err`, which they read as the
//! function running in the interpreter (or as a callee without an entry).

use super::*;
use quench_runtime::vm::specializer::Specializer;

/// The tier a Wasm function is specialized for, with the inputs that tier reads.
pub(crate) enum WasmTierChoice<'s, 'a> {
    /// The u-jit: a Cranelift body and entry, compiled later (`compile_built_functions`).
    Optimizing {
        analysis: &'s Analysis<'a>,
        isa: &'s dyn cranelift_codegen::isa::TargetIsa,
        guarded: bool,
        fixed_base: Option<i32>,
        diagnostics_enabled: bool,
    },
    /// The b-jit: the function's stitched rows, which are its entry (`stitched_entry`).
    Baseline {
        vm: &'s Vm,
        program_id: ProgramId,
        env: Value,
        memory_slot: Option<u32>,
        entry_cells: &'s [std::cell::Cell<usize>],
        callable: &'s [bool],
        grows_memory: &'s quench_runtime::bitset::BitSet,
    },
}

/// One Wasm function of one program, to be specialized for `tier`.
pub(crate) struct WasmSpecializer<'s, 'a> {
    /// The function's index in the program.
    pub(crate) index: usize,
    /// Every function's interface plan (`Plan::interface`), which callers read.
    pub(crate) plans: &'s [Option<Plan>],
    /// The function's full plan, which its compilation reads.
    pub(crate) full_plan: &'s Plan,
    /// The function's indirect reference sites: (pc, cache site id).
    pub(crate) reference_site_ids: &'a [(usize, usize)],
    pub(crate) tier: WasmTierChoice<'s, 'a>,
}

/// What a Wasm specialization publishes.
pub(crate) enum WasmResidual {
    /// The Cranelift body and entry of the function, not yet compiled to code.
    Compiled(ir::Function, ir::Function),
    /// The function's stitched rows: its whole entry, in the `WasmEntry` ABI.
    Rows(Vec<u8>),
}

impl Specializer for WasmSpecializer<'_, '_> {
    type Residual = WasmResidual;

    fn specialize(self, program: &ResidualProgram) -> Result<Self::Residual, String> {
        let Self { index, plans, full_plan, reference_site_ids, tier } = self;
        match tier {
            WasmTierChoice::Optimizing { analysis, isa, guarded, fixed_base, diagnostics_enabled } => {
                // The body, then the entry, in the order the u-jit driver always built them.
                let body = analysis.build_function(
                    isa,
                    index,
                    full_plan,
                    plans,
                    guarded,
                    fixed_base,
                    reference_site_ids,
                    diagnostics_enabled,
                );
                let plan = plans
                    .get(index)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| format!("function {index} has no plan"))?;
                let entry = analysis.build_entry(isa, index, plan, MemoryArguments::of(fixed_base, guarded));
                let body = body.ok_or_else(|| format!("function {index} does not build a body"))?;
                Ok(WasmResidual::Compiled(body, entry))
            }
            WasmTierChoice::Baseline { vm, program_id, env, memory_slot, entry_cells, callable, grows_memory } => {
                stitched_entry(
                    vm,
                    program_id,
                    program,
                    index,
                    (plans, entry_cells, callable, grows_memory),
                    memory_slot,
                    (full_plan, reference_site_ids, env),
                )
                .map(WasmResidual::Rows)
                .ok_or_else(|| format!("function {index} does not stitch"))
            }
        }
    }
}
