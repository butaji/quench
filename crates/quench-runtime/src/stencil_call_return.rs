//! Guarded bounded call-return specialization over canonical function facts.

use crate::ir::Opcode;
use crate::machine::BaselineEntry;
use std::{cell::RefCell, rc::Rc};

const ZERO_ARGUMENT_LEN: usize = 3;
const ONE_ARGUMENT_LEN: usize = 4;
const MAX_TARGETS: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CallReturnSelection {
    Zero {
        callee_slot: u16,
    },
    One {
        callee_slot: u16,
        argument_slot: u16,
    },
}

enum InstalledBody {
    Constant(crate::machine::NativeLoadConstPlan),
    Affine(crate::function_physical::NumericAffineI32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TargetKey {
    Constant(u64),
    Affine(crate::function_physical::NumericAffineI32),
}

struct InstalledTarget {
    key: TargetKey,
    body: InstalledBody,
}

pub(crate) struct NativeCallReturnPlan {
    selection: CallReturnSelection,
    policy: crate::stencil_policy::ExecutionPolicy,
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    targets: Vec<InstalledTarget>,
    affine: Option<crate::stencil_numeric_integer_loop::NativeAffineI32Transform>,
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
            targets: Vec::with_capacity(MAX_TARGETS),
            affine: None,
        })
    }

    pub(crate) const fn span(&self) -> usize {
        match self.selection {
            CallReturnSelection::Zero { .. } => ZERO_ARGUMENT_LEN,
            CallReturnSelection::One { .. } => ONE_ARGUMENT_LEN,
        }
    }

    pub(crate) fn route(&self) -> impl Iterator<Item = &'static str> {
        ["LoadLocalChecked", "Call", "Return"].into_iter()
    }

    pub(crate) fn profile_name(&self) -> &'static str {
        if self.targets.len() > 1 {
            "polymorphic_call_return"
        } else {
            "monomorphic_call_return"
        }
    }

    pub(crate) fn execute(
        &mut self,
        environment: &crate::environment::Environment,
    ) -> Option<crate::value::Value> {
        let target = self.read_target(environment)?;
        let key = self.target_key(&target)?;
        let index = self.target_index(key).or_else(|| self.install(key))?;
        match &mut self.targets[index].body {
            InstalledBody::Constant(constant) => {
                let bits = constant.execute().ok()?;
                crate::register_file::own_tagged_bits(bits)
            }
            InstalledBody::Affine(fact) => {
                let fact = *fact;
                let input = self.read_argument(environment)?;
                let result = self
                    .affine
                    .as_mut()?
                    .execute(input, fact.multiplier, fact.addend)?;
                Some(crate::value::Value::Number(f64::from(result)))
            }
        }
    }

    fn read_target(
        &self,
        environment: &crate::environment::Environment,
    ) -> Option<Rc<crate::value::FunctionValue>> {
        let slot = match self.selection {
            CallReturnSelection::Zero { callee_slot }
            | CallReturnSelection::One { callee_slot, .. } => callee_slot,
        };
        let bits = environment.proven_tagged_bits(slot)?;
        let crate::value::Value::Function(target) = crate::register_file::own_tagged_bits(bits)?
        else {
            return None;
        };
        Some(target)
    }

    fn read_argument(&self, environment: &crate::environment::Environment) -> Option<i32> {
        let CallReturnSelection::One { argument_slot, .. } = self.selection else {
            return None;
        };
        let bits = environment.proven_tagged_bits(argument_slot)?;
        let crate::value::Value::Number(value) = crate::register_file::own_tagged_bits(bits)?
        else {
            return None;
        };
        crate::stencil_numeric_integer_selection::exact_i32(value)
    }

    fn target_key(&self, target: &crate::value::FunctionValue) -> Option<TargetKey> {
        match self.selection {
            CallReturnSelection::Zero { .. } => {
                constant_return_bits(target).map(TargetKey::Constant)
            }
            CallReturnSelection::One { .. } => {
                crate::function_physical::numeric_affine_callable(target).map(TargetKey::Affine)
            }
        }
    }

    fn target_index(&self, key: TargetKey) -> Option<usize> {
        self.targets
            .iter()
            .position(|installed| installed.key == key)
    }

    fn install(&mut self, key: TargetKey) -> Option<usize> {
        (self.targets.len() < MAX_TARGETS).then_some(())?;
        let body = match key {
            TargetKey::Constant(bits) => self.constant_body(bits)?,
            TargetKey::Affine(fact) => self.affine_body(fact)?,
        };
        let index = self.targets.len();
        self.targets.push(InstalledTarget { key, body });
        Some(index)
    }

    fn constant_body(&self, bits: u64) -> Option<InstalledBody> {
        crate::machine::NativeLoadConstPlan::new_with_shared(
            bits,
            self.policy,
            Rc::clone(&self.owner),
        )
        .map(InstalledBody::Constant)
    }

    fn affine_body(
        &mut self,
        fact: crate::function_physical::NumericAffineI32,
    ) -> Option<InstalledBody> {
        if self.affine.is_none() {
            self.affine = Some(
                crate::stencil_numeric_integer_loop::NativeAffineI32Transform::new(Rc::clone(
                    &self.owner,
                ))?,
            );
        }
        Some(InstalledBody::Affine(fact))
    }
}

pub(crate) fn select_call_return(
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<CallReturnSelection> {
    select_one_argument(entries, cfg, start).or_else(|| select_zero_arguments(entries, cfg, start))
}

fn select_zero_arguments(
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<CallReturnSelection> {
    let [load, call, ret] = entries.get(start..start.checked_add(ZERO_ARGUMENT_LEN)?)? else {
        return None;
    };
    let (load, call, ret) = (load.instruction, call.instruction, ret.instruction);
    (load.opcode == Opcode::LoadLocal
        && call.opcode == Opcode::Call
        && call.flags == 0
        && call.b == load.a
        && ret.opcode == Opcode::Return
        && ret.a == call.a)
        .then_some(())?;
    cfg.region_control(start, start + ZERO_ARGUMENT_LEN)?;
    Some(CallReturnSelection::Zero {
        callee_slot: load.b,
    })
}

fn select_one_argument(
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<CallReturnSelection> {
    let [callee, argument, call, ret] = entries.get(start..start.checked_add(ONE_ARGUMENT_LEN)?)?
    else {
        return None;
    };
    let (callee, argument) = (callee.instruction, argument.instruction);
    let (call, ret) = (call.instruction, ret.instruction);
    (callee.opcode == Opcode::LoadLocal
        && argument.opcode == Opcode::LoadLocal
        && call.opcode == Opcode::Call
        && call.flags == 1
        && call.b == callee.a
        && call.c == argument.a
        && ret.opcode == Opcode::Return
        && ret.a == call.a)
        .then_some(())?;
    cfg.region_control(start, start + ONE_ARGUMENT_LEN)?;
    Some(CallReturnSelection::One {
        callee_slot: callee.b,
        argument_slot: argument.b,
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
    (ret.opcode == Opcode::Return
        && ret.a == result
        && matches!(trailing.1, crate::ops::Constant::Undefined)
        && trailing_return.opcode == Opcode::Return
        && trailing_return.a == trailing.0)
        .then(|| crate::machine::constant_word_bits(constant))
        .flatten()
}
