//! The comparison jumps on the interpreter's stage (`Concrete`).
use super::*;
use crate::bytecode::{Op, Register};
use crate::staged::tests::ConcreteFrame;
use crate::staged::{Concrete, Constant, Edge, Frame, GlobalFrame, NoPlans, PropertyFrame};
use crate::typed_ir::Type;
use crate::ValueType;
use oxc_ast::ast::BinaryOperator;

/// A register file that records the decisions it is asked for, answering them with `answer`, and
/// the facts a jump proves along its edges; `types` are the registers' proved types.
struct Probe {
    registers: Vec<u64>,
    calls: Vec<CallOut>,
    answer: Option<bool>,
    types: Vec<Option<Type>>,
    refined: Vec<(Register, Type, Edge)>,
    html_dda: bool,
}

impl Probe {
    fn of(values: &[Value]) -> Self {
        Self { registers: values.iter().map(|value| value.to_bits()).collect(), calls: Vec::new(), answer: None,
            types: Vec::new(), refined: Vec::new(), html_dda: true }
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

impl PropertyFrame<Concrete> for Probe {
    type Plans = NoPlans;
    fn plans(&self) -> NoPlans { NoPlans }
}

impl NumericFrame<Concrete> for Probe {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> u64 { unreachable!("no Number operand") }
    fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> u64 { unreachable!("no Number operand") }
    fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { None }
    fn miss(&self) {}
    fn html_dda(&self) -> bool { self.html_dda }
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

/// The step of the comparison jump of registers 0 and 1 over `frame`.
fn run(frame: &mut Probe, comparison: Comparison, negate: bool) -> Step<u64, Edge> {
    let operation = Operation::CompareJump { comparison, left: Operand::register(0), right: Operand::register(1), negate };
    crate::staged::step(&mut Concrete::new(std::ptr::null()), frame, operation)
}

/// The edge a comparison jump of registers 0 and 1 takes on the interpreter's stage.
fn edge(comparison: Comparison, [left, right]: [Value; 2], negate: bool) -> Edge {
    let mut stage = Concrete::new(std::ptr::null());
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
        Operation::<u64, ()>::decode(instruction, |_| None)
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

/// Every operator of the equality family over pairs the words decide: the jump takes the edge
/// the language's own equality names, and the VM is never asked.
#[test]
fn an_equality_the_words_decide_jumps_without_the_vm() {
    let values = [Value::integer(1), Value::integer(2), Value::number(f64::NAN), Value::NULL, Value::UNDEFINED, Value::TRUE];
    for (strict, negated) in [(false, false), (false, true), (true, false), (true, true)] {
        for (left, right) in values.iter().flat_map(|left| values.iter().map(move |right| (*left, *right))) {
            let mut frame = Probe::of(&[left, right]);
            let nullish_pair = !strict && [left, right].iter().all(|value| value.is_null() || value.is_undefined());
            let same = left == right && left.as_number().is_none_or(|number| !number.is_nan());
            let decided = left == right || (left.as_int().is_some() && right.as_int().is_some()) || nullish_pair;
            if !decided { continue; }
            let equal = same || nullish_pair;
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
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![Value::number(1.5).to_bits()], heap: None };
    let operation = Operation::CompareJump { comparison: Comparison::Equality { strict: true, negated: false },
        left: Operand::register(0), right: Operand::constant(0).expect("a constant operand"), negate: true };
    assert!(matches!(crate::staged::step(&mut stage, &mut frame, operation), Step::Unresolved));
}

/// The nullish test of register 0 against `expected` (`Operand` 1 is its constant side).
fn nullish_test(expected: Nullish, strict: bool, negated: bool) -> Comparison {
    Comparison::Nullish { expected, strict, negated }
}

/// A strict test is identity with its constant, whichever nullish value it names; the edge where
/// the register equals it carries the register's exact type, and a jump that `negate`s the test
/// takes the other edge.
#[test]
fn a_strict_nullish_test_is_identity_and_refines_the_edge_where_it_holds() {
    let values = [Value::NULL, Value::UNDEFINED, Value::integer(0), Value::TRUE, Value::number(0.5)];
    for (expected, constant) in [(Nullish::Null, Value::NULL), (Nullish::Undefined, Value::UNDEFINED)] {
        for (negated, negate) in [(false, false), (false, true), (true, false), (true, true)] {
            for value in values {
                let mut frame = Probe::of(&[value, constant]);
                let step = run(&mut frame, nullish_test(expected, true, negated), negate);
                let equal = value == constant;
                let taken = equal ^ negated ^ negate;
                assert!(matches!(step, Step::Control(edge) if edge == if taken { Edge::Target } else { Edge::Following }),
                    "{expected:?} {negated} {negate} {value:?}");
                let equal_edge = if negated == negate { Edge::Target } else { Edge::Following };
                assert_eq!(frame.refined, [(0, expected.ty(), equal_edge)], "the equal edge carries the exact type");
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
    for expected in [Nullish::Null, Nullish::Undefined] {
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
    for (ty, strict, expected, equal) in [(undefined, true, Nullish::Undefined, true), (undefined, true, Nullish::Null, false),
        (undefined, false, Nullish::Null, true), (Type::INT32, true, Nullish::Null, false), (Type::NUMBER, false, Nullish::Undefined, false),
        (Type::DIRECT_OBJECT_FAMILY, false, Nullish::Null, false)] {
        let mut frame = Probe::of(&[Value::NULL, Value::NULL]);
        frame.types = vec![Some(ty)];
        let step = run(&mut frame, nullish_test(expected, strict, false), false);
        assert!(matches!(step, Step::Control(edge) if edge == if equal { Edge::Target } else { Edge::Following }), "{ty:?} {strict}");
        assert!(frame.refined.is_empty());
    }
    let mut frame = Probe::of(&[Value::NULL, Value::NULL]);
    frame.types = vec![Some(Type::ANY)];
    run(&mut frame, nullish_test(Nullish::Null, true, false), false);
    assert_eq!(frame.refined.len(), 1, "an undecided type is tested by its tag");
}

/// The constants a nullish test names are exactly `null` and `undefined`.
#[test]
fn only_null_and_undefined_are_nullish_constants() {
    assert_eq!(Nullish::of(Value::NULL), Some(Nullish::Null));
    assert_eq!(Nullish::of(Value::UNDEFINED), Some(Nullish::Undefined));
    assert_eq!(Nullish::of(Value::FALSE), None);
}
