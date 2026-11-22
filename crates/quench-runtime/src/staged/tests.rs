use super::*;
use crate::bytecode::Operand;
use crate::heap::{Cell, Heap, Object};
use crate::Value;

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
    fn int_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        let register = operand.register_index().expect("a numeric operand is a register");
        Some(u64::from(Value::from_bits(self.read(stage, register)).as_int().expect("a proved int") as u32))
    }
    fn number_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        let register = operand.register_index().expect("a numeric operand is a register");
        Some(Value::from_bits(self.read(stage, register)).as_number().expect("a proved Number").to_bits())
    }
    fn prove_number(&mut self, stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        Value::from_bits(self.read(stage, register)).as_number().map(|_| ())
    }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

#[test]
fn step_moves_and_defines_constants_on_the_concrete_stage() {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![0; 4], heap: None };
    let moved = Value::number(41.0);
    frame.registers[1] = moved.to_bits();
    let Step::Value(word) = step(&mut stage, &mut frame, Operation::Move { destination: 2, source: 1 })
        else { panic!("a move produces a value") };
    assert_eq!(Value::from_bits(word), moved);
    assert_eq!(Value::from_bits(frame.registers[2]), moved);
    let constant = Value::number(7.0);
    let Step::Value(word) = step(&mut stage, &mut frame, Operation::LoadConst {
        destination: 3, constant: Constant::untyped(constant) }) else { panic!("a constant produces a value") };
    assert_eq!(Value::from_bits(word), constant);
    assert_eq!(Value::from_bits(frame.registers[3]), constant);
}

/// Logical negation is ToBoolean then the canonical boolean: one operation for the
/// interpreter and every tier.
#[test]
fn staged_logical_not_is_the_javascript_truthiness_negation() {
    let mut stage = Concrete::new(std::ptr::null());
    for (value, expected) in [
        (Value::TRUE, false), (Value::FALSE, true), (Value::number(0.0), true),
        (Value::number(-0.0), true), (Value::number(f64::NAN), true),
        (Value::number(3.0), false), (Value::NULL, true), (Value::UNDEFINED, true),
    ] {
        let mut frame = ConcreteFrame { registers: vec![value.to_bits(), 0], heap: None };
        let Step::Value(word) = step(&mut stage, &mut frame,
            Operation::LogicalNot { destination: 1, source: 0 }) else {
            panic!("logical not produces a value")
        };
        assert_eq!(Value::from_bits(word), Value::boolean(expected), "{value:?}");
        assert_eq!(Value::from_bits(frame.registers[1]), Value::boolean(expected));
    }
}

/// The `typeof` name index the row defines for `value`, or `None` where it leaves for the
/// value's cell.
fn typeof_index_of(value: Value) -> Option<u64> {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![value.to_bits(), 0], heap: None };
    match step(&mut stage, &mut frame, Operation::TypeofIndex { destination: 1, source: 0 }) {
        Step::Value(word) => {
            assert_eq!(frame.registers[1], word, "{value:?}");
            Some(word)
        }
        Step::Unresolved => None,
        Step::Control(_) => panic!("typeof transfers no control"),
    }
}

/// `typeof` of every value its own word decides names what the interpreter names, and a heap
/// value (whose cell decides, the IsHTMLDDA object included) is left to the host.
#[test]
fn staged_typeof_index_names_every_immediate_and_leaves_heap_values() {
    let name = |text: &str| crate::bytecode::typeof_name_index(text).map(|index| index as u64);
    let mut heap = Heap::new();
    let object = heap.alloc_object(Object::new(Value::NULL));
    let text = heap.alloc_string("x".into());
    let cases = [
        (Value::number(0.0), "number"), (Value::number(-0.0), "number"), (Value::number(1.5), "number"),
        (Value::number(f64::NAN), "number"), (Value::number(f64::INFINITY), "number"),
        (Value::integer(0), "number"), (Value::integer(i32::MIN), "number"), (Value::integer(7), "number"),
        (Value::TRUE, "boolean"), (Value::FALSE, "boolean"), (Value::NULL, "object"),
        (Value::UNDEFINED, "undefined"), (Value::DELETED, "undefined"),
    ];
    for (value, expected) in cases {
        assert_eq!(typeof_index_of(value), name(expected), "{value:?}");
    }
    assert_eq!(typeof_index_of(object), None);
    assert_eq!(typeof_index_of(text), None);
}

