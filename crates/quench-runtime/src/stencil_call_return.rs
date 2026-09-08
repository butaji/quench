//! Guarded monomorphic call-return specialization for pure constant callees.

use crate::ir::Opcode;
use crate::machine::{BaselineEntry, CodeView};
use std::{cell::RefCell, rc::Rc};

const REGION_LEN: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CallReturnSelection {
    callee_slot: u16,
}

struct InstalledTarget {
    target: Rc<crate::value::FunctionValue>,
    constant: crate::machine::NativeLoadConstPlan,
}

pub(crate) struct NativeCallReturnPlan {
    selection: CallReturnSelection,
    policy: crate::stencil_policy::ExecutionPolicy,
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    installed: Option<InstalledTarget>,
}

impl NativeCallReturnPlan {
    pub(crate) fn new(
        selection: CallReturnSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.local_fusions.numeric().then_some(Self {
            selection,
            policy: policy.with_leaf_dependencies(),
            owner,
            installed: None,
        })
    }

    pub(crate) const fn span(&self) -> usize {
        REGION_LEN
    }

    pub(crate) fn route() -> impl Iterator<Item = &'static str> {
        ["LoadLocalChecked", "Call", "Return"].into_iter()
    }

    pub(crate) fn execute(
        &mut self,
        environment: &crate::environment::Environment,
    ) -> Option<crate::value::Value> {
        let bits = environment.proven_tagged_bits(self.selection.callee_slot)?;
        let crate::value::Value::Function(target) = crate::register_file::own_tagged_bits(bits)?
        else {
            return None;
        };
        if !self.target_matches(&target) {
            self.install(target.clone())?;
        }
        let installed = self.installed.as_mut()?;
        let bits = installed.constant.execute().ok()?;
        crate::register_file::own_tagged_bits(bits)
    }

    fn target_matches(&self, target: &Rc<crate::value::FunctionValue>) -> bool {
        self.installed
            .as_ref()
            .is_some_and(|installed| Rc::ptr_eq(&installed.target, target))
    }

    fn install(&mut self, target: Rc<crate::value::FunctionValue>) -> Option<()> {
        let bits = constant_return_bits(&target)?;
        let constant = crate::machine::NativeLoadConstPlan::new_with_shared(
            bits,
            self.policy,
            Rc::clone(&self.owner),
        )?;
        self.installed = Some(InstalledTarget { target, constant });
        Some(())
    }
}

pub(crate) fn select_call_return(
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<CallReturnSelection> {
    let [load, call, ret] = entries.get(start..start.checked_add(REGION_LEN)?)? else {
        return None;
    };
    let load = load.instruction;
    let call = call.instruction;
    let ret = ret.instruction;
    let shape = load.opcode == Opcode::LoadLocal
        && call.opcode == Opcode::Call
        && call.flags == 0
        && call.b == load.a
        && ret.opcode == Opcode::Return
        && ret.a == call.a;
    shape.then_some(())?;
    cfg.region_control(start, start + REGION_LEN)?;
    Some(CallReturnSelection {
        callee_slot: load.b,
    })
}

fn constant_return_bits(function: &crate::value::FunctionValue) -> Option<u64> {
    let callable_kind = matches!(
        function.kind,
        crate::ops::FunctionKind::Ordinary | crate::ops::FunctionKind::Arrow
    );
    (callable_kind && !function.is_async && function.params == 0).then_some(())?;
    let code = function.code.code()?;
    (code.len() == 4).then_some(())?;
    let (result, constant) = code.constant_at(0)?;
    let ret = code.instruction(1)?;
    let trailing = code.constant_at(2)?;
    let trailing_return = code.instruction(3)?;
    let canonical_epilogue = ret.opcode == Opcode::Return
        && ret.a == result
        && matches!(trailing.1, crate::ops::Constant::Undefined)
        && trailing_return.opcode == Opcode::Return
        && trailing_return.a == trailing.0;
    canonical_epilogue
        .then(|| crate::machine::constant_word_bits(constant))
        .flatten()
}
