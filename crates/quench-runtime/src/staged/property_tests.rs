//! The property rows on the concrete stage, against the heap's own object model.

use super::ic::IcOp;
use super::property::{read_plan, Arm, FieldPlan, PlanSource, StorePlan};
use super::*;
use crate::bytecode::{FieldBase, Operand};
use crate::code_dependencies::PublishedCode;
use crate::heap::{Cell, Heap, Object, NATIVE_OBJECT_LAYOUT as OBJECT};
use crate::Value;
use std::rc::Rc;

/// The word a host callout answers, and what each callout was asked.
const ANSWER: u64 = 0xA5A5;

/// The plans of a test site: `lookup`'s read, cache site `store`'s store.
struct Plans {
    read: FieldPlan<'static>,
    store: StorePlan,
}

impl PlanSource for Plans {
    fn field_plan(&self, _lookup: FieldLookup) -> Option<FieldPlan<'_>> { Some(self.read) }
    fn store_plan(&self, _site: u16) -> Option<StorePlan> { Some(self.store) }
}

/// A frame over a few registers and the heap's object records, whose callouts log their name.
struct Host<'p> {
    registers: Vec<u64>,
    records: u64,
    plans: &'p Plans,
    answers: bool,
    log: Vec<&'static str>,
}

impl<'p> Host<'p> {
    fn new(records: u64, plans: &'p Plans, registers: Vec<u64>) -> Self {
        Self { registers, records, plans, answers: true, log: Vec::new() }
    }

    fn call(&mut self, name: &'static str) -> bool {
        self.log.push(name);
        self.answers
    }
}

impl Frame<Concrete> for Host<'_> {
    type Fact = ();
    type Control = Edge;
    fn fact(&self, _register: Register) {}
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 { self.registers[usize::from(register)] }
    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: ()) {
        self.registers[usize::from(register)] = value;
    }
    fn this(&mut self, _stage: &mut Concrete) -> u64 { self.registers[0] }
    fn define_this(&mut self, _stage: &mut Concrete, destination: Register, this: u64) {
        self.registers[usize::from(destination)] = this;
    }
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, ()) { (constant.value.to_bits(), ()) }
    fn truthiness(&mut self, _stage: &mut Concrete, _register: Register) -> Option<bool> { None }
    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> Edge {
        if condition { Edge::Target } else { Edge::Following }
    }
    fn jump(&mut self, _stage: &mut Concrete) -> Edge { Edge::Target }
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) { (Value::boolean(condition).to_bits(), ()) }
}

impl GlobalFrame<Concrete> for Host<'_> {}

impl<'p> PropertyFrame<Concrete> for Host<'p> {
    type Plans = &'p Plans;
    fn plans(&self) -> &'p Plans { self.plans }
    fn object_records(&mut self, _stage: &mut Concrete) -> Option<u64> { Some(self.records) }
    fn field_get_host(&mut self, _stage: &mut Concrete, _object: u64, _atom: u32, _site: u16, _miss: ()) -> Option<u64> {
        self.call("field_get_host").then_some(ANSWER)
    }
    fn field_add(&mut self, _stage: &mut Concrete, _object: u64, _atom: u32, _site: u16, _value: u64, _miss: ()) -> Option<()> {
        self.call("field_add").then_some(())
    }
    fn field_store_host(&mut self, _stage: &mut Concrete, _object: u64, _atom: u32, _site: u16, _value: u64, _miss: ()) -> Option<()> {
        self.call("field_store_host").then_some(())
    }
    fn delete_property(&mut self, _stage: &mut Concrete, _base: u64, _key: u64, strict: bool, _miss: ()) -> Option<u64> {
        self.call(if strict { "delete_strict" } else { "delete" }).then_some(ANSWER)
    }
    fn has_property(&mut self, _stage: &mut Concrete, _key: u64, _base: u64, _miss: ()) -> Option<u64> {
        self.call("has_property").then_some(ANSWER)
    }
    fn index_get_host(&mut self, _stage: &mut Concrete, _base: u64, _key: u64, _miss: ()) -> Option<u64> {
        self.call("index_get_host").then_some(ANSWER)
    }
}

