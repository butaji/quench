//! The property rows on the concrete stage, against the heap's own object model.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt};
use super::ic::IcOp;
use super::property::{read_with, Arm, FieldPlan, Owner, PlanSource, StoreArm, StoreJoin, StorePlan};
use super::test_words::ANSWER;
use super::*;
use crate::bytecode::{FieldBase, Operand};
use crate::heap::{Heap, Object};
use crate::typed_ir::Type;
use crate::{Value, ValueType};

/// The plans of a test site: `lookup`'s read, cache site `store`'s store.
struct Plans {
    read: FieldPlan<'static>,
    store: StorePlan<'static>,
}

impl PlanSource for Plans {
    fn field_plan(&self, _lookup: FieldLookup) -> Option<FieldPlan<'_>> { Some(self.read) }
    fn store_plan(&self, _site: u16) -> Option<StorePlan<'_>> { Some(self.store) }
}

/// A frame over a few registers and the heap's object records, whose callouts log their name.
struct Host<'p> {
    registers: Vec<u64>,
    records: u64,
    plans: &'p Plans,
    answers: bool,
    log: Vec<&'static str>,
    /// The shape this frame claims it proved of every owner, and the shapes its guards recorded.
    proven: Option<u32>,
    proved: Vec<u32>,
}

impl<'p> Host<'p> {
    fn new(records: u64, plans: &'p Plans, registers: Vec<u64>) -> Self {
        Self { registers, records, plans, answers: true, log: Vec::new(), proven: None, proved: Vec::new() }
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

impl crate::staged::CallFrame<Concrete> for Host<'_> {}

impl<'p> PropertyFrame<Concrete> for Host<'p> {
    type Plans = &'p Plans;
    fn plans(&self) -> &'p Plans { self.plans }
    fn object_records(&mut self, _stage: &mut Concrete) -> Option<u64> { Some(self.records) }
    fn proven_shape(&mut self, _stage: &mut Concrete, _owner: Owner) -> Option<(u32, crate::staged::ShapeFact)> {
        self.proven.map(|shape| (shape, crate::staged::ShapeFact::entry_guard()))
    }
    fn prove_shape(&mut self, _stage: &mut Concrete, _owner: Owner, (shape, _): (u32, crate::staged::Shaped<u64>), _direct: bool) { self.proved.push(shape); }
    fn field_get_host(&mut self, _stage: &mut Concrete, _object: u64, _atom: u32, _site: u16, _miss: ()) -> Option<u64> {
        self.call("field_get_host").then_some(ANSWER)
    }
    fn field_add(&mut self, _stage: &mut Concrete, _object: u64, _atom: u32, _site: u16, _value: u64, _miss: ()) -> Option<()> {
        self.call("field_add").then_some(())
    }
    fn field_store_host(&mut self, _stage: &mut Concrete, _object: u64, _atom: u32, _site: u16, _value: u64, _miss: ()) -> Option<()> {
        self.call("field_store_host").then_some(())
    }
    fn field_store_bound(&mut self, _stage: &mut Concrete, _owner: Owner, _object: u64, _site: (u32, u16), _value: (u64, Type),
        _miss: ()) -> Option<()> {
        self.call("field_store_bound").then_some(())
    }
    fn entry_slot_store(&mut self, _stage: &mut Concrete, _site: u16, _value: u64, _miss: ()) -> Option<()> {
        self.call("entry_slot_store").then_some(())
    }
    fn delete_property(&mut self, _stage: &mut Concrete, _base: u64, _key: u64, strict: bool, _miss: ()) -> Option<(u64, ())> {
        self.call(if strict { "delete_strict" } else { "delete" }).then_some((ANSWER, ()))
    }
    fn index_get_host(&mut self, _stage: &mut Concrete, _base: u64, _key: u64, _miss: ()) -> Option<u64> {
        self.call("index_get_host").then_some(ANSWER)
    }
}

impl ElementFrame<Concrete> for Host<'_> {}

impl NumericFrame<Concrete> for Host<'_> {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<u64> { unreachable!("no numbers") }
    fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<u64> { unreachable!("no numbers") }
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
        IcOp::LoadSlot { slot, form: Form::Either, storage_len: len, rep: crate::typed_ir::Representation::Tagged }]))
}

