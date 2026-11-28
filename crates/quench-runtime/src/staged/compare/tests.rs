//! The comparison jumps on the interpreter's stage (`Concrete`).
use super::*;
use crate::bytecode::{Op, Register};
use crate::staged::tests::ConcreteFrame;
use crate::staged::{Concrete, Constant, Edge, Frame, GlobalFrame, NoPlans, PropertyFrame, Stage};
use crate::typed_ir::Type;
use crate::ValueType;
use crate::operator::BinaryOperator;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// A register file that records the decisions it is asked for, answering them with `answer`, and
/// the facts a jump proves along its edges; `types` are the registers' proved types.
struct Probe {
    registers: Vec<u64>,
    calls: Vec<CallOut>,
    answer: Option<bool>,
    types: Vec<Option<Type>>,
    refined: Vec<(Register, Type, Edge)>,
    html_dda: bool,
    /// The object record table the frame walks prototypes in, and the constructor `prototype` link
    /// it holds as a Stable fact.
    records: Option<u64>,
    prototype: Option<u64>,
}

impl Probe {
    fn of(values: &[Value]) -> Self {
        Self { registers: values.iter().map(|value| value.to_bits()).collect(), calls: Vec::new(), answer: None,
            types: Vec::new(), refined: Vec::new(), html_dda: true, records: None, prototype: None }
    }
}

impl Frame<Concrete> for Probe {
    type Fact = ();
    type Control = Edge;
    fn fact(&self, _register: Register) {}
    fn register_type(&self, register: Register) -> Option<Type> { self.types.get(usize::from(register)).copied().flatten() }
    fn refine(&mut self, register: Register, ty: Type, edge: Edge) { self.refined.push((register, ty, edge)); }
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
    fn vm_step(&mut self, _stage: &mut Concrete, operation: CallOut, _result: Option<Register>) -> Option<(u64, ())> {
        self.calls.push(operation);
        self.answer.map(|holds| (Value::boolean(holds).to_bits(), ()))
    }
}

impl GlobalFrame<Concrete> for Probe {}

impl crate::staged::CallFrame<Concrete> for Probe {}
impl crate::staged::ElementFrame<Concrete> for Probe {}
impl PropertyFrame<Concrete> for Probe {
    type Plans = NoPlans;
    fn plans(&self) -> NoPlans { NoPlans }
    fn object_records(&mut self, _stage: &mut Concrete) -> Option<u64> { self.records }
}

impl NumericFrame<Concrete> for Probe {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<crate::staged::Int32<u64>> { unreachable!("no int operand") }
    fn int32_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<crate::staged::Int32<u64>> {
        let number = Value::from_bits(self.registers[usize::from(operand.register_index()?)]).as_number()?;
        Some(crate::staged::Int32::by_fact(u64::from(crate::value::number_to_u32(number)), crate::staged::IntFact::int32_home()))
    }
    fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<crate::staged::Float64<u64>> { unreachable!("no Number operand") }
    fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { None }
    fn miss(&self) {}
    fn html_dda_possible(&self) -> bool { self.html_dda }
    fn stable_prototype(&mut self, _stage: &mut Concrete, _constructor: Register) -> Option<u64> { self.prototype }
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

/// The step of the comparison jump of registers 0 and 1 over `frame`.
fn run(frame: &mut Probe, comparison: Comparison, negate: bool) -> Step<u64, Edge> {
    let operation = Operation::CompareJump { comparison, left: Operand::register(0), right: Operand::register(1), negate };
    crate::staged::step(&mut Concrete::pure(), frame, operation)
}

/// The edge a comparison jump of registers 0 and 1 takes on the interpreter's stage.
fn edge(comparison: Comparison, [left, right]: [Value; 2], negate: bool) -> Edge {
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![left.to_bits(), right.to_bits()], heap: None };
    let operation = Operation::CompareJump { comparison, left: Operand::register(0), right: Operand::register(1), negate };
    match crate::staged::step(&mut stage, &mut frame, operation) {
        Step::Control(edge) => edge,
        _ => panic!("a relation of Numbers jumps one way or the other"),
    }
}