/// `~x` of an int32 is `-1 - x`: no overflow, every int32 stays an int32.
#[test]
fn staged_bitwise_not_of_an_int32_is_its_complement() {
    let mut stage = Concrete::new(std::ptr::null());
    for value in [0, 1, -1, 7, i32::MAX, i32::MIN] {
        let mut frame = ConcreteFrame { registers: vec![Value::integer(value).to_bits(), 0], heap: None };
        let operation = Operation::Number(NumberOperation::BitwiseNot { destination: 1, source: Operand::register(0) });
        assert!(matches!(step(&mut stage, &mut frame, operation), Step::Value(_)));
        assert_eq!(frame.registers[1] as u32 as i32, !value, "~{value}");
    }
}

/// The catalog's staged column and `Operation::decode` agree: every opcode that always decodes
/// has a decoder, one decoded by a tier's proof (`Decoder::Proved`) decodes through
/// `decode_load_name`, and no opcode without a decoder decodes at all.
#[test]
fn every_staged_opcode_decodes_to_one_operation() {
    use crate::bytecode::{Decoder, Op, WideInstruction};
    let constants = [Constant::untyped(Value::number(1.0))];
    let context = crate::staged::DecodeContext::of_constants(&constants);
    let always = [Op::Move, Op::LoadConst, Op::LoadLocalPlain, Op::StoreLocalPlain,
        Op::Jump, Op::JumpFalse, Op::JumpTrue, Op::LoadThis, Op::RequireObjectCoercible,
        Op::ToNumeric, Op::ToPropertyKey, Op::LoadEnvLocal, Op::StoreEnvLocal, Op::ArgumentsLength, Op::LoadNameTypeof,
        Op::StoreName, Op::StoreVarBinding, Op::LoadGlobalLexical, Op::StoreGlobalLexical, Op::ForInNext, Op::ForInStart, Op::GetIterator,
        Op::IteratorClose, Op::AppendArray,
        Op::LoadCapture, Op::StoreCapture, Op::JumpBinaryFalse, Op::Return];
    for op in always {
        let instruction = WideInstruction::new(op, 1, 1, 1, 0);
        assert!(op.staged().is_some(), "{op:?}");
        assert!(Operation::<u64, ()>::decode(instruction, context).is_some(), "{op:?} must decode to a staged operation");
    }
    for index in 0..Op::COUNT {
        let op = Op::from_index(index).unwrap();
        let instruction = WideInstruction::new(op, 1, 1, 1, 0);
        let decoded = Operation::<u64, ()>::decode(instruction, context).is_some();
        assert!(op.staged().is_some() || !decoded, "{op:?} decodes though the catalog names no decoder");
        if op.staged() == Some(Decoder::Proved) {
            assert!(!decoded && Operation::<u64, ()>::decode_load_name(instruction, ()).is_some(), "{op:?}");
        }
    }
}

/// `await`, `yield` and `yield*` decode to their suspension rows, with the registers the general
/// step reads (`b` the value, `c` the delegate's iterator, the immediate's pair its state and `next`
/// method) and resumes into (`a`), and a frame that answers none of the call-outs leaves the row to
/// the interpreter.
#[test]
fn suspensions_decode_to_rows_a_plain_frame_leaves_unresolved() {
    use crate::bytecode::{Op, WideInstruction};
    let context = crate::staged::DecodeContext::of_constants(&[]);
    let pair = crate::bytecode::ImmediateLayout::register_pair_immediate(4, 5);
    let delegation = Suspension::YieldStar { iterator: 3, state: 4, next: 5 };
    for (op, expected) in [(Op::Await, Suspension::Await), (Op::Yield, Suspension::Yield { conversion: crate::staged::YieldConversion::Resolve }), (Op::YieldStar, delegation)] {
        let imm = if op == Op::YieldStar { pair } else { 0 };
        let row = Operation::<u64, ()>::decode(WideInstruction::new(op, 1, 2, 3, imm), context);
        let Some(Operation::Suspend { kind, destination: 1, value: 2 }) = row else { panic!("{op:?} decodes to a suspension row") };
        assert_eq!(kind, expected, "{op:?}");
        if let Suspension::YieldStar { iterator, state, next } = kind {
            assert_eq!((iterator, state, next), (3, 4, 5), "the delegate's iterator is `c`, its state and method the pair");
        }
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = ConcreteFrame { registers: vec![0; 6], heap: None };
        assert!(matches!(step(&mut stage, &mut frame, Operation::Suspend { kind, destination: 1, value: 2 }), Step::Unresolved), "{op:?}");
    }
}

