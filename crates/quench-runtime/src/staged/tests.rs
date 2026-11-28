#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt, OperationNamesExt};
use super::*;
use crate::bytecode::Operand;
use crate::heap::{Cell, Heap, Object};
use crate::Value;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

mod allocations;
mod branches;
mod callouts;
mod decoding;
mod values;

/// The registers an allocation row reads from its window.
fn window_values(allocation: Allocation) -> i32 {
    match allocation {
        Allocation::ObjectLiteral { window, .. } | Allocation::Concat { window } => i32::from(window.count),
        Allocation::ObjectPair { .. } => 2,
        Allocation::RegExp { callee: Some(_), .. } => 1,
        Allocation::ArrayOf { elements } => elements.registers().len() as i32,
        Allocation::Empty | Allocation::Array { .. } | Allocation::RegExp { .. } | Allocation::ConstArray { .. } | Allocation::Closure { .. } | Allocation::Template { .. } => 0,
    }
}

/// A concrete register frame: the interpreter keeps no facts about its registers. A heap
/// value whose cell decides its truthiness reads the cell through `heap`, as the lane does.
pub(super) struct ConcreteFrame<'a> {
    pub(super) registers: Vec<u64>,
    pub(super) heap: Option<&'a Heap>,
}

impl Frame<Concrete> for ConcreteFrame<'_> {
    type Fact = ();
    type Control = Edge;

    fn fact(&self, _register: Register) -> Self::Fact {}

    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        self.registers[usize::from(register)]
    }

    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: Self::Fact) {
        self.registers[usize::from(register)] = value;
    }

    /// The test frame's activation holds no `this`.
    fn this(&mut self, _stage: &mut Concrete) -> u64 { Value::DELETED.to_bits() }

    fn define_this(&mut self, _stage: &mut Concrete, destination: Register, this: u64) {
        self.registers[usize::from(destination)] = this;
    }

    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, Self::Fact) {
        (constant.value.to_bits(), ())
    }

    /// A frame with a heap allocates the value `window_values` names: the number of registers
    /// the allocation reads, written to the destination.
    fn allocate_value(&mut self, _stage: &mut Concrete, destination: Register, allocation: Allocation, _proof: ()) -> Option<u64> {
        self.heap?;
        let value = Value::integer(window_values(allocation)).to_bits();
        self.registers[usize::from(destination)] = value;
        Some(value)
    }

    fn truthiness(&mut self, stage: &mut Concrete, register: Register) -> Option<bool> {
        let value = Value::from_bits(self.read(stage, register));
        value.truthiness().or_else(|| self.heap?.string(value).map(|text| !text.units().is_empty()))
    }

    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> Edge {
        if condition { Edge::Target } else { Edge::Following }
    }

    fn jump(&mut self, _stage: &mut Concrete) -> Edge {
        Edge::Target
    }

    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) {
        (Value::boolean(condition).to_bits(), ())
    }
}

impl GlobalFrame<Concrete> for ConcreteFrame<'_> {}

impl crate::staged::CallFrame<Concrete> for ConcreteFrame<'_> {}

impl PropertyFrame<Concrete> for ConcreteFrame<'_> {
    type Plans = NoPlans;

    fn plans(&self) -> NoPlans { NoPlans }
}

impl ElementFrame<Concrete> for ConcreteFrame<'_> {}

impl NumericFrame<Concrete> for ConcreteFrame<'_> {
    fn int_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<crate::staged::Int32<u64>> {
        let register = operand.register_index().expect("a numeric operand is a register");
        Some(crate::staged::Int32::checked(Value::from_bits(self.read(stage, register))).expect("a proved int"))
    }
    fn number_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<crate::staged::Float64<u64>> {
        let register = operand.register_index().expect("a numeric operand is a register");
        Some(crate::staged::Float64::checked(Value::from_bits(self.read(stage, register))).expect("a proved Number"))
    }
    fn prove_number(&mut self, stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        Value::from_bits(self.read(stage, register)).as_number().map(|_| ())
    }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}
