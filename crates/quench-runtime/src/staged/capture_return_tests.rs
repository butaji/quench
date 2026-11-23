//! The staged Number return on the concrete stage.

use super::*;
use crate::bytecode::Register;
use crate::Value;

/// A concrete frame over a few registers and the word
/// the last staged return left with.
struct ScopeFrame {
    registers: Vec<u64>,
    returned: Option<u64>,
}

impl Frame<Concrete> for ScopeFrame {
    type Fact = ();
    type Control = Edge;

    fn fact(&self, _register: Register) {}
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 { self.registers[usize::from(register)] }
    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: ()) {
        self.registers[usize::from(register)] = value;
    }
    fn this(&mut self, _stage: &mut Concrete) -> u64 { Value::DELETED.to_bits() }
    fn define_this(&mut self, _stage: &mut Concrete, _destination: Register, _this: u64) {}
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, ()) { (constant.value.to_bits(), ()) }
    fn truthiness(&mut self, _stage: &mut Concrete, _register: Register) -> Option<bool> { None }
    fn branch(&mut self, _stage: &mut Concrete, _condition: bool) -> Edge { Edge::Following }
    fn jump(&mut self, _stage: &mut Concrete) -> Edge { Edge::Target }
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) { (Value::boolean(condition).to_bits(), ()) }

    fn ret(&mut self, stage: &mut Concrete, register: Register) -> Option<Edge> {
        let value = self.read(stage, register);
        Value::from_bits(value).as_number()?;
        self.returned = Some(value);
        Some(Edge::Following)
    }
}

impl crate::staged::ElementFrame<Concrete> for ScopeFrame {}

impl crate::staged::PropertyFrame<Concrete> for ScopeFrame {
    type Plans = crate::staged::NoPlans;

    fn plans(&self) -> crate::staged::NoPlans { crate::staged::NoPlans }
}

impl GlobalFrame<Concrete> for ScopeFrame {}

impl crate::staged::CallFrame<Concrete> for ScopeFrame {}

impl NumericFrame<Concrete> for ScopeFrame {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: crate::bytecode::Operand, _miss: ()) -> Option<u64> { unreachable!() }
    fn number_operand(&mut self, _stage: &mut Concrete, _operand: crate::bytecode::Operand, _miss: ()) -> Option<u64> { unreachable!() }
    fn prove_number(&mut self, stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        Value::from_bits(self.read(stage, register)).as_number().map(|_| ())
    }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

pub fn frame(registers: Vec<u64>) -> ScopeFrame {
    ScopeFrame { registers, returned: None }
}

#[test]
fn a_staged_return_leaves_with_a_number_and_leaves_unresolved_for_any_other_value() {
    let mut stage = Concrete::new(std::ptr::null());
    let mut numeric = frame(vec![Value::number(2.5).to_bits()]);
    assert!(matches!(step(&mut stage, &mut numeric, Operation::Return { source: 0 }), Step::Control(_)));
    assert_eq!(numeric.returned, Some(Value::number(2.5).to_bits()));
    let mut other = frame(vec![Value::UNDEFINED.to_bits()]);
    assert!(matches!(step(&mut stage, &mut other, Operation::Return { source: 0 }), Step::Unresolved));
    assert_eq!(other.returned, None);
}