/// The relation by the language's own operators over binary64, independent of the staged one.
fn probe(relation: NumberRelation, left: f64, right: f64) -> bool {
    match relation {
        NumberRelation::Equal => left == right,
        NumberRelation::NotEqual => left != right,
        NumberRelation::LessThan => left < right,
        NumberRelation::LessEqual => left <= right,
        NumberRelation::GreaterThan => left > right,
        NumberRelation::GreaterEqual => left >= right,
    }
}

const RELATIONS: [NumberRelation; 6] = [NumberRelation::Equal, NumberRelation::NotEqual, NumberRelation::LessThan,
    NumberRelation::LessEqual, NumberRelation::GreaterThan, NumberRelation::GreaterEqual];

#[test]
fn a_relation_jump_takes_the_edge_the_language_decides_in_both_domains() {
    let numbers = [0.0, -0.0, 1.0, -3.0, 2.5, f64::NAN, f64::INFINITY];
    for relation in RELATIONS {
        for (left, right) in numbers.iter().flat_map(|left| numbers.iter().map(move |right| (*left, *right))) {
            let holds = probe(relation, left, right);
            let values = [Value::number(left), Value::number(right)];
            let float = Comparison::Relation { relation, domain: Domain::Float64 };
            assert_eq!(edge(float, values, false), if holds { Edge::Target } else { Edge::Following }, "{relation:?} {left} {right}");
            assert_eq!(edge(float, values, true), if holds { Edge::Following } else { Edge::Target }, "negated {relation:?} {left} {right}");
        }
    }
}

#[test]
fn an_int32_relation_jump_compares_the_integers() {
    let integers = [-7, 0, 3, i32::MAX, i32::MIN];
    for relation in RELATIONS {
        for (left, right) in integers.iter().flat_map(|left| integers.iter().map(move |right| (*left, *right))) {
            let holds = probe(relation, f64::from(left), f64::from(right));
            let values = [Value::integer(left), Value::integer(right)];
            let int = Comparison::Relation { relation, domain: Domain::Int32 };
            assert_eq!(edge(int, values, true), if holds { Edge::Following } else { Edge::Target }, "{relation:?} {left} {right}");
        }
    }
}

/// `JumpBinaryFalse` decodes to a jump on the failing comparison: an equality to its strictness
/// and negation, a relational operator to its relation, any other operator to itself.
#[test]
fn jump_binary_false_decodes_to_a_jump_where_the_comparison_fails() {
    let decode = |operator: BinaryOperator| {
        let (left, right) = (Operand::register(1).0, Operand::register(2).0);
        let instruction = WideInstruction::new(Op::JumpBinaryFalse, operator as u16, left, right, 9);
        Operation::<u64, ()>::decode(instruction, crate::staged::DecodeContext::EMPTY)
    };
    let Some(Operation::CompareJump { comparison, left, right, negate }) = decode(BinaryOperator::LessThan)
        else { panic!("a comparison jump decodes") };
    assert_eq!(comparison, Comparison::Relation { relation: NumberRelation::LessThan, domain: Domain::Float64 });
    assert_eq!((left.register_index(), right.register_index(), negate), (Some(1), Some(2), true));
    for (operator, strict, negated) in [(BinaryOperator::Equality, false, false), (BinaryOperator::Inequality, false, true),
        (BinaryOperator::StrictEquality, true, false), (BinaryOperator::StrictInequality, true, true)] {
        let Some(Operation::CompareJump { comparison, .. }) = decode(operator) else { panic!("a comparison jump decodes") };
        assert_eq!(comparison, Comparison::Equality { strict, negated });
        assert_eq!(comparison.operator(), Some(operator as u32), "the VM decides it by its own operator");
    }
    let Some(Operation::CompareJump { comparison, .. }) = decode(BinaryOperator::Instanceof)
        else { panic!("a comparison jump decodes") };
    assert_eq!((comparison, comparison.operator()), (Comparison::Instanceof, Some(BinaryOperator::Instanceof as u32)));
    let Some(Operation::CompareJump { comparison, .. }) = decode(BinaryOperator::In)
        else { panic!("a comparison jump decodes") };
    assert_eq!(comparison, Comparison::Operator(BinaryOperator::In as u32));
}