impl NumericFrame<Concrete> for Host<'_> {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> u64 { unreachable!("no numbers") }
    fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> u64 { unreachable!("no numbers") }
    fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { None }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

/// The leaked op list of a plan, which a test holds for its whole run.
fn ops(list: Vec<IcOp<'static>>) -> &'static [IcOp<'static>] { Box::leak(list.into_boxed_slice()) }

/// An own read of `slot` of an object of `shape` that keeps `len` values.
fn own(shape: u32, slot: u16, len: usize) -> FieldPlan<'static> {
    FieldPlan::Ops(ops(vec![IcOp::GuardShape(shape),
        IcOp::LoadSlot { slot, form: Form::Either, storage_len: len }]))
}

const NO_STORE: StorePlan = StorePlan::Host;

/// A heap whose shapes `1..=count` extend one another.
fn heap_of(count: u32) -> Heap {
    let mut heap = Heap::new();
    for shape in 1..=count {
        heap.register_property_shape(shape, shape as usize, None);
    }
    heap
}

/// An object holding `values` in slots `0..`, and its exact shape.
fn object_of(heap: &mut Heap, values: &[Value]) -> (Value, u32) {
    let object = heap.alloc(Cell::Object(Object::new(Value::NULL)));
    for (index, value) in values.iter().enumerate() {
        heap.property_add(object, *value, index as u32 + 1);
    }
    let shape = heap.object(object).unwrap().exact_shape();
    (object, shape)
}

fn records(heap: &Heap) -> u64 { heap.object_record_base().map_or(0, |base| base as u64) }

/// `plan` read from `object` on the concrete stage.
fn read(heap: &Heap, plan: FieldPlan<'static>, object: Value) -> Option<Value> {
    let plans = Plans { read: plan, store: NO_STORE };
    let mut frame = Host::new(records(heap), &plans, vec![0]);
    let mut stage = Concrete::new(heap.property_values_base());
    read_plan(&mut stage, &mut frame, object.to_bits(), plan, ()).map(Value::from_bits)
}

#[test]
fn an_own_plan_reads_its_slot_and_leaves_for_any_other_shape_or_a_primitive() {
    let mut heap = heap_of(2);
    let (object, shape) = object_of(&mut heap, &[Value::integer(7), Value::integer(8)]);
    assert_eq!(read(&heap, own(shape, 1, 2), object), Some(Value::integer(8)));
    assert_eq!(read(&heap, own(shape + 1, 1, 2), object), None, "another shape");
    assert_eq!(read(&heap, own(shape, 1, 2), Value::integer(3)), None, "a primitive has no record");
}

#[test]
fn a_polymorphic_plan_reads_through_the_arm_that_holds_the_shape() {
    let mut heap = heap_of(2);
    let (first, first_shape) = object_of(&mut heap, &[Value::integer(10)]);
    let (second, second_shape) = object_of(&mut heap, &[Value::integer(20), Value::integer(31)]);
    let (third, _) = object_of(&mut heap, &[]);
    let arms: &'static [Arm<'static>] = Box::leak(Box::new([
        Arm { shapes: Box::leak(Box::new([first_shape])), ops: ops(vec![IcOp::LoadSlot { slot: 0, form: Form::Either, storage_len: 1 }]) },
        Arm { shapes: Box::leak(Box::new([second_shape])), ops: ops(vec![IcOp::LoadSlot { slot: 1, form: Form::Either, storage_len: 2 }]) },
    ]));
    let plan = FieldPlan::Arms(arms);
    assert_eq!(read(&heap, plan, first), Some(Value::integer(10)));
    assert_eq!(read(&heap, plan, second), Some(Value::integer(31)));
    assert_eq!(read(&heap, plan, third), None, "no arm holds the shape");
}

