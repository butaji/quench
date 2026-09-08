//! Guarded monomorphic method regions over canonical property and call facts.

use crate::ir::Opcode;
use crate::machine::{BaselineEntry, CodeView};
use std::{cell::RefCell, rc::Rc};

const REGION_LEN: usize = 5;
pub(crate) const PROFILE_NAME: &str = "monomorphic_method_call_return";

#[derive(Clone, Copy)]
pub(crate) struct MethodCallSelection {
    receiver_slot: u16,
    method_pc: u8,
    argument: i32,
}

struct InstalledMethod {
    function: Rc<crate::value::FunctionValue>,
    fact: crate::function_call_fact::OwnFieldAddMethod,
}

pub(crate) struct NativeMethodCallPlan {
    selection: MethodCallSelection,
    method: Option<InstalledMethod>,
}

impl NativeMethodCallPlan {
    pub(crate) fn new(
        selection: MethodCallSelection,
        _policy: crate::stencil_policy::ExecutionPolicy,
        _owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        Some(Self {
            selection,
            method: None,
        })
    }

    pub(crate) fn execute(
        &mut self,
        code: CodeView<'_>,
        environment: &crate::environment::Environment,
    ) -> Option<i32> {
        environment.with_proven_object(self.selection.receiver_slot, |receiver| {
            self.execute_receiver(code, receiver)
        })?
    }

    fn execute_receiver(
        &mut self,
        code: CodeView<'_>,
        receiver: &crate::value::ObjectData,
    ) -> Option<i32> {
        receiver_is_native_plain(receiver)?;
        let function = self.read_method(code, receiver)?;
        let fact = self.method_fact(&function)?;
        let access = guarded_field_slot(function.code.code()?, &fact, receiver)?;
        let mut context =
            crate::native_property::NativeMethodAddContext::new(access, self.selection.argument);
        let status = crate::native_property::execute_method_add_i32(&mut context);
        context.result(status)
    }

    fn read_method(
        &mut self,
        code: CodeView<'_>,
        receiver: &crate::value::ObjectData,
    ) -> Option<Rc<crate::value::FunctionValue>> {
        let metadata = code.metadata_at(usize::from(self.selection.method_pc))?;
        let key = metadata.name.as_deref()?;
        crate::vm::derive_plain_prototype_function(receiver, key)
    }

    fn method_fact(
        &mut self,
        function: &Rc<crate::value::FunctionValue>,
    ) -> Option<crate::function_call_fact::OwnFieldAddMethod> {
        if let Some(installed) = &self.method {
            return Rc::ptr_eq(&installed.function, function).then(|| installed.fact.clone());
        }
        let fact = crate::function_call_fact::own_field_add_method(function)?;
        self.method = Some(InstalledMethod {
            function: Rc::clone(function),
            fact: fact.clone(),
        });
        Some(fact)
    }

    pub(crate) const fn span(&self) -> usize {
        REGION_LEN
    }
}

fn receiver_is_native_plain(object: &crate::value::ObjectData) -> Option<()> {
    (!object.has_replacement()
        && !object.is_dictionary()
        && !object.is_realm_global()
        && !object.is_script_global_view()
        && !object.has_regexp_internal_slot())
    .then_some(())
}

fn guarded_field_slot(
    code: CodeView<'_>,
    fact: &crate::function_call_fact::OwnFieldAddMethod,
    receiver: &crate::value::ObjectData,
) -> Option<crate::native_property::GuardedPropertySlot> {
    let metadata = code.metadata_at(4)?;
    let key = metadata.name.as_deref()?;
    (key == fact.field.as_ref()).then_some(())?;
    let (layout, slot) = crate::machine::unpack_named_cache(metadata.named_cache.get())?;
    let access = receiver.guarded_plain_slot(layout, slot, key)?;
    access.accepts_non_owning_store().then_some(access)
}

pub(crate) fn select_method_call(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<MethodCallSelection> {
    let [receiver, method, argument, call, ret] =
        entries.get(start..start.checked_add(REGION_LEN)?)?
    else {
        return None;
    };
    let (receiver, method, argument, call, ret) = (
        receiver.instruction,
        method.instruction,
        argument.instruction,
        call.instruction,
        ret.instruction,
    );
    validate_call_shape(code, start, receiver, method, argument, call, ret)?;
    cfg.region_control(start, start + REGION_LEN)?;
    Some(MethodCallSelection {
        receiver_slot: receiver.b,
        method_pc: u8::try_from(start + 1).ok()?,
        argument: number_constant(code, argument.b)?,
    })
}

fn validate_call_shape(
    code: CodeView<'_>,
    start: usize,
    receiver: crate::ir::Instruction,
    method: crate::ir::Instruction,
    argument: crate::ir::Instruction,
    call: crate::ir::Instruction,
    ret: crate::ir::Instruction,
) -> Option<()> {
    (receiver.opcode == Opcode::LoadLocal
        && method.opcode == Opcode::GetN
        && method.b == receiver.a
        && argument.opcode == Opcode::LoadConst
        && call.opcode == Opcode::CallN
        && call.flags == 1
        && call.b == receiver.a
        && call.c == method.a
        && code.operand_window_at(start + 3)? == [argument.a]
        && ret == crate::ir::Instruction::ret(call.a)
        && code.metadata_at(start + 1)?.name == code.metadata_at(start + 3)?.name)
        .then_some(())
}

fn number_constant(code: CodeView<'_>, id: u16) -> Option<i32> {
    let crate::ops::Constant::Number(value) = code.constant(id)? else {
        return None;
    };
    crate::stencil_numeric_integer_selection::exact_i32(*value)
}

pub(crate) fn route() -> impl Iterator<Item = &'static str> {
    ["GetN", "CallN", "Return"].into_iter()
}
