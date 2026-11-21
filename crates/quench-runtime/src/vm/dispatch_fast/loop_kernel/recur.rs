//! The scalar pass over a kernel's recurrences (`Recurrence`): the operations that read a
//! value the previous iteration computed, in body order, once per iteration.
//!
//! A kernel runs one recurrence whose cycle is a chain: each operator reads the one before
//! and a column or scalar. A one-operator cycle (`s += x`, `s = (s + x) | 0`) is a fused loop
//! per operator (`Fold`); a longer one runs its links per iteration (`Chain`). Nodes that read
//! the recurrence without feeding it run as columns after the pass. A loop with any other
//! recurrence keeps its own records.
use super::super::super::operations::{NumberArithmetic, NumberBitwise};
use super::run::{CHUNK, apply, arith};
use super::shape::{Arg, NodeOp, Shape};

/// One recurrence folded by one arithmetic operator, maybe truncated to int32 after
/// (`(s op x) | 0`): `s = op(s, x[k])`, or `op(x[k], s)` when the recurrence is the right operand.
pub(super) struct Fold {
    op: NumberArithmetic,
    left: bool,
    operand: Arg,
    int32: bool,
    /// The columns the chain's nodes write, where a store or a side node reads them.
    outs: [Option<u16>; 2],
    /// The nodes only a written register's final value reads: their last value only.
    lasts: [Option<u16>; 2],
    /// For `(s + i) | 0` and `(s - i) | 0` with `i` an induction (an iota node no other reader
    /// needs as a column): the iota's affine slot and node. The fold then runs on int32 words,
    /// the induction stepped in a register, and the iota's column is never written.
    words: Option<(u8, u16)>,
    /// For `s + x` and `s - x` (no truncation) with `x` an induction or a bitwise column only
    /// the fold reads: their words, summed on integers where binary64 is exact (`run_exact`).
    exact: Option<Fusion>,
}

/// The inductions of a chunk: each affine slot's value at iteration 0 and its step, and the
/// chunk's first iteration.
#[derive(Clone, Copy)]
pub(super) struct Iotas<'a> {
    pub(super) firsts: &'a [i64],
    pub(super) steps: &'a [i64],
    pub(super) done: u64,
}

/// The longest chain a kernel runs.
const MAX_LINKS: usize = 24;

/// One link of a chain: `v = code(v, x)` with `x` the column (or broadcast scalar) at element
/// offset `operand` of the scratch, the result written at offset `out` (a dump column when no
/// later reader needs it).
#[derive(Clone, Copy)]
struct Link {
    code: Code,
    operand: u32,
    out: u32,
}

/// A link's operator with its operand order: `v op x`, or `x op v` for the reversed forms.
#[derive(Clone, Copy)]
pub(super) enum Code {
    Add,
    Multiply,
    Subtract,
    SubtractFrom,
    Divide,
    DivideInto,
    Bits { op: NumberBitwise, reversed: bool },
}

impl Code {
    pub(super) fn of(op: NodeOp, left: bool) -> Self {
        match (op, left) {
            (NodeOp::Arith(NumberArithmetic::Add), _) => Self::Add,
            (NodeOp::Arith(NumberArithmetic::Multiply), _) => Self::Multiply,
            (NodeOp::Arith(NumberArithmetic::Subtract), true) => Self::Subtract,
            (NodeOp::Arith(NumberArithmetic::Subtract), false) => Self::SubtractFrom,
            (NodeOp::Arith(NumberArithmetic::Divide), true) => Self::Divide,
            (NodeOp::Arith(NumberArithmetic::Divide), false) => Self::DivideInto,
            (NodeOp::Bits(op), left) => Self::Bits { op, reversed: !left },
            (NodeOp::Load { .. } | NodeOp::Iota { .. } | NodeOp::Cmp(_), _) => unreachable!("a recurrence's nodes are arithmetic or bitwise"),
        }
    }

    #[inline(always)]
    pub(super) fn apply(self, v: f64, x: f64) -> f64 {
        match self {
            Self::Add => v + x,
            Self::Multiply => v * x,
            Self::Subtract => v - x,
            Self::SubtractFrom => x - v,
            Self::Divide => v / x,
            Self::DivideInto => x / v,
            Self::Bits { op, reversed: false } => apply(NodeOp::Bits(op), v, x),
            Self::Bits { op, reversed: true } => apply(NodeOp::Bits(op), x, v),
        }
    }
}

/// A single recurrence whose cycle is a chain of operators, each reading the previous one's
/// value and one column or scalar.
pub(super) struct Chain {
    links: Vec<Link>,
    /// Scalars the links read, each broadcast to its column after the nodes' and the dump's:
    /// (scalar slot, element offset).
    broadcasts: Vec<(u8, u32)>,
    /// Whether every link is an int32 bitwise operator (all but the zero-fill shift): the
    /// recurrence's value is then an int32 after its first link, whatever its operands, and the
    /// chain runs on int32 words (`run_int32`).
    int32: bool,
    /// Whether every link is `& | ^` and only the last one's result column is read: the chain
    /// is then one `v = (v & A) ^ B` per iteration, its masks computed column by column
    /// (`run_masks`).
    masks: bool,
    /// Whether the chain is integer arithmetic ending in an int32 bitwise link (`(a * 3 + i) | 0`):
    /// it then runs on int32 words where binary64 is exact (`run_wrap`).
    wrap: bool,
    /// The column nodes only the chain reads, whose words it computes itself.
    fusion: Fusion,
    /// The dump column's element offset.
    dump: u32,
}

enum Pass {
    None,
    Fold(Fold),
    Chain(Chain),
    Machine(super::machine::Machine),
}

/// A kernel's recurrence pass, and the nodes that read the recurrence without feeding it
/// (`side`, in body order), which run as columns after the pass.
pub(super) struct Recurrence {
    pass: Pass,
    side: Vec<u16>,
}

/// Whether node `id`'s whole column is read: by a store or a side node.
fn read_as_column(shape: &Shape, side: &[u16], id: u16) -> bool {
    let read = Arg::Col(id);
    shape.stores.iter().any(|store| store.value == read)
        || side.iter().any(|&n| shape.nodes[usize::from(n)].a == read || shape.nodes[usize::from(n)].b == read)
}

