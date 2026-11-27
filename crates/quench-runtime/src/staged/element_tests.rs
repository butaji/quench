//! The element rows on the concrete stage, against the heap's own array model.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt};
use super::element_rows::concrete_record_replace;
use super::elements::concrete_record_element;
use super::test_words::{ANSWER, READ, WRITE};
use super::*;
use crate::bytecode::Operand;
use crate::typed_ir::Type;
use crate::heap::{ArrayObject, Heap, Object};
use crate::Value;

/// A frame over a few registers and the heap's array records, whose host paths log their name.
struct Probe {
    registers: Vec<u64>,
    records: u64,
    fact: Type,
    log: Vec<&'static str>,
}

impl Frame<Concrete> for Probe {
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
    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> Edge {
        if condition { Edge::Target } else { Edge::Following }
    }
    fn jump(&mut self, _stage: &mut Concrete) -> Edge { Edge::Target }
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) { (Value::boolean(condition).to_bits(), ()) }
}

impl GlobalFrame<Concrete> for Probe {}

impl PropertyFrame<Concrete> for Probe {
    type Plans = NoPlans;
    fn plans(&self) -> NoPlans { NoPlans }
    fn value_fact(&self, _register: Register) -> Type { self.fact }
    fn index_get_host(&mut self, _stage: &mut Concrete, _base: u64, _key: u64, _miss: ()) -> Option<u64> {
        self.log.push("index_get_host");
        Some(ANSWER)
    }
}

impl ElementFrame<Concrete> for Probe {
    fn array_records(&mut self, _stage: &mut Concrete) -> Option<u64> { Some(self.records) }
    fn index_set_host(&mut self, _stage: &mut Concrete, _base: u64, _key: u64, _value: u64, _miss: ()) -> Option<()> {
        self.log.push("index_set_host");
        Some(())
    }
}

impl crate::staged::CallFrame<Concrete> for Probe {}

impl NumericFrame<Concrete> for Probe {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<u64> { unreachable!("no numbers") }
    fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<u64> { unreachable!("no numbers") }
    fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { None }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

/// An array of `elements` in `heap`, and a probe frame holding it in register 0 and `key` in 1.
fn array_of(heap: &mut Heap, elements: &[Value]) -> Value {
    heap.alloc_array(ArrayObject::new(Object::new(Value::NULL), elements.to_vec()))
}

fn probe(heap: &Heap, registers: [Value; 3], fact: Type) -> Probe {
    Probe { registers: registers.map(Value::to_bits).to_vec(), records: heap.array_record_base().map_or(0, |base| base as u64),
        fact, log: Vec::new() }
}

/// The row `operation` run on the concrete stage over `frame`.
fn run(frame: &mut Probe, operation: Operation<u64, ()>) -> Option<Value> {
    match step(&mut Concrete::pure(), frame, operation) {
        Step::Value(word) => Some(Value::from_bits(word)),
        Step::Unresolved => None,
        Step::Control(_) => unreachable!("an element access transfers no control"),
    }
}

#[test]
#[cfg(not(feature = "profile-aggregate"))]
fn a_read_takes_a_present_element_and_the_host_for_a_hole_or_an_index_past_the_backing() {
    let mut heap = Heap::new();
    let array = array_of(&mut heap, &[Value::integer(5), Value::DELETED, Value::number(1.5)]);
    for (key, expected, host) in [(0, Value::integer(5), false), (2, Value::number(1.5), false),
        (1, Value::from_bits(ANSWER), true), (3, Value::from_bits(ANSWER), true)] {
        let mut frame = probe(&heap, [array, Value::integer(key), Value::UNDEFINED], Type::ANY);
        assert_eq!(run(&mut frame, READ), Some(expected), "index {key}");
        assert_eq!(frame.log.contains(&"index_get_host"), host, "index {key}");
    }
}

#[test]
#[cfg(not(feature = "profile-aggregate"))]
fn a_read_leaves_for_a_base_that_is_no_array_and_a_key_that_is_no_int32() {
    let mut heap = Heap::new();
    let array = array_of(&mut heap, &[Value::integer(5)]);
    let mut frame = probe(&heap, [Value::integer(1), Value::integer(0), Value::UNDEFINED], Type::ANY);
    assert_eq!(run(&mut frame, READ), None, "a primitive base");
    let mut frame = probe(&heap, [array, Value::number(0.5), Value::UNDEFINED], Type::ANY);
    assert_eq!(run(&mut frame, READ), None, "a fractional key");
    assert!(frame.log.is_empty(), "the host read follows a hole or an index past the backing only");
}

#[test]
#[cfg(not(feature = "profile-aggregate"))]
fn a_store_replaces_a_present_element_in_place_and_leaves_the_rest_to_the_host() {
    let mut heap = Heap::new();
    let array = array_of(&mut heap, &[Value::integer(5), Value::DELETED]);
    let mut frame = probe(&heap, [array, Value::integer(0), Value::number(2.5)], Type::NUMBER);
    assert_eq!(run(&mut frame, WRITE), Some(Value::number(2.5)));
    assert!(frame.log.is_empty());
    assert_eq!(heap.array(array).unwrap().elements().get(0), Some(Value::number(2.5)));
    for (key, fact, value) in [(1, Type::NUMBER, Value::number(1.0)), (2, Type::NUMBER, Value::number(1.0)), (0, Type::ANY, array)] {
        let mut frame = probe(&heap, [array, Value::integer(key), value], fact);
        assert_eq!(run(&mut frame, WRITE), Some(value), "index {key}");
        assert_eq!(frame.log, ["index_set_host"], "a hole, an index past the backing or a value that needs the barrier");
    }
}

#[test]
#[cfg(not(feature = "profile-aggregate"))]
fn the_frameless_arms_are_the_rows_dense_arms() {
    let mut heap = Heap::new();
    let array = array_of(&mut heap, &[Value::integer(5), Value::DELETED]);
    let records = heap.array_record_base().expect("an array record");
    assert_eq!(concrete_record_element(records, array, 0), Some(Value::integer(5)));
    assert_eq!(concrete_record_element(records, array, 1), None, "a hole");
    assert_eq!(concrete_record_element(records, array, 2), None, "past the backing");
    assert_eq!(concrete_record_element(records, Value::integer(0), 0), None, "no array");
    assert!(concrete_record_replace(records, array, 0, Value::number(0.5)));
    assert!(!concrete_record_replace(records, array, 1, Value::integer(1)), "a hole is the store's to add");
    heap.array_mut(array).unwrap().set_frozen(true);
    assert!(!concrete_record_replace(records, array, 0, Value::integer(1)), "a frozen array");
    assert_eq!(concrete_record_element(records, array, 0), Some(Value::number(0.5)));
}

#[test]
fn the_element_instructions_decode_to_their_rows() {
    use crate::bytecode::{Op, WideInstruction};
    let read = WideInstruction::new(Op::GetIndex, 3, 4, 5, 0);
    assert!(matches!(Operation::<u64, ()>::decode(read, crate::staged::DecodeContext::EMPTY),
        Some(Operation::ElementRead { base: 4, key, .. }) if key.register_index() == Some(5)));
    let write = WideInstruction::new(Op::SetIndex, 3, 4, 5, 0);
    assert!(matches!(Operation::<u64, ()>::decode(write, crate::staged::DecodeContext::EMPTY),
        Some(Operation::ElementWrite { value: 3, base: 4, key: 5, strict: false })));
}
