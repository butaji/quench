//! Running a loop kernel: entry facts, the trip count, chunks of columns, the scalar pass
//! over recurrences, the chunk's stores, and the registers written back.
use super::super::super::operations::{NumberArithmetic, NumberBitwise, NumberRelation};
use super::shape::{Arg, NodeOp, Scalar};
use super::*;

/// Iterations per chunk: each column holds this many f64s.
pub(super) const CHUNK: usize = 256;
/// The fewest iterations worth a kernel's entry.
const MIN_TRIP: u64 = 16;
/// The shortest chunk a load/store distance may impose.
const MIN_CHUNK: usize = 16;
/// Most distinct base arrays a kernel reads or writes.
const MAX_BASES: usize = 8;
/// Slots of scalars, affine values and recurrences a run keeps on the stack.
const SLOTS: usize = 32;

/// A base array's storage for the run (`Heap::array_storage_span`).
#[derive(Clone, Copy)]
struct Span {
    base: Value,
    values: *mut Value,
    readable: usize,
    writable: usize,
}

const NO_SPAN: Span = Span { base: Value::UNDEFINED, values: std::ptr::null_mut(), readable: 0, writable: 0 };

/// A run's state: the entry facts and the recurrences' current values.
struct State {
    scalars: [f64; SLOTS],
    /// Each affine value's index at the run's first iteration.
    firsts: [i64; SLOTS],
    spans: [Span; MAX_BASES],
    phis: [f64; SLOTS],
    /// The written registers' values at the last committed iteration: recurrences, then temps.
    finals: [f64; 2 * SLOTS],
    trip: u64,
    limit: u64,
    chunk: usize,
}

