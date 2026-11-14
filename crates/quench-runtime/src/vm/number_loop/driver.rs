//! The loop itself: update, safepoint, test and body as one Rust loop, monomorphic in the
//! counter's kind and direction and in the test's operator and operand forms.

use super::compile;
use super::ints::{IntConstant, IntOf};
use super::nodes::{Ctx, Int, NumNode, Stmt};
use super::plan::{Tree, i32_of};
use std::sync::atomic::{AtomicUsize, Ordering};

/// How a run ended.
pub(super) enum Exit {
    /// The test failed.
    Done,
    /// A step left at this PC.
    Leave(u32),
}

/// A planned loop.
pub(super) trait Driver {
    fn run(&self, cx: &mut Ctx, floor: &AtomicUsize) -> Exit;
}

/// The update `dst = src ± 1`: `false` where an int32 overflows.
trait Counter {
    fn step(&self, cx: &mut Ctx) -> bool;
}

struct IntStep<const DECREMENT: bool>(u16, u16);
struct DoubleStep<const DECREMENT: bool>(u16, u16);

impl<const DECREMENT: bool> Counter for IntStep<DECREMENT> {
    #[inline(always)]
    fn step(&self, cx: &mut Ctx) -> bool {
        let current = cx.slot(self.1) as i32;
        let Some(next) = (if DECREMENT { current.checked_sub(1) } else { current.checked_add(1) }) else { return false };
        cx.set(self.0, next as i64 as u64);
        true
    }
}

impl<const DECREMENT: bool> Counter for DoubleStep<DECREMENT> {
    #[inline(always)]
    fn step(&self, cx: &mut Ctx) -> bool {
        let current = f64::from_bits(cx.slot(self.1));
        cx.set(self.0, (if DECREMENT { current - 1.0 } else { current + 1.0 }).to_bits());
        true
    }
}

/// The loop's test.
trait Test {
    fn holds(&self, cx: &mut Ctx) -> bool;
}

/// A relation of two int32 operands, `OP` its operator.
struct IntTest<const OP: u32, L, R>(L, R);
/// A relation of two Numbers.
struct NumberTest(u32, Box<dyn NumNode>, Box<dyn NumNode>);

impl<const OP: u32, L: Int, R: Int> Test for IntTest<OP, L, R> {
    #[inline(always)]
    fn holds(&self, cx: &mut Ctx) -> bool {
        let (Some(left), Some(right)) = (self.0.int(cx), self.1.int(cx)) else { return false };
        crate::vm::operations::integer_relation_i32(OP, left, right).unwrap_or(false)
    }
}

impl Test for NumberTest {
    #[inline(always)]
    fn holds(&self, cx: &mut Ctx) -> bool {
        let (Some(left), Some(right)) = (self.1.eval(cx), self.2.eval(cx)) else { return false };
        crate::vm::operations::double_relation(self.0, left, right)
    }
}

struct Loop<C, T> {
    counter: C,
    test: T,
    body: Box<[Box<dyn Stmt>]>,
    update: u32,
    test_pc: u32,
}

impl<C: Counter, T: Test> Driver for Loop<C, T> {
    fn run(&self, cx: &mut Ctx, floor: &AtomicUsize) -> Exit {
        loop {
            if !self.counter.step(cx) {
                return Exit::Leave(self.update);
            }
            // The back edge's safepoint: a raised floor leaves at the test, after the update.
            if super::super::stack_pointer() < floor.load(Ordering::Relaxed) {
                return Exit::Leave(self.test_pc);
            }
            if !self.test.holds(cx) {
                return Exit::Done;
            }
            for statement in self.body.iter() {
                if statement.exec(cx) {
                    return Exit::Leave(cx.at);
                }
            }
        }
    }
}

/// What a driver is built from.
pub(super) struct Parts {
    pub(super) counter: (u16, u16),
    pub(super) int_counter: bool,
    pub(super) decrement: bool,
    pub(super) operator: u32,
    pub(super) test: (Tree, Tree),
    pub(super) body: Box<[Box<dyn Stmt>]>,
    pub(super) update: u32,
    pub(super) test_pc: u32,
}

/// The driver for `parts`.
pub(super) fn driver(parts: Parts) -> Box<dyn Driver> {
    match (parts.int_counter, parts.decrement) {
        (true, false) => with_counter(IntStep::<false>(parts.counter.0, parts.counter.1), parts),
        (true, true) => with_counter(IntStep::<true>(parts.counter.0, parts.counter.1), parts),
        (false, false) => with_counter(DoubleStep::<false>(parts.counter.0, parts.counter.1), parts),
        (false, true) => with_counter(DoubleStep::<true>(parts.counter.0, parts.counter.1), parts),
    }
}

/// An int32 test operand: a slot or a constant; `None` for any other.
enum IntLeaf {
    Slot(u16),
    Constant(i32),
}

fn int_leaf(tree: &Tree) -> Option<IntLeaf> {
    match tree {
        Tree::Int(register) => Some(IntLeaf::Slot(*register)),
        Tree::Constant(number) => i32_of(*number).map(IntLeaf::Constant),
        _ => None,
    }
}

fn with_counter<C: Counter + 'static>(counter: C, parts: Parts) -> Box<dyn Driver> {
    use oxc_ast::ast::BinaryOperator as B;
    let relations = [B::LessThan, B::LessEqualThan, B::GreaterThan, B::GreaterEqualThan];
    let relational = relations.iter().any(|&relation| relation as u32 == parts.operator);
    let (Some(left), Some(right), true) = (int_leaf(&parts.test.0), int_leaf(&parts.test.1), relational) else {
        let Parts { test: (left, right), operator, body, update, test_pc, .. } = parts;
        let test = NumberTest(operator, compile::node(left), compile::node(right));
        return Box::new(Loop { counter, test, body, update, test_pc });
    };
    match parts.operator {
        op if op == B::LessThan as u32 => with_left::<C, { B::LessThan as u32 }>(counter, left, right, parts),
        op if op == B::LessEqualThan as u32 => with_left::<C, { B::LessEqualThan as u32 }>(counter, left, right, parts),
        op if op == B::GreaterThan as u32 => with_left::<C, { B::GreaterThan as u32 }>(counter, left, right, parts),
        _ => with_left::<C, { B::GreaterEqualThan as u32 }>(counter, left, right, parts),
    }
}

fn with_left<C: Counter + 'static, const OP: u32>(counter: C, left: IntLeaf, right: IntLeaf, parts: Parts) -> Box<dyn Driver> {
    match left {
        IntLeaf::Slot(register) => with_right::<C, OP, _>(counter, IntOf(register), right, parts),
        IntLeaf::Constant(value) => with_right::<C, OP, _>(counter, IntConstant(value), right, parts),
    }
}

fn with_right<C: Counter + 'static, const OP: u32, L: Int + 'static>(counter: C, left: L, right: IntLeaf, parts: Parts) -> Box<dyn Driver> {
    let Parts { body, update, test_pc, .. } = parts;
    match right {
        IntLeaf::Slot(register) => Box::new(Loop { counter, test: IntTest::<OP, L, _>(left, IntOf(register)), body, update, test_pc }),
        IntLeaf::Constant(value) => Box::new(Loop { counter, test: IntTest::<OP, L, _>(left, IntConstant(value)), body, update, test_pc }),
    }
}