/// Whether node `id`'s column is read by a store, a side node or a written register's final
/// value.
pub(super) fn read_later(shape: &Shape, side: &[u16], id: u16) -> bool {
    read_as_column(shape, side, id) || shape.temps.iter().any(|&(_, arg)| arg == Arg::Col(id))
}

/// The nodes of the scalar pass that no recurrence's next value reads, directly or through
/// other nodes, and that read no recurrence directly: they run as columns after the pass.
fn feeds_none(shape: &Shape) -> Vec<u16> {
    let mut needed = vec![false; shape.nodes.len()];
    let mut work: Vec<u16> = shape.phis.iter().filter_map(|&(_, arg)| if let Arg::Col(n) = arg { Some(n) } else { None }).collect();
    while let Some(n) = work.pop() {
        let node = shape.nodes[usize::from(n)];
        if !node.seq || std::mem::replace(&mut needed[usize::from(n)], true) {
            continue;
        }
        work.extend([node.a, node.b].into_iter().filter_map(|arg| if let Arg::Col(m) = arg { Some(m) } else { None }));
    }
    let mut side: Vec<u16> = Vec::new();
    for &id in &shape.seq {
        let node = shape.nodes[usize::from(id)];
        let column = |arg: Arg| match arg {
            Arg::Phi(_) => false,
            Arg::Col(m) => !shape.nodes[usize::from(m)].seq || needed[usize::from(m)] || side.contains(&m),
            _ => true,
        };
        if !needed[usize::from(id)] && column(node.a) && column(node.b) {
            side.push(id);
        }
    }
    side
}

/// Whether `arg` is the recurrence or a value computed from it this iteration.
fn recurrent(shape: &Shape, arg: Arg) -> bool {
    match arg {
        Arg::Phi(_) => true,
        Arg::Col(n) => shape.nodes[usize::from(n)].seq,
        _ => false,
    }
}

impl Recurrence {
    pub(super) fn of(shape: &Shape) -> Option<Self> {
        if shape.phis.is_empty() {
            return Some(Self { pass: Pass::None, side: Vec::new() });
        }
        // A branch whose condition reads a recurrence has no pass (`if (s < k) s++`).
        if shape.seq.iter().any(|&n| matches!(shape.nodes[usize::from(n)].op, NodeOp::Cmp(_))) {
            return None;
        }
        Self::single(shape)
            .or_else(|| {
                let side = feeds_none(shape);
                Some(Self { pass: Pass::Machine(super::machine::Machine::of(shape, &side)?), side })
            })
            .filter(|recurrence| !recurrence.slower_than_records())
    }

    /// Whether the pass runs the body slower than the interpreter's int32 records would, so the
    /// loop keeps them: a machine with bitwise steps, which interprets each through the binary64
    /// operator (shl_shr 83 → 458 Ir per iteration), and a general chain with bitwise links.
    fn slower_than_records(&self) -> bool {
        match &self.pass {
            Pass::Machine(machine) => machine.has_bitwise(),
            // A general chain dispatches each link at about 40 Ir: with a bitwise link it beats
            // the records only where they run a multiply or divide out of line (int_mul 171 →
            // 113, ushr 87 → 149).
            Pass::Chain(chain) if !chain.int32 && !chain.masks && !chain.wrap => {
                chain.links.iter().any(|link| matches!(link.code, Code::Bits { .. }))
                    && !chain.links.iter().any(|link| matches!(link.code, Code::Multiply | Code::Divide | Code::DivideInto))
            }
            _ => false,
        }
    }