/// The edge a `JumpIf` takes on the concrete stage for `value`.
fn jump_if_edge(value: Value, taken: bool, heap: &Heap) -> Edge {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![value.to_bits()], heap: Some(heap) };
    match step(&mut stage, &mut frame, Operation::JumpIf { register: 0, taken }) {
        Step::Control(edge) => edge,
        Step::Unresolved => panic!("{value:?} needs its cell, which the frame bound"),
        Step::Value(_) => panic!("a jump produces no value"),
    }
}

#[test]
fn step_jumps_on_the_javascript_truthiness_of_every_value_kind() {
    let mut heap = Heap::new();
    let empty = heap.alloc_string("".into());
    let text = heap.alloc_string("x".into());
    let cases = [
        (Value::number(1.0), Edge::Target),
        (Value::number(-3.5), Edge::Target),
        (Value::number(0.0), Edge::Following),
        (Value::number(-0.0), Edge::Following),
        (Value::number(f64::NAN), Edge::Following),
        (Value::TRUE, Edge::Target),
        (Value::FALSE, Edge::Following),
        (Value::NULL, Edge::Following),
        (Value::UNDEFINED, Edge::Following),
        (empty, Edge::Following),
        (text, Edge::Target),
    ];
    for (value, truthy_edge) in cases {
        assert_eq!(jump_if_edge(value, true, &heap), truthy_edge, "JumpTrue {value:?}");
        let falsy_edge = match truthy_edge {
            Edge::Target => Edge::Following,
            Edge::Following => Edge::Target,
            Edge::Entry(_) => unreachable!("a conditional jump takes no table entry"),
            Edge::Leave => unreachable!("a conditional jump leaves no run"),
            Edge::Transfer => unreachable!("a conditional jump transfers no control"),
        };
        assert_eq!(jump_if_edge(value, false, &heap), falsy_edge, "JumpFalse {value:?}");
    }
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![0], heap: None };
    assert!(matches!(step(&mut stage, &mut frame, Operation::Jump), Step::Control(Edge::Target)));
}

/// The staged ToBoolean decides every value the interpreter's `Value::truthiness` decides
/// from its word alone, the same way, and leaves exactly the heap values whose cell decides.
#[test]
fn staged_truthiness_agrees_with_the_interpreter() {
    let mut heap = Heap::new();
    let object = heap.alloc_object(Object::new(Value::NULL));
    let text = heap.alloc(Cell::Error("text".into()));
    let numbers = [0.0, -0.0, 1.0, -2.5, f64::NAN, f64::INFINITY, 1e300].map(Value::number);
    let values = [Value::TRUE, Value::FALSE, Value::NULL, Value::UNDEFINED, Value::DELETED,
        Value::integer(0), Value::integer(7), Value::integer(-1), object, text];
    for value in numbers.into_iter().chain(values) {
        let (undecided, truthy) = super::value::tagged_truthiness(&mut Concrete::new(std::ptr::null()), value.to_bits());
        let interpreter = value.truthiness().filter(|_| value.direct_object_index().is_some() || !value.is_heap()
            || value.direct_array_index().is_some());
        assert_eq!((!undecided).then_some(truthy), interpreter, "{value:?}");
    }
}

#[test]
fn staged_require_object_coercible_rejects_only_null_and_undefined() {
    let coercible = |value: Value| {
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = ConcreteFrame { registers: vec![value.to_bits()], heap: None };
        match step(&mut stage, &mut frame, Operation::RequireObjectCoercible { register: 0 }) {
            Step::Value(word) => { assert_eq!(word, value.to_bits()); true }
            Step::Unresolved => false,
            Step::Control(_) => panic!("a coercibility test transfers no control"),
        }
    };
    assert!(!coercible(Value::NULL));
    assert!(!coercible(Value::UNDEFINED));
    for value in [Value::TRUE, Value::FALSE, Value::number(0.0), Value::number(f64::NAN), Value::number(-0.0)] {
        assert!(coercible(value), "{value:?}");
    }
}

