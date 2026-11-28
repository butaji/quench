// The rows, operand states, synthetic Concrete frame and ECMAScript references of the tier
// homomorphism tests (TOD-2177, S1.14). Two tests mount this file: the Concrete-against-reference
// test (`quench-runtime/tests/tier_homomorphism.rs`) and the Emit-against-Concrete test
// (`quench-runtime-engine/src/compiled/object_slots/emit/tier_homomorphism_tests.rs`, which reaches
// the crate-private `Emit`). Both crates mount it with `include!`, so it holds no inner attributes or
// inner doc comments. Design: `/mnt/project-files/v2-merged/s114/design.md`.

use quench_runtime::bytecode::{Operand, Register};
use quench_runtime::staged::{
    op_kind, step_of, CallFrame, Concrete, Constant, Domain, Edge, ElementFrame, Float64, Frame, GlobalFrame, Int32,
    IntFact, NegativeZero, NoPlans, NumberFact, NumberOperation, NumericFrame, Operation, Overflow, PropertyFrame, Step,
};
use quench_runtime::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};

// ---------------------------------------------------------------------------------------------
// Rows: enumerated from the shared tables. Each consumer turns one table into a list.
// ---------------------------------------------------------------------------------------------

macro_rules! arithmetic_table {
    ($($operation:ident => $operator:ident, $method:ident, $instruction:ident, $symbol:tt, $float:ident, $checked:ident, $overflow:ident;)+) => {
        /// Each arithmetic row, and whether it has an int32 form (`Divide` has none: its checked
        /// form is `None` in the table, and no tier builds one).
        pub(crate) fn arithmetic_rows() -> Vec<(NumberArithmetic, bool)> {
            vec![$((NumberArithmetic::$operation, stringify!($checked) != "None")),+]
        }
    };
}
quench_runtime::number_arithmetic_rows!(arithmetic_table);

macro_rules! relation_table {
    ($($relation:ident => [$($operator:ident $symbol:literal),+ $(,)?], $stage_method:ident;)+) => {
        /// Each relation row.
        pub(crate) fn relation_rows() -> Vec<NumberRelation> { vec![$(NumberRelation::$relation),+] }
    };
}
quench_runtime::number_relation_rows!(relation_table);

macro_rules! bitwise_table {
    ($($operation:ident => $operator:ident, $stage_method:ident, $symbol:literal;)+) => {
        /// Each bitwise row.
        pub(crate) fn bitwise_rows() -> Vec<NumberBitwise> { vec![$(NumberBitwise::$operation),+] }
    };
}
quench_runtime::number_bitwise_rows!(bitwise_table);

/// The `NumberOperation` rows whose operator is a table row, with the shapes a tier decodes them
/// into. Remainder and increment are single rows of the Number family, enumerated here by hand.
pub(crate) const REMAINDER_DOMAINS: [Domain; 2] = [Domain::Int32, Domain::Float64];
pub(crate) const INCREMENT_DIRECTIONS: [bool; 2] = [false, true];

/// One row: its operation with its decoded fields (operands in registers 0 and 1, destination
/// register 2), and the operand states it runs on. An increment reads register 0 alone.
pub(crate) struct Row {
    pub(crate) label: String,
    pub(crate) operation: NumberOperation,
    pub(crate) states: Vec<(NumberWord, NumberWord)>,
}

fn arithmetic(operation: NumberArithmetic, domain: Domain, overflow: Overflow, zero: NegativeZero) -> NumberOperation {
    NumberOperation::Arithmetic { destination: 2, left: Operand::register(0), right: Operand::register(1), operation, domain,
        overflow, zero }
}

