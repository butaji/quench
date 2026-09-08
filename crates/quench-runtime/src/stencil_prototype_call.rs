//! Prototype-data call composition over canonical residual and cache facts.

use crate::machine::{BaselineEntry, CodeView};

const REGION_LEN: usize = 4;
pub(crate) const PROFILE_NAME: &str = "prototype_data_add_const_return";

#[derive(Clone, Copy)]
pub(crate) struct SingleArgumentCallSelection {
    pub(crate) callee_slot: u16,
    pub(crate) receiver_slot: u16,
}

pub(crate) struct NativePrototypeCallPlan {
    selection: SingleArgumentCallSelection,
    fact: Option<crate::function_call_fact::OwnFieldAddReturn>,
}

impl NativePrototypeCallPlan {
    pub(crate) const fn new(selection: SingleArgumentCallSelection) -> Self {
        Self {
            selection,
            fact: None,
        }
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<i32> {
        let target = environment.retain_proven_function(self.selection.callee_slot)?;
        let fact = crate::function_call_fact::stable_own_field_add_return(&mut self.fact, &target)?;
        environment.with_proven_object(self.selection.receiver_slot, |receiver| {
            execute_prototype_call(&target, &fact, receiver)
        })?
    }

    pub(crate) const fn span(&self) -> usize {
        REGION_LEN
    }

    pub(crate) const fn installed(&self) -> bool {
        self.fact.is_some()
    }
}

fn execute_prototype_call(
    target: &crate::value::FunctionValue,
    fact: &crate::function_call_fact::OwnFieldAddReturn,
    receiver: &crate::value::ObjectData,
) -> Option<i32> {
    let code = target.code.code()?;
    let metadata = code.metadata_at(1)?;
    (metadata.name.as_deref()? == fact.field.as_ref()).then_some(())?;
    let access =
        crate::vm::get_named_cached_prototype_guard(receiver, &fact.field, &metadata.named_cache)?;
    let mut context = crate::native_property::NativePrototypeAddContext::new(access, fact.addend);
    let status = crate::native_property::execute_prototype_add_i32(&mut context);
    context.result(status)
}

pub(crate) fn select_single_argument_call(
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<SingleArgumentCallSelection> {
    let [callee, receiver, call, ret] = entries.get(start..start.checked_add(REGION_LEN)?)? else {
        return None;
    };
    let (callee, receiver, call, ret) = (
        callee.instruction,
        receiver.instruction,
        call.instruction,
        ret.instruction,
    );
    validate_shape(callee, receiver, call, ret)?;
    cfg.region_control(start, start + REGION_LEN)?;
    Some(SingleArgumentCallSelection {
        callee_slot: callee.b,
        receiver_slot: receiver.b,
    })
}

fn validate_shape(
    callee: crate::ir::Instruction,
    receiver: crate::ir::Instruction,
    call: crate::ir::Instruction,
    ret: crate::ir::Instruction,
) -> Option<()> {
    use crate::ir::Opcode::{Call, LoadLocal};
    (callee.opcode == LoadLocal
        && receiver.opcode == LoadLocal
        && call.opcode == Call
        && call.flags == 1
        && call.b == callee.a
        && call.c == receiver.a
        && ret == crate::ir::Instruction::ret(call.a))
    .then_some(())
}

pub(crate) fn route() -> impl Iterator<Item = &'static str> {
    ["GetNQuickened", "AddConst", "Return"].into_iter()
}
