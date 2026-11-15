use super::super::super::operations::{NumberArithmetic, NumberBitwise};
use super::shape::NodeOp;

/// One f64 operand of a column operation.
#[derive(Clone, Copy)]
pub(super) enum Column<'a> {
    Col(&'a [f64]),
    Scalar(f64),
}

#[inline(always)]
fn map2(out: &mut [f64], a: Column, b: Column, f: impl Fn(f64, f64) -> f64) {
    match (a, b) {
        (Column::Col(x), Column::Col(y)) => out.iter_mut().zip(x).zip(y).for_each(|((o, &x), &y)| *o = f(x, y)),
        (Column::Col(x), Column::Scalar(y)) => out.iter_mut().zip(x).for_each(|(o, &x)| *o = f(x, y)),
        (Column::Scalar(x), Column::Col(y)) => out.iter_mut().zip(y).for_each(|(o, &y)| *o = f(x, y)),
        (Column::Scalar(x), Column::Scalar(y)) => out.fill(f(x, y)),
    }
}

/// `op` over two column operands, one loop per operator.
pub(super) fn operate(op: NodeOp, out: &mut [f64], a: Column, b: Column) {
    macro_rules! each {
        ($($kind:ident :: $operation:ident => $f:ident),*) => {
            match op {
                $(NodeOp::$kind($kind::$operation) => map2(out, a, b, |x, y| $f($kind::$operation, x, y)),)*
                NodeOp::Load { .. } | NodeOp::Iota { .. } => unreachable!("not an operator"),
            }
        };
    }
    type Arith = NumberArithmetic;
    type Bits = NumberBitwise;
    each!(
        Arith::Add => arith, Arith::Subtract => arith, Arith::Multiply => arith, Arith::Divide => arith,
        Bits::And => bits, Bits::Or => bits, Bits::Xor => bits,
        Bits::ShiftLeft => bits, Bits::ShiftRight => bits, Bits::ShiftRightUnsigned => bits
    )
}

#[inline(always)]
pub(super) fn arith(operation: NumberArithmetic, a: f64, b: f64) -> f64 {
    match operation {
        NumberArithmetic::Add => a + b,
        NumberArithmetic::Subtract => a - b,
        NumberArithmetic::Multiply => a * b,
        NumberArithmetic::Divide => a / b,
    }
}

#[inline(always)]
fn bits(operation: NumberBitwise, a: f64, b: f64) -> f64 {
    let int32 = |value: f64| crate::value::number_to_u32(value) as i32;
    super::super::super::operations::number_bitwise_f64_result(operation, int32(a), int32(b))
}

#[inline(always)]
pub(super) fn apply(op: NodeOp, a: f64, b: f64) -> f64 {
    match op {
        NodeOp::Arith(operation) => arith(operation, a, b),
        NodeOp::Bits(operation) => bits(operation, a, b),
        NodeOp::Load { .. } | NodeOp::Iota { .. } => unreachable!("loads and iotas are column operations"),
    }
}