/// Every row with its states, in a fixed order that draws the same random states for a given seed.
/// Arithmetic: binary64 (`Divide` too), and int32 under each overflow and negative-zero policy, the
/// policies of one row sharing its states. Relations in both domains. Bitwise of int32 operands only
/// (ToInt32 of a binary64 is the frame's own read, not under test here). Remainder in both domains
/// (the divisor is the right operand, so a zero divisor is an edge state). Increment and decrement of
/// an int32 under each overflow policy.
pub(crate) fn rows(rng: &mut Rng, cases: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    for (operation, int_form) in arithmetic_rows() {
        let states = number_states(rng, cases);
        rows.push(Row { label: format!("{operation:?} Float64"), states,
            operation: arithmetic(operation, Domain::Float64, Overflow::Exit, NegativeZero::Exit) });
        if !int_form {
            continue;
        }
        let states = int_states(rng, cases);
        for overflow in [Overflow::Exit, Overflow::Wrap, Overflow::Widen] {
            for zero in [NegativeZero::Exit, NegativeZero::Ignore] {
                rows.push(Row { label: format!("{operation:?} Int32 overflow {overflow:?} zero {zero:?}"), states: states.clone(),
                    operation: arithmetic(operation, Domain::Int32, overflow, zero) });
            }
        }
    }
    for relation in relation_rows() {
        for domain in [Domain::Int32, Domain::Float64] {
            rows.push(Row { label: format!("{relation:?} {domain:?}"), states: states_of(rng, cases, domain),
                operation: NumberOperation::Relation { left: Operand::register(0), right: Operand::register(1), relation, domain } });
        }
    }
    for operation in bitwise_rows() {
        rows.push(Row { label: format!("{operation:?}"), states: int_states(rng, cases),
            operation: NumberOperation::Bitwise { destination: 2, left: Operand::register(0), right: Operand::register(1), operation } });
    }
    for domain in REMAINDER_DOMAINS {
        rows.push(Row { label: format!("Remainder {domain:?}"), states: states_of(rng, cases, domain),
            operation: NumberOperation::Remainder { destination: 2, left: Operand::register(0), right: Operand::register(1), domain,
                zero: NegativeZero::Exit, divisor_nonzero: false } });
    }
    for decrement in INCREMENT_DIRECTIONS {
        for overflow in [Overflow::Exit, Overflow::Wrap, Overflow::Widen] {
            let states = int_states(rng, cases).into_iter().map(|(left, _)| (left, left)).collect();
            rows.push(Row { label: format!("IncDec decrement {decrement} overflow {overflow:?}"), states,
                operation: NumberOperation::IncDec { destination: 2, source: 0, decrement, overflow } });
        }
    }
    rows
}

/// The row count `rows` makes: each arithmetic row once for binary64, and once per int32 policy pair
/// (6 more) where it has an int32 form; each relation in both domains; each bitwise, remainder domain
/// and increment policy.
pub(crate) fn expected_row_count() -> usize {
    arithmetic_rows().iter().map(|(_, int_form)| if *int_form { 7 } else { 1 }).sum::<usize>()
        + relation_rows().len() * 2 + bitwise_rows().len() + REMAINDER_DOMAINS.len() + INCREMENT_DIRECTIONS.len() * 3
}

// ---------------------------------------------------------------------------------------------
// Operand states and the synthetic frame the rows run on.
// ---------------------------------------------------------------------------------------------

/// A Number word a register holds, in the representation its frame fact names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum NumberWord {
    Int(i32),
    Float(f64),
}

pub(crate) fn int_word(value: i32) -> u64 { u64::from(value as u32) }

impl NumberWord {
    /// The machine word a register of this word's representation holds: an int32's low 32 bits, or
    /// a binary64's bits.
    pub(crate) fn bits(self) -> u64 {
        match self {
            NumberWord::Int(value) => int_word(value),
            NumberWord::Float(value) => value.to_bits(),
        }
    }
    pub(crate) fn domain(self) -> Domain {
        match self {
            NumberWord::Int(_) => Domain::Int32,
            NumberWord::Float(_) => Domain::Float64,
        }
    }
}

/// A register frame over Number words. Its operand reads are the frame's own conversions: an int32
/// reads as itself, a binary64 as its bits, and each domain reads the other representation
/// converted (`i32 as f64` exactly, or the binary64 of an int). The synthetic frame's int32 read of a
/// binary64 truncates with `as`, not ToInt32, so int32 operands are generated as int32s only.
struct NumberFrame {
    values: Vec<NumberWord>,
}