const NO_STORE: StorePlan<'static> = StorePlan::Host;

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
    let object = heap.alloc_object(Object::new(Value::NULL));
    for (index, value) in values.iter().enumerate() {
        heap.property_add(object, *value, index as u32 + 1);
    }
    let shape = heap.object_ref(object).unwrap().exact_shape();
    (object, shape)
}

pub fn records(heap: &Heap) -> u64 { heap.object_record_base().map_or(0, |base| base as u64) }

/// `plan` read from `object` on the concrete stage, by a frame that holds no proof.
fn read(heap: &Heap, plan: FieldPlan<'static>, object: Value) -> Option<Value> {
    let plans = Plans { read: plan, store: NO_STORE };
    let mut frame = Host::new(records(heap), &plans, vec![0]);
    let mut stage = Concrete::new(heap.property_values_base());
    read_with(&mut stage, &mut frame, (Owner::Temporary, false), object.to_bits(), plan, ()).map(Value::from_bits)
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
        Arm { shapes: Box::leak(Box::new([first_shape])), ops: ops(vec![IcOp::LoadSlot { slot: 0, form: Form::Either, storage_len: 1, rep: crate::typed_ir::Representation::Tagged }]), holds: None },
        Arm { shapes: Box::leak(Box::new([second_shape])), ops: ops(vec![IcOp::LoadSlot { slot: 1, form: Form::Either, storage_len: 2, rep: crate::typed_ir::Representation::Tagged }]), holds: None },
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
    let cases: [(Operation<u64, ()>, &str); 6] = [
        (Operation::GetField { base: FieldBase::register(1), lookup, destination: 0 }, "field_get_host"),
        (Operation::FieldGetHost { object: 0, atom: 1, site: 2 }, "field_get_host"),
        (Operation::FieldAdd { object: 0, atom: 1, site: 2, value: 5 }, "field_add"),
        (Operation::Delete { destination: 0, base: 1, key: 2, strict: true }, "delete_strict"),
        (Operation::Delete { destination: 0, base: 1, key: 2, strict: false }, "delete"),
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
        let stored = step(&mut stage, &mut frame, Operation::SetField { value: 2, base: Some(1), atom: 7, site: 3, strict: false });
        assert!(matches!(stored, Step::Value(word) if word == value));
        assert_eq!(frame.log, [name]);
    }
}

#[test]
fn the_property_instructions_decode_to_their_operations() {
    use crate::bytecode::{Op, WideInstruction};
    let decode = |instruction| Operation::<u64, ()>::decode(instruction, crate::staged::DecodeContext::EMPTY);
    let get = decode(WideInstruction::new(Op::GetField, 1, 2, 3, 4)).expect("a register-based read decodes");
    assert!(matches!(get, Operation::GetField { base, lookup: FieldLookup::Atom { atom: 4, cache_site: 3, .. }, .. } if base == FieldBase::register(2)));
    let set = decode(WideInstruction::new(Op::SetField, 1, 2, 3, 9)).expect("a store decodes");
    assert!(matches!(set, Operation::SetField { value: 1, base: Some(2), atom: 9, site: 3, .. }));
    let this = decode(WideInstruction::new(Op::SetThisField, 1, 0, 3, 9)).expect("a store to this decodes");
    assert!(matches!(this, Operation::SetField { base: None, .. }));
    let delete = decode(WideInstruction::new(Op::Delete, 1, 2, 3, 1)).expect("delete decodes");
    assert!(matches!(delete, Operation::Delete { destination: 1, base: 2, key: 3, strict: true }));
    let has = decode(WideInstruction::new(Op::Binary, 1, Operand(2).0, Operand(3).0,
        crate::vm::operations::binary_operator::In)).expect("`in` of registers decodes");
    assert!(matches!(has, Operation::Binary { left, right, .. } if left == Operand::register(2) && right == Operand::register(3)), "`in` is the VM's Binary call-out row");
}