#[test]
fn an_operator_no_relation_names_is_left_to_the_tier() {
    let mut frame = Probe::of(&[Value::UNDEFINED, Value::UNDEFINED]);
    assert!(matches!(run(&mut frame, Comparison::Operator(BinaryOperator::In as u32), true), Step::Unresolved));
    assert!(frame.calls.is_empty());
}

/// Whether the language's equality holds of `left` and `right`, for a pair their words decide;
/// `None` for any other pair.
fn equality_the_words_decide(left: Value, right: Value, strict: bool) -> Option<bool> {
    let nullish_pair = !strict && [left, right].iter().all(|value| value.is_null() || value.is_undefined());
    let same = left == right && left.as_number().is_none_or(|number| !number.is_nan());
    let decided = left == right || (left.as_int().is_some() && right.as_int().is_some()) || nullish_pair;
    decided.then_some(same || nullish_pair)
}

/// Every operator of the equality family over pairs the words decide: the jump takes the edge
/// the language's own equality names, and the VM is never asked.
#[test]
fn an_equality_the_words_decide_jumps_without_the_vm() {
    let values = [Value::integer(1), Value::integer(2), Value::number(f64::NAN), Value::NULL, Value::UNDEFINED, Value::TRUE];
    for (strict, negated) in [(false, false), (false, true), (true, false), (true, true)] {
        for (left, right) in values.iter().flat_map(|left| values.iter().map(move |right| (*left, *right))) {
            let mut frame = Probe::of(&[left, right]);
            let Some(equal) = equality_the_words_decide(left, right, strict) else { continue };
            let step = run(&mut frame, Comparison::Equality { strict, negated }, true);
            let holds = equal != negated;
            assert!(matches!(step, Step::Control(edge) if edge == if holds { Edge::Following } else { Edge::Target }),
                "{strict} {negated} {left:?} {right:?}");
            assert!(frame.calls.is_empty(), "{left:?} {right:?} is decided by its words");
        }
    }
}

/// A pair the words do not decide is the VM's decision, by the comparison's own operator, and a
/// frame with no VM leaves the row to the interpreter.
#[test]
fn an_equality_the_words_leave_is_the_vms_decision() {
    let pair = [Value::number(1.5), Value::number(2.5)];
    let comparison = Comparison::Equality { strict: false, negated: true };
    for answer in [true, false] {
        let mut frame = Probe::of(&pair);
        frame.answer = Some(answer);
        let step = run(&mut frame, comparison, false);
        assert!(matches!(step, Step::Control(edge) if edge == if answer { Edge::Target } else { Edge::Following }));
        assert_eq!(frame.calls, [CallOut::Decide { left: 0, right: 1, operator: BinaryOperator::Inequality as u32 }]);
    }
    let mut frame = Probe::of(&pair);
    assert!(matches!(run(&mut frame, comparison, false), Step::Unresolved), "no VM decision leaves the row");
}

/// A constant the frame holds no word for leaves the row to the interpreter.
#[test]
fn an_equality_of_a_constant_the_frame_has_no_word_for_is_unresolved() {
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![Value::number(1.5).to_bits()], heap: None };
    let operation = Operation::CompareJump { comparison: Comparison::Equality { strict: true, negated: false },
        left: Operand::register(0), right: Operand::constant(0).expect("a constant operand"), negate: true };
    assert!(matches!(crate::staged::step(&mut stage, &mut frame, operation), Step::Unresolved));
}

/// The nullish test of register 0 against `expected` (`Operand` 1 is its constant side).
fn nullish_test(expected: ValueType, strict: bool, negated: bool) -> Comparison {
    Comparison::Nullish { expected, strict, negated }
}

/// A strict test is identity with its constant, whichever nullish value it names; the edge where
/// the register equals it carries the register's exact type, and a jump that `negate`s the test
/// takes the other edge.
#[test]
fn a_strict_nullish_test_is_identity_and_refines_the_edge_where_it_holds() {
    let values = [Value::NULL, Value::UNDEFINED, Value::integer(0), Value::TRUE, Value::number(0.5)];
    for (expected, constant) in [(ValueType::Null, Value::NULL), (ValueType::Undefined, Value::UNDEFINED)] {
        for (negated, negate) in [(false, false), (false, true), (true, false), (true, true)] {
            for value in values {
                let mut frame = Probe::of(&[value, constant]);
                let step = run(&mut frame, nullish_test(expected, true, negated), negate);
                let equal = value == constant;
                let taken = equal ^ negated ^ negate;
                assert!(matches!(step, Step::Control(edge) if edge == if taken { Edge::Target } else { Edge::Following }),
                    "{expected:?} {negated} {negate} {value:?}");
                let equal_edge = if negated == negate { Edge::Target } else { Edge::Following };
                assert_eq!(frame.refined, [(0, Type::language(expected), equal_edge)], "the equal edge carries the exact type");
                assert!(frame.calls.is_empty());
            }
        }
    }
}