#[test]
fn a_nested_chain_reads_each_link_from_the_previous_value() {
    let mut heap = heap_of(1);
    let (inner, shape) = object_of(&mut heap, &[Value::integer(5)]);
    let (middle, _) = object_of(&mut heap, &[inner]);
    let (outer, _) = object_of(&mut heap, &[middle]);
    let links: &'static [FieldPlan<'static>] = Box::leak(Box::new([own(shape, 0, 1), own(shape, 0, 1), own(shape, 0, 1)]));
    assert_eq!(read(&heap, FieldPlan::Chain(links), outer), Some(Value::integer(5)));
    let (broken, _) = object_of(&mut heap, &[Value::integer(1)]);
    let (outer, _) = object_of(&mut heap, &[broken]);
    assert_eq!(read(&heap, FieldPlan::Chain(links), outer), None, "a link that is no object leaves");
}

#[test]
fn a_holder_plan_needs_the_shape_the_link_and_the_watch() {
    let mut heap = heap_of(1);
    let prototype = heap.alloc(Cell::Object(Object::new(Value::NULL)));
    let object = heap.alloc(Cell::Object(Object::new(prototype)));
    heap.property_add(object, Value::integer(1), 1);
    let (shape, link) = (heap.object(object).unwrap().exact_shape(), crate::heap::ProtoLink::of(prototype).bits());
    let watch = Rc::new(PublishedCode::new(()));
    let method = Value::integer(42);
    let plan = |shape, link| FieldPlan::Holder { shape, link, watch: Rc::as_ptr(&watch) as u64, value: method.to_bits() };
    assert_eq!(read(&heap, plan(shape, link), object), Some(method));
    assert_eq!(read(&heap, plan(shape + 1, link), object), None);
    assert_eq!(read(&heap, plan(shape, link ^ 1), object), None);
    watch.retire();
    assert_eq!(read(&heap, plan(shape, link), object), None, "a retired watch");
}

#[test]
fn a_string_receiver_reads_its_method_under_the_watch() {
    let heap = Heap::new();
    let watch = Rc::new(PublishedCode::new(()));
    let method = Value::integer(9);
    let plan = FieldPlan::StringHolder { watch: Rc::as_ptr(&watch) as u64, value: method.to_bits() };
    assert_eq!(read(&heap, plan, Value::direct_string(3)), Some(method));
    assert_eq!(read(&heap, plan, Value::integer(1)), None);
}

/// The step of `operation` on a host frame, with its log.
fn run(answers: bool, operation: Operation<u64, ()>) -> (Option<u64>, Vec<&'static str>) {
    let plans = Plans { read: FieldPlan::Host { atom: 1, site: 2 }, store: StorePlan::Add };
    let mut frame = Host::new(0, &plans, vec![Value::integer(1).to_bits(); 4]);
    frame.answers = answers;
    let mut stage = Concrete::new(std::ptr::null());
    let word = match step(&mut stage, &mut frame, operation) {
        Step::Value(word) => Some(word),
        Step::Unresolved => None,
        Step::Control(_) => panic!("a property operation transfers no control"),
    };
    (word, frame.log)
}

#[test]
fn host_plans_and_callouts_reach_the_frame_and_a_declining_frame_leaves() {
    let lookup = FieldLookup::Atom { atom: 1, base: FieldBase::register(1), cache_site: 2 };
    let cases: [(Operation<u64, ()>, &str); 7] = [
        (Operation::GetField { base: FieldBase::register(1), lookup }, "field_get_host"),
        (Operation::FieldGetHost { object: 0, atom: 1, site: 2 }, "field_get_host"),
        (Operation::FieldAdd { object: 0, atom: 1, site: 2, value: 5 }, "field_add"),
        (Operation::Delete { base: 1, key: 2, strict: true }, "delete_strict"),
        (Operation::Delete { base: 1, key: 2, strict: false }, "delete"),
        (Operation::HasProperty { key: 2, base: 1 }, "has_property"),
        (Operation::IndexGetHost { base: 0, key: 1 }, "index_get_host"),
    ];
    for (operation, name) in cases {
        let (word, log) = run(true, operation);
        assert!(word.is_some(), "{name} answers");
        assert_eq!(log, [name]);
        assert_eq!(run(false, operation).0, None, "{name} declined leaves to the host path");
    }
}