/// `plan` read from `object` as register 1's owner, by a frame claiming `proven`; the word read
/// and the shapes its guards recorded.
fn read_owned_by(heap: &Heap, plan: FieldPlan<'static>, object: Value, proven: Option<u32>) -> (Option<Value>, Vec<u32>) {
    let plans = Plans { read: plan, store: NO_STORE };
    let mut frame = Host::new(records(heap), &plans, vec![0]);
    frame.proven = proven;
    let mut stage = Concrete::new(heap.property_values_base());
    let lookup = FieldLookup::Atom { atom: 0, base: FieldBase::register(1), cache_site: 0 };
    frame.registers = vec![0, object.to_bits()];
    let word = match step(&mut stage, &mut frame, Operation::GetField { base: FieldBase::register(1), lookup, destination: 0 }) {
        Step::Value(word) => Some(Value::from_bits(word)),
        _ => None,
    };
    (word, frame.proved)
}

#[test]
fn a_guard_is_recorded_as_the_owners_proof_and_a_proven_shape_is_not_tested_again() {
    let mut heap = heap_of(2);
    let (object, shape) = object_of(&mut heap, &[Value::integer(7), Value::integer(8)]);
    let (read, proved) = read_owned_by(&heap, own(shape, 1, 2), object, None);
    assert_eq!((read, proved), (Some(Value::integer(8)), vec![shape]), "a guard that holds is recorded");
    let (read, proved) = read_owned_by(&heap, own(shape + 1, 1, 2), object, None);
    assert_eq!((read, proved), (None, vec![]), "a guard that fails proves nothing");
    // A frame that holds the proof makes no guard: the read does not leave for a shape it was
    // told is proved, and records nothing again.
    let (read, proved) = read_owned_by(&heap, own(shape + 1, 1, 2), object, Some(shape + 1));
    assert_eq!((read, proved), (Some(Value::integer(8)), vec![]));
}