#[test]
fn staged_to_numeric_copies_numbers_and_leaves_for_the_rest() {
    let convert = |value: Value| {
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = ConcreteFrame { registers: vec![value.to_bits(), Value::UNDEFINED.to_bits()], heap: None };
        match step(&mut stage, &mut frame, Operation::Number(NumberOperation::ToNumeric { destination: 1, source: 0 })) {
            Step::Value(_) => Some(Value::from_bits(frame.registers[1])),
            Step::Unresolved => { assert_eq!(frame.registers[1], Value::UNDEFINED.to_bits()); None }
            Step::Control(_) => panic!("a conversion transfers no control"),
        }
    };
    for value in [Value::number(1.5), Value::number(-0.0), Value::number(f64::NAN), Value::number(7.0)] {
        assert_eq!(convert(value).map(Value::to_bits), Some(value.to_bits()), "{value:?}");
    }
    for value in [Value::NULL, Value::UNDEFINED, Value::TRUE] {
        assert_eq!(convert(value), None, "{value:?}");
    }
}

#[test]
fn staged_load_this_leaves_where_the_activation_holds_no_this() {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![Value::NULL.to_bits()], heap: None };
    assert!(matches!(step(&mut stage, &mut frame, Operation::LoadThis { destination: 0 }), Step::Unresolved));
    assert_eq!(frame.registers[0], Value::NULL.to_bits(), "a load that left writes nothing");
}


#[test]
fn an_allocation_is_a_staged_operation_with_a_host_boundary() {
    // The interpreter stage has no allocator of its own: an allocation is the host callout
    // boundary, so the frame declines it and the lane's Emit stage provides the semantics.
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![0; 3], heap: None };
    for operation in [
        Operation::Allocate { destination: 0, allocation: Allocation::Empty, proof: () },
    ] {
        assert!(matches!(step(&mut stage, &mut frame, operation), Step::Unresolved));
    }
}

/// The window, closure, template and concatenation rows are one staged operation over the
/// frame's allocator: a frame with none declines each, writing nothing, and one with a heap
/// defines the destination with the word it made.
#[test]
fn an_allocation_row_defines_its_destination_through_the_frame_or_declines() {
    use crate::bytecode::{RegisterList, RegisterWindow};
    let window = RegisterWindow { base: 1, count: 2 };
    let rows = [
        (Allocation::ObjectLiteral { site: 4, window }, 2),
        (Allocation::Concat { window }, 2),
        (Allocation::Closure { function: 1 }, 0),
        (Allocation::Template { template: 0 }, 0),
        (Allocation::Empty, 0),
        (Allocation::Array { length: 3, numbers: false }, 0),
        (Allocation::RegExp { site: 0, callee: None }, 0),
        (Allocation::RegExp { site: 0, callee: Some(1) }, 1),
        (Allocation::ConstArray { constant: 0, count: 2 }, 0),
        (Allocation::ObjectPair { site: 1, first: 1, second: 2 }, 2),
        (Allocation::ArrayOf { elements: RegisterList::collect(2, |index| 2 - index as u16).expect("two fit") }, 2),
    ];
    let heap = Heap::new();
    let mut stage = Concrete::new(std::ptr::null());
    for (allocation, made) in rows {
        let operation = Operation::Allocate { destination: 0, allocation, proof: () };
        let mut bare = ConcreteFrame { registers: vec![0; 3], heap: None };
        assert!(matches!(step(&mut stage, &mut bare, operation), Step::Unresolved), "{allocation:?}");
        assert_eq!(bare.registers[0], 0, "a declined row writes nothing");
        let mut held = ConcreteFrame { registers: vec![0; 3], heap: Some(&heap) };
        let Step::Value(word) = step(&mut stage, &mut held, operation) else { panic!("{allocation:?} allocates") };
        assert_eq!(Value::from_bits(word), Value::integer(made), "{allocation:?}");
        assert_eq!(held.registers[0], word, "the row defines its destination");
    }
}

/// Only a closure writes the frame (its promotion), only concatenation may run guest code,
/// and every allocation row may allocate: the rows a specializer keeps facts across.
#[test]
fn allocation_rows_declare_their_effects() {
    use crate::bytecode::RegisterWindow;
    let window = RegisterWindow { base: 0, count: 1 };
    let row = |allocation| Operation::<u64, ()>::Allocate { destination: 0, allocation, proof: () }.effects();
    let (literal, closure) = (row(Allocation::ObjectLiteral { site: 0, window }), row(Allocation::Closure { function: 0 }));
    assert!(literal.may_alloc && !literal.may_call_js && !literal.writes.contains(Region::Frame));
    let elements = row(Allocation::ArrayOf { elements: crate::bytecode::RegisterList::collect(1, |_| 0).expect("one fits") });
    assert!(elements.may_alloc && !elements.may_call_js && !elements.writes.contains(Region::Frame), "its elements are stored fresh, with no barrier");
    assert!(closure.may_alloc && closure.writes.contains(Region::Frame));
    assert!(row(Allocation::Concat { window }).may_call_js, "ToString of an object runs guest code");
}