#[test]
fn a_store_plan_chooses_the_adding_or_the_vm_store_and_stores_the_value_it_was_given() {
    let (add, host) = (StorePlan::Add, StorePlan::Host);
    for (plan, name) in [(add, "field_add"), (host, "field_store_host")] {
        let plans = Plans { read: FieldPlan::Host { atom: 0, site: 0 }, store: plan };
        let value = Value::integer(4).to_bits();
        let mut frame = Host::new(0, &plans, vec![0, Value::integer(1).to_bits(), value]);
        let mut stage = Concrete::new(std::ptr::null());
        let stored = step(&mut stage, &mut frame, Operation::SetField { value: 2, base: Some(1), atom: 7, site: 3 });
        assert!(matches!(stored, Step::Value(word) if word == value));
        assert_eq!(frame.log, [name]);
    }
}

#[test]
fn an_own_store_writes_the_slot_where_the_shape_matches_and_leaves_otherwise() {
    let mut heap = heap_of(2);
    let (object, shape) = object_of(&mut heap, &[Value::integer(1), Value::integer(2)]);
    let id = object.direct_object_index().unwrap();
    // SAFETY: a live record of this heap, which nothing else borrows during the test.
    let record = unsafe { heap.unlogged_object_record(id) } as u64;
    let base = record - u64::from(id) * OBJECT.record_size as u64;
    for (value, planned, stored) in [(Value::integer(9), shape, true), (Value::integer(11), shape + 1, false)] {
        let plans = Plans { read: FieldPlan::Host { atom: 0, site: 0 }, store: StorePlan::Own { shape: planned, slot: 1 } };
        let mut frame = Host::new(base, &plans, vec![0, object.to_bits(), value.to_bits()]);
        let mut stage = Concrete::storing(&heap);
        let word = step(&mut stage, &mut frame, Operation::SetField { value: 2, base: Some(1), atom: 7, site: 3 });
        assert_eq!(matches!(word, Step::Value(_)), stored);
        // SAFETY: the object is live with a shape covering slot 1.
        let slot = unsafe { crate::staged::concrete_own_field_hit(heap.object(object).unwrap(), heap.arena_values_base(), shape, 1) };
        assert_eq!(slot == Some(value), stored);
        assert!(frame.log.is_empty(), "an own store calls out to nothing");
    }
}

#[test]
fn the_property_instructions_decode_to_their_operations() {
    use crate::bytecode::{Op, WideInstruction};
    let decode = |instruction| Operation::<u64, ()>::decode(instruction, |_| None);
    let get = decode(WideInstruction::new(Op::GetField, 1, 2, 3, 4)).expect("a register-based read decodes");
    assert!(matches!(get, Operation::GetField { base, lookup: FieldLookup::Atom { atom: 4, cache_site: 3, .. } } if base == FieldBase::register(2)));
    let set = decode(WideInstruction::new(Op::SetField, 1, 2, 3, 9)).expect("a store decodes");
    assert!(matches!(set, Operation::SetField { value: 1, base: Some(2), atom: 9, site: 3 }));
    let this = decode(WideInstruction::new(Op::SetThisField, 1, 0, 3, 9)).expect("a store to this decodes");
    assert!(matches!(this, Operation::SetField { base: None, .. }));
    let delete = decode(WideInstruction::new(Op::Delete, 1, 2, 3, 1)).expect("delete decodes");
    assert!(matches!(delete, Operation::Delete { base: 2, key: 3, strict: true }));
    let has = decode(WideInstruction::new(Op::Binary, 1, Operand(2).0, Operand(3).0,
        oxc_ast::ast::BinaryOperator::In as u32)).expect("`in` of registers decodes");
    assert!(matches!(has, Operation::HasProperty { key: 2, base: 3 }));
    let add = decode(WideInstruction::new(Op::Binary, 1, Operand(2).0, Operand(3).0,
        oxc_ast::ast::BinaryOperator::Addition as u32));
    assert!(add.is_none(), "another binary operator is not a property operation");
}