/// `operation` run over `registers` under `plan`, by a frame that holds no proof: the word the
/// row stored (none where it left) and the callouts it made.
fn store_step(heap: &mut Heap, plan: StorePlan<'static>, registers: Vec<u64>, operation: Operation<u64, ()>)
    -> (Option<Value>, Vec<&'static str>) {
    let plans = Plans { read: FieldPlan::Host { atom: 0, site: 0 }, store: plan };
    let mut frame = Host::new(records(heap), &plans, registers);
    let mut stage = Concrete::storing(&*heap);
    let word = match step(&mut stage, &mut frame, operation) {
        Step::Value(word) => Some(Value::from_bits(word)),
        _ => None,
    };
    (word, frame.log)
}

/// `plan` run as the store of `value` to `object`'s field.
fn store(heap: &mut Heap, plan: StorePlan<'static>, object: Value, value: Value) -> (Option<Value>, Vec<&'static str>) {
    let operation = Operation::SetField { value: 2, base: Some(1), atom: 7, site: 3, strict: false };
    store_step(heap, plan, vec![0, object.to_bits(), value.to_bits()], operation)
}

#[test]
fn an_arm_is_the_slots_type_or_the_test_of_the_type_that_keeps_its_representation() {
    let arm = |slot_ty, fact| StoreArm::of((5, 2), slot_ty, fact);
    let plain = |holds| Some(StoreArm { shape: 5, slot: 2, holds, raw_double: false, unformed: false, layout: false });
    assert_eq!(arm(Type::NUMBER, Type::INT32), plain(None), "the slot holds the value's type");
    assert_eq!(arm(Type::INT32, Type::NUMBER), plain(Some(Type::INT32)), "a Number may widen an int32 slot: test the int32");
    let objects = Type::DIRECT_OBJECT_FAMILY.join(Type::NULLISH);
    assert_eq!(arm(objects, Type::ANY), plain(Some(objects)), "an object slot admits objects and its nullish");
    assert_eq!(arm(Type::language(ValueType::String), Type::INT32), None, "no value stores in place without widening");
    assert_eq!(StorePlan::of(&[], StoreJoin::Add), StorePlan::Add);
    assert_eq!(StorePlan::of(&[], StoreJoin::Host), StorePlan::Host);
}

#[test]
fn a_widening_arm_stores_the_values_its_type_admits_in_place_and_leaves_the_rest_to_the_vm() {
    let mut heap = heap_of(1);
    let (object, shape) = object_of(&mut heap, &[Value::integer(7)]);
    let arms: &'static [StoreArm] = Box::leak(Box::new([StoreArm { shape, slot: 0, holds: Some(Type::INT32), raw_double: false, unformed: false, layout: false }]));
    let plan = StorePlan::of(arms, StoreJoin::Host);
    let (word, log) = store(&mut heap, plan, object, Value::integer(9));
    assert_eq!((word, log), (Some(Value::integer(9)), Vec::new()));
    assert_eq!(read(&heap, own(shape, 0, 1), object), Some(Value::integer(9)));
    let (word, log) = store(&mut heap, plan, object, Value::number(1.5));
    assert_eq!((word, log), (Some(Value::number(1.5)), vec!["field_store_host"]), "the VM joins the slot's type");
    assert_eq!(read(&heap, own(shape, 0, 1), object), Some(Value::integer(9)), "the arm changed nothing");
}

#[test]
fn several_arms_store_through_the_one_that_holds_the_receivers_shape() {
    let mut heap = heap_of(2);
    let (first, first_shape) = object_of(&mut heap, &[Value::integer(1)]);
    let (second, second_shape) = object_of(&mut heap, &[Value::integer(2), Value::integer(3)]);
    let (third, _) = object_of(&mut heap, &[]);
    let arms: &'static [StoreArm] = Box::leak(Box::new([
        StoreArm { shape: first_shape, slot: 0, holds: None, raw_double: false, unformed: false, layout: false }, StoreArm { shape: second_shape, slot: 1, holds: None, raw_double: false, unformed: false, layout: false }]));
    let plan = StorePlan::of(arms, StoreJoin::Host);
    assert_eq!(store(&mut heap, plan, first, Value::integer(10)).1, Vec::<&str>::new());
    assert_eq!(read(&heap, own(first_shape, 0, 1), first), Some(Value::integer(10)));
    assert_eq!(store(&mut heap, plan, second, Value::integer(11)).1, Vec::<&str>::new());
    assert_eq!(read(&heap, own(second_shape, 1, 2), second), Some(Value::integer(11)));
    assert_eq!(read(&heap, own(second_shape, 0, 2), second), Some(Value::integer(2)), "the other slot stays");
    assert_eq!(store(&mut heap, plan, third, Value::integer(12)).1, ["field_store_host"], "a shape no arm names");
    let adding = StorePlan::of(arms, StoreJoin::Add);
    assert_eq!(store(&mut heap, adding, third, Value::integer(12)).1, ["field_add"], "the join the plan names");
}

#[test]
fn bound_and_entry_plans_reach_their_frame_callouts_and_a_bound_join_completes_what_no_arm_takes() {
    let mut heap = heap_of(1);
    let (object, shape) = object_of(&mut heap, &[Value::integer(7)]);
    let (other, _) = object_of(&mut heap, &[]);
    assert_eq!(store(&mut heap, StorePlan::Bound, object, Value::integer(1)).1, ["field_store_bound"]);
    assert_eq!(store(&mut heap, StorePlan::Entry, object, Value::integer(1)).1, ["entry_slot_store"]);
    let arms: &'static [StoreArm] = Box::leak(Box::new([StoreArm { shape, slot: 0, holds: None, raw_double: false, unformed: false, layout: false }]));
    let plan = StorePlan::of(arms, StoreJoin::Bound);
    assert_eq!(store(&mut heap, plan, object, Value::integer(2)).1, Vec::<&str>::new(), "the arm takes its shape");
    assert_eq!(store(&mut heap, plan, other, Value::integer(2)).1, ["field_store_bound"], "the join the plan names");
}

