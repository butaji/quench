//! Guarded call forwarding through the canonical affine callable fact.

use crate::ir::Opcode;
use crate::machine::{BaselineEntry, CodeView};
use std::{cell::RefCell, rc::Rc};

const MAX_TARGETS: usize = 2;
const PAIR_REGION_LEN: usize = 10;
pub(crate) const PROFILE_NAME: &str = "polymorphic_call_return";

enum LazyAffineTransform {
    Cold(Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>),
    Ready(crate::stencil_numeric_integer_loop::NativeAffineI32Transform),
    Rejected,
}

impl LazyAffineTransform {
    fn execute(&mut self, input: i32, multiplier: i32, addend: i32) -> Option<i32> {
        if let Self::Cold(owner) = self {
            *self = crate::stencil_numeric_integer_loop::NativeAffineI32Transform::new(Rc::clone(
                owner,
            ))
            .map(Self::Ready)
            .unwrap_or(Self::Rejected);
        }
        let Self::Ready(machine) = self else {
            return None;
        };
        machine.execute(input, multiplier, addend)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ForwardCallSelection {
    callee: u16,
    target: u16,
    argument: u16,
    destination: u16,
}

pub(crate) struct NativeForwardCallPlan {
    selection: ForwardCallSelection,
    targets: Vec<crate::function_physical::NumericAffineI32>,
    machine: LazyAffineTransform,
}

#[derive(Clone, Copy)]
pub(crate) struct ForwardPairSelection {
    forwarder_slots: [u16; 2],
    target_slots: [u16; 2],
    arguments: [i32; 2],
}

pub(crate) struct NativeForwardPairPlan {
    selection: ForwardPairSelection,
    targets: Vec<crate::function_physical::NumericAffineI32>,
    machine: LazyAffineTransform,
}

impl NativeForwardPairPlan {
    pub(crate) fn new(
        selection: ForwardPairSelection,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        Some(Self {
            selection,
            targets: Vec::with_capacity(MAX_TARGETS),
            machine: LazyAffineTransform::Cold(owner),
        })
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<i32> {
        let facts = self.facts(environment)?;
        facts.iter().try_for_each(|fact| self.observe(*fact))?;
        let left = self.machine.execute(
            self.selection.arguments[0],
            facts[0].multiplier,
            facts[0].addend,
        )?;
        let right = self.machine.execute(
            self.selection.arguments[1],
            facts[1].multiplier,
            facts[1].addend,
        )?;
        left.checked_add(right)
    }

    pub(crate) const fn span(&self) -> usize {
        PAIR_REGION_LEN
    }

    fn facts(
        &self,
        environment: &crate::environment::Environment,
    ) -> Option<[crate::function_physical::NumericAffineI32; 2]> {
        Some([self.fact_at(environment, 0)?, self.fact_at(environment, 1)?])
    }

    fn fact_at(
        &self,
        environment: &crate::environment::Environment,
        index: usize,
    ) -> Option<crate::function_physical::NumericAffineI32> {
        let forwarder = environment_function(environment, self.selection.forwarder_slots[index])?;
        crate::function_physical::forwards_one_argument(&forwarder)?;
        let target = environment_function(environment, self.selection.target_slots[index])?;
        crate::function_physical::numeric_affine_callable(&target)
    }

    fn observe(&mut self, fact: crate::function_physical::NumericAffineI32) -> Option<()> {
        if !self.targets.contains(&fact) {
            (self.targets.len() < MAX_TARGETS).then_some(())?;
            self.targets.push(fact);
        }
        Some(())
    }
}

pub(crate) fn route() -> impl Iterator<Item = &'static str> {
    ["LoadLocalChecked", "Call", "Return"].into_iter()
}

pub(crate) const fn pair_profile_entries() -> usize {
    2
}

impl NativeForwardCallPlan {
    pub(crate) fn new(
        selection: ForwardCallSelection,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        Some(Self {
            selection,
            targets: Vec::with_capacity(MAX_TARGETS),
            machine: LazyAffineTransform::Cold(owner),
        })
    }

    pub(crate) const fn destination(&self) -> u16 {
        self.selection.destination
    }

    pub(crate) fn execute(
        &mut self,
        registers: &crate::register_file::RegisterFile,
    ) -> Option<i32> {
        let forwarder = function_at(registers, self.selection.callee)?;
        crate::function_physical::forwards_one_argument(&forwarder)?;
        let target = function_at(registers, self.selection.target)?;
        let fact = crate::function_physical::numeric_affine_callable(&target)?;
        self.observe(fact)?;
        let input = number_at(registers, self.selection.argument)?;
        self.machine.execute(input, fact.multiplier, fact.addend)
    }

    fn observe(&mut self, fact: crate::function_physical::NumericAffineI32) -> Option<()> {
        if self.targets.contains(&fact) {
            return Some(());
        }
        (self.targets.len() < MAX_TARGETS).then_some(())?;
        self.targets.push(fact);
        Some(())
    }
}

pub(crate) fn select_forward_call(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    pc: usize,
) -> Option<ForwardCallSelection> {
    let instruction = entries.get(pc)?.instruction;
    let [target, argument] = code.operand_window_at(pc)? else {
        return None;
    };
    (instruction.opcode == Opcode::Call && instruction.flags == 2).then_some(())?;
    let end = pc.checked_add(1)?;
    cfg.region_control(pc, end)?;
    Some(ForwardCallSelection {
        callee: instruction.b,
        target: *target,
        argument: *argument,
        destination: instruction.a,
    })
}

pub(crate) fn select_forward_pair(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    pc: usize,
) -> Option<ForwardPairSelection> {
    let ops = entries.get(pc..pc.checked_add(PAIR_REGION_LEN)?)?;
    let op = |index: usize| ops[index].instruction;
    let expected = [
        Opcode::LoadLocal,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::Call,
    ];
    for base in [0, 4] {
        expected
            .iter()
            .enumerate()
            .try_for_each(|(offset, opcode)| (op(base + offset).opcode == *opcode).then_some(()))?;
        let call = op(base + 3);
        let window = code.operand_window_at(pc + base + 3)?;
        (call.flags == 2 && call.b == op(base).a && window == [op(base + 1).a, op(base + 2).a])
            .then_some(())?;
    }
    let add = op(8);
    (add.opcode == Opcode::Add
        && add.b == op(3).a
        && add.c == op(7).a
        && op(9) == crate::ir::Instruction::ret(add.a))
    .then_some(())?;
    cfg.region_control(pc, pc + PAIR_REGION_LEN)?;
    Some(ForwardPairSelection {
        forwarder_slots: [op(0).b, op(4).b],
        target_slots: [op(1).b, op(5).b],
        arguments: [
            number_constant(code, op(2).b)?,
            number_constant(code, op(6).b)?,
        ],
    })
}

fn function_at(
    registers: &crate::register_file::RegisterFile,
    register: u16,
) -> Option<Rc<crate::value::FunctionValue>> {
    let crate::value::Value::Function(function) = registers.get(usize::from(register))? else {
        return None;
    };
    Some(function)
}

fn number_at(registers: &crate::register_file::RegisterFile, register: u16) -> Option<i32> {
    let crate::value::Value::Number(value) = registers.get(usize::from(register))? else {
        return None;
    };
    crate::stencil_numeric_integer_selection::exact_i32(value)
}

fn environment_function(
    environment: &crate::environment::Environment,
    slot: u16,
) -> Option<Rc<crate::value::FunctionValue>> {
    let bits = environment.proven_tagged_bits(slot)?;
    let crate::value::Value::Function(function) = crate::register_file::own_tagged_bits(bits)?
    else {
        return None;
    };
    Some(function)
}

fn number_constant(code: CodeView<'_>, id: u16) -> Option<i32> {
    let crate::ops::Constant::Number(value) = code.constant(id)? else {
        return None;
    };
    crate::stencil_numeric_integer_selection::exact_i32(*value)
}