/// A loose test holds for `null` and `undefined` alike and for nothing else, and where an IsHTMLDDA
/// object can exist a heap cell outside the records leaves the row to the interpreter.
#[test]
fn a_loose_nullish_test_holds_for_both_nullish_values_and_leaves_a_cell_that_may_be_htmldda() {
    let cell = Value::from_bits(crate::value::ValueLayout::HEAP_TAG | 1);
    for expected in [ValueType::Null, ValueType::Undefined] {
        for value in [Value::NULL, Value::UNDEFINED, Value::integer(0), Value::FALSE, Value::number(0.5), Value::NAN] {
            let mut frame = Probe::of(&[value, Value::NULL]);
            let step = run(&mut frame, nullish_test(expected, false, false), true);
            let equal = value.is_null() || value.is_undefined();
            assert!(matches!(step, Step::Control(edge) if edge == if equal { Edge::Following } else { Edge::Target }), "{value:?}");
            assert_eq!(frame.refined, [(0, Type::NULLISH, Edge::Following)]);
        }
        let mut frame = Probe::of(&[cell, Value::NULL]);
        assert!(matches!(run(&mut frame, nullish_test(expected, false, false), true), Step::Unresolved));
        assert!(frame.refined.is_empty(), "a left test proves nothing");
        frame.html_dda = false;
        let step = run(&mut frame, nullish_test(expected, false, false), true);
        assert!(matches!(step, Step::Control(Edge::Target)), "with no IsHTMLDDA object a cell is no nullish value");
    }
}

/// A register type that decides the test folds it: no word is read, and no fact is offered.
#[test]
fn a_type_that_decides_a_nullish_test_folds_it() {
    let undefined = Type::language(ValueType::Undefined);
    for (ty, strict, expected, equal) in [(undefined, true, ValueType::Undefined, true), (undefined, true, ValueType::Null, false),
        (undefined, false, ValueType::Null, true), (Type::INT32, true, ValueType::Null, false), (Type::NUMBER, false, ValueType::Undefined, false),
        (Type::DIRECT_OBJECT_FAMILY, false, ValueType::Null, false)] {
        let mut frame = Probe::of(&[Value::NULL, Value::NULL]);
        frame.types = vec![Some(ty)];
        let step = run(&mut frame, nullish_test(expected, strict, false), false);
        assert!(matches!(step, Step::Control(edge) if edge == if equal { Edge::Target } else { Edge::Following }), "{ty:?} {strict}");
        assert!(frame.refined.is_empty());
    }
    let mut frame = Probe::of(&[Value::NULL, Value::NULL]);
    frame.types = vec![Some(Type::ANY)];
    run(&mut frame, nullish_test(ValueType::Null, true, false), false);
    assert_eq!(frame.refined.len(), 1, "an undecided type is tested by its tag");
}

/// The constants a nullish test names are exactly `null` and `undefined`.
#[test]
fn only_null_and_undefined_are_nullish_constants() {
    assert_eq!(nullish_of(Value::NULL), Some(ValueType::Null));
    assert_eq!(nullish_of(Value::UNDEFINED), Some(ValueType::Undefined));
    assert_eq!(nullish_of(Value::FALSE), None);
}

/// The int32 bits the language's own operators make of two int32 values, independent of the staged ones.
fn bits(operation: NumberBitwise, left: i32, right: i32) -> i32 {
    let count = right as u32 & 31;
    match operation {
        NumberBitwise::And => left & right,
        NumberBitwise::Or => left | right,
        NumberBitwise::Xor => left ^ right,
        NumberBitwise::ShiftLeft => left.wrapping_shl(count),
        NumberBitwise::ShiftRight => left >> count,
        NumberBitwise::ShiftRightUnsigned => ((left as u32) >> count) as i32,
    }
}

