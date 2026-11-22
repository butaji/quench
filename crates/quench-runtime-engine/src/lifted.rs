//! Methods of the core's engine types that run engine code; the core calls them through
//! `quench_runtime::vm::engine_calls2`.
use crate::engine_prelude::*;

#[quench_macros::ext(AheadImagesLifted160)]
impl AheadImages {


    /// The image of `program`, lowered under its closed-world facts for the baseline ISA.
    #[inline]
    pub fn compile(program: &ResidualProgram) -> Result<Self, String> {
        let facts = quench_runtime::compile::aot_facts::analyze(program);
        let image = aot_image::lower(program, &facts, &ImageTarget::default())?;
        Ok(Self { facts: std::cell::OnceCell::from(facts), ..Self::of(image) })
    }
}



#[quench_macros::ext(LaneViewLifted648)]
impl LaneView {




    /// Retires field hints and native bodies (and plans) that name shape IDs when compaction renumbers them.
    pub fn invalidate_shape_bindings(&self) {
        // SAFETY: no lane body runs during a collection.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        if let LaneBody::Compiling { pending, .. } = body { pending.retire_named_shapes(); }
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
