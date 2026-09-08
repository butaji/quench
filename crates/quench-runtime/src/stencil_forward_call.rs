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
enum PairCallSelection {
    Forward {
        callee_slot: u16,
        target_slot: u16,
        argument: i32,
    },
    IntegerSwitch {
        callee_slot: u16,
        discriminant: i32,
        argument: i32,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct ForwardPairSelection {
    calls: [PairCallSelection; 2],
}

pub(crate) struct NativeForwardPairPlan {
    selection: ForwardPairSelection,
    targets: Vec<crate::function_physical::NumericAffineI32>,
    switches: Vec<InstalledSwitch>,
    machine: LazyAffineTransform,
}

struct InstalledSwitch {
    target: Rc<crate::value::FunctionValue>,
    fact: crate::function_call_fact::IntegerSwitchI32,
}

impl NativeForwardPairPlan {
    pub(crate) fn new(
        selection: ForwardPairSelection,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        Some(Self {
            selection,
            targets: Vec::with_capacity(MAX_TARGETS),
            switches: Vec::with_capacity(MAX_TARGETS),
            machine: LazyAffineTransform::Cold(owner),
        })
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<i32> {
        let calls = self.calls(environment)?;
        let facts = [calls[0].0, calls[1].0];
        facts.iter().try_for_each(|fact| self.observe(*fact))?;
        let left = self
            .machine
            .execute(calls[0].1, facts[0].multiplier, facts[0].addend)?;
        let right = self
            .machine
            .execute(calls[1].1, facts[1].multiplier, facts[1].addend)?;
        left.checked_add(right)
    }

    pub(crate) const fn span(&self) -> usize {
        PAIR_REGION_LEN
    }

    pub(crate) const fn profile_name(&self) -> &'static str {
        match self.selection.calls {
            [PairCallSelection::IntegerSwitch { .. }, PairCallSelection::IntegerSwitch { .. }] => {
                "integer_switch_arithmetic_return"
            }
            _ => PROFILE_NAME,
        }
    }

    pub(crate) const fn route(&self) -> &'static [&'static str] {
        match self.selection.calls {
            [PairCallSelection::IntegerSwitch { .. }, PairCallSelection::IntegerSwitch { .. }] => {
                &["Switch", "AddConst", "MulConst", "SubConst", "Return"]
            }
            _ => &["LoadLocalChecked", "Call", "Return"],
        }
    }

    fn calls(
        &mut self,
        environment: &crate::environment::Environment,
    ) -> Option<[(crate::function_physical::NumericAffineI32, i32); 2]> {
        Some([
            self.call_at(environment, self.selection.calls[0])?,
            self.call_at(environment, self.selection.calls[1])?,
        ])
    }

    fn call_at(
        &mut self,
        environment: &crate::environment::Environment,
        call: PairCallSelection,
    ) -> Option<(crate::function_physical::NumericAffineI32, i32)> {
        match call {
            PairCallSelection::Forward {
                callee_slot,
                target_slot,
                argument,
            } => {
                let forwarder = environment_function(environment, callee_slot)?;
                crate::function_call_fact::forwards_one_argument(&forwarder)?;
                let target = environment_function(environment, target_slot)?;
                Some((
                    crate::function_call_fact::numeric_affine_callable(&target)?,
                    argument,
                ))
            }
            PairCallSelection::IntegerSwitch {
                callee_slot,
                discriminant,
                argument,
            } => {
                let callee = environment_function(environment, callee_slot)?;
                let fact = self.switch_fact(callee)?;
                Some((fact.select(discriminant), argument))
            }
        }
    }

    fn switch_fact(
        &mut self,
        target: Rc<crate::value::FunctionValue>,
    ) -> Option<&crate::function_call_fact::IntegerSwitchI32> {
        if let Some(index) = self
            .switches
            .iter()
            .position(|installed| Rc::ptr_eq(&installed.target, &target))
        {
            return Some(&self.switches[index].fact);
        }
        (self.switches.len() < MAX_TARGETS).then_some(())?;
        let fact = crate::function_call_fact::integer_switch_callable(&target)?;
        self.switches.push(InstalledSwitch { target, fact });
        self.switches.last().map(|installed| &installed.fact)
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
        crate::function_call_fact::forwards_one_argument(&forwarder)?;
        let target = function_at(registers, self.selection.target)?;
        let fact = crate::function_call_fact::numeric_affine_callable(&target)?;
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
    let calls = [
        select_pair_call(code, pc, ops, 0)?,
        select_pair_call(code, pc, ops, 4)?,
    ];
    let add = op(8);
    (add.opcode == Opcode::Add
        && add.b == op(3).a
        && add.c == op(7).a
        && op(9) == crate::ir::Instruction::ret(add.a))
    .then_some(())?;
    cfg.region_control(pc, pc + PAIR_REGION_LEN)?;
    Some(ForwardPairSelection { calls })
}

fn select_pair_call(
    code: CodeView<'_>,
    start: usize,
    entries: &[BaselineEntry],
    base: usize,
) -> Option<PairCallSelection> {
    let op = |offset: usize| entries.get(base + offset).map(|entry| entry.instruction);
    let (callee, first, second, call) = (op(0)?, op(1)?, op(2)?, op(3)?);
    (callee.opcode == Opcode::LoadLocal
        && second.opcode == Opcode::LoadConst
        && call.opcode == Opcode::Call
        && call.flags == 2
        && call.b == callee.a
        && code.operand_window_at(start + base + 3)? == [first.a, second.a])
    .then_some(())?;
    let second = number_constant(code, second.b)?;
    match first.opcode {
        Opcode::LoadLocal => Some(PairCallSelection::Forward {
            callee_slot: callee.b,
            target_slot: first.b,
            argument: second,
        }),
        Opcode::LoadConst => Some(PairCallSelection::IntegerSwitch {
            callee_slot: callee.b,
            discriminant: number_constant(code, first.b)?,
            argument: second,
        }),
        _ => None,
    }
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