impl State {
    const EMPTY: Self = Self {
        scalars: [0.0; SLOTS],
        firsts: [0; SLOTS],
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
fn apply(op: NodeOp, a: f64, b: f64) -> f64 {
    match op {
        NodeOp::Arith(operation) => arith(operation, a, b),
        NodeOp::Bits(operation) => bits(operation, a, b),
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
    let mut k = ((bound - start) / step).floor().clamp(0.0, 1e15) as u64;
    while k > 0 && !holds(start + step * k as f64) {
        k -= 1;
    }
    while holds(start + step * (k + 1) as f64) {
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

impl LoopKernel {
    /// Runs the loop from the registers' current state (module documentation).
    pub(super) unsafe fn run<H: Host>(&self, cx: *mut LaneContext<H>, r: Registers, ip: Ip) {
        let Some(mut state) = self.enter(cx, r) else { return };
        // SAFETY: the kernel's scratch, used by one run at a time.
        let scratch = unsafe { &mut *self.scratch.get() };
        if scratch.len() < self.shape.nodes.len() * CHUNK {
            scratch.resize(self.shape.nodes.len() * CHUNK, 0.0);
        }
        let mut done = 0u64;
        while done < state.limit {
            let len = (state.limit - done).min(state.chunk as u64) as usize;
            if !self.chunk(&mut state, scratch, done, len) {
                // A chunk met a value the kernel does not compute: the loop's own records
                // run from its first iteration, now and at every later entry.
                requicken::<H>(ip, self.original::<H>());
                break;
            }
            done += len as u64;
            if script_below_floor(cx) {
                break;
            }
        }
        if done == 0 {
            return;
        }
        self.write_back(&state, r, done);
    }

    /// The run's entry facts, or `None` where the kernel runs nothing.
    fn enter<H: Host>(&self, cx: *mut LaneContext<H>, r: Registers) -> Option<State> {
        let shape = &self.shape;
        let mut state = State::EMPTY;
        for (slot, affine) in shape.affines.iter().enumerate() {
            state.firsts[slot] = i64::from(r.get(affine.register).as_int()?) + i64::from(affine.offset);
        }
        let counter = shape.affines[usize::from(shape.counter)];
        let start = state.firsts[usize::from(shape.counter)];
        self.scalars(&mut state, r)?;
        state.trip = trip_count(start as f64, f64::from(counter.step), shape.relation, state.scalars[usize::from(shape.bound)]);
        if state.trip < MIN_TRIP {
            self.declines.set(self.declines.get() + 1);
            return None;
        }
        for (slot, &(register, _)) in shape.phis.iter().enumerate() {
            state.phis[slot] = r.get(register).as_number()?;
        }
        // SAFETY: the running VM; the spans stay valid while the run calls and allocates nothing.
        let heap = unsafe { &mut vm(cx).heap };
        for (slot, &register) in shape.bases.iter().enumerate() {
            let base = r.get(register);
            let (values, readable, writable) = heap.array_storage_span(base)?;
            state.spans[slot] = Span { base, values: values.as_ptr(), readable, writable };
        }
        state.limit = self.limit(&state)?;
        state.chunk = self.chunk_length(&state)?;
        Some(state)
    }

    /// The scalars' values at entry: constants, unwritten registers, and the loop-invariant
    /// arithmetic over them.
    fn scalars(&self, state: &mut State, r: Registers) -> Option<()> {
        for (slot, scalar) in self.shape.scalars.iter().enumerate() {
            state.scalars[slot] = match *scalar {
                Scalar::Const(value) => value,
                Scalar::Register(register) => r.get(register).as_number()?,
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
                let step = i64::from(shape.affines[usize::from(index)].step);
                limit = limit.min(fits(state.firsts[usize::from(index)], step, state.spans[usize::from(base)].readable));
            }
        }
        for store in &shape.stores {
            let step = i64::from(shape.affines[usize::from(store.index)].step);
            limit = limit.min(fits(state.firsts[usize::from(store.index)], step, state.spans[usize::from(store.base)].writable));
        }
        (limit >= MIN_TRIP).then_some(limit)
    }

    /// The chunk length that keeps every load of a stored array from reading an element an
    /// earlier iteration of the chunk (or the same one, earlier in the body) stores.
    fn chunk_length(&self, state: &State) -> Option<usize> {
        let shape = &self.shape;
        let mut chunk = CHUNK;
        for store in &shape.stores {
            let stored = state.spans[usize::from(store.base)].base;
            let step = i64::from(shape.affines[usize::from(store.index)].step);
            for &(node, order) in &shape.loads {
                let NodeOp::Load { base, index } = shape.nodes[usize::from(node)].op else { continue };
                if state.spans[usize::from(base)].base != stored {
                    continue;
                }
                if i64::from(shape.affines[usize::from(index)].step) != step {
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
        self.recurrence.run(&mut state.phis[..phis], &state.scalars, scratch, len);
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
        state.firsts[usize::from(slot)] + i64::from(self.shape.affines[usize::from(slot)].step) * k as i64
    }

    /// The column operations, in node order (operands precede their users); `false` where a
    /// loaded element is no Number.
    fn columns(&self, state: &State, scratch: &mut [f64], done: u64, len: usize) -> bool {
        let mut numbers = true;
        for (id, node) in self.shape.nodes.iter().enumerate() {
            if node.seq || node.hoisted {
                continue;
            }
            let (prev, rest) = scratch.split_at_mut(id * CHUNK);
            let out = &mut rest[..len];
            let column = |arg: Arg| match arg {
                Arg::Col(n) => Column::Col(&prev[usize::from(n) * CHUNK..][..len]),
                Arg::Scalar(slot) => Column::Scalar(state.scalars[usize::from(slot)]),
                Arg::Phi(_) | Arg::Affine(_) => unreachable!("a column operation reads neither"),
            };
            match node.op {
                NodeOp::Load { base, index } => numbers &= self.load(state, out, base, index, done),
                NodeOp::Iota { index } => {
                    out.iter_mut().enumerate().for_each(|(k, o)| *o = self.index(state, index, done + k as u64) as f64);
                }
                NodeOp::Arith(_) | NodeOp::Bits(_) => operate(node.op, out, column(node.a), column(node.b)),
            }
        }
        numbers
    }

    /// Decodes `bases[base][index + step * k]` into `out`; `false` where one is no Number.
    fn load(&self, state: &State, out: &mut [f64], base: u8, index: u8, done: u64) -> bool {
        let values = state.spans[usize::from(base)].values;
        let first = self.index(state, index, done);
        let step = i64::from(self.shape.affines[usize::from(index)].step);
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
            let values = state.spans[usize::from(store.base)].values;
            let first = self.index(state, store.index, done);
            let step = i64::from(self.shape.affines[usize::from(store.index)].step);
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
            let values = state.spans[usize::from(store.base)].values;
            let first = self.index(state, store.index, done) as isize;
            let step = self.shape.affines[usize::from(store.index)].step as isize;
            // SAFETY: as in `stores_present`; a Number needs no write barrier.
            unsafe {
                match store.value {
                    Arg::Col(node) if step == 1 => {
                        let mut bits = [0u64; CHUNK];
                        let column = &scratch[usize::from(node) * CHUNK..][..len];
                        for (bits, &x) in bits.iter_mut().zip(column) {
                            *bits = Value::number_unpredictable(x).to_bits();
                        }
                        crate::staged::elements::concrete_replace_run(values, first as usize, &bits[..len])
                    }
                    Arg::Col(node) => {
                        let column = scratch[usize::from(node) * CHUNK..][..len].iter().map(|&x| Value::number_unpredictable(x));
                        crate::staged::elements::concrete_replace_elements(values, first, step, column)
                    }
                    value => {
                        let numbers = (0..len).map(|k| Value::number_unpredictable(self.value(state, scratch, value, k, done)));
                        crate::staged::elements::concrete_replace_elements(values, first, step, numbers)
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
            r.set(register, Value::number((entry + i64::from(step) * done as i64) as f64));
        }
        for (slot, &(register, _)) in shape.phis.iter().chain(&shape.temps).enumerate() {
            r.set(register, Value::number(state.finals[slot]));
        }
    }
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