/// A bitwise test jumps where the operator's bits are zero, whether its operands are int32s or
/// Numbers truncated modulo 2^32; an operand that is no Number leaves the row, and the Number the
/// operator would make is never made (no VM decision either).
#[test]
fn a_bitwise_jump_tests_the_bits_the_operator_makes_of_the_truncated_operands() {
    let operations = [NumberBitwise::And, NumberBitwise::Or, NumberBitwise::Xor, NumberBitwise::ShiftLeft,
        NumberBitwise::ShiftRight, NumberBitwise::ShiftRightUnsigned];
    let operands = [(Value::integer(0), 0), (Value::integer(1), 1), (Value::integer(-1), -1), (Value::integer(5), 5),
        (Value::integer(i32::MIN), i32::MIN), (Value::integer(32), 32), (Value::integer(33), 33),
        (Value::number(4294967297.0), 1), (Value::number(-4294967296.0), 0), (Value::number(2.5), 2)];
    for operation in operations {
        for (left, left_bits) in operands {
            for (right, right_bits) in operands {
                let mut frame = Probe::of(&[left, right]);
                let nonzero = bits(operation, left_bits, right_bits) != 0;
                let step = run(&mut frame, Comparison::Bitwise(operation), true);
                assert!(matches!(step, Step::Control(edge) if edge == if nonzero { Edge::Following } else { Edge::Target }),
                    "{operation:?} {left:?} {right:?}");
                assert!(frame.calls.is_empty());
            }
        }
        let mut frame = Probe::of(&[Value::TRUE, Value::integer(1)]);
        assert!(matches!(run(&mut frame, Comparison::Bitwise(operation), true), Step::Unresolved), "a Boolean is no Number");
    }
}

/// The bitwise operators decode to their test; the shifts too, and `Comparison::operator` names no
/// VM operator for a test no VM step decides.
#[test]
fn a_bitwise_operator_decodes_to_the_bits_test() {
    for (operator, operation) in [(BinaryOperator::BitwiseAnd, NumberBitwise::And), (BinaryOperator::BitwiseOR, NumberBitwise::Or),
        (BinaryOperator::BitwiseXOR, NumberBitwise::Xor), (BinaryOperator::ShiftLeft, NumberBitwise::ShiftLeft),
        (BinaryOperator::ShiftRight, NumberBitwise::ShiftRight), (BinaryOperator::ShiftRightZeroFill, NumberBitwise::ShiftRightUnsigned)] {
        let (left, right) = (Operand::register(1).0, Operand::register(2).0);
        let instruction = WideInstruction::new(Op::JumpBinaryFalse, operator as u16, left, right, 9);
        let Some(Operation::CompareJump { comparison, .. }) = Operation::<u64, ()>::decode(instruction, crate::staged::DecodeContext::EMPTY)
            else { panic!("a comparison jump decodes") };
        assert_eq!(comparison, Comparison::Bitwise(operation));
        assert_eq!(comparison.operator(), None);
    }
}

/// `instanceof` is the VM's decision of its two registers by its own operator: the jump takes the
/// edge the answer names, a frame with no VM leaves the row, and an operand that is no register
/// has no decision to ask for.
#[test]
fn an_instanceof_jump_is_the_vms_decision() {
    for answer in [true, false] {
        let mut frame = Probe::of(&[Value::UNDEFINED, Value::NULL]);
        frame.answer = Some(answer);
        let step = run(&mut frame, Comparison::Instanceof, true);
        assert!(matches!(step, Step::Control(edge) if edge == if answer { Edge::Following } else { Edge::Target }));
        assert_eq!(frame.calls, [CallOut::Decide { left: 0, right: 1, operator: BinaryOperator::Instanceof as u32 }]);
    }
    let mut frame = Probe::of(&[Value::UNDEFINED, Value::NULL]);
    assert!(matches!(run(&mut frame, Comparison::Instanceof, true), Step::Unresolved), "no VM leaves the row");
    let mut stage = Concrete::pure();
    let operation = Operation::CompareJump { comparison: Comparison::Instanceof, left: Operand::register(0),
        right: Operand::constant(0).expect("a constant operand"), negate: true };
    frame.answer = Some(true);
    frame.calls.clear();
    assert!(matches!(crate::staged::step(&mut stage, &mut frame, operation), Step::Unresolved));
    assert!(frame.calls.is_empty(), "a constant operand is no register the VM can read");
}

