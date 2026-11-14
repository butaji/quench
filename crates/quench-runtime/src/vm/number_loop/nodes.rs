//! The nodes a number loop's body compiles to: monomorphic Rust code per operator and
//! operand form, composed once per plan. An expression's value flows through machine
//! registers from operand to operator; only each instruction's own result register is
//! stored, as the general path would read it after a leave.
//!
//! An operand form is a slot, a constant, an element read by an int32 key (the leaves,
//! inlined into the operator that reads them) or a boxed subtree (one indirect call).
//! A node that leaves records the PC of its instruction and returns `None`, which every
//! operator that reads a subtree passes up.

use super::*;

/// The bits of a slot a run has not written yet: a signaling NaN no Number has, since
/// every Number a run reads or computes is a canonical or hardware quiet NaN when it is a
/// NaN, and outside every sign-extended int32.
pub(super) const UNSET: u64 = 0x7ff4_dead_0000_0000;

/// A run's machine state: unboxed slots by register, the resolved arrays, and the PC a
/// node left at.
pub(super) struct Ctx {
    pub(super) slots: [u64; SLOTS],
    pub(super) values: [*mut Value; ARRAYS],
    pub(super) lengths: [usize; ARRAYS],
    pub(super) at: u32,
}

impl Ctx {
    #[inline(always)]
    pub(super) fn slot(&self, register: u16) -> u64 {
        // SAFETY: `NumberLoop::find` admits only registers below `SLOTS`.
        unsafe { *self.slots.get_unchecked(usize::from(register)) }
    }

    #[inline(always)]
    pub(super) fn set(&mut self, register: u16, bits: u64) {
        // SAFETY: as `slot`.
        unsafe { *self.slots.get_unchecked_mut(usize::from(register)) = bits };
    }

    #[inline(always)]
    fn leave<T>(&mut self, at: u32) -> Option<T> {
        self.at = at;
        None
    }

    /// The element of array `array` at `index`, as a Number, or `None` past the length or
    /// for any other value.
    #[inline(always)]
    fn element(&self, array: u8, index: usize) -> Option<f64> {
        // SAFETY: plans name arrays below `ARRAYS` (`plan::Builder::array`).
        let (length, values) = unsafe { (*self.lengths.get_unchecked(usize::from(array)), *self.values.get_unchecked(usize::from(array))) };
        if index >= length {
            return None;
        }
        // SAFETY: below the length of values the run resolved, which nothing it does moves.
        unsafe { values.add(index).read() }.as_number()
    }
}

/// A node computing a Number, or `None` where it left.
pub(super) trait Num {
    fn num(&self, cx: &mut Ctx) -> Option<f64>;
}

/// A node computing an int32, or `None` where it left.
pub(super) trait Int {
    fn int(&self, cx: &mut Ctx) -> Option<i32>;
}

/// A body statement: `true` when it left.
pub(super) trait Stmt {
    fn exec(&self, cx: &mut Ctx) -> bool;
}

pub(super) struct DoubleSlot(pub(super) u16);
pub(super) struct IntSlot(pub(super) u16);
pub(super) struct Constant(pub(super) f64);

impl Num for DoubleSlot {
    #[inline(always)]
    fn num(&self, cx: &mut Ctx) -> Option<f64> {
        Some(f64::from_bits(cx.slot(self.0)))
    }
}

impl Num for IntSlot {
    #[inline(always)]
    fn num(&self, cx: &mut Ctx) -> Option<f64> {
        Some(f64::from(cx.slot(self.0) as i32))
    }
}

impl Num for Constant {
    #[inline(always)]
    fn num(&self, _: &mut Ctx) -> Option<f64> {
        Some(self.0)
    }
}

impl Num for Box<dyn NumNode> {
    #[inline(always)]
    fn num(&self, cx: &mut Ctx) -> Option<f64> {
        (**self).eval(cx)
    }
}

/// `Num` as a trait object: a subtree an operator reads through one call.
pub(super) trait NumNode {
    fn eval(&self, cx: &mut Ctx) -> Option<f64>;
}

impl<N: Num> NumNode for N {
    fn eval(&self, cx: &mut Ctx) -> Option<f64> {
        self.num(cx)
    }
}

/// An array key: an index below 2^31, or `usize::MAX` for any other key; `None` where
/// computing it left.
pub(super) trait Key {
    fn index(&self, cx: &mut Ctx) -> Option<usize>;
}

pub(super) struct IntKey(pub(super) u16);
pub(super) struct DoubleKey(pub(super) u16);
pub(super) struct ConstantKey(pub(super) usize);

impl Key for IntKey {
    #[inline(always)]
    fn index(&self, cx: &mut Ctx) -> Option<usize> {
        // A negative int32 widens past every length.
        Some(cx.slot(self.0) as i32 as isize as usize)
    }
}

impl Key for DoubleKey {
    #[inline(always)]
    fn index(&self, cx: &mut Ctx) -> Option<usize> {
        let number = f64::from_bits(cx.slot(self.0));
        let index = number as i32;
        Some(if index >= 0 && f64::from(index) == number { index as usize } else { usize::MAX })
    }
}

impl Key for ConstantKey {
    #[inline(always)]
    fn index(&self, _: &mut Ctx) -> Option<usize> {
        Some(self.0)
    }
}

/// `++register` or `--register` of an int32 slot, then its value as the key: an overflow
/// leaves at the update, before it writes.
pub(super) struct StepKey<const DECREMENT: bool> {
    pub(super) register: u16,
    pub(super) at: u32,
}

