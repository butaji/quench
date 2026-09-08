//! Effect-safe own-property store/return call composition.

use crate::machine::{BaselineEntry, CodeView};

const REGION_LEN: usize = 5;
pub(crate) const PROFILE_NAME: &str = "guarded_own_store_load_return";

#[derive(Clone, Copy)]
pub(crate) struct PropertyStoreCallSelection {
    callee_slot: u16,
    receiver_slot: u16,
    value: f64,
}

pub(crate) struct NativePropertyStoreCallPlan {
    selection: PropertyStoreCallSelection,
    fact: Option<crate::function_call_fact::OwnFieldStoreReturn>,
}

impl NativePropertyStoreCallPlan {
    pub(crate) const fn new(selection: PropertyStoreCallSelection) -> Self {
        Self {
            selection,
            fact: None,
        }
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<f64> {
        let target = environment.retain_proven_function(self.selection.callee_slot)?;
        let fact =
            crate::function_call_fact::stable_own_field_store_return(&mut self.fact, &target)?;
        environment.with_proven_object(self.selection.receiver_slot, |receiver| {
            execute_store(&target, &fact, receiver, self.selection.value)
        })?
    }

    pub(crate) const fn span(&self) -> usize {
        REGION_LEN
    }
}

fn execute_store(
    target: &crate::value::FunctionValue,
    fact: &crate::function_call_fact::OwnFieldStoreReturn,
    receiver: &crate::value::ObjectData,
    value: f64,
) -> Option<f64> {
    let code = target.code.code()?;
    let metadata = code.metadata_at(4)?;
    (metadata.name.as_deref()? == fact.field.as_ref()).then_some(())?;
    let shape = crate::identity::ShapeId(receiver.semantic_layout_id());
    let property = crate::identity::property_key_id(&fact.field);
    let slot = code
        .quickening_site(4)?
        .borrow_mut()
        .probe_shape(shape, property)?;
    let access = receiver.guarded_plain_slot(shape.0, slot, &fact.field)?;
    let mut context = crate::native_property::NativeOwnStoreContext::new(access, value);
    (crate::native_property::execute_own_store_number(&mut context) == 1).then_some(value)
}

pub(crate) fn select_property_store_call(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<PropertyStoreCallSelection> {
    let [callee, receiver, value, call, ret] = entries.get(start..start + REGION_LEN)? else {
        return None;
    };
    let (callee, receiver, value, call, ret) = (
        callee.instruction,
        receiver.instruction,
        value.instruction,
        call.instruction,
        ret.instruction,
    );
    validate_shape(code, start, callee, receiver, value, call, ret)?;
    cfg.region_control(start, start + REGION_LEN)?;
    let crate::ops::Constant::Number(number) = code.constant(value.b)? else {
        return None;
    };
    Some(PropertyStoreCallSelection {
        callee_slot: callee.b,
        receiver_slot: receiver.b,
        value: *number,
    })
}

fn validate_shape(
    code: CodeView<'_>,
    start: usize,
    callee: crate::ir::Instruction,
    receiver: crate::ir::Instruction,
    value: crate::ir::Instruction,
    call: crate::ir::Instruction,
    ret: crate::ir::Instruction,
) -> Option<()> {
    use crate::ir::Opcode::{Call, LoadConst, LoadLocal};
    (callee.opcode == LoadLocal
        && receiver.opcode == LoadLocal
        && value.opcode == LoadConst
        && call.opcode == Call
        && call.flags == 2
        && call.b == callee.a
        && code.operand_window_at(start + 3)? == [receiver.a, value.a]
        && ret == crate::ir::Instruction::ret(call.a))
    .then_some(())
}

pub(crate) fn route() -> impl Iterator<Item = &'static str> {
    ["SetN", "GetNQuickened", "Return"].into_iter()
}