    /// The pass's name, for the kernel trace.
    pub(super) fn name(&self) -> &'static str {
        match &self.pass {
            Pass::None => "kernel (no recurrence)",
            Pass::Fold(fold) if fold.int32 => "kernel (int32 fold)",
            Pass::Fold(_) => "kernel (fold)",
            Pass::Chain(chain) if chain.masks => "kernel (mask chain)",
            Pass::Chain(chain) if chain.int32 => "kernel (int32 chain)",
            Pass::Chain(chain) if chain.wrap => "kernel (integer chain)",
            Pass::Chain(_) => "kernel (chain)",
            Pass::Machine(_) => "kernel (machine)",
        }
    }

    /// Whether the pass interprets its recurrences per iteration (`Machine`) rather than
    /// running them as fused or chained loops.
    pub(super) fn interprets(&self) -> bool {
        matches!(self.pass, Pass::Machine(_))
    }

    /// The pass of a single recurrence whose cycle is a chain (`cycle`), whose other nodes
    /// read columns only.
    fn single(shape: &Shape) -> Option<Self> {
        let cycle = cycle(shape)?;
        let side: Vec<u16> = shape.seq.iter().copied().filter(|id| !cycle.contains(id)).collect();
        // A side node reads columns only: the recurrence's value before an iteration has none.
        if side.iter().any(|&n| matches!(shape.nodes[usize::from(n)].a, Arg::Phi(_)) || matches!(shape.nodes[usize::from(n)].b, Arg::Phi(_))) {
            return None;
        }
        let pass = match Self::fold(shape, &cycle, &side) {
            Some(fold) => Pass::Fold(fold),
            None => Pass::Chain(Chain::of(shape, &cycle, &side)?),
        };
        Some(Self { pass, side })
    }

    /// The fold of a cycle that is one arithmetic node, maybe followed by `| 0`.
    fn fold(shape: &Shape, cycle: &[u16], side: &[u16]) -> Option<Fold> {
        let (&first, &last) = (cycle.first()?, cycle.last()?);
        let node = shape.nodes[usize::from(first)];
        let NodeOp::Arith(op) = node.op else { return None };
        let (left, operand) = if node.a == Arg::Phi(0) { (true, node.b) } else { (false, node.a) };
        let int32 = match cycle.len() {
            1 => false,
            2 if is_int32_truncation(shape, first, last) => true,
            _ => return None,
        };
        let outs = [first, last].map(|id| read_as_column(shape, side, id).then_some(id));
        let lasts = [first, last].map(|id| shape.temps.iter().any(|&(_, arg)| arg == Arg::Col(id)).then_some(id));
        // `(s ± i) | 0` of int32s is the int32 wrapping sum: the binary64 sum of two int32s is
        // exact, and its ToInt32 is that sum modulo 2^32.
        let words = match operand {
            Arg::Col(n) if int32 && matches!(op, NumberArithmetic::Add | NumberArithmetic::Subtract) && outs == [None, None] => {
                match shape.nodes[usize::from(n)].op {
                    NodeOp::Iota { index } => {
                        let read_elsewhere = read_later(shape, side, n)
                            || shape.nodes.iter().enumerate().any(|(id, other)| id != usize::from(first) && (other.a == Arg::Col(n) || other.b == Arg::Col(n)));
                        (!read_elsewhere).then_some((index, n))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        let exact = match operand {
            Arg::Col(n) if !int32 && left && matches!(op, NumberArithmetic::Add | NumberArithmetic::Subtract) && outs == [None, None] => {
                let (mut iotas, mut fused) = (Vec::new(), Vec::new());
                fuse(shape, side, n, &mut iotas, &mut fused).then_some(Fusion { iotas, fused })
            }
            _ => None,
        };
        Some(Fold { op, left, operand, int32, outs, lasts, words, exact })
    }

    /// The scratch columns a run needs.
    pub(super) fn columns(&self, shape: &Shape) -> usize {
        shape.nodes.len()
            + match &self.pass {
                Pass::Chain(chain) => chain.columns(),
                Pass::Machine(machine) => machine.columns(),
                _ => 0,
            }
    }

    /// Side nodes, in body order.
    pub(super) fn side(&self) -> &[u16] {
        &self.side
    }

    /// The pass over a chunk of `len` iterations: updates `phis`, writes the columns read later.
    /// Whether node `id`'s column is never read: the pass reads it as an induction in a register.
    pub(super) fn unread_column(&self, id: usize) -> bool {
        match &self.pass {
            Pass::Fold(Fold { words: Some((_, n)), .. }) => usize::from(*n) == id,
            Pass::Fold(Fold { exact: Some(fusion), .. }) => fusion.contains(id),
            Pass::Chain(chain) => chain.fusion.contains(id),
            _ => false,
        }
    }

    pub(super) fn run(&self, phis: &mut [f64], scalars: &[f64], scratch: &mut [f64], len: usize, iotas: Iotas) {
        match &self.pass {
            Pass::None => {}
            Pass::Fold(fold) => {
                if let Some((slot, n)) = fold.words {
                    if fold.run_words(phis, scratch, len, iotas, slot) {
                        return;
                    }
                    // The words do not apply to this chunk: its iota's column, then the fold.
                    let (first, step) = (iotas.firsts[usize::from(slot)], iotas.steps[usize::from(slot)]);
                    for (k, out) in scratch[usize::from(n) * CHUNK..][..len].iter_mut().enumerate() {
                        *out = (first + step * (iotas.done + k as u64) as i64) as f64;
                    }
                }
                if let Some(fusion) = &fold.exact {
                    if fold.run_exact(fusion, phis, scalars, scratch, len, iotas) {
                        return;
                    }
                    fusion.materialize(scalars, scratch, len, iotas);
                }
                fold.run(phis, scalars, scratch, len)
            }
            Pass::Chain(chain) => {
                if chain.masks {
                    return chain.run_masks(phis, scalars, scratch, len, iotas);
                }
                if chain.wrap && chain.run_wrap(phis, scalars, scratch, len, iotas) {
                    return;
                }
                // Not this chunk: the fused columns, then the chain.
                chain.fusion.materialize(scalars, scratch, len, iotas);
                chain.run(phis, scalars, scratch, len)
            }
            Pass::Machine(machine) => machine.run(phis, scalars, scratch, len),
        }
    }
}

/// The nodes from the single recurrence to its value at the iteration's end, in order, where
/// each reads the one before (the first, the recurrence) and otherwise a column or scalar.
fn cycle(shape: &Shape) -> Option<Vec<u16>> {
    let [(_, Arg::Col(last))] = shape.phis.as_slice() else { return None };
    let mut cycle = Vec::new();
    let mut at = Arg::Col(*last);
    while let Arg::Col(id) = at {
        let node = shape.nodes[usize::from(id)];
        if !node.seq {
            return None;
        }
        cycle.push(id);
        at = match (recurrent(shape, node.a), recurrent(shape, node.b)) {
            (true, false) => node.a,
            (false, true) => node.b,
            _ => return None,
        };
    }
    let column_or_scalar = |id: &u16| {
        let node = shape.nodes[usize::from(*id)];
        let other = if recurrent(shape, node.a) { node.b } else { node.a };
        matches!(other, Arg::Col(_) | Arg::Scalar(_))
    };
    (at == Arg::Phi(0) && cycle.len() <= MAX_LINKS && cycle.iter().all(column_or_scalar)).then(|| {
        cycle.reverse();
        cycle
    })
}

/// Whether node `last` is `node(first) | c` for a constant `c` whose ToInt32 is zero (`| 0`): the
/// staged bitwise row then yields ToInt32 of `node(first)`.
fn is_int32_truncation(shape: &Shape, first: u16, last: u16) -> bool {
    use crate::staged::NumericStage;
    let node = shape.nodes[usize::from(last)];
    let zero = |arg: Arg| matches!(arg, Arg::Scalar(s) if shape.scalar_constant(s).is_some_and(|value| {
        let mut stage = crate::staged::Concrete::new(std::ptr::null());
        NumericStage::to_int32(&mut stage, value.to_bits(), ()) == Some(0)
    }));
    matches!(node.op, NodeOp::Bits(NumberBitwise::Or))
        && ((node.a == Arg::Col(first) && zero(node.b)) || (node.b == Arg::Col(first) && zero(node.a)))
}

impl Fold {
    /// `run` on int32 words with the induction in a register; `false` (nothing done) where the
    /// recurrence is no int32 at entry or the induction leaves int32 in this chunk.
    fn run_words(&self, phis: &mut [f64], scratch: &mut [f64], len: usize, iotas: Iotas, slot: u8) -> bool {
        let (first, step) = (iotas.firsts[usize::from(slot)], iotas.steps[usize::from(slot)]);
        let start = first + step * iotas.done as i64;
        let end = start + step * (len as i64 - 1).max(0);
        let entry = phis[0] as i32;
        if f64::from(entry) != phis[0] || i32::try_from(start).is_err() || i32::try_from(end).is_err() {
            return false;
        }
        let (mut value, mut x, step) = (entry, start as i32, step as i32);
        macro_rules! words {
            ($f:expr) => {
                for _ in 0..len {
                    value = $f(value, x);
                    x = x.wrapping_add(step);
                }
            };
        }
        match (self.op, self.left) {
            (NumberArithmetic::Add, _) => words!(|v: i32, x: i32| v.wrapping_add(x)),
            (NumberArithmetic::Subtract, true) => words!(|v: i32, x: i32| v.wrapping_sub(x)),
            (NumberArithmetic::Subtract, false) => words!(|v: i32, x: i32| x.wrapping_sub(v)),
            _ => return false,
        }
        if len > 0 && self.lasts != [None, None] {
            // The last iteration's nodes, for the registers whose final value they are: the
            // value before it (the words' step undone), its sum, and that sum's int32.
            let last_x = end as i32;
            let before = match (self.op, self.left) {
                (NumberArithmetic::Add, _) => value.wrapping_sub(last_x),
                (_, true) => value.wrapping_add(last_x),
                _ => last_x.wrapping_sub(value),
            };
            let sum = if self.left { arith(self.op, f64::from(before), f64::from(last_x)) } else { arith(self.op, f64::from(last_x), f64::from(before)) };
            for (out, result) in self.lasts.iter().zip([sum, f64::from(value)]) {
                if let Some(id) = out {
                    scratch[usize::from(*id) * CHUNK + len - 1] = result;
                }
            }
        }
        phis[0] = f64::from(value);
        true
    }

    /// `s += x` or `s -= x` with `x`'s words (`exact`) summed on i64: `false` (nothing done)
    /// where the sum might leave the integers binary64 holds exactly, or `s` is -0 under a
    /// subtraction (`-0 - 0` is -0). Every partial sum is then an exact integer, as is the
    /// binary64 sum of an exact integer and an int32.
    fn run_exact(&self, fusion: &Fusion, phis: &mut [f64], scalars: &[f64], scratch: &mut [f64], len: usize, iotas: Iotas) -> bool {
        const EXACT: f64 = 9007199254740992.0;
        let Arg::Col(n) = self.operand else { return false };
        let entry = phis[0];
        let subtract = self.op == NumberArithmetic::Subtract;
        if !(entry.abs() < EXACT && entry.trunc() == entry) || (subtract && entry == 0.0 && entry.is_sign_negative()) {
            return false;
        }
        let sum: i64 = if let Some(&(_, slot)) = fusion.iotas.iter().find(|&&(m, _)| m == n) {
            let (first, step) = (iotas.firsts[usize::from(slot)], iotas.steps[usize::from(slot)]);
            let start = first + step * iotas.done as i64;
            let end = start + step * (len as i64 - 1).max(0);
            let magnitude = start.unsigned_abs().max(end.unsigned_abs()) as f64;
            if entry.abs() + magnitude * len as f64 >= EXACT {
                return false;
            }
            let len = len as i64;
            start * len + step * (len * (len - 1) / 2)
        } else {
            let unsigned = fusion.fused.iter().any(|&(m, op, ..)| m == n && op == NumberBitwise::ShiftRightUnsigned);
            if entry.abs() + 4294967296.0 * len as f64 >= EXACT {
                return false;
            }
            let mut words = [0i32; CHUNK];
            let words = &mut words[..len];
            fusion.eval(Arg::Col(n), scalars, scratch, len, iotas, words);
            if unsigned { words.iter().map(|&w| i64::from(w as u32)).sum() } else { words.iter().map(|&w| i64::from(w)).sum() }
        };
        let value = if subtract { entry as i64 - sum } else { entry as i64 + sum };
        // `-0 + 0` is +0, as is the integer sum's binary64.
        let result = value as f64;
        if len > 0 {
            if let Some(id) = self.lasts[0] {
                scratch[usize::from(id) * CHUNK + len - 1] = result;
            }
            phis[0] = result;
        }
        true
    }

    /// `(s ± c) | 0` with `c` an int32 scalar, in closed form: the binary64 sum of two int32s is
    /// exact, so each step is the wrapping int32 sum, and `len` of them add `c * len` modulo 2^32
    /// (`c - s` alternates between two values). `false` (nothing done) where `s` is no int32.
    fn run_constant(&self, phis: &mut [f64], scalars: &[f64], scratch: &mut [f64], len: usize) -> bool {
        let Arg::Scalar(_) = self.operand else { return false };
        if !self.int32 || self.outs != [None, None] || len == 0 {
            return false;
        }
        let entry = phis[0] as i32;
        if f64::from(entry) != phis[0] {
            return false;
        }
        let Some(x) = super::run::exact_int32(self.scalar(scalars)) else { return false };
        // The last step's untruncated sum keeps a -0 operand's sign (`-0 + -0` is -0).
        let negative_zero = |v: f64| v == 0.0 && v.is_sign_negative();
        if self.lasts != [None, None] && (negative_zero(phis[0]) || negative_zero(self.scalar(scalars))) {
            return false;
        }
        // The value after `count` steps.
        let after = |count: usize| -> Option<i32> {
            Some(match (self.op, self.left) {
                (NumberArithmetic::Add, _) => entry.wrapping_add(x.wrapping_mul(count as i32)),
                (NumberArithmetic::Subtract, true) => entry.wrapping_sub(x.wrapping_mul(count as i32)),
                (NumberArithmetic::Subtract, false) => if count % 2 == 0 { entry } else { x.wrapping_sub(entry) },
                _ => return None,
            })
        };
        let (Some(before), Some(value)) = (after(len - 1), after(len)) else { return false };
        if self.lasts != [None, None] {
            let sum = if self.left { arith(self.op, f64::from(before), f64::from(x)) } else { arith(self.op, f64::from(x), f64::from(before)) };
            for (out, result) in self.lasts.iter().zip([sum, f64::from(value)]) {
                if let Some(id) = out {
                    scratch[usize::from(*id) * CHUNK + len - 1] = result;
                }
            }
        }
        phis[0] = f64::from(value);
        true
    }

    fn run(&self, phis: &mut [f64], scalars: &[f64], scratch: &mut [f64], len: usize) {
        if self.run_constant(phis, scalars, scratch, len) {
            return;
        }
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
            (sum, if INT32 { f64::from(crate::value::number_to_i32(sum)) } else { sum })
        };
        let mut value = phis[0];
        // The value before the last iteration, from which the last's nodes are redone.
        let mut before = value;
        if self.outs == [None, None] {
            match self.operand {
                Arg::Col(n) => {
                    for &x in &scratch[usize::from(n) * CHUNK..][..len] {
                        before = value;
                        value = step(value, x).1;
                    }
                }
                _ => {
                    let x = self.scalar(scalars);
                    (0..len).for_each(|_| {
                        before = value;
                        value = step(value, x).1;
                    });
                }
            }
        } else {
            for k in 0..len {
                let x = match self.operand { Arg::Col(n) => scratch[usize::from(n) * CHUNK + k], _ => self.scalar(scalars) };
                let (first, last) = step(value, x);
                before = value;
                value = last;
                for (out, result) in self.outs.iter().zip([first, last]) {
                    if let Some(id) = out {
                        scratch[usize::from(*id) * CHUNK + k] = result;
                    }
                }
            }
        }
        if len > 0 && self.lasts != [None, None] {
            let x = match self.operand { Arg::Col(n) => scratch[usize::from(n) * CHUNK + len - 1], _ => self.scalar(scalars) };
            let (first, last) = step(before, x);
            for (out, result) in self.lasts.iter().zip([first, last]) {
                if let Some(id) = out {
                    scratch[usize::from(*id) * CHUNK + len - 1] = result;
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

impl Chain {
    fn of(shape: &Shape, cycle: &[u16], side: &[u16]) -> Option<Self> {
        let column = |n: usize| u32::try_from(n * CHUNK).ok();
        let dump = column(shape.nodes.len())?;
        let mut broadcasts = Vec::new();
        let mut links = Vec::with_capacity(cycle.len());
        for &id in cycle {
            let node = shape.nodes[usize::from(id)];
            let left = recurrent(shape, node.a);
            let operand = match if left { node.b } else { node.a } {
                Arg::Col(n) => column(usize::from(n))?,
                Arg::Scalar(slot) => {
                    let offset = column(shape.nodes.len() + 1 + broadcasts.len())?;
                    broadcasts.push((slot, offset));
                    offset
                }
                _ => return None,
            };
            let out = if read_later(shape, side, id) { column(usize::from(id))? } else { dump };
            links.push(Link { code: Code::of(node.op, left), operand, out });
        }
        let int32 = links.iter().all(|link| matches!(link.code, Code::Bits { op, .. } if op != NumberBitwise::ShiftRightUnsigned));
        let logic = |link: &Link| matches!(link.code, Code::Bits { op: NumberBitwise::And | NumberBitwise::Or | NumberBitwise::Xor, .. });
        let masks = links.iter().all(logic) && links[..links.len().saturating_sub(1)].iter().all(|link| link.out == dump);
        let integer = |link: &Link| matches!(link.code, Code::Add | Code::Multiply | Code::Subtract | Code::SubtractFrom);
        let wrap = !int32
            && !masks
            && links.len() >= 2
            && links.last().is_some_and(|link| matches!(link.code, Code::Bits { .. }))
            && links[..links.len() - 1].iter().all(|link| integer(link) && link.out == dump);
        let mut iotas = Vec::new();
        let mut fused = Vec::new();
        if wrap || masks {
            for link in &links {
                let n = link.operand as usize / CHUNK;
                if n < shape.nodes.len() && !is_broadcast(&broadcasts, link.operand) {
                    fuse(shape, side, n as u16, &mut iotas, &mut fused);
                }
            }
        }
        Some(Self { links, broadcasts, int32, masks, wrap, fusion: Fusion { iotas, fused }, dump })
    }

    /// Columns past the nodes' this chain uses: the dump and the broadcasts.
    fn columns(&self) -> usize {
        1 + self.broadcasts.len()
    }

    fn run(&self, phis: &mut [f64], scalars: &[f64], scratch: &mut [f64], len: usize) {
        for &(slot, offset) in &self.broadcasts {
            scratch[offset as usize..][..len].fill(scalars[usize::from(slot)]);
        }
        if self.int32 {
            return self.run_int32(phis, scratch, len);
        }
        let base = scratch.as_mut_ptr();
        let mut value = phis[0];
        for k in 0..len {
            // SAFETY: every offset is a column's start inside `scratch` (`Recurrence::columns`)
            // and `k < len <= CHUNK`; no link's operand column is a link's result column.
            unsafe {
                let row = base.add(k);
                for link in &self.links {
                    value = link.code.apply(value, *row.add(link.operand as usize));
                    *row.add(link.out as usize) = value;
                }
            }
        }
        phis[0] = value;
    }

    /// `run` for an integer chain on int32 words: `false` (nothing done) where an operand is no
    /// int32 or binary64 might round. Each arithmetic link maps the value as `v -> v * a + b`
    /// (`+ x`: (1, x); `- x`: (1, -x); `x -`: (-1, x); `* x`: (x, 0)); maps of that form compose,
    /// so the links fold into one pair per iteration. Where every intermediate value stays below
    /// 2^53 the binary64 results are the exact integers, and the last link's ToInt32 of an exact
    /// integer is that integer modulo 2^32: the same as wrapping int32 arithmetic. A pair's
    /// parts stay constants, or for `b` a linear function of the iteration (an induction), until
    /// a column operand makes them columns; the recurrence then runs one multiply and add per
    /// iteration, with the induction in a register (`(a * 3 + i) | 0`: imul, add, add).
    fn run_wrap(&self, phis: &mut [f64], scalars: &[f64], scratch: &mut [f64], len: usize, iotas: Iotas) -> bool {
        const EXACT: f64 = 9007199254740992.0;
        const WORD: f64 = 4294967296.0;
        if len == 0 {
            return true;
        }
        let entry = phis[0];
        if !(entry.abs() <= WORD && entry.trunc() == entry) {
            return false;
        }
        let Some((last, links)) = self.links.split_last() else { return false };
        let mut x = [0i32; CHUNK];
        let x = &mut x[..len];
        let mut bound = WORD;
        let mut a = Part::Const(1);
        let mut b = Part::Const(0);
        let (mut a_column, mut b_column) = ([0i32; CHUNK], [0i32; CHUNK]);
        let (a_column, b_column) = (&mut a_column[..len], &mut b_column[..len]);
        for link in links {
            let Some((operand, magnitude)) = self.operand(link, scalars, scratch, len, iotas, x) else { return false };
            let negate = matches!(link.code, Code::Subtract);
            match link.code {
                Code::Add | Code::Subtract => {
                    let operand = if negate { operand.neg(x) } else { operand };
                    b = b.add(operand, b_column, x);
                    bound += magnitude;
                }
                Code::SubtractFrom => {
                    a = a.neg(a_column);
                    b = b.neg(b_column).add(operand, b_column, x);
                    bound += magnitude;
                }
                Code::Multiply => {
                    match operand {
                        Part::Const(c) => {
                            a = a.scale(c, a_column);
                            b = b.scale(c, b_column);
                        }
                        _ => {
                            operand.fill(x);
                            a.fill(a_column);
                            b.fill(b_column);
                            a_column.iter_mut().zip(b_column.iter_mut()).zip(x.iter()).for_each(|((a, b), &x)| {
                                *a = a.wrapping_mul(x);
                                *b = b.wrapping_mul(x);
                            });
                            (a, b) = (Part::Column, Part::Column);
                        }
                    }
                    bound *= magnitude;
                }
                _ => return false,
            }
            if bound >= EXACT {
                return false;
            }
        }
        let Code::Bits { op, reversed } = last.code else { return false };
        let Some((operand, _)) = self.operand(last, scalars, scratch, len, iotas, x) else { return false };
        // The last link as masks `(v & m) ^ p` where it is `& | ^` of a constant.
        let masks = match (op, operand) {
            (NumberBitwise::And, Part::Const(c)) => Some((c, 0)),
            (NumberBitwise::Or, Part::Const(c)) => Some((!c, c)),
            (NumberBitwise::Xor, Part::Const(c)) => Some((-1, c)),
            _ => None,
        };
        if masks.is_none() {
            operand.fill(x);
        }
        let unsigned = op == NumberBitwise::ShiftRightUnsigned;
        let entry = entry as i64 as i32;
        let write = last.out != self.dump;
        let out = &mut scratch[last.out as usize..][..len];
        let (a_column, b_column, x) = (&*a_column, &*b_column, &*x);
        macro_rules! last {
            ($a:expr, $b:expr) => {
                match (masks, write) {
                    (Some((-1, 0)), false) => sequence::<false>(entry, len, $a, $b, |v, _| v, out, unsigned),
                    (Some((m, p)), false) => sequence::<false>(entry, len, $a, $b, |v, _| (v & m) ^ p, out, unsigned),
                    (Some((m, p)), true) => sequence::<true>(entry, len, $a, $b, |v, _| (v & m) ^ p, out, unsigned),
                    (None, _) => sequence::<true>(entry, len, $a, $b, |v, k| {
                        let (left, right) = if reversed { (x[k], v) } else { (v, x[k]) };
                        crate::vm::operations::int32_bitwise_bits(op, left, right)
                    }, out, unsigned),
                }
            };
        }
        macro_rules! b {
            ($a:expr) => {
                match b {
                    Part::Const(c) => last!($a, |_| c),
                    Part::Linear { start, step } => last!($a, |k: usize| start.wrapping_add(step.wrapping_mul(k as i32))),
                    Part::Column => last!($a, |k: usize| b_column[k]),
                }
            };
        }
        let value = match a {
            Part::Const(1) => b!(|_| 1),
            Part::Const(c) => b!(|_| c),
            _ => b!(|k: usize| a_column[k]),
        };
        phis[0] = if unsigned { f64::from(value as u32) } else { f64::from(value) };
        true
    }

    /// A link's operand as a pair part (a broadcast constant, an induction, or a column of
    /// int32 words written into `x`) and its largest magnitude in the chunk; `None` where an
    /// element is no int32.
    fn operand(&self, link: &Link, scalars: &[f64], scratch: &[f64], len: usize, iotas: Iotas, x: &mut [i32]) -> Option<(Part, f64)> {
        if let Some(&(slot, _)) = self.broadcasts.iter().find(|&&(_, offset)| offset == link.operand) {
            let c = super::run::exact_int32(scalars[usize::from(slot)])?;
            return Some((Part::Const(c), f64::from(c).abs()));
        }
        let n = link.operand as usize / CHUNK;
        if let Some(&(_, slot)) = self.fusion.iotas.iter().find(|&&(m, _)| usize::from(m) == n) {
            let (first, step) = (iotas.firsts[usize::from(slot)], iotas.steps[usize::from(slot)]);
            let start = first + step * iotas.done as i64;
            let end = start + step * (len as i64 - 1);
            let magnitude = start.unsigned_abs().max(end.unsigned_abs()) as f64;
            return Some((Part::Linear { start: start as i32, step: step as i32 }, magnitude));
        }
        if let Some(&(_, op, ..)) = self.fusion.fused.iter().find(|&&(m, ..)| usize::from(m) == n) {
            self.fusion.eval(Arg::Col(n as u16), scalars, scratch, len, iotas, x);
            let magnitude = if op == NumberBitwise::ShiftRightUnsigned { 4294967296.0 } else { 2147483648.0 };
            return Some((Part::Column, magnitude));
        }
        let column = &scratch[link.operand as usize..][..len];
        let mut exact = true;
        for (o, &value) in x.iter_mut().zip(column) {
            let word = super::run::exact_int32(value);
            exact &= word.is_some();
            *o = word.unwrap_or(0);
        }
        exact.then_some((Part::Column, 2147483648.0))
    }

    /// A chain of `& | ^` links as masks: each link maps the value as `v -> (v & a) ^ b` (`& x`:
    /// a = x, b = 0; `| x`: a = !x, b = x; `^ x`: a = !0, b = x), and maps of that form compose
    /// (`(v & a1 ^ b1) & a2 ^ b2 = v & (a1 & a2) ^ (b1 & a2 ^ b2)`), so the links fold column by
    /// column into one pair per iteration and the recurrence runs one and, one xor per iteration.
    fn run_masks(&self, phis: &mut [f64], scalars: &[f64], scratch: &mut [f64], len: usize, iotas: Iotas) {
        if len == 0 {
            return;
        }
        let mut and = [-1i32; CHUNK];
        let mut xor = [0i32; CHUNK];
        let mut words = [0i32; CHUNK];
        let (and, xor, words) = (&mut and[..len], &mut xor[..len], &mut words[..len]);
        for link in &self.links {
            let Code::Bits { op, .. } = link.code else { unreachable!("a mask chain's links are bitwise") };
            let broadcast = self.broadcasts.iter().find(|&&(_, offset)| offset == link.operand).map(|&(slot, _)| int32_word(scalars[usize::from(slot)]));
            if broadcast.is_none() {
                self.fusion.eval(Arg::Col((link.operand as usize / CHUNK) as u16), scalars, scratch, len, iotas, words);
            }
            macro_rules! fold {
                (|$x:ident, $a:ident, $b:ident| $body:expr) => {
                    match broadcast {
                        Some($x) => and.iter_mut().zip(xor.iter_mut()).for_each(|($a, $b)| $body),
                        None => and.iter_mut().zip(xor.iter_mut()).zip(words.iter()).for_each(|(($a, $b), &$x)| $body),
                    }
                };
            }
            match op {
                NumberBitwise::And => fold!(|x, a, b| { *a &= x; *b &= x; }),
                NumberBitwise::Or => fold!(|x, a, b| { *a &= !x; *b |= x; }),
                NumberBitwise::Xor => fold!(|x, _a, b| { *b ^= x; }),
                _ => unreachable!("a mask chain's links are & | ^"),
            }
        }
        let mut value = int32_word(phis[0]);
        let last = self.links.last().map_or(self.dump, |link| link.out);
        if last == self.dump {
            for (&a, &b) in and.iter().zip(xor.iter()) {
                value = (value & a) ^ b;
            }
        } else {
            for ((&a, &b), out) in and.iter().zip(xor.iter()).zip(&mut scratch[last as usize..][..len]) {
                value = (value & a) ^ b;
                *out = f64::from(value);
            }
        }
        phis[0] = f64::from(value);
    }

    /// The chain of int32 bitwise links on int32 words: each link's ToInt32 of its operand is
    /// the operand itself for an int32 Number (one conversion and compare), the recurrence's value
    /// stays a word between links, and only result columns a later reader needs see a Number.
    fn run_int32(&self, phis: &mut [f64], scratch: &mut [f64], len: usize) {
        if len == 0 {
            return;
        }
        let base = scratch.as_mut_ptr();
        let mut value = int32_word(phis[0]);
        for k in 0..len {
            // SAFETY: as `run`'s.
            unsafe {
                let row = base.add(k);
                for link in &self.links {
                    let Code::Bits { op, reversed } = link.code else { unreachable!("an int32 chain's links are bitwise") };
                    let x = int32_word(*row.add(link.operand as usize));
                    let (left, right) = if reversed { (x, value) } else { (value, x) };
                    value = crate::vm::operations::int32_bitwise_bits(op, left, right);
                    *row.add(link.out as usize) = f64::from(value);
                }
            }
        }
        phis[0] = f64::from(value);
    }
}

/// A part of an integer chain's per-iteration pair `v -> v * a + b` (`Chain::run_wrap`): the same
/// word every iteration, an induction's `start + step * k`, or a column of words.
#[derive(Clone, Copy)]
enum Part {
    Const(i32),
    Linear { start: i32, step: i32 },
    Column,
}

impl Part {
    #[inline(always)]
    fn at(self, k: usize, column: &[i32]) -> i32 {
        match self {
            Self::Const(c) => c,
            Self::Linear { start, step } => start.wrapping_add(step.wrapping_mul(k as i32)),
            Self::Column => column[k],
        }
    }

    /// Writes the part's words into `column` (a column part's words are there already).
    fn fill(self, column: &mut [i32]) {
        match self {
            Self::Const(c) => column.fill(c),
            Self::Linear { .. } => {
                for (k, o) in column.iter_mut().enumerate() {
                    *o = self.at(k, &[]);
                }
            }
            Self::Column => {}
        }
    }

    fn neg(self, column: &mut [i32]) -> Self {
        match self {
            Self::Const(c) => Self::Const(c.wrapping_neg()),
            Self::Linear { start, step } => Self::Linear { start: start.wrapping_neg(), step: step.wrapping_neg() },
            Self::Column => {
                column.iter_mut().for_each(|o| *o = o.wrapping_neg());
                Self::Column
            }
        }
    }

    fn scale(self, c: i32, column: &mut [i32]) -> Self {
        match self {
            Self::Const(v) => Self::Const(v.wrapping_mul(c)),
            Self::Linear { start, step } => Self::Linear { start: start.wrapping_mul(c), step: step.wrapping_mul(c) },
            Self::Column => {
                column.iter_mut().for_each(|o| *o = o.wrapping_mul(c));
                Self::Column
            }
        }
    }

    /// `self + other`, with a column part's words in `column` (self's) and `x` (other's).
    fn add(self, other: Self, column: &mut [i32], x: &[i32]) -> Self {
        match (self, other) {
            (Self::Const(a), Self::Const(b)) => Self::Const(a.wrapping_add(b)),
            (Self::Const(c), Self::Linear { start, step }) | (Self::Linear { start, step }, Self::Const(c)) => {
                Self::Linear { start: start.wrapping_add(c), step }
            }
            (Self::Linear { start: s1, step: t1 }, Self::Linear { start: s2, step: t2 }) => {
                Self::Linear { start: s1.wrapping_add(s2), step: t1.wrapping_add(t2) }
            }
            _ => {
                self.fill(column);
                for (k, o) in column.iter_mut().enumerate() {
                    *o = o.wrapping_add(other.at(k, x));
                }
                Self::Column
            }
        }
    }
}

/// The recurrence `v = last(v * a(k) + b(k), k)` over `len` iterations from `value`, writing
/// each value to `out` where `WRITE`.
#[inline(always)]
fn sequence<const WRITE: bool>(mut value: i32, len: usize, a: impl Fn(usize) -> i32, b: impl Fn(usize) -> i32,
    last: impl Fn(i32, usize) -> i32, out: &mut [f64], unsigned: bool) -> i32 {
    for k in 0..len {
        value = last(value.wrapping_mul(a(k)).wrapping_add(b(k)), k);
        if WRITE {
            out[k] = if unsigned { f64::from(value as u32) } else { f64::from(value) };
        }
    }
    value
}

/// Column nodes only one pass reads, whose int32 words it computes itself (`eval`): iotas from
/// their inductions (`iotas`: node, affine slot), and bitwise nodes over scalars and columns in
/// node order (`fused`). Their binary64 columns are written only where the pass falls back.
pub(super) struct Fusion {
    iotas: Vec<(u16, u8)>,
    fused: Vec<(u16, NumberBitwise, Arg, Arg)>,
}

impl Fusion {
    fn contains(&self, id: usize) -> bool {
        self.iotas.iter().any(|&(n, _)| usize::from(n) == id) || self.fused.iter().any(|&(n, ..)| usize::from(n) == id)
    }

    /// The binary64 columns of the fused nodes, for a pass's fallback.
    fn materialize(&self, scalars: &[f64], scratch: &mut [f64], len: usize, iotas: Iotas) {
        for &(n, slot) in &self.iotas {
            let (first, step) = (iotas.firsts[usize::from(slot)], iotas.steps[usize::from(slot)]);
            for (k, out) in scratch[usize::from(n) * CHUNK..][..len].iter_mut().enumerate() {
                *out = (first + step * (iotas.done + k as u64) as i64) as f64;
            }
        }
        for &(n, op, ..) in &self.fused {
            let mut words = [0i32; CHUNK];
            self.eval(Arg::Col(n), scalars, scratch, len, iotas, &mut words[..len]);
            let unsigned = op == NumberBitwise::ShiftRightUnsigned;
            for (out, &word) in scratch[usize::from(n) * CHUNK..][..len].iter_mut().zip(&words[..len]) {
                *out = if unsigned { f64::from(word as u32) } else { f64::from(word) };
            }
        }
    }

    /// The int32 words (ToInt32) of `arg` over the chunk into `out`: a fused node's computed
    /// here from its operands, an exclusive iota's from its induction, a scalar's broadcast, and
    /// any other column's converted.
    fn eval(&self, arg: Arg, scalars: &[f64], scratch: &[f64], len: usize, iotas: Iotas, out: &mut [i32]) {
        match arg {
            Arg::Scalar(slot) => out.fill(int32_word(scalars[usize::from(slot)])),
            Arg::Col(n) => {
                if let Some(&(_, slot)) = self.iotas.iter().find(|&&(m, _)| m == n) {
                    let (first, step) = (iotas.firsts[usize::from(slot)], iotas.steps[usize::from(slot)]);
                    let (mut x, step) = ((first + step * iotas.done as i64) as i32, step as i32);
                    for o in out.iter_mut() {
                        *o = x;
                        x = x.wrapping_add(step);
                    }
                } else if let Some(&(_, op, a, b)) = self.fused.iter().find(|&&(m, ..)| m == n) {
                    self.eval(a, scalars, scratch, len, iotas, out);
                    let mut right = [0i32; CHUNK];
                    let right = &mut right[..len];
                    let constant = match b {
                        Arg::Scalar(slot) => Some(int32_word(scalars[usize::from(slot)])),
                        _ => {
                            self.eval(b, scalars, scratch, len, iotas, right);
                            None
                        }
                    };
                    macro_rules! combine {
                        ($f:expr) => {
                            match constant {
                                Some(c) => out.iter_mut().for_each(|o| *o = $f(*o, c)),
                                None => out.iter_mut().zip(right.iter()).for_each(|(o, &r)| *o = $f(*o, r)),
                            }
                        };
                    }
                    match op {
                        NumberBitwise::And => combine!(|l: i32, r: i32| l & r),
                        NumberBitwise::Or => combine!(|l: i32, r: i32| l | r),
                        NumberBitwise::Xor => combine!(|l: i32, r: i32| l ^ r),
                        _ => combine!(|l: i32, r: i32| crate::vm::operations::int32_bitwise_bits(op, l, r)),
                    }
                } else {
                    for (o, &x) in out.iter_mut().zip(&scratch[usize::from(n) * CHUNK..][..len]) {
                        *o = int32_word(x);
                    }
                }
            }
            Arg::Phi(_) | Arg::Affine(_) => unreachable!("a fused node reads columns and scalars"),
        }
    }

}

/// Whether node `n` only the chain reads (one reader, no later reader), and then marks it fused:
/// an iota (`iotas`), or a bitwise column node whose operands are scalars or columns (`fused`,
/// its operands fused first where they can be). `false` where `n` keeps its column.
fn fuse(shape: &Shape, side: &[u16], n: u16, iotas: &mut Vec<(u16, u8)>, fused: &mut Vec<(u16, NumberBitwise, Arg, Arg)>) -> bool {
    let node = shape.nodes[usize::from(n)];
    let readers = shape.nodes.iter().map(|other| usize::from(other.a == Arg::Col(n)) + usize::from(other.b == Arg::Col(n))).sum::<usize>();
    if readers != 1 || read_later(shape, side, n) || node.seq || node.dead {
        return false;
    }
    if iotas.iter().any(|&(m, _)| m == n) || fused.iter().any(|&(m, ..)| m == n) {
        return true;
    }
    match node.op {
        NodeOp::Iota { index } => {
            iotas.push((n, index));
            true
        }
        NodeOp::Bits(op) if !node.hoisted => {
            let leaf = |arg: Arg| matches!(arg, Arg::Scalar(_) | Arg::Col(_));
            if !leaf(node.a) || !leaf(node.b) {
                return false;
            }
            for arg in [node.a, node.b] {
                if let Arg::Col(m) = arg {
                    fuse(shape, side, m, iotas, fused);
                }
            }
            fused.push((n, op, node.a, node.b));
            true
        }
        _ => false,
    }
}

fn is_broadcast(broadcasts: &[(u8, u32)], offset: u32) -> bool {
    broadcasts.iter().any(|&(_, o)| o == offset)
}

/// ToInt32 of a Number: the Number itself for an int32 (one conversion and compare).
#[inline(always)]
fn int32_word(x: f64) -> i32 {
    super::run::exact_int32(x).unwrap_or_else(|| crate::units::to_int32(x))
}
