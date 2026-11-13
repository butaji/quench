//! The name, environment and iteration rows on the interpreter's stage (`Concrete`).
use crate::bytecode::{Op, Operand, Register, WideInstruction};
use crate::heap::EnvironmentSlots;
use crate::staged::{step, CallOut, Concrete, Constant, Edge, Frame, GlobalFrame, NumericFrame, Operation, Protocol, Step};
use crate::Value;
use crate::staged::Operation as Row;

/// A register file whose callouts record what they are asked, or decline when `declines`.
#[derive(Default)]
struct Probe {
    registers: Vec<u64>,
    declines: bool,
    storage: Option<u64>,
    environment_writes: Vec<(u16, u64)>,
    keys: Vec<u64>,
    calls: Vec<(CallOut, Option<Register>)>,
    global_writes: Vec<(u32, u16, u64, bool)>,
}

impl Probe {
    fn new(registers: usize) -> Self { Self { registers: vec![0; registers], ..Self::default() } }
    fn answers(&self) -> bool { !self.declines }
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

    fn environment(&mut self, _stage: &mut Concrete) -> Option<(u64, ())> { self.storage.map(|storage| (storage, ())) }
    fn environment_write(&mut self, _stage: &mut Concrete, slot: u16, value: u64) -> Option<()> {
        self.answers().then(|| self.environment_writes.push((slot, value)))
    }
    fn arguments_length(&mut self, _stage: &mut Concrete, slot: u16) -> Option<(u64, ())> {
        (self.answers() && slot == 1).then(|| (Value::integer(3).to_bits(), ()))
    }
    fn vm_step(&mut self, _stage: &mut Concrete, operation: CallOut, result: Option<Register>) -> Option<(u64, ())> {
        if !self.answers() { return None; }
        self.calls.push((operation, result));
        Some(match operation {
            CallOut::ForInNext { .. } => (self.keys.pop().unwrap_or(Value::DELETED.to_bits()), ()),
            _ if result.is_some() => (Value::number(9.0).to_bits(), ()),
            _ => (0, ()),
        })
    }
}

impl GlobalFrame<Concrete> for Probe {
    fn global_write(&mut self, _stage: &mut Concrete, _source: Register, atom: u32, site: u16, value: u64, initialize: bool) -> Option<()> {
        self.answers().then(|| self.global_writes.push((atom, site, value, initialize)))
    }
    fn global_typeof(&mut self, _stage: &mut Concrete, destination: Register, _atom: u32, _site: u16) -> Option<()> {
        self.answers().then(|| self.registers[usize::from(destination)] = Value::number(1.0).to_bits())
    }
}