#[test]
fn a_word_no_register_names_stores_through_the_sites_plan_with_the_fact_it_is_given() {
    let mut heap = heap_of(1);
    let (object, shape) = object_of(&mut heap, &[Value::integer(7)]);
    let arms: &'static [StoreArm] = Box::leak(Box::new([StoreArm { shape, slot: 0, holds: None, raw_double: false, unformed: false, layout: false }]));
    let plan = StorePlan::of(arms, StoreJoin::Host);
    let word = Value::integer(5);
    let operation = Operation::StoreField { base: Some(1), value: word.to_bits(), fact: Type::INT32, atom: 7, site: 3 };
    let (stored, log) = store_step(&mut heap, plan, vec![0, object.to_bits()], operation);
    assert_eq!((stored, log), (Some(word), Vec::new()));
    assert_eq!(read(&heap, own(shape, 0, 1), object), Some(word));
    let (_, log) = store_step(&mut heap, StorePlan::Host, vec![0, object.to_bits()], operation);
    assert_eq!(log, ["field_store_host"]);
}

#[test]
fn a_layout_plan_stores_in_place_under_its_layout_id_and_leaves_any_other_receiver_to_its_join() {
    let mut heap = heap_of(2);
    let (object, shape) = object_of(&mut heap, &[Value::integer(7)]);
    let (other, _) = object_of(&mut heap, &[Value::integer(1), Value::integer(2)]);
    let id = heap.record_object_value(object).expect("record").hint_shape();
    let plan = StorePlan::Layout { id, slot: 0, join: StoreJoin::Host };
    assert_eq!(store(&mut heap, plan, object, Value::integer(9)), (Some(Value::integer(9)), Vec::new()));
    assert_eq!(read(&heap, own(shape, 0, 1), object), Some(Value::integer(9)));
    assert_eq!(store(&mut heap, plan, other, Value::integer(3)).1, ["field_store_host"], "another layout");
    assert_eq!(read(&heap, own(shape, 0, 1), object), Some(Value::integer(9)));
}

/// Under reference counts every store into a record the last collection left marked logs, an
/// immediate overwrite too (it deletes an edge): the gate and the typed store's immediate skip
/// (`slot_store::gate_open`) both refuse it, while without counts an immediate store passes both
/// unlogged.
#[test]
fn an_immediate_store_into_a_marked_record_logs_under_counts_and_passes_without() {
    for counts in [false, true] {
        let mut heap = heap_of(2);
        let held = heap.alloc_object(Object::new(Value::NULL));
        let (object, _) = object_of(&mut heap, &[held, Value::integer(1)]);
        if counts {
            heap.set_rc(true);
        }
        heap.collect([object]);
        let record_id = object.to_bits() as u32;
        let number = Value::integer(5).to_bits();
        let watch = heap.store_watch_word() as u64;
        assert_eq!(watch & crate::staged::layout_offsets::STORE_WATCH_COUNTS != 0, counts, "the gate word names counts");
        let mut stage = Concrete::storing(&heap);
        assert_eq!(stage.immediate_store_open(), !counts, "counts: {counts}");
        assert_eq!(stage.store_gate(StagedRecordSpace::Object, u64::from(record_id), number), !counts, "counts: {counts}");
        let open = super::slot_store::gate_open(&mut stage, record_id as u64, (number, Type::INT32), ());
        assert_eq!(open.is_some(), !counts, "a proved-immediate store skips the gate only without counts");
    }
}
