//! The staged capture read and Number return on the concrete stage.

use super::*;
use crate::bytecode::Register;
use crate::Value;

/// A concrete frame over a few registers, the environments `depth` scopes out, and the word
/// the last staged return left with.
struct ScopeFrame {
    registers: Vec<u64>,
    scopes: Vec<Vec<Value>>,
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

    fn capture(&mut self, stage: &mut Concrete, depth: u16, slot: u16, miss: ()) -> Option<u64> {
        let run = self.scopes.get(usize::from(depth))?;
        environment::dense_capture(stage, run.as_ptr() as u64, run.len() as u64, slot, miss)
    }

    fn ret(&mut self, stage: &mut Concrete, register: Register) -> Option<Edge> {
        self.returned = Some(self.read(stage, register));
        Some(Edge::Following)
    }
}

impl GlobalFrame<Concrete> for ScopeFrame {}

impl NumericFrame<Concrete> for ScopeFrame {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: crate::bytecode::Operand, _miss: ()) -> u64 { unreachable!() }
    fn number_operand(&mut self, _stage: &mut Concrete, _operand: crate::bytecode::Operand, _miss: ()) -> u64 { unreachable!() }
    fn prove_number(&mut self, stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        Value::from_bits(self.read(stage, register)).as_number().map(|_| ())
    }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

fn frame(registers: Vec<u64>, scopes: Vec<Vec<Value>>) -> ScopeFrame {
    ScopeFrame { registers, scopes, returned: None }
}

#[test]
fn a_staged_capture_reads_every_depth_and_leaves_at_markers_and_past_the_run() {
    let scopes = vec![vec![Value::number(1.5), Value::DELETED], vec![Value::UNDEFINED]];
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = frame(vec![], scopes);
    let mut read = |depth, slot| match step(&mut stage, &mut frame, Operation::LoadCapture { depth, slot }) {
        Step::Value(word) => Some(Value::from_bits(word)),
        Step::Unresolved => None,
        Step::Control(_) => unreachable!("a capture read transfers no control"),
    };
    assert_eq!(read(0, 0), Some(Value::number(1.5)));
    assert_eq!(read(0, 1), None, "an internal marker leaves");
    assert_eq!(read(0, 2), None, "past the run leaves");
    assert_eq!(read(1, 0), Some(Value::UNDEFINED));
    assert_eq!(read(2, 0), None, "no scope that far out leaves");
}

#[test]
fn a_staged_return_leaves_with_a_number_and_leaves_unresolved_for_any_other_value() {
    let mut stage = Concrete::new(std::ptr::null());
    let mut numeric = frame(vec![Value::number(2.5).to_bits()], vec![]);
    assert!(matches!(step(&mut stage, &mut numeric, Operation::Return { register: 0 }), Step::Control(_)));
    assert_eq!(numeric.returned, Some(Value::number(2.5).to_bits()));
    let mut other = frame(vec![Value::UNDEFINED.to_bits()], vec![]);
    assert!(matches!(step(&mut stage, &mut other, Operation::Return { register: 0 }), Step::Unresolved));
    assert_eq!(other.returned, None);
}
