//! The scalar pass over a kernel's recurrences (`Recurrence`): the operations that read a
//! value the previous iteration computed, in body order, once per iteration.
//!
//! The recurrence kernels run is one register folded by one operator over a column (`s += x`,
//! `s = (s + x) | 0`), as a fused loop per operator (`Fold`). A loop with any other recurrence
//! keeps its own records: an interpreted step list cost more per operation than they do.
use super::super::super::operations::{NumberArithmetic, NumberBitwise};
use super::run::{CHUNK, arith};
use super::shape::{Arg, NodeOp, Shape};

/// One recurrence folded by one arithmetic operator, maybe truncated to int32 after
/// (`(s op x) | 0`): `s = op(s, x[k])`, or `op(x[k], s)` when the recurrence is the right operand.
#[derive(Clone, Copy)]
pub(super) struct Fold {
    op: NumberArithmetic,
    left: bool,
    operand: Arg,
    int32: bool,
    /// The columns the chain's nodes write, where a store or a final value reads them.
    outs: [Option<u16>; 2],
}

pub(super) enum Recurrence {
    None,
    Fold(Fold),
}

/// Whether node `id`'s column is read by a store or a written register's final value.
fn read_later(shape: &Shape, id: u16) -> bool {
    shape.stores.iter().any(|store| store.value == Arg::Col(id))
        || shape.temps.iter().any(|&(_, arg)| arg == Arg::Col(id))
}

impl Recurrence {
    pub(super) fn of(shape: &Shape) -> Option<Self> {
        if shape.phis.is_empty() {
            return Some(Self::None);
        }
        Self::fold(shape).map(Self::Fold)
    }

    /// The fold of a single recurrence whose chain is one arithmetic node, maybe followed by
    /// `| 0`.
    fn fold(shape: &Shape) -> Option<Fold> {
        let ([(_, Arg::Col(last))], first) = (shape.phis.as_slice(), *shape.seq.first()?) else { return None };
        let last = *last;
        let node = shape.nodes[usize::from(first)];
        let NodeOp::Arith(op) = node.op else { return None };
        let (left, operand) = match (node.a, node.b) {
            (Arg::Phi(0), other) => (true, other),
            (other, Arg::Phi(0)) => (false, other),
            _ => return None,
        };
        let column_or_scalar = |arg: Arg| match arg {
            Arg::Col(n) => !shape.nodes[usize::from(n)].seq,
            Arg::Scalar(_) => true,
            _ => false,
        };
        if !column_or_scalar(operand) {
            return None;
        }
        let int32 = match shape.seq.as_slice() {
            [only] if *only == last => false,
            [_, second] if *second == last && is_int32_truncation(shape, first, last) => true,
            _ => return None,
        };
        let outs = [first, last].map(|id| read_later(shape, id).then_some(id));
        Some(Fold { op, left, operand, int32, outs })
    }

    /// The pass over a chunk of `len` iterations: updates `phis`, writes the columns read later.
    pub(super) fn run(&self, phis: &mut [f64], scalars: &[f64], scratch: &mut [f64], len: usize) {
        match self {
            Self::None => {}
            Self::Fold(fold) => fold.run(phis, scalars, scratch, len),
        }
    }
}

/// Whether node `last` is `node(first) | 0`.
fn is_int32_truncation(shape: &Shape, first: u16, last: u16) -> bool {
    let node = shape.nodes[usize::from(last)];
    let zero = |arg: Arg| matches!(arg, Arg::Scalar(s) if shape.scalar_constant(s) == Some(0.0));
    matches!(node.op, NodeOp::Bits(NumberBitwise::Or))
        && ((node.a == Arg::Col(first) && zero(node.b)) || (node.b == Arg::Col(first) && zero(node.a)))
}

#[inline(always)]
fn to_int32(value: f64) -> f64 {
    f64::from(crate::value::number_to_u32(value) as i32)
}

impl Fold {
    fn run(&self, phis: &mut [f64], scalars: &[f64], scratch: &mut [f64], len: usize) {
        macro_rules! fold {
            ($left:expr, $int32:expr) => {
                match self.op {
                    NumberArithmetic::Add => self.fold::<$left, $int32>(phis, scalars, scratch, len, |a, b| arith(NumberArithmetic::Add, a, b)),
                    NumberArithmetic::Subtract => self.fold::<$left, $int32>(phis, scalars, scratch, len, |a, b| arith(NumberArithmetic::Subtract, a, b)),
                    NumberArithmetic::Multiply => self.fold::<$left, $int32>(phis, scalars, scratch, len, |a, b| arith(NumberArithmetic::Multiply, a, b)),
                    NumberArithmetic::Divide => self.fold::<$left, $int32>(phis, scalars, scratch, len, |a, b| arith(NumberArithmetic::Divide, a, b)),
                }
            };
        }
        match (self.left, self.int32) {
            (true, true) => fold!(true, true),
            (true, false) => fold!(true, false),
            (false, true) => fold!(false, true),
            (false, false) => fold!(false, false),
        }
    }

    #[inline(always)]
    fn fold<const LEFT: bool, const INT32: bool>(
        &self,
        phis: &mut [f64],
        scalars: &[f64],
        scratch: &mut [f64],
        len: usize,
        f: impl Fn(f64, f64) -> f64,
    ) {
        let step = |value: f64, x: f64| {
            let sum = if LEFT { f(value, x) } else { f(x, value) };
            (sum, if INT32 { to_int32(sum) } else { sum })
        };
        let mut value = phis[0];
        if self.outs == [None, None] {
            match self.operand {
                Arg::Col(n) => {
                    for &x in &scratch[usize::from(n) * CHUNK..][..len] {
                        value = step(value, x).1;
                    }
                }
                _ => {
                    let x = self.scalar(scalars);
                    (0..len).for_each(|_| value = step(value, x).1);
                }
            }
        } else {
            for k in 0..len {
                let x = match self.operand { Arg::Col(n) => scratch[usize::from(n) * CHUNK + k], _ => self.scalar(scalars) };
                let (first, last) = step(value, x);
                value = last;
                for (out, result) in self.outs.iter().zip([first, last]) {
                    if let Some(id) = out {
                        scratch[usize::from(*id) * CHUNK + k] = result;
                    }
                }
            }
        }
        phis[0] = value;
    }

    fn scalar(&self, scalars: &[f64]) -> f64 {
        match self.operand {
            Arg::Scalar(s) => scalars[usize::from(s)],
            _ => 0.0,
        }
    }
}
