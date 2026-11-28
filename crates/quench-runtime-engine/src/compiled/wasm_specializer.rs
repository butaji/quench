//! The Wasm tier as a specializer of the interpreter onto one program (`vm::specializer`), as
//! the JS tiers are (`RegionSpecializer`). The u-jit's Cranelift body and entry
//! (`Analysis::build_function`, `build_entry`) are its publication. The callers keep their drivers,
//! their order and their fallbacks: a failed specialization is an `Err`, which they read as the
//! function running in the interpreter.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::vm::specializer::Specializer;

/// The inputs the u-jit's Cranelift body and entry read: the analysis, the ISA and the memory's guard.
pub(crate) struct WasmOptimizing<'s, 'a> {
    pub(crate) analysis: &'s Analysis<'a>,
    pub(crate) isa: &'s dyn cranelift_codegen::isa::TargetIsa,
    pub(crate) guarded: bool,
    pub(crate) fixed_base: Option<i32>,
    pub(crate) diagnostics_enabled: bool,
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
    /// The gate's proof that this function's Wasm compile may run (`UnitKind::Wasm`, minted by `specialize`).
    pub(crate) admitted: super::admission::CompiledUnit,
    pub(crate) tier: WasmOptimizing<'s, 'a>,
}

/// What a Wasm specialization publishes: the Cranelift body and entry of the function, not yet compiled to code.
pub(crate) enum WasmResidual {
    Compiled(ir::Function, ir::Function),
}

impl Specializer for WasmSpecializer<'_, '_> {
    type Residual = WasmResidual;

    fn specialize(self, _program: &ResidualProgram) -> Result<Self::Residual, String> {
        let Self { index, plans, full_plan, reference_site_ids, tier, admitted } = self;
        let _ = admitted;
        let WasmOptimizing { analysis, isa, guarded, fixed_base, diagnostics_enabled } = tier;
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
}