impl NumericFrame<Concrete> for Probe {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> u64 { unreachable!("no numeric row") }
    fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> u64 { unreachable!("no numeric row") }
    fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { None }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

fn run(frame: &mut Probe, operation: Row<u64, ()>) -> Step<u64, Edge> {
    step(&mut Concrete::new(std::ptr::null()), frame, operation)
}

#[test]
fn an_environment_local_reads_the_dense_slot_and_leaves_at_a_marker_or_without_storage() {
    let slots = EnvironmentSlots::from_values([Value::number(7.0), Value::DELETED].into_iter());
    let mut frame = Probe::new(2);
    frame.storage = Some(&slots as *const EnvironmentSlots as u64);
    assert!(matches!(run(&mut frame, Row::LoadEnvLocal { destination: 1, slot: 0 }), Step::Value(_)));
    assert_eq!(Value::from_bits(frame.registers[1]), Value::number(7.0));
    assert!(matches!(run(&mut frame, Row::LoadEnvLocal { destination: 0, slot: 1 }), Step::Unresolved), "a marker leaves");
    assert!(matches!(run(&mut frame, Row::LoadEnvLocal { destination: 0, slot: 2 }), Step::Unresolved), "past the run leaves");
    frame.storage = None;
    assert!(matches!(run(&mut frame, Row::LoadEnvLocal { destination: 0, slot: 0 }), Step::Value(_)), "the VM reads it");
    assert_eq!(frame.calls, [(CallOut::LoadEnvLocal { slot: 0 }, Some(0))]);
    frame.declines = true;
    assert!(matches!(run(&mut frame, Row::LoadEnvLocal { destination: 0, slot: 0 }), Step::Unresolved));
}

#[test]
fn an_environment_store_writes_the_slot_and_the_result_or_declines() {
    let mut frame = Probe::new(3);
    frame.registers[0] = Value::number(4.0).to_bits();
    let stored = Row::StoreEnvLocal { source: 0, slot: 5, result: Some(2), initialize: false };
    assert!(matches!(run(&mut frame, stored), Step::Value(_)));
    assert_eq!(frame.environment_writes, [(5, Value::number(4.0).to_bits())]);
    assert_eq!(frame.registers[2], frame.registers[0]);
    frame.declines = true;
    frame.registers[2] = 0;
    assert!(matches!(run(&mut frame, stored), Step::Unresolved));
    assert_eq!(frame.registers[2], 0, "a declined store defines nothing");
}

#[test]
fn an_arguments_length_defines_the_count_or_declines() {
    let mut frame = Probe::new(1);
    assert!(matches!(run(&mut frame, Row::ArgumentsLength { destination: 0, slot: 1 }), Step::Value(_)));
    assert_eq!(Value::from_bits(frame.registers[0]).as_int(), Some(3));
    assert!(matches!(run(&mut frame, Row::ArgumentsLength { destination: 0, slot: 2 }), Step::Value(_)), "the VM counts it");
    frame.declines = true;
    assert!(matches!(run(&mut frame, Row::ArgumentsLength { destination: 0, slot: 2 }), Step::Unresolved));
}

#[test]
fn a_global_store_passes_its_value_and_a_typeof_defines_its_name() {
    let mut frame = Probe::new(2);
    frame.registers[1] = Value::number(2.0).to_bits();
    assert!(matches!(run(&mut frame, Row::GlobalWrite { source: 1, atom: 8, site: 3, initialize: true }), Step::Value(_)));
    assert_eq!(frame.global_writes, [(8, 3, Value::number(2.0).to_bits(), true)]);
    assert!(matches!(run(&mut frame, Row::GlobalTypeof { destination: 0, atom: 8, site: 3 }), Step::Value(_)));
    assert_eq!(Value::from_bits(frame.registers[0]), Value::number(1.0));
    frame.declines = true;
    assert!(matches!(run(&mut frame, Row::GlobalWrite { source: 1, atom: 8, site: 3, initialize: false }), Step::Unresolved));
    assert!(matches!(run(&mut frame, Row::GlobalTypeof { destination: 0, atom: 8, site: 3 }), Step::Unresolved));
}

#[test]
fn a_protocol_step_names_its_operands_defines_its_result_and_declines_with_the_host() {
    let mut frame = Probe::new(3);
    let iterator = Row::Protocol { step: Protocol::GetIterator, destination: Some(2), operands: [None, Some(1)] };
    assert!(matches!(run(&mut frame, iterator), Step::Value(_)));
    assert_eq!(frame.calls, [(CallOut::Protocol { step: Protocol::GetIterator, operands: [None, Some(1)] }, Some(2))]);
    assert_eq!(Value::from_bits(frame.registers[2]), Value::number(9.0));
    frame.registers[2] = 0;
    let append = Row::Protocol { step: Protocol::AppendArray { length: 1, spread: true }, destination: None, operands: [Some(1), None] };
    assert!(matches!(run(&mut frame, append), Step::Value(_)));
    assert_eq!(frame.registers[2], 0, "an append defines no register");
    frame.declines = true;
    assert!(matches!(run(&mut frame, iterator), Step::Unresolved));
}

#[test]
fn a_for_in_next_defines_the_next_key_then_takes_the_jump_when_none_is_left() {
    let mut frame = Probe::new(2);
    frame.keys = vec![Value::number(2.0).to_bits(), Value::number(1.0).to_bits()];
    let next = Row::ForInNext { destination: 0, state: 1 };
    assert!(matches!(run(&mut frame, next), Step::Control(Edge::Following)));
    assert_eq!(Value::from_bits(frame.registers[0]), Value::number(1.0));
    assert!(matches!(run(&mut frame, next), Step::Control(Edge::Following)));
    assert_eq!(Value::from_bits(frame.registers[0]), Value::number(2.0));
    assert!(matches!(run(&mut frame, next), Step::Control(Edge::Target)));
    assert_eq!(Value::from_bits(frame.registers[0]), Value::DELETED, "exhaustion defines the marker");
    frame.declines = true;
    assert!(matches!(run(&mut frame, next), Step::Unresolved));
}

#[test]
fn a_row_that_calls_out_names_the_call_its_frame_receives() {
    let mut frame = Probe::new(3);
    let rows = [
        Row::Protocol { step: Protocol::IteratorClose, destination: Some(2), operands: [None, Some(1)] },
        Row::ForInNext { destination: 2, state: 1 },
    ];
    for row in rows {
        run(&mut frame, row);
        let (call, result) = row.call_out().expect("a calling row");
        assert_eq!(frame.calls.last(), Some(&(call, result)));
    }
    assert_eq!(CallOut::ForInNext { state: 1 }.operands(), [None, Some(1)]);
    assert!(Row::<u64, ()>::Jump.call_out().is_none());
}

#[test]
fn an_equality_or_key_the_words_do_not_decide_calls_the_vm_and_a_decided_one_does_not() {
    let mut frame = Probe::new(3);
    frame.registers[..2].copy_from_slice(&[Value::number(1.5).to_bits(), Value::number(2.5).to_bits()]);
    let equality = Row::Equality { destination: 2, left: 0, right: 1, strict: true, negated: true };
    assert!(matches!(run(&mut frame, equality), Step::Value(_)));
    assert_eq!(frame.calls, [(CallOut::Equality { left: 0, right: 1, strict: true, negated: true }, Some(2))]);
    frame.calls.clear();
    frame.registers[1] = frame.registers[0];
    assert!(matches!(run(&mut frame, equality), Step::Value(_)));
    assert!(frame.calls.is_empty(), "the same word decides it");
    assert_eq!(Value::from_bits(frame.registers[2]), Value::FALSE);
    let key = Row::PropertyKey { destination: 2, source: 0 };
    assert!(matches!(run(&mut frame, key), Step::Value(_)));
    assert_eq!(frame.calls, [(CallOut::PropertyKey { source: 0 }, Some(2))]);
    frame.declines = true;
    assert!(matches!(run(&mut frame, key), Step::Unresolved));
    assert!(matches!(run(&mut frame, Row::Equality { destination: 2, left: 0, right: 1, strict: true, negated: false }),
        Step::Value(_)), "a decided pair needs no host");
    frame.registers[1] = Value::number(2.5).to_bits();
    assert!(matches!(run(&mut frame, equality), Step::Unresolved));
}

#[test]
fn the_catalog_decodes_each_name_environment_and_iteration_opcode() {
    let decode = |op, imm| Operation::<u64, ()>::decode(WideInstruction::new(op, 1, 1, 1, imm), |_| None);
    assert!(matches!(decode(Op::LoadEnvLocal, 2), Some(Row::LoadEnvLocal { destination: 1, slot: 2 })));
    assert!(matches!(decode(Op::StoreEnvLocal, 2), Some(Row::StoreEnvLocal { source: 1, slot: 2, .. })));
    assert!(matches!(decode(Op::ArgumentsLength, 2), Some(Row::ArgumentsLength { destination: 1, slot: 2 })));
    assert!(matches!(decode(Op::LoadNameTypeof, 4), Some(Row::GlobalTypeof { destination: 1, atom: 4, site: 1 })));
    assert!(matches!(decode(Op::StoreName, 4), Some(Row::GlobalWrite { source: 1, atom: 4, site: 1, initialize: true })));
    assert!(matches!(decode(Op::ForInNext, 0), Some(Row::ForInNext { destination: 1, state: 1 })));
    assert!(matches!(decode(Op::GetIterator, 0),
        Some(Row::Protocol { step: Protocol::GetIterator, destination: Some(1), operands: [None, Some(1)] })));
    assert!(matches!(decode(Op::AppendArray, 2),
        Some(Row::Protocol { step: Protocol::AppendArray { length: 2, spread: true }, destination: None, .. })));
}
