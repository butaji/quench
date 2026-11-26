//! Running a loop kernel: entry facts, the trip count, chunks of columns, the scalar pass
//! over recurrences, the chunk's stores, and the registers written back.
#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};
use super::shape::{Arg, NodeOp, Scalar, Stride};
use super::typed;
use quench_runtime::tier_surface::TypedArrayKind;
use super::*;

/// Iterations per chunk: each column holds this many f64s.
pub(crate) const CHUNK: usize = 256;
use crate::compiled::tier_policy::KERNEL_MIN_TRIP;
/// The shortest chunk a load/store distance may impose.
const MIN_CHUNK: usize = 16;
/// Most distinct base arrays a kernel reads or writes.
const MAX_BASES: usize = 8;
/// Slots of scalars, affine values and recurrences a run keeps on the stack.
const SLOTS: usize = 32;

/// A base array's storage for the run (`Heap::array_storage_span`).
#[derive(Clone, Copy)]
struct Span {
    pub base: Value,
    /// The first element: a `Value` of a record array, or of a typed array's kind.
    pub values: *mut Value,
    pub readable: usize,
    pub writable: usize,
    /// A typed array's element kind, and its buffer's bytes (which other views may share).
    pub typed: Option<(TypedArrayKind, (usize, usize))>,
}

const NO_SPAN: Span = Span { base: Value::UNDEFINED, values: std::ptr::null_mut(), readable: 0, writable: 0, typed: None };

/// A run's state: the entry facts and the recurrences' current values.
pub(crate) struct State {
    pub scalars: [f64; SLOTS],
    /// Each affine value's index at the run's first iteration.
    pub firsts: [i64; SLOTS],
    /// Each affine value's step per iteration.
    pub steps: [i64; SLOTS],
    pub spans: [Span; MAX_BASES],
    pub phis: [f64; SLOTS],
    /// The written registers' values at the last committed iteration: recurrences, then temps.
    pub finals: [f64; 2 * SLOTS],
    pub trip: u64,
    pub limit: u64,
    pub chunk: usize,
}

#[quench_macros::ext(StateOps1)]
impl State {
    pub(crate) const EMPTY: Self = Self {
        scalars: [0.0; SLOTS],
        firsts: [0; SLOTS],
        steps: [0; SLOTS],
        spans: [NO_SPAN; MAX_BASES],
        phis: [0.0; SLOTS],
        finals: [0.0; 2 * SLOTS],
        trip: 0,
        limit: 0,
        chunk: CHUNK,
    };
}