impl Frame<Concrete> for NumberFrame {
    type Fact = Domain;
    type Control = Edge;

    fn fact(&self, _register: Register) -> Domain { Domain::Int32 }
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 { self.values[usize::from(register)].bits() }
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
    fn branch(&mut self, _stage: &mut Concrete, _condition: bool) -> Edge { unreachable!("a Number operation branches nowhere") }
    fn jump(&mut self, _stage: &mut Concrete) -> Edge { unreachable!("a Number operation jumps nowhere") }
    fn boolean(&mut self, _stage: &mut Concrete, _condition: bool) -> (u64, Domain) {
        unreachable!("a Number operation makes no boolean")
    }
}

impl GlobalFrame<Concrete> for NumberFrame {}
impl CallFrame<Concrete> for NumberFrame {}
impl PropertyFrame<Concrete> for NumberFrame {
    type Plans = NoPlans;
    fn plans(&self) -> NoPlans { NoPlans }
}
impl ElementFrame<Concrete> for NumberFrame {}

impl NumericFrame<Concrete> for NumberFrame {
    fn int_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<Int32<u64>> {
        let word = match self.values[usize::from(operand.register_index().expect("a register operand"))] {
            NumberWord::Int(value) => int_word(value),
            NumberWord::Float(value) => int_word(value as i32),
        };
        Some(Int32::by_fact(word, IntFact::int32_home()))
    }
    fn number_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<Float64<u64>> {
        let word = match self.values[usize::from(operand.register_index().expect("a register operand"))] {
            NumberWord::Int(value) => f64::from(value).to_bits(),
            NumberWord::Float(value) => value.to_bits(),
        };
        Some(Float64::by_fact(word, NumberFact::float64_home()))
    }
    fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { Some(()) }
    fn miss(&self) {}
    fn int_fact(&self) -> Domain { Domain::Int32 }
    fn number_fact(&self) -> Domain { Domain::Float64 }
}

/// The result of one row run: the word it defined (as a Number), or `None` where it left at its
/// miss edge.
pub(crate) type Outcome = Option<NumberWord>;

/// Runs `operation` on Concrete over a frame holding `left` and `right` in registers 0 and 1; the
/// result is register 2, or the predicate word a relation returns. The Number arm of the staged
/// step (`op_kind::NUMBER`), the arm `step` takes for an `Operation::Number`.
pub(crate) fn run(left: NumberWord, right: NumberWord, operation: NumberOperation) -> Outcome {
    let is_relation = matches!(operation, NumberOperation::Relation { .. });
    let mut stage = Concrete::pure();
    let mut frame = NumberFrame { values: vec![left, right, NumberWord::Int(0)] };
    match step_of::<{ op_kind::NUMBER }, _, _>(&mut stage, &mut frame, Operation::Number(operation)) {
        Step::Value(predicate) if is_relation => Some(NumberWord::Int(i32::from(predicate != 0))),
        Step::Value(_) => Some(frame.values[2]),
        Step::Unresolved => None,
        Step::Control(_) => panic!("a Number row transfers no control"),
    }
}

