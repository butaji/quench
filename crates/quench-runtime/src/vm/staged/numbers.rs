//! JavaScript's operations on Numbers, written once over a number machine (`Numbers`): the
//! interpreter runs them on values, the specializer on IR values with its register facts.
//! What a machine knows of an operand (an int32 proved, a double held, a constant) is
//! static: the semantics branches on it in Rust, so the residual keeps only the arm taken.

use super::Stage;
use crate::bytecode::Register;
use crate::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};

/// An operand as the semantics sees it, decoded once from the instruction: a register, or a
/// constant whose value is static.
#[derive(Clone, Copy)]
pub(crate) enum Source {
    Register(Register),
    Constant(crate::Value),
}

/// A number result whose representation the semantics knows statically: an int32 or a
/// double. A join of arms of different representations takes the double.
#[derive(Clone, Copy)]
pub(crate) enum Number<I, F> {
    Int(I),
    Float(F),
}

/// The number domain of a stage: int32 and double words, the operations on them, typed
/// register reads and the static facts that decide which apply. The interpreter, which proves
/// nothing ahead, knows only what its quickening guessed and tests the values; the specializer
/// folds what its register state proves.
pub(crate) trait Numbers: Stage {
    type Int: Copy;
    type Float: Copy;
    /// Whether `register` is proved an int32.
    fn known_int(&self, register: Register) -> bool;
    /// Whether `register` is held as a double.
    fn known_float(&self, register: Register) -> bool;
    /// Whether the machine keeps operands proved int32 in int32 and leaves where an exact
    /// result is none, as a specializer does; the interpreter, which nothing falls back from,
    /// takes the double.
    fn speculates(&self) -> bool;
    /// Whether the current operation's int32 sum or difference may wrap: its result is read
    /// only through ToInt32 (`| 0` and its kin), or proved within int32.
    fn wraps(&self) -> bool;
    /// Whether the operation's result is read only where the sign of a zero is lost (ToInt32,
    /// a relation, a sum read so): a product then needs no negative-zero test.
    fn zero_signless(&self) -> bool { false }
    fn int_register(&mut self, register: Register, miss: Self::Miss) -> Option<Self::Int>;
    fn float_register(&mut self, register: Register, miss: Self::Miss) -> Option<Self::Float>;
    /// Whether `register` holds an int32.
    fn int_held(&mut self, register: Register) -> Self::Cond;
    /// In the arm where `int_held(register)` holds: an int32 from here on, never tested again.
    fn assume_int(&mut self, register: Register);
    fn int_constant(&mut self, value: i32) -> Self::Int;
    fn float_constant(&mut self, value: f64) -> Self::Float;
    fn cond_constant(&mut self, value: bool) -> Self::Cond;
    /// `operation` on two int32s, and whether its exact result leaves int32.
    fn int_checked(&mut self, operation: NumberArithmetic, left: Self::Int, right: Self::Int) -> (Self::Int, Self::Cond);
    /// A sum or difference of int32s, wrapped to int32.
    fn int_wrapping(&mut self, operation: NumberArithmetic, left: Self::Int, right: Self::Int) -> Self::Int;
    fn int_bitwise(&mut self, operation: NumberBitwise, left: Self::Int, right: Self::Int) -> Self::Int;
    fn int_relation(&mut self, relation: NumberRelation, left: Self::Int, right: Self::Int) -> Self::Cond;
    fn float_op(&mut self, operation: NumberArithmetic, left: Self::Float, right: Self::Float) -> Self::Float;
    /// `relation` of two doubles by IEEE 754: NaN is unordered and unequal, -0 equals +0.
    fn float_relation(&mut self, relation: NumberRelation, left: Self::Float, right: Self::Float) -> Self::Cond;
    /// The Number of `bits` read as an unsigned 32-bit integer.
    fn unsigned(&mut self, bits: Self::Int) -> Self::Float;
    /// The double of an int32.
    fn float_of_int(&mut self, int: Self::Int) -> Self::Float;
    /// ToInt32 of a double; `miss` where the machine does not compute it exactly.
    fn float_to_int32(&mut self, number: Self::Float, miss: Self::Miss) -> Option<Self::Int>;
    fn or_cond(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    fn and_cond(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    fn not_cond(&mut self, a: Self::Cond) -> Self::Cond;
    /// The number `then` makes where `cond` holds, else `otherwise`'s, each arm's register
    /// facts joined after; the second arm is the rare one.
    fn choose_number(
        &mut self,
        cond: Self::Cond,
        then: impl FnOnce(&mut Self) -> Option<Number<Self::Int, Self::Float>>,
        otherwise: impl FnOnce(&mut Self) -> Option<Number<Self::Int, Self::Float>>,
    ) -> Option<Number<Self::Int, Self::Float>>;
    /// The condition `then` makes where `cond` holds, else `otherwise`'s, as `choose_number`.
    fn choose_cond(
        &mut self,
        cond: Self::Cond,
        then: impl FnOnce(&mut Self) -> Option<Self::Cond>,
        otherwise: impl FnOnce(&mut Self) -> Option<Self::Cond>,
    ) -> Option<Self::Cond>;
    fn set_number(&mut self, register: Register, number: Number<Self::Int, Self::Float>);
    /// `register = cond`, as a Boolean.
    fn set_boolean(&mut self, register: Register, cond: Self::Cond);
}

/// Whether `source` is proved an int32: a register by the machine's facts, a constant by its value.
#[inline(always)]
fn known_int<N: Numbers>(machine: &N, source: Source) -> bool {
    match source {
        Source::Register(register) => machine.known_int(register),
        Source::Constant(value) => value.as_int().is_some(),
    }
}

/// Whether `source` is held as a double, or is a constant no int32 holds.
#[inline(always)]
fn known_float<N: Numbers>(machine: &N, source: Source) -> bool {
    match source {
        Source::Register(register) => machine.known_float(register),
        Source::Constant(value) => value.as_int().is_none(),
    }
}

/// A path that never runs past here: `miss` taken unconditionally.
#[inline(always)]
fn never<N: Numbers>(machine: &mut N, miss: N::Miss) -> Option<()> {
    let never = machine.cond_constant(false);
    machine.guard(never, miss)
}

#[inline(always)]
fn int_operand<N: Numbers>(machine: &mut N, source: Source, miss: N::Miss) -> Option<N::Int> {
    match source {
        Source::Register(register) => machine.int_register(register, miss),
        Source::Constant(value) => match value.as_int() {
            Some(int) => Some(machine.int_constant(int)),
            None => never(machine, miss).map(|()| machine.int_constant(0)),
        },
    }
}

#[inline(always)]
fn float_operand<N: Numbers>(machine: &mut N, source: Source, miss: N::Miss) -> Option<N::Float> {
    match source {
        Source::Register(register) => machine.float_register(register, miss),
        Source::Constant(value) => match value.as_number() {
            Some(number) => Some(machine.float_constant(number)),
            None => never(machine, miss).map(|()| machine.float_constant(0.0)),
        },
    }
}

/// Whether the operands not proved int32 hold int32s; one of them is not proved.
#[inline(always)]
fn ints_held<N: Numbers>(machine: &mut N, sources: &[Source]) -> N::Cond {
    let mut ints = None;
    for &source in sources {
        if !known_int(machine, source) {
            let held = match source {
                Source::Register(register) => machine.int_held(register),
                Source::Constant(_) => machine.cond_constant(false),
            };
            ints = Some(match ints { Some(ints) => machine.and_cond(ints, held), None => held });
        }
    }
    ints.expect("an operand is not proved int32")
}

/// `source`'s int32 in an arm where `ints_held` holds of it.
#[inline(always)]
fn assumed_int<N: Numbers>(machine: &mut N, source: Source, miss: N::Miss) -> Option<N::Int> {
    if let Source::Register(register) = source && !machine.known_int(register) {
        machine.assume_int(register);
    }
    int_operand(machine, source, miss)
}

/// Whether the int32 product `result` of `left` and `right` is -0, which no int32 holds: a
/// zero product with a negative factor, whose sign the factors' OR carries.
#[inline(always)]
fn negative_zero_product<N: Numbers>(machine: &mut N, result: N::Int, left: N::Int, right: N::Int) -> N::Cond {
    let zero = machine.int_constant(0);
    let zero_product = machine.int_relation(NumberRelation::Equal, result, zero);
    let signs = machine.int_bitwise(NumberBitwise::Or, left, right);
    let negative = machine.int_relation(NumberRelation::LessThan, signs, zero);
    machine.and_cond(zero_product, negative)
}

/// `operation` on int32s, with the condition under which its exact result is no int32: an
/// overflow, or a product's -0.
#[inline(always)]
fn int_arithmetic<N: Numbers>(machine: &mut N, operation: NumberArithmetic, left: N::Int, right: N::Int) -> (N::Int, N::Cond) {
    let (result, overflow) = machine.int_checked(operation, left, right);
    if operation != NumberArithmetic::Multiply || machine.zero_signless() {
        return (result, overflow);
    }
    let negative_zero = negative_zero_product(machine, result, left, right);
    (result, machine.or_cond(overflow, negative_zero))
}

/// `operation` on the doubles of `left` and `right`.
#[inline(always)]
fn float_arithmetic<N: Numbers>(machine: &mut N, operation: NumberArithmetic, left: Source, right: Source, miss: N::Miss) -> Option<Number<N::Int, N::Float>> {
    let (a, b) = (float_operand(machine, left, miss)?, float_operand(machine, right, miss)?);
    Some(Number::Float(machine.float_op(operation, a, b)))
}

/// `left op right` for Number operands, JavaScript's arithmetic on Numbers: int32 operands
/// give an int32 where the exact result is one, else the IEEE double; a division takes the
/// doubles. Operands proved int32 stay int32 and leave where the exact result is no int32,
/// as C keeps them; a sum or difference read only through ToInt32 wraps. `miss` for an
/// operand that is not a Number.
#[inline(always)]
pub(crate) fn arithmetic<N: Numbers>(
    machine: &mut N,
    operation: NumberArithmetic,
    wraps: bool,
    left: Source,
    right: Source,
    miss: N::Miss,
) -> Option<Number<N::Int, N::Float>> {
    let wraps = wraps && matches!(operation, NumberArithmetic::Add | NumberArithmetic::Subtract);
    let ints = |machine: &mut N, a: N::Int, b: N::Int| if wraps {
        (machine.int_wrapping(operation, a, b), None)
    } else {
        let (result, inexact) = int_arithmetic(machine, operation, a, b);
        (result, Some(inexact))
    };
    if operation == NumberArithmetic::Divide || known_float(machine, left) || known_float(machine, right) {
        return float_arithmetic(machine, operation, left, right, miss);
    }
    if known_int(machine, left) && known_int(machine, right) {
        let (a, b) = (int_operand(machine, left, miss)?, int_operand(machine, right, miss)?);
        return match ints(machine, a, b) {
            (result, None) => Some(Number::Int(result)),
            (result, Some(inexact)) if machine.speculates() => {
                let exact = machine.not_cond(inexact);
                machine.guard(exact, miss)?;
                Some(Number::Int(result))
            }
            (result, Some(inexact)) => machine.choose_number(inexact, |machine| float_arithmetic(machine, operation, left, right, miss), |_| Some(Number::Int(result))),
        };
    }
    let held = ints_held(machine, &[left, right]);
    machine.choose_number(held, |machine| {
        let (a, b) = (assumed_int(machine, left, miss)?, assumed_int(machine, right, miss)?);
        match ints(machine, a, b) {
            (result, None) => Some(Number::Int(result)),
            (result, Some(inexact)) => machine.choose_number(inexact, |machine| float_arithmetic(machine, operation, left, right, miss), |_| Some(Number::Int(result))),
        }
    }, |machine| match float_arithmetic(machine, operation, left, right, miss)? {
        Number::Float(number) if wraps => Some(Number::Int(machine.float_to_int32(number, miss)?)),
        number => Some(number),
    })
}

/// ToInt32 of `source`'s double.
#[inline(always)]
fn truncate<N: Numbers>(machine: &mut N, source: Source, miss: N::Miss) -> Option<Number<N::Int, N::Float>> {
    let number = float_operand(machine, source, miss)?;
    Some(Number::Int(machine.float_to_int32(number, miss)?))
}

/// `source`'s ToInt32 (ECMA-262 7.1.6): an int32 as it is, any other Number truncated
/// modulo 2^32. `miss` for an operand that is not a Number.
#[inline(always)]
fn to_int32<N: Numbers>(machine: &mut N, source: Source, miss: N::Miss) -> Option<N::Int> {
    if known_int(machine, source) {
        return int_operand(machine, source, miss);
    }
    let number = if known_float(machine, source) {
        truncate(machine, source, miss)?
    } else {
        let held = ints_held(machine, &[source]);
        machine.choose_number(held, |machine| Some(Number::Int(assumed_int(machine, source, miss)?)), |machine| truncate(machine, source, miss))?
    };
    match number {
        Number::Int(int) => Some(int),
        Number::Float(_) => unreachable!("int32 arms join as an int32"),
    }
}

/// The int32 a double holds exactly; `miss` for any other double (a fraction, beyond int32,
/// NaN). -0 holds 0.
#[inline(always)]
fn exact_int<N: Numbers>(machine: &mut N, number: N::Float, miss: N::Miss) -> Option<N::Int> {
    let int = machine.float_to_int32(number, miss)?;
    let back = machine.float_of_int(int);
    let exact = machine.float_relation(NumberRelation::Equal, back, number);
    machine.guard(exact, miss)?;
    Some(int)
}

/// `source` as an element index: a Number that holds an int32, in whichever representation
/// the machine holds it; `miss` for any other value.
#[inline(always)]
pub(crate) fn index<N: Numbers>(machine: &mut N, source: Source, miss: N::Miss) -> Option<N::Int> {
    if known_int(machine, source) {
        return int_operand(machine, source, miss);
    }
    let number = if known_float(machine, source) {
        let number = float_operand(machine, source, miss)?;
        Number::Int(exact_int(machine, number, miss)?)
    } else {
        let held = ints_held(machine, &[source]);
        machine.choose_number(held, |machine| Some(Number::Int(assumed_int(machine, source, miss)?)), |machine| {
            let number = float_operand(machine, source, miss)?;
            Some(Number::Int(exact_int(machine, number, miss)?))
        })?
    };
    match number {
        Number::Int(int) => Some(int),
        Number::Float(_) => unreachable!("int32 arms join as an int32"),
    }
}

/// `left op right` for a bitwise operator: ToInt32 of both operands, the operator on their
/// bits, and the int32 result, but for a zero-fill shift, whose result is the Number of its
/// unsigned bits. `miss` for an operand that is not a Number.
#[inline(always)]
pub(crate) fn bitwise<N: Numbers>(
    machine: &mut N,
    operation: NumberBitwise,
    left: Source,
    right: Source,
    miss: N::Miss,
) -> Option<Number<N::Int, N::Float>> {
    let (left, right) = (to_int32(machine, left, miss)?, to_int32(machine, right, miss)?);
    let bits = machine.int_bitwise(operation, left, right);
    Some(if operation == NumberBitwise::ShiftRightUnsigned {
        Number::Float(machine.unsigned(bits))
    } else {
        Number::Int(bits)
    })
}

/// Whether `relation` holds of the doubles of `left` and `right`.
#[inline(always)]
fn float_relation<N: Numbers>(machine: &mut N, relation: NumberRelation, left: Source, right: Source, miss: N::Miss) -> Option<N::Cond> {
    let (a, b) = (float_operand(machine, left, miss)?, float_operand(machine, right, miss)?);
    Some(machine.float_relation(relation, a, b))
}

/// Whether Number `relation` holds of `left` and `right`: int32s compare as int32s, any
/// other Numbers as doubles, the two agreeing where both apply. `miss` for an operand that
/// is not a Number.
#[inline(always)]
pub(crate) fn relation<N: Numbers>(
    machine: &mut N,
    relation: NumberRelation,
    left: Source,
    right: Source,
    miss: N::Miss,
) -> Option<N::Cond> {
    if known_float(machine, left) || known_float(machine, right) {
        return float_relation(machine, relation, left, right, miss);
    }
    if known_int(machine, left) && known_int(machine, right) {
        let (a, b) = (int_operand(machine, left, miss)?, int_operand(machine, right, miss)?);
        return Some(machine.int_relation(relation, a, b));
    }
    let held = ints_held(machine, &[left, right]);
    machine.choose_cond(held, |machine| {
        let (a, b) = (assumed_int(machine, left, miss)?, assumed_int(machine, right, miss)?);
        Some(machine.int_relation(relation, a, b))
    }, |machine| float_relation(machine, relation, left, right, miss))
}

/// A binary operator on Numbers, as the semantics dispatches it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumberOperator {
    Arithmetic(NumberArithmetic),
    Bitwise(NumberBitwise),
    Relation(NumberRelation),
}

impl NumberOperator {
    /// The Number operator a bytecode and its operator denote (`from_js_instruction` of each family).
    #[inline(always)]
    pub(crate) fn from_js_instruction(instruction: crate::bytecode::Op, operator: u32) -> Option<Self> {
        NumberArithmetic::from_js_instruction(instruction, operator).map(Self::Arithmetic)
            .or_else(|| NumberBitwise::from_js_instruction(instruction, operator).map(Self::Bitwise))
            .or_else(|| (instruction == crate::bytecode::Op::Binary).then(|| NumberRelation::from_operator(operator)).flatten().map(Self::Relation))
    }
}

/// `destination = left op right` for a Number operator: a Number for arithmetic and bitwise
/// operators, a Boolean for a relation. `miss` for an operand that is not a Number.
#[inline(always)]
pub(crate) fn binary<N: Numbers>(
    machine: &mut N,
    operator: NumberOperator,
    destination: Register,
    left: Source,
    right: Source,
    miss: N::Miss,
) -> Option<()> {
    match operator {
        NumberOperator::Arithmetic(operation) => {
            let wraps = machine.wraps();
            let number = arithmetic(machine, operation, wraps, left, right, miss)?;
            machine.set_number(destination, number);
        }
        NumberOperator::Bitwise(operation) => {
            let number = bitwise(machine, operation, left, right, miss)?;
            machine.set_number(destination, number);
        }
        NumberOperator::Relation(operation) => {
            let holds = relation(machine, operation, left, right, miss)?;
            machine.set_boolean(destination, holds);
        }
    }
    Some(())
}

/// The interpreter's number machine: values from its register file, of which it proves
/// nothing ahead; a value it holds is a constant (`Source::Constant`).
impl<R: super::RegisterFile> Numbers for super::ConcreteFrame<R> {
    type Int = i32;
    type Float = f64;
    #[inline(always)]
    fn speculates(&self) -> bool { false }
    #[inline(always)]
    fn known_int(&self, _: Register) -> bool { false }
    #[inline(always)]
    fn known_float(&self, _: Register) -> bool { false }
    #[inline(always)]
    fn wraps(&self) -> bool { false }
    #[inline(always)]
    fn int_register(&mut self, register: Register, _: ()) -> Option<i32> { self.value(register).as_int() }
    #[inline(always)]
    fn float_register(&mut self, register: Register, _: ()) -> Option<f64> { self.value(register).as_number() }
    #[inline(always)]
    fn int_held(&mut self, register: Register) -> bool { self.value(register).as_int().is_some() }
    #[inline(always)]
    fn assume_int(&mut self, _: Register) {}
    #[inline(always)]
    fn int_constant(&mut self, value: i32) -> i32 { value }
    #[inline(always)]
    fn float_constant(&mut self, value: f64) -> f64 { value }
    #[inline(always)]
    fn cond_constant(&mut self, value: bool) -> bool { value }
    #[inline(always)]
    fn int_checked(&mut self, operation: NumberArithmetic, left: i32, right: i32) -> (i32, bool) {
        match crate::vm::operations::checked_int32_arithmetic(operation, left, right) {
            Some(result) => (result, false),
            None => (0, true),
        }
    }
    #[inline(always)]
    fn int_wrapping(&mut self, operation: NumberArithmetic, left: i32, right: i32) -> i32 {
        if operation == NumberArithmetic::Add { left.wrapping_add(right) } else { left.wrapping_sub(right) }
    }
    #[inline(always)]
    fn int_bitwise(&mut self, operation: NumberBitwise, left: i32, right: i32) -> i32 {
        crate::vm::operations::int32_bitwise(operation, left, right)
    }
    #[inline(always)]
    fn int_relation(&mut self, relation: NumberRelation, left: i32, right: i32) -> bool {
        crate::vm::operations::ordered(relation, left, right)
    }
    #[inline(always)]
    fn float_op(&mut self, operation: NumberArithmetic, left: f64, right: f64) -> f64 {
        crate::vm::operations::number_arithmetic_result(operation, left, right)
    }
    #[inline(always)]
    fn float_relation(&mut self, relation: NumberRelation, left: f64, right: f64) -> bool {
        crate::vm::operations::ordered(relation, left, right)
    }
    #[inline(always)]
    fn unsigned(&mut self, bits: i32) -> f64 { f64::from(bits as u32) }
    #[inline(always)]
    fn float_of_int(&mut self, int: i32) -> f64 { f64::from(int) }
    #[inline(always)]
    fn float_to_int32(&mut self, number: f64, _: ()) -> Option<i32> { Some(crate::value::number_to_u32(number) as i32) }
    #[inline(always)]
    fn or_cond(&mut self, a: bool, b: bool) -> bool { a || b }
    #[inline(always)]
    fn and_cond(&mut self, a: bool, b: bool) -> bool { a && b }
    #[inline(always)]
    fn not_cond(&mut self, a: bool) -> bool { !a }
    #[inline(always)]
    fn choose_number(
        &mut self,
        cond: bool,
        then: impl FnOnce(&mut Self) -> Option<Number<i32, f64>>,
        otherwise: impl FnOnce(&mut Self) -> Option<Number<i32, f64>>,
    ) -> Option<Number<i32, f64>> {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn choose_cond(
        &mut self,
        cond: bool,
        then: impl FnOnce(&mut Self) -> Option<bool>,
        otherwise: impl FnOnce(&mut Self) -> Option<bool>,
    ) -> Option<bool> {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn set_number(&mut self, register: Register, number: Number<i32, f64>) {
        let value = match number {
            Number::Int(int) => crate::Value::integer(int),
            Number::Float(number) => crate::Value::number(number),
        };
        self.registers.set(register, value.to_bits());
    }
    #[inline(always)]
    fn set_boolean(&mut self, register: Register, cond: bool) {
        self.registers.set(register, crate::Value::boolean(cond).to_bits());
    }
}

/// The register file of one binary operation's result (`binary_values`).
struct Result(crate::Value);

const RESULT: Register = 0;

impl super::RegisterFile for Result {
    #[inline(always)]
    fn get(&mut self, _: Register) -> u64 { self.0.to_bits() }
    #[inline(always)]
    fn set(&mut self, _: Register, value: u64) { self.0 = crate::Value::from_bits(value) }
}

/// `left op right` for Number operator `operator`, as the interpreter computes it on two
/// values, which it knows as the specializer knows a constant: `None` where either is not a
/// Number.
#[inline(always)]
pub(crate) fn binary_values(operator: NumberOperator, left: crate::Value, right: crate::Value) -> Option<crate::Value> {
    let mut frame = super::ConcreteFrame { stage: super::Concrete::DETACHED, registers: Result(crate::Value::UNDEFINED) };
    binary(&mut frame, operator, RESULT, Source::Constant(left), Source::Constant(right), ())?;
    Some(frame.registers.0)
}
