//! Local predicate plans with bounded native control-flow alternatives.

use crate::machine::{NativeNullishPlan, NativeTruthinessPlan};
use crate::stencil_plan::{LocalPredicate, LocalPredicateSelection};

pub(crate) struct NativeLocalPredicatePlan {
    selection: LocalPredicateSelection,
    physical: LocalPredicatePhysical,
}

enum LocalPredicatePhysical {
    Truthiness(NativeTruthinessPlan),
    TruthinessWithConstantBranch {
        leaf: NativeTruthinessPlan,
        branch: crate::stencil_word_composition::NativeWordConstantBranchPlan,
    },
    Nullish(NativeNullishPlan),
}

pub(crate) struct LocalPredicateExecution {
    next: usize,
    live_source: Option<(crate::ir::Register, u64)>,
    result: Option<(crate::ir::Register, u64)>,
    discarded: crate::stencil_plan::DiscardedRegisters,
}

impl LocalPredicateExecution {
    pub(crate) fn commit(
        self,
        registers: &mut crate::register_file::RegisterFile,
    ) -> Option<usize> {
        for register in self.discarded.into_iter().flatten() {
            registers.clear_word(usize::from(register));
        }
        if let Some((register, bits)) = self.live_source {
            registers.write_tagged_bits(usize::from(register), bits)?;
        }
        if let Some((register, bits)) = self.result {
            registers.write_tagged_bits(usize::from(register), bits)?;
        }
        Some(self.next)
    }
}

impl NativeLocalPredicatePlan {
    pub(crate) fn new(
        selection: LocalPredicateSelection,
        branch: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
        arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
        code: crate::machine::CodeView<'_>,
        entries: &[crate::machine::BaselineEntry],
        branch_pc: usize,
    ) -> Option<Self> {
        let physical =
            predicate_physical(selection, branch, policy, arena, code, entries, branch_pc)?;
        Some(Self {
            selection,
            physical,
        })
    }

    fn execute(
        &mut self,
        environment: &crate::environment::Environment,
    ) -> Option<LocalPredicateExecution> {
        let bits = environment.proven_tagged_bits(self.selection.source_slot)?;
        if let Some(result) = self.execute_constant_branch(bits) {
            return Some(result);
        }
        let truthy = execute_leaf(
            &mut self.physical,
            environment,
            self.selection.source_slot,
            bits,
        )?;
        Some(LocalPredicateExecution {
            next: selected_pc(self.selection, truthy),
            live_source: self.selection.live_source.map(|register| (register, bits)),
            result: None,
            discarded: self.selection.discarded,
        })
    }

    fn execute_constant_branch(&mut self, bits: u64) -> Option<LocalPredicateExecution> {
        let LocalPredicatePhysical::TruthinessWithConstantBranch { branch, .. } =
            &mut self.physical
        else {
            return None;
        };
        let arm = branch.execute(bits)?;
        Some(LocalPredicateExecution {
            next: arm.next,
            live_source: self.selection.live_source.map(|register| (register, bits)),
            result: Some((arm.register, arm.bits)),
            discarded: self.selection.discarded,
        })
    }

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        match &self.physical {
            LocalPredicatePhysical::Truthiness(plan) => plan.native_entry_count(),
            LocalPredicatePhysical::TruthinessWithConstantBranch { leaf, branch } => {
                leaf.native_entry_count() + branch.native_entry_count()
            }
            LocalPredicatePhysical::Nullish(plan) => plan.native_entry_count(),
        }
    }

    #[cfg(test)]
    pub(crate) fn composed_identity(
        &self,
    ) -> Option<crate::stencil_region_layout::RegionImageIdentity> {
        match &self.physical {
            LocalPredicatePhysical::TruthinessWithConstantBranch { branch, .. } => {
                Some(branch.identity())
            }
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn composed_entry_count(&self) -> u64 {
        match &self.physical {
            LocalPredicatePhysical::TruthinessWithConstantBranch { branch, .. } => {
                branch.native_entry_count()
            }
            _ => 0,
        }
    }

    #[cfg(test)]
    pub(crate) const fn selection(&self) -> LocalPredicateSelection {
        self.selection
    }
}

fn predicate_physical(
    selection: LocalPredicateSelection,
    branch: crate::ir::Instruction,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    code: crate::machine::CodeView<'_>,
    entries: &[crate::machine::BaselineEntry],
    branch_pc: usize,
) -> Option<LocalPredicatePhysical> {
    match selection.predicate {
        LocalPredicate::Truthiness => {
            truthiness_physical(selection, branch, policy, arena, code, entries, branch_pc)
        }
        LocalPredicate::Nullish => nullish_physical(branch, policy, arena),
    }
}

fn truthiness_physical(
    selection: LocalPredicateSelection,
    branch: crate::ir::Instruction,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    code: crate::machine::CodeView<'_>,
    entries: &[crate::machine::BaselineEntry],
    branch_pc: usize,
) -> Option<LocalPredicatePhysical> {
    let leaf = NativeTruthinessPlan::new_with_shared(branch, policy, arena.clone())?;
    let composed = crate::stencil_word_composition::NativeWordConstantBranchPlan::new(
        code,
        entries,
        branch_pc,
        selection.true_pc,
        selection.false_pc,
        arena,
    );
    Some(match composed {
        Some(branch) => LocalPredicatePhysical::TruthinessWithConstantBranch { leaf, branch },
        None => LocalPredicatePhysical::Truthiness(leaf),
    })
}

fn nullish_physical(
    branch: crate::ir::Instruction,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
) -> Option<LocalPredicatePhysical> {
    let unary =
        crate::ir::Instruction::unary_operator(branch.a, crate::ops::UnaryOp::IsNullish, branch.a);
    NativeNullishPlan::new_with_shared(unary, policy, arena).map(LocalPredicatePhysical::Nullish)
}

fn execute_leaf(
    physical: &mut LocalPredicatePhysical,
    environment: &crate::environment::Environment,
    slot: u16,
    bits: u64,
) -> Option<bool> {
    match physical {
        LocalPredicatePhysical::Truthiness(plan)
        | LocalPredicatePhysical::TruthinessWithConstantBranch { leaf: plan, .. } => {
            match environment.get_number(slot) {
                Some(value) => plan.execute(value).ok(),
                None => plan.execute_tagged_bits(bits).ok(),
            }
        }
        LocalPredicatePhysical::Nullish(plan) => plan.execute(bits).ok(),
    }
}

fn selected_pc(selection: LocalPredicateSelection, truthy: bool) -> usize {
    if truthy {
        selection.true_pc
    } else {
        selection.false_pc
    }
}

pub(crate) fn execute_local_predicate(
    plan: &std::cell::RefCell<NativeLocalPredicatePlan>,
    environment: &crate::environment::Environment,
) -> Option<LocalPredicateExecution> {
    plan.borrow_mut().execute(environment)
}