/// One f64 operand of a column operation.
#[derive(Clone, Copy)]
enum Column<'a> {
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
fn operate(op: NodeOp, out: &mut [f64], a: Column, b: Column) {
    if let NodeOp::Bits(operation) = op {
        if bits_column(operation, out, a, b) {
            return;
        }
    }
    macro_rules! each {
        ($($kind:ident :: $operation:ident => $f:ident),*) => {
            match op {
                $(NodeOp::$kind($kind::$operation) => map2(out, a, b, |x, y| $f($kind::$operation, x, y)),)*
                // One loop per relation, its test folded into the loop.
                NodeOp::Cmp(relation) => match relation {
                    NumberRelation::LessThan => map2(out, a, b, |x, y| compare(NumberRelation::LessThan, x, y)),
                    NumberRelation::LessEqual => map2(out, a, b, |x, y| compare(NumberRelation::LessEqual, x, y)),
                    NumberRelation::GreaterThan => map2(out, a, b, |x, y| compare(NumberRelation::GreaterThan, x, y)),
                    NumberRelation::GreaterEqual => map2(out, a, b, |x, y| compare(NumberRelation::GreaterEqual, x, y)),
                    NumberRelation::Equal => map2(out, a, b, |x, y| compare(NumberRelation::Equal, x, y)),
                    NumberRelation::NotEqual => map2(out, a, b, |x, y| compare(NumberRelation::NotEqual, x, y)),
                },
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

/// The int32 words of a column operand into `words` (a scalar's once): `false` where one is no
/// int32 Number (the per-element row then converts it). Branch-free, so the loop vectorizes.
fn column_words<'a>(column: Column, words: &'a mut [i32]) -> Option<Result<i32, &'a [i32]>> {
    const MAGIC: f64 = 6755399441055744.0;
    const LIMIT: f64 = 2251799813685248.0;
    match column {
        Column::Scalar(x) => exact_int32(x).map(Ok),
        Column::Col(xs) => {
            // A column whose first element is no int32 (a quotient) rarely has only int32s: the
            // per-element row takes it without a wasted pass.
            if xs.first().is_some_and(|&x| exact_int32(x).is_none()) {
                return None;
            }
            let mut exact = true;
            for (o, &x) in words.iter_mut().zip(xs) {
                let shifted = x + MAGIC;
                *o = shifted.to_bits() as u32 as i32;
                exact &= (shifted - MAGIC == x) & (x.abs() < LIMIT);
            }
            exact.then_some(Err(&words[..xs.len()]))
        }
    }
}

/// A bitwise column whose operands are int32 Numbers, on int32 words: `false` (nothing written)
/// where an operand element is another Number.
fn bits_column(operation: NumberBitwise, out: &mut [f64], a: Column, b: Column) -> bool {
    let (mut left, mut right) = ([0i32; CHUNK], [0i32; CHUNK]);
    let len = out.len();
    let Some(a) = column_words(a, &mut left[..len]) else { return false };
    let Some(b) = column_words(b, &mut right[..len]) else { return false };
    let unsigned = operation == NumberBitwise::ShiftRightUnsigned;
    macro_rules! words {
        ($f:expr) => {{
            let f = $f;
            match (a, b) {
                (Err(l), Err(r)) => out.iter_mut().zip(l).zip(r).for_each(|((o, &l), &r)| *o = f(l, r)),
                (Err(l), Ok(r)) => out.iter_mut().zip(l).for_each(|(o, &l)| *o = f(l, r)),
                (Ok(l), Err(r)) => out.iter_mut().zip(r).for_each(|(o, &r)| *o = f(l, r)),
                (Ok(l), Ok(r)) => out.fill(f(l, r)),
            }
        }};
    }
    match operation {
        NumberBitwise::And => words!(|l: i32, r: i32| f64::from(l & r)),
        NumberBitwise::Or => words!(|l: i32, r: i32| f64::from(l | r)),
        NumberBitwise::Xor => words!(|l: i32, r: i32| f64::from(l ^ r)),
        _ if unsigned => words!(|l: i32, r: i32| f64::from(quench_runtime::vm::operations::int32_bitwise_bits(operation, l, r) as u32)),
        _ => words!(|l: i32, r: i32| f64::from(quench_runtime::vm::operations::int32_bitwise_bits(operation, l, r))),
    }
    true
}

/// `operation` of two Numbers as the staged Number row computes it: the interpreter's stage
/// (`Concrete`) runs `NumericStage::float_arithmetic` on the binary64 words.
#[inline(always)]
pub(crate) fn arith(operation: NumberArithmetic, a: f64, b: f64) -> f64 {
    let mut stage = quench_runtime::staged::Concrete::new(std::ptr::null());
    f64::from_bits(quench_runtime::staged::NumericStage::float_arithmetic(&mut stage, operation, a.to_bits(), b.to_bits()))
}

/// `operation` of two Numbers as the staged bitwise row computes it: both operands' ToInt32 (the
/// interpreter's stage), the int32 bitwise word, then its Number (unsigned for a zero-fill shift).
#[inline(always)]
fn bits(operation: NumberBitwise, a: f64, b: f64) -> f64 {
    use quench_runtime::staged::NumericStage;
    // Two int32s (almost every bitwise operand: an int32 constant, an induction, another bitwise
    // result) are their own ToInt32: the operation runs on them as is.
    if let (Some(left), Some(right)) = (exact_int32(a), exact_int32(b)) {
        let word = quench_runtime::vm::operations::int32_bitwise_bits(operation, left, right);
        // A zero-fill shift's Number is its unsigned bits.
        return if operation == NumberBitwise::ShiftRightUnsigned { f64::from(word as u32) } else { f64::from(word) };
    }
    let mut stage = quench_runtime::staged::Concrete::new(std::ptr::null());
    let left = NumericStage::to_int32(&mut stage, a.to_bits(), ()).expect("the interpreter stage converts every Number");
    let right = NumericStage::to_int32(&mut stage, b.to_bits(), ()).expect("the interpreter stage converts every Number");
    let word = NumericStage::int_bitwise(&mut stage, operation, left, right);
    f64::from_bits(if operation == NumberBitwise::ShiftRightUnsigned {
        NumericStage::u32_to_f64(&mut stage, word)
    } else {
        NumericStage::i32_to_f64(&mut stage, word)
    })
}

/// The int32 a Number that is a safe integer wraps to (its ToInt32), `None` for any other Number
/// (`operations::safe_integer`, the one rule; `-0` as `0`).
#[inline(always)]
pub(crate) fn exact_int32(x: f64) -> Option<i32> {
    quench_runtime::vm::operations::safe_integer(x).map(|integer| integer as i32)
}

/// `relation` of two Numbers as 1 or 0, as the staged relation row decides it (a NaN operand
/// fails every relation but `!=`).
#[inline(always)]
fn compare(relation: NumberRelation, a: f64, b: f64) -> f64 {
    let mut stage = quench_runtime::vm::operations::PartialOrderStage;
    if relation.apply(&mut stage, a, b) { 1.0 } else { 0.0 }
}

#[inline(always)]
pub(crate) fn apply(op: NodeOp, a: f64, b: f64) -> f64 {
    match op {
        NodeOp::Arith(operation) => arith(operation, a, b),
        NodeOp::Bits(operation) => bits(operation, a, b),
        NodeOp::Cmp(relation) => compare(relation, a, b),
        NodeOp::Load { .. } | NodeOp::Iota { .. } => unreachable!("loads and iotas are column operations"),
    }
}

/// The trip count of `counter REL bound` from `start`, stepping by `step`: how many
/// iterations' tests hold before the first that fails (unbounded when the step moves the
/// counter away from failing; the accesses' bounds then limit the run).
fn trip_count(start: f64, step: f64, relation: NumberRelation, bound: f64) -> u64 {
    let holds = |value: f64| match relation {
        NumberRelation::LessThan => value < bound,
        NumberRelation::LessEqual => value <= bound,
        NumberRelation::GreaterThan => value > bound,
        NumberRelation::GreaterEqual => value >= bound,
        NumberRelation::Equal | NumberRelation::NotEqual => false,
    };
    if !holds(start) {
        return 0;
    }
    let toward = matches!(relation, NumberRelation::LessThan | NumberRelation::LessEqual) == (step > 0.0);
    if !toward {
        return u64::MAX;
    }
    // An estimate from the distance, then exact steps around it.
    let k = ((bound - start) / step).floor().clamp(0.0, 1e15) as u64;
    // The estimate or the one before it is the last iteration whose test holds, but for
    // rounding: test those two first, and step only past them (the compiler vectorized the
    // steps, which made every entry pay for a long walk).
    let at = |k: u64| holds(start + step * k as f64);
    if !at(k + 1) {
        if at(k) {
            return k + 1;
        }
        if k > 0 && at(k - 1) {
            return k;
        }
    }
    trip_walk(k, at)
}

/// `trip_count`'s exact steps from the estimate `k`, where neither it nor the one before it is
/// the last iteration whose test holds (`at`).
#[cold]
#[inline(never)]
fn trip_walk(mut k: u64, at: impl Fn(u64) -> bool) -> u64 {
    while k > 0 && !at(k) {
        k -= 1;
    }
    while at(k + 1) {
        k += 1;
    }
    k + 1
}

/// The iterations an access at the affine index `first + step * k` stays below `length` for.
fn fits(first: i64, step: i64, length: usize) -> u64 {
    let length = length as i64;
    if first < 0 || first >= length {
        0
    } else if step == 0 {
        u64::MAX
    } else if step > 0 {
        ((length - 1 - first) / step + 1) as u64
    } else {
        (first / -step + 1) as u64
    }
}

/// `State::chunk` of an entry `enter` declined and counted itself (a short trip).
const DECLINE_COUNTED: usize = usize::MAX;

/// Work-clock units (typical-host nanoseconds) charged per kernel iteration.
const WORK_PER_ITERATION: u64 = 4;

#[quench_macros::ext(LoopKernelOps2)]
impl LoopKernel {
    /// Runs the loop from the registers' current state (module documentation): whether it ran any iteration.
    pub(crate) unsafe fn run(&self, cx: *mut LaneContext, r: Registers, ip: Ip) -> bool {
        // SAFETY: the kernel's state, used by one run at a time (the body calls nothing).
        let state: &mut State = unsafe { &mut **(*self.state.get()).get_or_insert_with(|| Box::new(State::EMPTY)) };
        let Some(()) = self.enter(cx, r, state) else {
            // Every declined entry counts, whatever declined it: a loop whose kernel never enters
            // stands down (`KERNEL_SHORT_DECLINES`) rather than trying again at each iteration's head.
            if state.chunk != DECLINE_COUNTED {
                self.declines.set(self.declines.get() + 1);
            }
            if quench_runtime::trace::Channel::LoopKernel.on() {
                eprintln!("loop-kernel: entry declined ({} declines)", self.declines.get());
            }
            return false;
        };
        // SAFETY: the kernel's scratch, used by one run at a time.
        let scratch = unsafe { &mut *self.scratch.get() };
        let columns = self.recurrence.columns(&self.shape) * CHUNK;
        if scratch.len() < columns {
            scratch.resize(columns, 0.0);
        }
        let mut done = 0u64;
        while done < state.limit {
            let len = (state.limit - done).min(state.chunk as u64) as usize;
            if !self.chunk(state, scratch, done, len) {
                // A chunk met a value the kernel does not compute: the loop's own records
                // run from its first iteration, now and at every later entry.
                if quench_runtime::trace::Channel::LoopKernel.on() {
                    eprintln!("loop-kernel: a chunk met a value the kernel does not compute; standing down");
                }
                self.stand_down(ip);
                break;
            }
            done += len as u64;
            if script_below_floor(cx) {
                break;
            }
        }
        if done == 0 { return false; }
        // The work clock counts the iterations the kernel ran in place of the records.
        quench_runtime::clock::charge_work(done * WORK_PER_ITERATION);
        self.write_back(state, r, done);
        true
    }

    /// The run's entry facts, or `None` where the kernel runs nothing.
    /// Every slot the shape uses is written here or by the run before it is read; the rest
    /// keep what an earlier run left and are never read.
    fn enter(&self, cx: *mut LaneContext, r: Registers, state: &mut State) -> Option<()> {
        let shape = &self.shape;
        state.chunk = CHUNK;
        for (slot, affine) in shape.affines.iter().enumerate() {
            state.firsts[slot] = i64::from(r.get(affine.register).as_int()?) + i64::from(affine.offset);
            state.steps[slot] = stride(affine.step, r)?;
        }
        // Every induction steps an int32 by an int32 (`i += 1`, `i -= h`), including one no
        // element access reads (`s += h` over a string is no induction, and declines here).
        for &(register, step) in &shape.inductions {
            r.get(register).as_int()?;
            stride(step, r)?;
        }
        let start = state.firsts[usize::from(shape.counter)];
        self.scalars(cx, state, r)?;
        let step = state.steps[usize::from(shape.counter)];
        state.trip = trip_count(start as f64, step as f64, shape.relation, state.scalars[usize::from(shape.bound)]);
        if state.trip < KERNEL_MIN_TRIP {
            self.declines.set(self.declines.get() + 1);
            state.chunk = DECLINE_COUNTED;
            return None;
        }
        for (slot, &(register, _)) in shape.phis.iter().enumerate() {
            state.phis[slot] = r.get(register).as_number()?;
        }
        // SAFETY: the running VM; the spans stay valid while the run calls and allocates nothing.
        let vm = unsafe { vm(cx) };
        for (slot, &register) in shape.bases.iter().enumerate() {
            state.spans[slot] = span(vm, r.get(register))?;
        }
        state.limit = self.limit(state)?;
        state.chunk = self.chunk_length(state)?;
        Some(())
    }

    /// The scalars' values at entry: constants, unwritten registers, and the loop-invariant
    /// arithmetic over them.
    fn scalars(&self, cx: *mut LaneContext, state: &mut State, r: Registers) -> Option<()> {
        for (slot, scalar) in self.shape.scalars.iter().enumerate() {
            state.scalars[slot] = match *scalar {
                Scalar::Const(value) => value,
                Scalar::Register(register) => r.get(register).as_number()?,
                Scalar::Capture(imm) => {
                    let (depth, slot) = (ImmediateLayout::capture_depth(imm), ImmediateLayout::capture_slot(imm));
                    super::super::capture_read_value(cx, depth, slot)?.as_number()?
                }
                Scalar::Length { register, atom } => {
                    // SAFETY: the running VM.
                    let vm = unsafe { vm(cx) };
                    let base = r.get(register);
                    // Another name is an own data field of a plain object, read once: the body calls
                    // nothing and stores only elements, so the field holds for the whole run.
                    let length = if atom == vm.length_atom {
                        super::super::own_length(vm, base).or_else(|| vm.typed_array_geometry_read(base, atom))
                    } else {
                        None
                    };
                    match length {
                        Some(length) => length.as_number()?,
                        None => {
                            let cache = vm.own_data_field_hint::<false>(base, atom)?;
                            vm.record_field_entry_hit(cache, base)?.as_number()?
                        }
                    }
                }
                Scalar::Computed { op, a, b } => apply(op, state.scalars[usize::from(a)], state.scalars[usize::from(b)]),
            };
        }
        Some(())
    }

    /// The iterations every access stays in bounds for, at most the trip count.
    fn limit(&self, state: &State) -> Option<u64> {
        let shape = &self.shape;
        let mut limit = state.trip;
        for node in &shape.nodes {
            if let NodeOp::Load { base, index } = node.op {
                let step = state.steps[usize::from(index)];
                limit = limit.min(fits(state.firsts[usize::from(index)], step, state.spans[usize::from(base)].readable));
            }
        }
        for store in &shape.stores {
            let step = state.steps[usize::from(store.index)];
            limit = limit.min(fits(state.firsts[usize::from(store.index)], step, state.spans[usize::from(store.base)].writable));
        }
        (limit >= KERNEL_MIN_TRIP).then_some(limit)
    }

    /// The chunk length that keeps every load of a stored array from reading an element an
    /// earlier iteration of the chunk (or the same one, earlier in the body) stores.
    fn chunk_length(&self, state: &State) -> Option<usize> {
        let shape = &self.shape;
        let mut chunk = CHUNK;
        for (n, store) in shape.stores.iter().enumerate() {
            let stored = state.spans[usize::from(store.base)].base;
            // Two stores into one array through two registers would commit out of order.
            // Stores through one register were proved disjoint (`disjoint_stores`).
            if shape.stores[..n].iter().any(|other| other.base != store.base && state.spans[usize::from(other.base)].base == stored) {
                return None;
            }
            // A typed array's buffer may be another view's: only its own view reads it.
            if let Some((_, bytes)) = state.spans[usize::from(store.base)].typed {
                let shared = |span: &Span| span.base != stored && span.typed.is_some_and(|(_, other)| overlaps(bytes, other));
                if state.spans[..shape.bases.len()].iter().any(shared) {
                    return None;
                }
            }
            let step = state.steps[usize::from(store.index)];
            for &(node, order) in &shape.loads {
                let NodeOp::Load { base, index } = shape.nodes[usize::from(node)].op else { continue };
                if state.spans[usize::from(base)].base != stored {
                    continue;
                }
                if state.steps[usize::from(index)] != step {
                    return None;
                }
                let distance = state.firsts[usize::from(index)] - state.firsts[usize::from(store.index)];
                chunk = chunk.min(hazard(distance, step, order < store.order)?);
            }
        }
        Some(chunk)
    }

    /// One chunk of `len` iterations from iteration `done`; `false` where it committed nothing.
    fn chunk(&self, state: &mut State, scratch: &mut [f64], done: u64, len: usize) -> bool {
        if !self.columns(state, scratch, done, len) {
            return false;
        }
        let phis = self.shape.phis.len();
        let iotas = super::recur::Iotas { firsts: &state.firsts, steps: &state.steps, done };
        self.recurrence.run(&mut state.phis[..phis], &state.scalars, scratch, len, iotas);
        for &id in self.recurrence.side() {
            self.operator_column(state, scratch, usize::from(id), len);
        }
        if !self.stores_present(state, done, len) {
            return false;
        }
        self.commit(state, scratch, done, len);
        self.finals(state, scratch, done, len);
        true
    }

    /// The value `arg` has at the chunk's iteration `k` (`done + k` of the run), after the
    /// scalar pass wrote its columns.
    #[inline(always)]
    fn value(&self, state: &State, scratch: &[f64], arg: Arg, k: usize, done: u64) -> f64 {
        match arg {
            Arg::Col(node) => scratch[usize::from(node) * CHUNK + k],
            Arg::Phi(slot) => state.phis[usize::from(slot)],
            Arg::Scalar(slot) => state.scalars[usize::from(slot)],
            Arg::Affine(slot) => self.index(state, slot, done + k as u64) as f64,
        }
    }

    #[inline(always)]
    fn index(&self, state: &State, slot: u8, k: u64) -> i64 {
        state.firsts[usize::from(slot)] + state.steps[usize::from(slot)] * k as i64
    }

    /// The column operations: the loads, then the operators in node order (operands precede
    /// their users); `false` where a loaded element is no Number.
    fn columns(&self, state: &State, scratch: &mut [f64], done: u64, len: usize) -> bool {
        let mut numbers = true;
        for (id, node) in self.shape.nodes.iter().enumerate() {
            match node.op {
                NodeOp::Load { base, index } => {
                    numbers &= self.load(state, &mut scratch[id * CHUNK..][..len], base, index, done);
                }
                NodeOp::Iota { .. } if node.dead || self.recurrence.unread_column(id) => {}
                NodeOp::Iota { index } => scratch[id * CHUNK..][..len]
                    .iter_mut()
                    .enumerate()
                    .for_each(|(k, o)| *o = self.index(state, index, done + k as u64) as f64),
                NodeOp::Arith(_) | NodeOp::Bits(_) | NodeOp::Cmp(_) => {}
            }
        }
        for (id, node) in self.shape.nodes.iter().enumerate() {
            if matches!(node.op, NodeOp::Arith(_) | NodeOp::Bits(_) | NodeOp::Cmp(_)) && !node.seq && !node.hoisted && !node.dead
                && !self.recurrence.unread_column(id)
            {
                self.operator_column(state, scratch, id, len);
            }
        }
        numbers
    }

    /// Node `id`'s operator over the columns and scalars it reads. A recurrence it reads is
    /// a column `depth` iterations late (`lagged`): before that, the values at entry of the
    /// recurrences it was copied through.
    fn operator_column(&self, state: &State, scratch: &mut [f64], id: usize, len: usize) {
        let node = self.shape.nodes[id];
        let base = scratch.as_mut_ptr();
        // SAFETY: a column `n` of the node's operands (or a lagged recurrence's) is another
        // node's, so it does not overlap the node's own; both lie inside `scratch`.
        let read = |n: usize, from: usize, to: usize| unsafe { std::slice::from_raw_parts(base.add(n * CHUNK + from), to - from) };
        let column = |arg: Arg, from: usize, to: usize| match arg {
            Arg::Col(n) => Column::Col(read(usize::from(n), from, to)),
            Arg::Scalar(slot) => Column::Scalar(state.scalars[usize::from(slot)]),
            Arg::Phi(p) => match self.lagged(p) {
                (_, depth) if from < depth => Column::Scalar(state.phis[self.lagged_entry(p, from)]),
                (n, depth) => Column::Col(read(n, from - depth, to - depth)),
            },
            Arg::Affine(_) => unreachable!("a column operation reads no affine value"),
        };
        let depth = |arg: Arg| if let Arg::Phi(p) = arg { self.lagged(p).1 } else { 0 };
        let head = depth(node.a).max(depth(node.b)).min(len);
        // SAFETY: as above.
        let out = unsafe { std::slice::from_raw_parts_mut(base.add(id * CHUNK), len) };
        for k in 0..head {
            operate(node.op, &mut out[k..k + 1], column(node.a, k, k + 1), column(node.b, k, k + 1));
        }
        operate(node.op, &mut out[head..], column(node.a, head, len), column(node.b, head, len));
    }

    /// A lagged recurrence's column and how many iterations late it reads it.
    fn lagged(&self, p: u8) -> (usize, usize) {
        let (mut at, mut depth) = (p, 1);
        loop {
            match self.shape.phis[usize::from(at)].1 {
                Arg::Col(n) => return (usize::from(n), depth),
                Arg::Phi(q) => (at, depth) = (q, depth + 1),
                _ => unreachable!("a lagged recurrence ends as a column"),
            }
        }
    }

    /// The recurrence whose value at entry a lagged recurrence `p` has at iteration `k`.
    fn lagged_entry(&self, p: u8, k: usize) -> usize {
        let mut at = p;
        for _ in 0..k {
            let Arg::Phi(q) = self.shape.phis[usize::from(at)].1 else { unreachable!("within the lag") };
            at = q;
        }
        usize::from(at)
    }

    /// Decodes `bases[base][index + step * k]` into `out`; `false` where one is no Number.
    fn load(&self, state: &State, out: &mut [f64], base: u8, index: u8, done: u64) -> bool {
        let span = state.spans[usize::from(base)];
        let values = span.values;
        let first = self.index(state, index, done);
        let step = state.steps[usize::from(index)];
        if let Some((kind, _)) = span.typed {
            // SAFETY: `limit` keeps every index of the run below the span's readable length.
            unsafe { typed::load(kind, values.cast(), first, step, out) };
            return true;
        }
        if step == 1 {
            // SAFETY: as below, for the chunk's contiguous run of elements.
            let elements = unsafe { std::slice::from_raw_parts(values.add(first as usize), out.len()) };
            return decode(out, elements);
        }
        let mut numbers = true;
        for (k, o) in out.iter_mut().enumerate() {
            // SAFETY: `limit` keeps every index of the run below the span's readable length.
            let value = unsafe { *values.add((first + step * k as i64) as usize) };
            let number = value.as_number();
            numbers &= number.is_some();
            *o = number.unwrap_or(0.0);
        }
        numbers
    }

    /// Whether every element the chunk's stores replace is present.
    fn stores_present(&self, state: &State, done: u64, len: usize) -> bool {
        self.shape.stores.iter().all(|store| {
            let span = state.spans[usize::from(store.base)];
            if span.typed.is_some() {
                return true;
            }
            let values = span.values;
            let first = self.index(state, store.index, done);
            let step = state.steps[usize::from(store.index)];
            let mut deleted = 0u64;
            for k in 0..len as i64 {
                // SAFETY: `limit` keeps every index below the span's writable length.
                deleted |= u64::from(unsafe { *values.add((first + step * k) as usize) }.is_deleted());
            }
            deleted == 0
        })
    }

    /// The chunk's stores, as `ArrayStorageProofs::replace` makes them.
    fn commit(&self, state: &State, scratch: &[f64], done: u64, len: usize) {
        for store in &self.shape.stores {
            let span = state.spans[usize::from(store.base)];
            let values = span.values;
            let first = self.index(state, store.index, done) as isize;
            let step = state.steps[usize::from(store.index)] as isize;
            if let Some((kind, _)) = span.typed {
                let numbers = (0..len).map(|k| self.value(state, scratch, store.value, k, done));
                // SAFETY: `limit` keeps every index below the span's writable length.
                unsafe { typed::store(kind, values.cast(), first, step, numbers) };
                continue;
            }
            // SAFETY: as in `stores_present`; a Number needs no write barrier.
            unsafe {
                match store.value {
                    Arg::Col(node) if step == 1 => {
                        let mut bits = [0u64; CHUNK];
                        let column = &scratch[usize::from(node) * CHUNK..][..len];
                        for (bits, &x) in bits.iter_mut().zip(column) {
                            *bits = Value::number_unpredictable(x).to_bits();
                        }
                        quench_runtime::staged::elements::concrete_replace_run(values, first as usize, &bits[..len])
                    }
                    Arg::Col(node) => {
                        let column = scratch[usize::from(node) * CHUNK..][..len].iter().map(|&x| Value::number_unpredictable(x));
                        quench_runtime::staged::elements::concrete_replace_elements(values, first, step, column)
                    }
                    value => {
                        let numbers = (0..len).map(|k| Value::number_unpredictable(self.value(state, scratch, value, k, done)));
                        quench_runtime::staged::elements::concrete_replace_elements(values, first, step, numbers)
                    }
                }
            };
        }
    }

    /// The written registers' values at the chunk's last iteration.
    fn finals(&self, state: &mut State, scratch: &[f64], done: u64, len: usize) {
        let shape = &self.shape;
        for slot in 0..shape.phis.len() {
            state.finals[slot] = state.phis[slot];
        }
        for (slot, &(_, arg)) in shape.temps.iter().enumerate() {
            state.finals[shape.phis.len() + slot] = self.value(state, scratch, arg, len - 1, done);
        }
    }

    /// Writes the registers as `done` iterations of the loop leave them.
    fn write_back(&self, state: &State, r: Registers, done: u64) {
        let shape = &self.shape;
        for &(register, step) in &shape.inductions {
            let entry = r.get(register).as_int().map_or(0, i64::from);
            let step = stride(step, r).unwrap_or(0);
            r.set(register, Value::number((entry + step * done as i64) as f64));
        }
        for (slot, &(register, _)) in shape.phis.iter().chain(&shape.temps).enumerate() {
            r.set(register, Value::number(state.finals[slot]));
        }
    }
}

/// An affine value's step at entry; `None` for a register step that is no nonzero int32.
fn stride(step: Stride, r: Registers) -> Option<i64> {
    match step {
        Stride::Const(step) => Some(i64::from(step)),
        Stride::Register(register, negative) => {
            let value = i64::from(r.get(register).as_int().filter(|&value| value != 0)?);
            Some(if negative { -value } else { value })
        }
    }
}

/// Whether two byte ranges, (start, length), share a byte.
fn overlaps((a, a_length): (usize, usize), (b, b_length): (usize, usize)) -> bool {
    a < b + b_length && b < a + a_length
}

/// The span of an array kernel accesses: a record array's or a typed array's elements.
fn span(vm: &mut Vm, base: Value) -> Option<Span> {
    if let Some((values, readable, writable)) = vm.array_storage_span(base) {
        return Some(Span { base, values: values.as_ptr(), readable, writable, typed: None });
    }
    let typed = vm.typed_array_span(base)?;
    let writable = if typed.writable { typed.length } else { 0 };
    Some(Span { base, values: typed.first.cast(), readable: typed.length, writable, typed: Some((typed.kind, typed.store)) })
}

/// The longest chunk for a load whose index is `distance` past a store's, both stepping by
/// `step`, the load first in the body when `load_first`; `None` where no chunk is long
/// enough to repay one.
fn hazard(distance: i64, step: i64, load_first: bool) -> Option<usize> {
    if step == 0 {
        return (distance != 0).then_some(CHUNK);
    }
    if distance % step != 0 {
        return Some(CHUNK);
    }
    // The load at iteration k reads what iteration k + distance / step stores.
    let ahead = distance / step;
    if ahead > 0 || (ahead == 0 && load_first) {
        return Some(CHUNK);
    }
    let behind = usize::try_from(-ahead).ok()?;
    (behind >= MIN_CHUNK).then_some(behind.min(CHUNK))
}

/// Decodes `elements` into `out` without branches, so the loop vectorizes; `false` where one
/// is no Number.
fn decode(out: &mut [f64], elements: &[Value]) -> bool {
    let mut others = 0u64;
    for (o, element) in out.iter_mut().zip(elements) {
        let (number, int, double) = element.number_parts();
        others |= u64::from(!number);
        *o = std::hint::select_unpredictable(int, f64::from(element.to_bits() as u32 as i32), double);
    }
    others == 0
}

