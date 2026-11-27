//! Methods of the core's engine types that run engine code; the core calls them through
//! `quench_runtime::vm::engine_calls2`.
#[allow(unused_imports)]
use crate::ext_traits::*;
#[allow(unused_imports)]
use crate::engine_prelude::*;
use quench_runtime::vm::engine_types::{AheadImages, LaneView, LaneBody};
#[cfg(feature = "jit")]
use crate::aot_image::{self, ImageTarget};

#[quench_macros::ext(AheadImagesLifted160)]
impl AheadImages {


    /// The image of `program`, lowered under its closed-world facts for this process's CPU, which
    /// runs it (`NativeTarget::host`).
    #[cfg(feature = "jit")]
    #[inline]
    pub fn compile(program: &ResidualProgram) -> Result<Self, String> {
        let facts = quench_runtime::compile::aot_facts::analyze(program);
        let image = aot_image::lower(program, &facts, &ImageTarget::host())?;
        Ok(Self { facts: std::cell::OnceCell::from(facts), ..Self::of(image) })
    }

    /// The image of Wasm program `id` (`aot_image::lower_wasm`), planned from the module's binding types.
    #[inline]
    pub fn compile_wasm(vm: &Vm, id: ProgramId, program: &ResidualProgram, bindings: &[quench_runtime::WasmBindingType]) -> Result<Self, String> {
        #[cfg(feature = "jit")]
        { Ok(Self::of(aot_image::lower_wasm(vm, id, program, bindings)?)) }
        #[cfg(not(feature = "jit"))]
        {
            let _ = (vm, id, program, bindings);
            Err("ahead Wasm image lowering requires the jit feature".into())
        }
    }
}



#[quench_macros::ext(LaneViewLifted648)]
impl LaneView {




    /// Retires field hints and native bodies (and plans) that name shape IDs when compaction renumbers them.
    pub fn invalidate_shape_bindings(&self) {
        // SAFETY: no lane body runs during a collection.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        if let LaneBody::Compiling { pending, .. } = body { crate::compiled::pending_lane_ref(&**pending).retire_named_shapes(); }
        if let LaneBody::Specialized { method, tier_prologue, .. } = body
            && method.lease().is_some_and(|method| method.names_shapes)
        {
            // SAFETY: as `claim_for_tier`; no lane body or handler runs during a collection,
            // and no method borrower is held.
            unsafe {
                (*self.prologue.get()).handler = *tier_prologue;
                method.retire();
            }
            self.drop_body(body);
        }
        self.tier().retired.borrow_mut().clear();
        self.clear_hints(&self.field_records);
        self.clear_name_hints();
        for site in &self.loops {
            let tier = site.tier.borrow();
            tier.retire_named_shapes();
            if let Some(publication) = &tier.code
                && publication.lease().is_some_and(|code| !code.facts.shape_dependencies.is_empty()) {
                publication.retire();
            }
        }
    }
}