#[test]
fn the_element_store_helper_is_the_host_callout_boundary() {
    // The interpreter stage has no helper table: an element-store helper is the host callout
    // boundary, so the frame declines it and the lane's Emit stage provides the call.
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![0; 3], heap: None };
    let step = step(&mut stage, &mut frame,
        Operation::IndexSetHelper { base: 0, key: 1, value: 2, proof: () });
    assert!(matches!(step, Step::Unresolved));
}

#[test]
fn a_global_name_read_is_the_host_callout_boundary() {
    // The interpreter reads globals through its own loop; the lane's helper call is the
    // callout boundary, so the frame declines it.
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![0], heap: None };
    let step = step(&mut stage, &mut frame,
        Operation::LoadName { destination: 0, atom: 0, site: 0, receiver: None, proof: () });
    assert!(matches!(step, Step::Unresolved));
}

#[test]
fn the_field_store_helper_is_the_host_callout_boundary() {
    // The host's cached field-store hit is a callout: the interpreter stage declines it.
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![0; 2], heap: None };
    let step = step(&mut stage, &mut frame,
        Operation::FieldStoreMiss { object: 0, atom: 0, site: 0, value: 1, proof: () });
    assert!(matches!(step, Step::Unresolved));
}

#[test]
fn every_host_callout_operation_has_a_concrete_reading() {
    // P5's criterion: each host/VM callout is a staged operation the interpreter stage either
    // declines (leaving it to the host) or runs (the safepoint), never an unknown opcode.
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![0; 4], heap: None };
    let declines = [
        Operation::Allocate { destination: 0, allocation: Allocation::Empty, proof: () },
        Operation::Allocate { destination: 0, allocation: Allocation::Closure { function: 0 }, proof: () },
        Operation::IndexSetHelper { base: 0, key: 1, value: 2, proof: () },
        Operation::LoadName { destination: 0, atom: 0, site: 0, receiver: None, proof: () },
        Operation::FieldStoreMiss { object: 0, atom: 0, site: 0, value: 1, proof: () },
    ];
    for operation in declines {
        assert!(matches!(step(&mut stage, &mut frame, operation), Step::Unresolved));
    }
    assert!(matches!(step(&mut stage, &mut frame, Operation::Safepoint { proof: () }), Step::Value(_)));
}

/// The fast arms of `RequireIteratorResult` and `ToPropertyKey` answer from the register's word
/// alone: an object or array record passes, a direct string is its own key, and every other
/// value leaves for the full operation, writing nothing.
#[test]
fn the_key_and_iterator_fast_arms_decide_from_the_word_alone() {
    let mut heap = Heap::new();
    let mut stage = Concrete::new(std::ptr::null());
    let object = heap.alloc_object(Object::new(Value::NULL));
    let string = heap.alloc_string(crate::vm::wtf16::JsString::from_str("key"));
    for (value, passes) in [(object, true), (string, false), (Value::UNDEFINED, false), (Value::integer(1), false), (Value::number(0.5), false)] {
        let mut frame = ConcreteFrame { registers: vec![value.to_bits()], heap: None };
        let step = step(&mut stage, &mut frame, Operation::RequireIteratorResult { register: 0 });
        assert_eq!(matches!(step, Step::Value(_)), passes, "{value:?}");
    }
    for (value, copies) in [(string, true), (object, false), (Value::integer(7), false), (Value::UNDEFINED, false)] {
        let mut frame = ConcreteFrame { registers: vec![value.to_bits(), Value::NULL.to_bits()], heap: None };
        let step = step(&mut stage, &mut frame, Operation::PropertyKey { destination: 1, source: 0 });
        assert_eq!(matches!(step, Step::Value(_)), copies, "{value:?}");
        let expected = if copies { value } else { Value::NULL };
        assert_eq!(Value::from_bits(frame.registers[1]), expected, "{value:?}");
    }
}