impl<const DECREMENT: bool> Key for StepKey<DECREMENT> {
    #[inline(always)]
    fn index(&self, cx: &mut Ctx) -> Option<usize> {
        let stepped = step_int::<DECREMENT>(cx, self.register, self.register, self.at)?;
        Some(stepped as isize as usize)
    }
}

/// `dst = src ± 1` of an int32 slot; `None`, having left at `at`, where it overflows.
#[inline(always)]
pub(super) fn step_int<const DECREMENT: bool>(cx: &mut Ctx, dst: u16, src: u16, at: u32) -> Option<i32> {
    let current = cx.slot(src) as i32;
    let Some(next) = (if DECREMENT { current.checked_sub(1) } else { current.checked_add(1) }) else { return cx.leave(at) };
    cx.set(dst, next as i64 as u64);
    Some(next)
}

/// `dst = src ± 1` of a double slot.
pub(super) struct DoubleStep<const DECREMENT: bool> {
    pub(super) dst: u16,
    pub(super) src: u16,
}

impl<const DECREMENT: bool> Num for DoubleStep<DECREMENT> {
    #[inline(always)]
    fn num(&self, cx: &mut Ctx) -> Option<f64> {
        let current = f64::from_bits(cx.slot(self.src));
        let next = if DECREMENT { current - 1.0 } else { current + 1.0 };
        cx.set(self.dst, next.to_bits());
        Some(next)
    }
}

/// `dst = array[key]` of a Number element; any other element, or a key past the length,
/// leaves at the read.
pub(super) struct Read<K> {
    pub(super) dst: u16,
    pub(super) array: u8,
    pub(super) key: K,
    pub(super) at: u32,
}

impl<K: Key> Num for Read<K> {
    #[inline(always)]
    fn num(&self, cx: &mut Ctx) -> Option<f64> {
        let index = self.key.index(cx)?;
        let Some(number) = cx.element(self.array, index) else { return cx.leave(self.at) };
        cx.set(self.dst, number.to_bits());
        Some(number)
    }
}

/// The four arithmetic operators a plan inlines, by code.
pub(super) const ADD: u8 = 0;
pub(super) const SUB: u8 = 1;
pub(super) const MUL: u8 = 2;
pub(super) const DIV: u8 = 3;

/// `dst = left OP right` on doubles.
pub(super) struct Arith<const OP: u8, L, R> {
    pub(super) dst: u16,
    pub(super) left: L,
    pub(super) right: R,
}

impl<const OP: u8, L: Num, R: Num> Num for Arith<OP, L, R> {
    #[inline(always)]
    fn num(&self, cx: &mut Ctx) -> Option<f64> {
        let left = self.left.num(cx)?;
        let right = self.right.num(cx)?;
        let value = match OP {
            ADD => left + right,
            SUB => left - right,
            MUL => left * right,
            _ => left / right,
        };
        cx.set(self.dst, value.to_bits());
        Some(value)
    }
}

/// `dst = left operator right` for any other numeric operator, by its own semantics.
pub(super) struct Numeric {
    pub(super) dst: u16,
    pub(super) operator: BinaryOperator,
    pub(super) left: Box<dyn NumNode>,
    pub(super) right: Box<dyn NumNode>,
}

impl Num for Numeric {
    fn num(&self, cx: &mut Ctx) -> Option<f64> {
        let left = self.left.num(cx)?;
        let right = self.right.num(cx)?;
        let value = crate::vm::operations::number_operator_result(self.operator, left, right);
        cx.set(self.dst, value.to_bits());
        Some(value)
    }
}

/// `dst = src` into a double slot.
pub(super) struct MoveDouble<N> {
    pub(super) dst: u16,
    pub(super) src: N,
}

impl<N: Num> Num for MoveDouble<N> {
    #[inline(always)]
    fn num(&self, cx: &mut Ctx) -> Option<f64> {
        let value = self.src.num(cx)?;
        cx.set(self.dst, value.to_bits());
        Some(value)
    }
}

/// A Number node run as a statement.
pub(super) struct NumStmt<N>(pub(super) N);

impl<N: Num> Stmt for NumStmt<N> {
    fn exec(&self, cx: &mut Ctx) -> bool {
        self.0.num(cx).is_none()
    }
}

/// An int32 node run as a statement.
pub(super) struct IntStmt<I>(pub(super) I);

impl<I: Int> Stmt for IntStmt<I> {
    fn exec(&self, cx: &mut Ctx) -> bool {
        self.0.int(cx).is_none()
    }
}

/// `array[key] = value` of a Number over a Number element, in place: a hole, another
/// element or a key past the length leaves before anything is written.
pub(super) struct Write<K, N> {
    pub(super) array: u8,
    pub(super) key: K,
    pub(super) value: N,
    /// The stored register's slot is an int32 (`Value::integer`), else a double.
    pub(super) int: bool,
    pub(super) at: u32,
}

impl<K: Key, N: Num> Stmt for Write<K, N> {
    fn exec(&self, cx: &mut Ctx) -> bool {
        let Some(value) = self.value.num(cx) else { return true };
        let Some(index) = self.key.index(cx) else { return true };
        if cx.element(self.array, index).is_none() {
            cx.at = self.at;
            return true;
        }
        let stored = if self.int { Value::integer(value as i32) } else { Value::number(value) };
        let values = cx.values[usize::from(self.array)];
        // SAFETY: the values of a backing the array owns alone (`unique_values`), `index`
        // below its length (`element`); a Number over a Number logs nothing.
        unsafe { crate::heap::ArrayElements::replace_through(std::ptr::NonNull::new_unchecked(values), index, stored) };
        false
    }
}
