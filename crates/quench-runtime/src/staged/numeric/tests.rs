//! The staged Number operations on the concrete stage, against the interpreter's results.

use crate::bytecode::{Operand, Register};
use crate::staged::*;
use crate::vm::operations::{NumberArithmetic, NumberRelation};

/// A Number word a concrete frame holds in one of the two representations the staging
/// names.
#[derive(Clone, Copy, Debug, PartialEq)]
enum NumberWord {
    Int(i32),
    Float(f64),
}

fn int_word(value: i32) -> u64 { u64::from(value as u32) }

/// A concrete frame whose registers hold Number words and whose write fact names their
/// representation, so staged arithmetic defines an int32 or a binary64 result.
struct NumberFrame {
    values: Vec<NumberWord>,
}

impl Frame<Concrete> for NumberFrame {
    type Fact = Domain;
    type Control = Edge;

    fn fact(&self, _register: Register) -> Domain { Domain::Int32 }
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        match self.values[usize::from(register)] {
            NumberWord::Int(value) => int_word(value),
            NumberWord::Float(value) => value.to_bits(),
        }
    }
    fn write(&mut self, _stage: &mut Concrete, register: Register, word: u64, fact: Domain) {
        self.values[usize::from(register)] = match fact {
            Domain::Int32 => NumberWord::Int(word as u32 as i32),
            Domain::Float64 => NumberWord::Float(f64::from_bits(word)),
        };
    }
    fn this(&mut self, _stage: &mut Concrete) -> u64 { unreachable!("Number operations read no this") }

    fn define_this(&mut self, _stage: &mut Concrete, _destination: Register, _this: u64) {
        unreachable!("Number operations read no this")
    }

    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, Domain) {
        let value = constant.value;
        match value.as_int() {
            Some(int) => (int_word(int), Domain::Int32),
            None => (value.as_number().expect("a Number constant").to_bits(), Domain::Float64),
        }
    }
    fn truthiness(&mut self, _stage: &mut Concrete, _register: Register) -> Option<bool> {
        unreachable!("a Number operation decides no truthiness")
    }
    fn branch(&mut self, _stage: &mut Concrete, _condition: bool) -> Edge {
        unreachable!("a Number operation branches nowhere")
    }
    fn jump(&mut self, _stage: &mut Concrete) -> Edge {
        unreachable!("a Number operation jumps nowhere")
    }
    fn boolean(&mut self, _stage: &mut Concrete, _condition: bool) -> (u64, Domain) {
        unreachable!("a Number operation makes no boolean")
    }
}

impl NumericFrame<Concrete> for NumberFrame {
    fn int_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> u64 {
        match self.values[usize::from(operand.register_index().expect("a register operand"))] {
            NumberWord::Int(value) => int_word(value),
            NumberWord::Float(value) => int_word(value as i32),
        }
    }
    fn number_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> u64 {
        match self.values[usize::from(operand.register_index().expect("a register operand"))] {
            NumberWord::Int(value) => f64::from(value).to_bits(),
            NumberWord::Float(value) => value.to_bits(),
        }
    }
    fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { Some(()) }
    fn miss(&self) {}
    fn int_fact(&self) -> Domain { Domain::Int32 }
    fn number_fact(&self) -> Domain { Domain::Float64 }
}

fn number_frame(left: NumberWord, right: NumberWord) -> NumberFrame {
    NumberFrame { values: vec![left, right, NumberWord::Int(0)] }
}

/// The arithmetic result, or `None` where the operation left at its miss edge.
fn concrete_arithmetic(left: NumberWord, right: NumberWord, operation: NumberArithmetic,
    domain: Domain, overflow: Overflow) -> Option<NumberWord> {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = number_frame(left, right);
    let operation = Operation::Number(NumberOperation::Arithmetic {
        destination: 2, left: Operand::register(0), right: Operand::register(1),
        operation, domain, overflow,
    });
    match step(&mut stage, &mut frame, operation) {
        Step::Value(_) => Some(frame.values[2]),
        Step::Unresolved => None,
        Step::Control(_) => panic!("arithmetic transfers no control"),
    }
}

fn concrete_relation(left: NumberWord, right: NumberWord, relation: NumberRelation, domain: Domain) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = number_frame(left, right);
    let operation = Operation::Number(NumberOperation::Relation {
        left: Operand::register(0), right: Operand::register(1), relation, domain,
    });
    match step(&mut stage, &mut frame, operation) {
        Step::Value(word) => word != 0,
        _ => panic!("a relation produces a predicate"),
    }
}

#[test]
fn staged_int32_arithmetic_matches_the_interpreter_results() {
    use NumberArithmetic::{Add, Multiply, Subtract};
    for (left, right, operation, expected) in [
        (2, 3, Add, 5), (-7, 4, Add, -3), (2, 3, Subtract, -1),
        (-7, -4, Subtract, -3), (6, 7, Multiply, 42), (-6, 7, Multiply, -42),
    ] {
        let result = concrete_arithmetic(NumberWord::Int(left), NumberWord::Int(right),
            operation, Domain::Int32, Overflow::Exit);
        assert_eq!(result, Some(NumberWord::Int(expected)), "{operation:?} {left} {right}");
    }
}