/// The operator names the comparison a `JumpBinaryFalse` decodes to, which a tier with the
/// operator alone (an interpreter record) asks for directly.
#[test]
fn an_operator_names_the_comparison_a_jump_decodes_to() {
    for operator in [BinaryOperator::LessThan, BinaryOperator::GreaterEqualThan, BinaryOperator::Equality,
        BinaryOperator::StrictInequality, BinaryOperator::BitwiseAnd, BinaryOperator::ShiftLeft,
        BinaryOperator::Instanceof, BinaryOperator::In] {
        let (left, right) = (Operand::register(1).0, Operand::register(2).0);
        let instruction = WideInstruction::new(Op::JumpBinaryFalse, operator as u16, left, right, 9);
        let Some(Operation::CompareJump { comparison, .. }) = Operation::<u64, ()>::decode(instruction, crate::staged::DecodeContext::EMPTY)
            else { panic!("a comparison jump decodes") };
        assert_eq!(comparison, Comparison::of_operator(operator as u32), "{operator:?}");
    }
}

/// A Boolean test is identity with its constant, whichever side the jump negates, and asks the VM
/// nothing.
#[test]
fn a_boolean_test_is_identity_with_its_constant() {
    let values = [Value::TRUE, Value::FALSE, Value::UNDEFINED, Value::integer(1), Value::number(0.5)];
    for expected in [true, false] {
        for (negated, negate) in [(false, false), (false, true), (true, false), (true, true)] {
            for value in values {
                let mut frame = Probe::of(&[value, Value::boolean(expected)]);
                let step = run(&mut frame, Comparison::Boolean { expected, negated }, negate);
                let holds = (value == Value::boolean(expected)) != negated;
                assert!(matches!(step, Step::Control(edge) if edge == if holds != negate { Edge::Target } else { Edge::Following }),
                    "{expected} {negated} {negate} {value:?}");
                assert!(frame.calls.is_empty());
            }
        }
    }
}

/// `in` is the VM's decision of its two registers by its own operator, as `instanceof` is: the
/// decode leaves it an operator, and a tier with the VM's residual names it (`Comparison::by_vm`).
#[test]
fn a_membership_jump_is_the_vms_decision() {
    let membership = Comparison::Operator(BinaryOperator::In as u32).by_vm();
    assert_eq!((membership, membership.operator()), (Comparison::Membership, Some(BinaryOperator::In as u32)));
    for comparison in [Comparison::Instanceof, Comparison::Operator(BinaryOperator::LessThan as u32),
        Comparison::Equality { strict: true, negated: false }] {
        assert_eq!(comparison.by_vm(), comparison, "only `in` is the VM's membership");
    }
    for answer in [true, false] {
        let mut frame = Probe::of(&[Value::UNDEFINED, Value::NULL]);
        frame.answer = Some(answer);
        let step = run(&mut frame, Comparison::Membership, true);
        assert!(matches!(step, Step::Control(edge) if edge == if answer { Edge::Following } else { Edge::Target }));
        assert_eq!(frame.calls, [CallOut::Decide { left: 0, right: 1, operator: BinaryOperator::In as u32 }]);
    }
    let mut frame = Probe::of(&[Value::UNDEFINED, Value::NULL]);
    assert!(matches!(run(&mut frame, Comparison::Membership, true), Step::Unresolved), "no VM leaves the row");
    let mut stage = Concrete::pure();
    frame.answer = Some(true);
    frame.calls.clear();
    let operation = Operation::CompareJump { comparison: Comparison::Membership, left: Operand::constant(0).expect("a constant operand"),
        right: Operand::register(1), negate: true };
    assert!(matches!(crate::staged::step(&mut stage, &mut frame, operation), Step::Unresolved));
    assert!(frame.calls.is_empty(), "a constant operand is no register the call-out can carry");
}

mod walk;