/// Two outcomes agree when they leave together, or define the same int32, or the same binary64 bits
/// with every NaN equal (ECMAScript has one NaN and the sign of a zero is observable).
pub(crate) fn same(expected: Outcome, actual: Outcome) -> bool {
    match (expected, actual) {
        (None, None) => true,
        (Some(NumberWord::Int(a)), Some(NumberWord::Int(b))) => a == b,
        (Some(NumberWord::Float(a)), Some(NumberWord::Float(b))) => (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits(),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// Independent references: the ECMAScript operations, written over i64 and f64.
// ---------------------------------------------------------------------------------------------

/// What the reference says `operation` answers on `left` and `right`, and whether a leave may stand
/// for that answer (`conservative_wrap_leave`).
pub(crate) fn reference(operation: NumberOperation, left: NumberWord, right: NumberWord) -> (Outcome, bool) {
    match operation {
        NumberOperation::Arithmetic { operation, domain, overflow, zero, .. } =>
            (reference_arithmetic(operation, left, right, domain, overflow, zero),
                conservative_wrap_leave(operation, left, right, domain, overflow, zero)),
        NumberOperation::Relation { relation, domain, .. } =>
            (Some(NumberWord::Int(i32::from(reference_relation(relation, left, right, domain)))), false),
        NumberOperation::Bitwise { operation, .. } => {
            let (NumberWord::Int(a), NumberWord::Int(b)) = (left, right) else { unreachable!("int states") };
            (reference_bitwise(operation, a, b), false)
        }
        NumberOperation::Remainder { domain, .. } => (reference_remainder(left, right, domain), false),
        NumberOperation::IncDec { decrement, overflow, .. } => {
            let NumberWord::Int(value) = left else { unreachable!("int states") };
            (reference_increment(value, decrement, overflow), false)
        }
        _ => unreachable!("a row `rows` makes"),
    }
}

/// The int32 range as an i64 interval test.
fn fits_int32(value: i64) -> bool { i64::from(i32::MIN) <= value && value <= i64::from(i32::MAX) }

/// `left op right` on Numbers, the operand values as ECMAScript reads them in `domain`.
fn reference_arithmetic(operation: NumberArithmetic, left: NumberWord, right: NumberWord, domain: Domain,
    overflow: Overflow, zero: NegativeZero) -> Outcome {
    let (a, b) = (int_value(left), int_value(right));
    match domain {
        Domain::Int32 => {
            let (a, b) = (i64::from(a.expect("an int32 operand")), i64::from(b.expect("an int32 operand")));
            let exact = match operation {
                NumberArithmetic::Add => a + b,
                NumberArithmetic::Subtract => a - b,
                NumberArithmetic::Multiply => a * b,
                NumberArithmetic::Divide => unreachable!("division has no int32 row"),
            };
            // A product of zero is -0 where its operands' signs differ (the one zero of an int32 product).
            if operation == NumberArithmetic::Multiply && exact == 0 && ((a < 0) != (b < 0)) {
                return match (overflow, zero) {
                    (Overflow::Widen, _) => Some(NumberWord::Float(-0.0)),
                    (_, NegativeZero::Ignore) => Some(NumberWord::Int(0)),
                    (_, NegativeZero::Exit) => None,
                };
            }
            if fits_int32(exact) {
                return Some(NumberWord::Int(exact as i32));
            }
            match overflow {
                Overflow::Exit | Overflow::Impossible => None,
                Overflow::Wrap => Some(NumberWord::Int(exact as i32)),
                Overflow::Widen => Some(NumberWord::Float(exact as f64)),
            }
        }
        Domain::Float64 => {
            let (x, y) = (value_f64(left), value_f64(right));
            let result = match operation {
                NumberArithmetic::Add => x + y,
                NumberArithmetic::Subtract => x - y,
                NumberArithmetic::Multiply => x * y,
                NumberArithmetic::Divide => x / y,
            };
            Some(NumberWord::Float(result))
        }
    }
}

fn int_value(word: NumberWord) -> Option<i32> {
    match word {
        NumberWord::Int(value) => Some(value),
        NumberWord::Float(_) => None,
    }
}

/// The binary64 value an operand has in a Float64 operation.
fn value_f64(word: NumberWord) -> f64 {
    match word {
        NumberWord::Int(value) => f64::from(value),
        NumberWord::Float(value) => value,
    }
}

/// `left rel right` as ECMAScript decides it: int32 operands compare as integers, otherwise as
/// binary64 values with IEEE semantics (NaN relates to nothing, and `!=` holds).
fn reference_relation(relation: NumberRelation, left: NumberWord, right: NumberWord, domain: Domain) -> bool {
    match domain {
        Domain::Int32 => {
            let (a, b) = (i64::from(int_value(left).expect("an int32 operand")), i64::from(int_value(right).expect("an int32 operand")));
            ordered(relation, a.cmp(&b))
        }
        Domain::Float64 => {
            let (x, y) = (value_f64(left), value_f64(right));
            match relation {
                NumberRelation::Equal => x == y,
                NumberRelation::NotEqual => x != y,
                NumberRelation::LessThan => x < y,
                NumberRelation::LessEqual => x <= y,
                NumberRelation::GreaterThan => x > y,
                NumberRelation::GreaterEqual => x >= y,
            }
        }
    }
}

fn ordered(relation: NumberRelation, ordering: std::cmp::Ordering) -> bool {
    match relation {
        NumberRelation::Equal => ordering.is_eq(),
        NumberRelation::NotEqual => ordering.is_ne(),
        NumberRelation::LessThan => ordering.is_lt(),
        NumberRelation::LessEqual => ordering.is_le(),
        NumberRelation::GreaterThan => ordering.is_gt(),
        NumberRelation::GreaterEqual => ordering.is_ge(),
    }
}

/// `left op right` for the bitwise operators (ToInt32 of each operand, the count modulo 32). A
/// zero-fill shift whose result is above `i32::MAX` is no int32, and leaves.
fn reference_bitwise(operation: NumberBitwise, left: i32, right: i32) -> Outcome {
    let count = (right as u32) & 31;
    let bits = match operation {
        NumberBitwise::And => left & right,
        NumberBitwise::Or => left | right,
        NumberBitwise::Xor => left ^ right,
        NumberBitwise::ShiftLeft => (i64::from(left) << count) as i32,
        NumberBitwise::ShiftRight => left >> count,
        NumberBitwise::ShiftRightUnsigned => {
            let unsigned = (left as u32) >> count;
            return (unsigned <= i32::MAX as u32).then_some(NumberWord::Int(unsigned as i32));
        }
    };
    Some(NumberWord::Int(bits))
}

/// `left % right` as ECMAScript computes it: an int32 result is the exact remainder, which leaves
/// where it would be NaN (a zero divisor) or `-0` (a zero remainder of a negative dividend). A
/// binary64 result is the IEEE remainder, which is the Rust `%` on binary64.
fn reference_remainder(left: NumberWord, right: NumberWord, domain: Domain) -> Outcome {
    match domain {
        Domain::Int32 => {
            let (a, b) = (i64::from(int_value(left).expect("an int32 operand")), i64::from(int_value(right).expect("an int32 operand")));
            if b == 0 || (a % b == 0 && a < 0) {
                return None;
            }
            Some(NumberWord::Int((a % b) as i32))
        }
        Domain::Float64 => Some(NumberWord::Float(value_f64(left) % value_f64(right))),
    }
}

/// `source + 1` or `source - 1` of an int32: in range, or per the overflow policy.
fn reference_increment(value: i32, decrement: bool, overflow: Overflow) -> Outcome {
    let exact = i64::from(value) + if decrement { -1 } else { 1 };
    if fits_int32(exact) {
        return Some(NumberWord::Int(exact as i32));
    }
    match overflow {
        Overflow::Exit | Overflow::Impossible => None,
        Overflow::Wrap => Some(NumberWord::Int(exact as i32)),
        Overflow::Widen => Some(NumberWord::Float(exact as f64)),
    }
}

/// A Wrap product whose exact value is no zero but whose wrapped word is, with a negative factor:
/// the row leaves there (`numeric.rs`, `negative_zero`, which tests the wrapped word and the
/// factors' sign bits). The leave is the interpreter's answer, so it stands for the value; the
/// exact rule would define the wrapped zero. Only Wrap with `NegativeZero::Exit`, where the
/// negative-zero test runs on a result that wrapped.
fn conservative_wrap_leave(operation: NumberArithmetic, left: NumberWord, right: NumberWord, domain: Domain,
    overflow: Overflow, zero: NegativeZero) -> bool {
    if domain != Domain::Int32 || operation != NumberArithmetic::Multiply || overflow != Overflow::Wrap || zero != NegativeZero::Exit {
        return false;
    }
    let (Some(a), Some(b)) = (int_value(left), int_value(right)) else { return false };
    let exact = i64::from(a) * i64::from(b);
    exact != 0 && exact as i32 == 0 && (a < 0 || b < 0)
}

// ---------------------------------------------------------------------------------------------
// Operand states: edge values, then seeded random ones.
// ---------------------------------------------------------------------------------------------

/// Seeded splitmix64: no dependency, same states on every run for a given seed.
pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, bound: usize) -> usize { (self.next() % bound as u64) as usize }
}

const INT_EDGES: [i32; 18] = [0, 1, -1, 2, -2, 3, -3, 7, -7, 46_341, -46_341, 1 << 15, -(1 << 15), 1 << 30, -(1 << 30),
    i32::MAX, i32::MIN, i32::MAX - 1];

fn float_edges() -> [f64; 22] {
    [0.0, -0.0, 0.5, -0.5, 1.0, -1.0, 2.5, -2.5, 3.0, -3.0, 1e-300, -1e-300, 9_007_199_254_740_992.0,
        9_007_199_254_740_994.0, 1e308, -1e308, f64::MAX, f64::MIN_POSITIVE, f64::INFINITY, f64::NEG_INFINITY,
        f64::NAN, -f64::NAN]
}

/// An int32 state: an edge, or a random int32.
fn int_state(rng: &mut Rng, edge: Option<usize>) -> NumberWord {
    match edge {
        Some(index) => NumberWord::Int(INT_EDGES[index]),
        None => NumberWord::Int(rng.next() as u32 as i32),
    }
}

/// A binary64 or int32 state for a Float64 operation: an edge, a random binary64 of random bits
/// (which covers every exponent and NaN payload), or a random int32.
fn number_state(rng: &mut Rng, edge: Option<usize>) -> NumberWord {
    let edges = float_edges();
    match edge {
        Some(index) if index < edges.len() => NumberWord::Float(edges[index]),
        Some(index) => NumberWord::Int(INT_EDGES[index - edges.len()]),
        None if rng.below(2) == 0 => NumberWord::Float(f64::from_bits(rng.next())),
        None => NumberWord::Int(rng.next() as u32 as i32),
    }
}

/// The states one row is run on: every edge pair, then `cases` random pairs of the domain's kind.
fn int_states(rng: &mut Rng, cases: usize) -> Vec<(NumberWord, NumberWord)> {
    let mut states = Vec::new();
    for left in 0..INT_EDGES.len() {
        for right in 0..INT_EDGES.len() {
            states.push((int_state(rng, Some(left)), int_state(rng, Some(right))));
        }
    }
    for _ in 0..cases {
        states.push((int_state(rng, None), int_state(rng, None)));
    }
    states
}

fn number_states(rng: &mut Rng, cases: usize) -> Vec<(NumberWord, NumberWord)> {
    let count = float_edges().len() + INT_EDGES.len();
    let mut states = Vec::new();
    for left in 0..count {
        for right in 0..count {
            states.push((number_state(rng, Some(left)), number_state(rng, Some(right))));
        }
    }
    for _ in 0..cases {
        states.push((number_state(rng, None), number_state(rng, None)));
    }
    states
}

fn states_of(rng: &mut Rng, cases: usize, domain: Domain) -> Vec<(NumberWord, NumberWord)> {
    match domain {
        Domain::Int32 => int_states(rng, cases),
        Domain::Float64 => number_states(rng, cases),
    }
}

pub(crate) fn seed() -> u64 {
    std::env::var("TIER_HOMOMORPHISM_SEED").ok().and_then(|text| u64::from_str_radix(text.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x5EED_2177)
}

pub(crate) fn cases() -> usize {
    std::env::var("TIER_HOMOMORPHISM_CASES").ok().and_then(|text| text.parse().ok()).unwrap_or(2_000)
}