#[test]
fn staged_int32_overflow_leaves_wraps_or_widens() {
    let max = i32::MAX;
    assert_eq!(concrete_arithmetic(NumberWord::Int(max), NumberWord::Int(1),
        NumberArithmetic::Add, Domain::Int32, Overflow::Exit), None);
    assert_eq!(concrete_arithmetic(NumberWord::Int(max), NumberWord::Int(1),
        NumberArithmetic::Add, Domain::Int32, Overflow::Wrap), Some(NumberWord::Int(i32::MIN)));
    assert_eq!(concrete_arithmetic(NumberWord::Int(max), NumberWord::Int(1),
        NumberArithmetic::Add, Domain::Int32, Overflow::Widen),
        Some(NumberWord::Float(f64::from(max) + 1.0)));
    assert_eq!(concrete_arithmetic(NumberWord::Int(0), NumberWord::Int(-1),
        NumberArithmetic::Multiply, Domain::Int32, Overflow::Exit), None);
    let widened = concrete_arithmetic(NumberWord::Int(0), NumberWord::Int(-1),
        NumberArithmetic::Multiply, Domain::Int32, Overflow::Widen);
    assert!(matches!(widened, Some(NumberWord::Float(value)) if value == 0.0 && value.is_sign_negative()));
}

#[test]
fn staged_float_arithmetic_keeps_zero_nan_and_infinity() {
    use NumberArithmetic::{Add, Divide, Multiply, Subtract};
    let float = |operation, left, right| concrete_arithmetic(
        NumberWord::Float(left), NumberWord::Float(right), operation, Domain::Float64, Overflow::Exit);
    assert!(matches!(float(Multiply, -1.0, 0.0), Some(NumberWord::Float(value))
        if value == 0.0 && value.is_sign_negative()));
    assert!(matches!(float(Divide, 0.0, 0.0), Some(NumberWord::Float(value)) if value.is_nan()));
    assert!(matches!(float(Divide, 1.0, 0.0), Some(NumberWord::Float(value)) if value.is_infinite() && value > 0.0));
    assert!(matches!(float(Divide, -1.0, 0.0), Some(NumberWord::Float(value)) if value.is_infinite() && value < 0.0));
    assert_eq!(float(Add, 0.5, 0.25), Some(NumberWord::Float(0.75)));
    assert_eq!(float(Subtract, 0.5, 0.25), Some(NumberWord::Float(0.25)));
}

#[test]
fn staged_mixed_int_float_operands_widen_to_binary64() {
    assert_eq!(concrete_arithmetic(NumberWord::Int(3), NumberWord::Float(0.5),
        NumberArithmetic::Add, Domain::Float64, Overflow::Exit), Some(NumberWord::Float(3.5)));
    assert_eq!(concrete_arithmetic(NumberWord::Float(0.5), NumberWord::Int(4),
        NumberArithmetic::Multiply, Domain::Float64, Overflow::Exit), Some(NumberWord::Float(2.0)));
}

#[test]
fn staged_number_relations_are_ieee_comparisons_in_both_domains() {
    use NumberRelation::{Equal, GreaterEqual, GreaterThan, LessEqual, LessThan, NotEqual};
    for (relation, expected) in [(Equal, false), (NotEqual, true), (LessThan, true),
        (LessEqual, true), (GreaterThan, false), (GreaterEqual, false)] {
        assert_eq!(concrete_relation(NumberWord::Int(2), NumberWord::Int(3), relation, Domain::Int32),
            expected, "int {relation:?}");
        assert_eq!(concrete_relation(NumberWord::Float(2.5), NumberWord::Float(3.5), relation, Domain::Float64),
            expected, "float {relation:?}");
    }
    for relation in [Equal, LessEqual, GreaterEqual] {
        assert!(concrete_relation(NumberWord::Int(2), NumberWord::Int(2), relation, Domain::Int32),
            "int equal {relation:?}");
        assert!(concrete_relation(NumberWord::Float(2.5), NumberWord::Float(2.5), relation, Domain::Float64),
            "float equal {relation:?}");
    }
    let nan = f64::NAN;
    for relation in [Equal, NotEqual, LessThan, LessEqual, GreaterThan, GreaterEqual] {
        let expected = relation == NotEqual;
        assert_eq!(concrete_relation(NumberWord::Float(nan), NumberWord::Float(nan), relation, Domain::Float64),
            expected, "NaN {relation:?}");
    }
    assert!(concrete_relation(NumberWord::Float(-0.0), NumberWord::Float(0.0), Equal, Domain::Float64));
    assert!(concrete_relation(NumberWord::Int(3), NumberWord::Float(3.5), LessThan, Domain::Float64));
    assert!(!concrete_relation(NumberWord::Float(f64::INFINITY), NumberWord::Float(1.0), LessThan, Domain::Float64));
}

/// `++`/`--` of an int32 register, or `None` where it left at its miss edge.
fn concrete_increment(value: i32, decrement: bool, overflow: Overflow) -> Option<NumberWord> {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = number_frame(NumberWord::Int(value), NumberWord::Int(0));
    let operation = Operation::Number(NumberOperation::IncDec { destination: 2, source: 0, decrement, overflow });
    match step(&mut stage, &mut frame, operation) {
        Step::Value(_) => Some(frame.values[2]),
        Step::Unresolved => None,
        Step::Control(_) => panic!("an increment transfers no control"),
    }
}

#[test]
fn staged_increments_are_int32_sums_with_one() {
    assert_eq!(concrete_increment(41, false, Overflow::Exit), Some(NumberWord::Int(42)));
    assert_eq!(concrete_increment(-41, true, Overflow::Exit), Some(NumberWord::Int(-42)));
    assert_eq!(concrete_increment(i32::MAX, false, Overflow::Exit), None);
    assert_eq!(concrete_increment(i32::MIN, true, Overflow::Exit), None);
    assert_eq!(concrete_increment(i32::MAX, false, Overflow::Wrap), Some(NumberWord::Int(i32::MIN)));
    assert_eq!(concrete_increment(i32::MIN, true, Overflow::Widen),
        Some(NumberWord::Float(f64::from(i32::MIN) - 1.0)));
}
