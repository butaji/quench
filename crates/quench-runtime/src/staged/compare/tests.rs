//! The comparison jumps on the interpreter's stage (`Concrete`).
use super::*;
use crate::bytecode::{Op, Register};
use crate::staged::tests::ConcreteFrame;
use crate::staged::{Concrete, Constant, Edge, Frame, GlobalFrame, NoPlans, PropertyFrame};
use oxc_ast::ast::BinaryOperator;

/// A register file that records the decisions it is asked for, answering them with `answer`.
pub(super) struct Probe {
    pub(super) registers: Vec<u64>,
    pub(super) calls: Vec<CallOut>,
    pub(super) answer: Option<bool>,
}

impl Probe {
    pub(super) fn of(values: &[Value]) -> Self {
        Self { registers: values.iter().map(|value| value.to_bits()).collect(), calls: Vec::new(), answer: None }
    }
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
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

/// The step of the comparison jump of registers 0 and 1 over `frame`.
pub(super) fn run(frame: &mut Probe, comparison: Comparison, negate: bool) -> Step<u64, Edge> {
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
