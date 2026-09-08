//! Bounded polymorphic own-property return through one call-region recipe.

pub(crate) const PROFILE_NAME: &str = "megamorphic_property_return";

pub(crate) struct NativePropertyReturnCallPlan {
    selection: crate::stencil_prototype_call::SingleArgumentCallSelection,
    fact: Option<crate::function_property_return_fact::OwnFieldReturn>,
}

impl NativePropertyReturnCallPlan {
    pub(crate) const fn new(
        selection: crate::stencil_prototype_call::SingleArgumentCallSelection,
    ) -> Self {
        Self {
            selection,
            fact: None,
        }
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<f64> {
        let target = environment.retain_proven_function(self.selection.callee_slot)?;
        let fact =
            crate::function_property_return_fact::stable_own_field_return(&mut self.fact, &target)?;
        environment.with_proven_object(self.selection.receiver_slot, |receiver| {
            execute_read(&target, &fact, receiver)
        })?
    }

    pub(crate) const fn span(&self) -> usize {
        4
    }

    pub(crate) const fn installed(&self) -> bool {
        self.fact.is_some()
    }
}

fn execute_read(
    target: &crate::value::FunctionValue,
    fact: &crate::function_property_return_fact::OwnFieldReturn,
    receiver: &crate::value::ObjectData,
) -> Option<f64> {
    let code = target.code.code()?;
    let metadata = code.metadata_at(1)?;
    (metadata.name.as_deref()? == fact.field.as_ref()).then_some(())?;
    let shape = crate::identity::ShapeId(receiver.semantic_layout_id());
    let property = crate::identity::property_key_id(&fact.field);
    let slot = code
        .quickening_site(1)?
        .borrow_mut()
        .probe_shape(shape, property)?;
    receiver
        .guarded_plain_slot(shape.0, slot, &fact.field)?
        .load_own_number_now()
}

pub(crate) fn select_property_return_call(
    entries: &[crate::machine::BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<crate::stencil_prototype_call::SingleArgumentCallSelection> {
    crate::stencil_prototype_call::select_single_argument_call(entries, cfg, start)
}

pub(crate) fn route() -> impl Iterator<Item = &'static str> {
    ["GetNQuickened", "Return"].into_iter()
}
