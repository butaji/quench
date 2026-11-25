//! Which checked operands a row finds a Number in, on every path into it: the operand of a `ToNumeric`
//! or unary `+` (`Decoded::checked`) whose number guard (`function::preludes`) is redundant.
//!
//! This runs the same kind of fact analysis as the u-jit's (`facts`), but its transfer is the b-jit's own:
//! a row continues natively where the u-jit's leaves (a string `+` takes `Variant::Concat` and writes a
//! string), so the u-jit's Number types are unsound for b-jit rows. Here a slot holds a Number only where the
//! row's stencil leaves on any other word (a numeric arithmetic row, a Number constant), or where an entry
//! guard checked the slot and no row wrote it since. Any other row writes an unknown value.
//!
//! The entry seeds are the entry guards (`link::entry::entry_rows`): a live web is a Number where its entry
//! row checks it. A resume stub (`resume::resume_stubs`) enters at its row with its own guards, so the state
//! there is the meet of the normal flow and the stub's guards. A stub the build later drops still meets,
//! which only loses facts. The state is keyed by frame slot (the web's original register), which is what the
//! guards check and what a write changes.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::repr::{variant_of, Variant};
use super::*;
use quench_runtime::dataflow::BitSet;

/// The frame slots (original registers) that hold a Number on every path into a row.
type Held = BitSet;

/// The numeric kinds whose stencil leaves on any operand that is no Number. `Concat`, the one other variant
/// of `Kind::Add`, is excluded where it is selected (`variant_of`).
fn leaves_on_other(kind: Kind) -> bool {
    matches!(kind, Kind::Add | Kind::Sub | Kind::Mul | Kind::Div | Kind::And | Kind::Or | Kind::Xor | Kind::Shl | Kind::Shr
        | Kind::UShr | Kind::Neg | Kind::Rem | Kind::Sqrt | Kind::Floor | Kind::Ceil | Kind::Trunc | Kind::Abs | Kind::Round
        | Kind::Min | Kind::Max | Kind::Imul | Kind::Clz32 | Kind::Fround | Kind::Sign | Kind::AddNoOverflow | Kind::SubNoOverflow)
}

/// Whether the entry at `row` checks the web `web` as a Number: the rule `entry_rows` applies to a live web.
fn checked_web(homes: &Homes, reads: &EntryReads, row: usize, web: usize) -> bool {
    match homes.place(web as Register) {
        Place::Pin(_) | Place::PinF(_) | Place::MemInt => true,
        Place::Mem => reads.guards(row, usize::from(homes.orig[web])),
        _ => false,
    }
}

/// The state an entry at `row` holds: the slots of its live webs that its guards check.
fn entry_state(span: &Span, homes: &Homes, webs: &Webs, reads: &EntryReads, row: usize) -> Held {
    let mut held = Held::with_capacity(homes.orig.len().max(1));
    for web in webs.live_at(span, row as u32) {
        if checked_web(homes, reads, row, web) {
            held.insert(usize::from(homes.orig[web]));
        }
    }
    held
}

/// Per row, whether the checked operand of a `checked` row is a Number on every path into it.
///
/// `reached` marks the rows control reaches, `kinds` is each row's site feedback (`variant_of`), `reads`
/// the entry guards the rows keep (`entry_reads`), and `entries` the first row of each caller PC a resume
/// stub enters at (none for a body, which only its entry enters).
pub(crate) fn proven_operands(span: &Span, decoded: &[Decoded], reached: &Bits, homes: &Homes, webs: &Webs,
    reads: &EntryReads, kinds: &[Option<OperandKinds>], ending: Ending, entries: &[(u32, u32)]) -> Vec<bool> {
    let state = solve(span, decoded, ending, entries, |row| entry_state(span, homes, webs, reads, row),
        |k, held| transfer(homes, &decoded[k], kinds.get(k).and_then(Option::as_ref), held));
    decoded.iter().enumerate().map(|(k, row)| {
        let (true, Some(Source::Register(web))) = (reached.contains(k) && row.checked, row.a) else { return false };
        state[k].as_ref().is_some_and(|held| held.contains(usize::from(homes.orig[usize::from(web)])))
    }).collect()
}

/// Per row, whether each operand (`a`, `b`) is an int32 on every path into it: the int32 counterpart of
/// `proven_operands`, over the same rows and entries.
///
/// Soundness: the frame slots change only through the registers a row writes (`dst`, `also`), the premise
/// `proven_operands` relies on. A slot holds an int32 only where its entry seed says so (a MemInt or int32 pinned
/// web, which the entry holds as an int32 word), or where the last write to it stored an int32 word: a copy or
/// constant of an int32, or a row whose result is always an int32 and which continues in its stencil
/// (`yields_int32`). Every other write kills the fact, and a meet keeps only the slots all paths hold.
pub(crate) fn proven_int_operands(span: &Span, decoded: &[Decoded], reached: &Bits, homes: &Homes, webs: &Webs,
    ending: Ending, entries: &[(u32, u32)]) -> Vec<(bool, bool)> {
    let state = solve(span, decoded, ending, entries, |row| int_entry_state(span, homes, webs, row),
        |k, held| int_transfer(homes, &decoded[k], held));
    let is_int = |k: usize, source: Option<Source>| match source {
        Some(Source::Register(web)) => reached.contains(k) && state[k].as_ref().is_some_and(|held| held.contains(usize::from(homes.orig[usize::from(web)]))),
        _ => false,
    };
    decoded.iter().enumerate().map(|(k, row)| (is_int(k, row.a), is_int(k, row.b))).collect()
}

/// The entry rows of a region: its header, and under `Ending::Region` the rows its resume stubs enter at.
fn entry_rows(span: &Span, count: usize, ending: Ending, entries: &[(u32, u32)]) -> Vec<usize> {
    let mut rows = vec![span.header as usize];
    if ending == Ending::Region {
        rows.extend(entries.iter().map(|(_, row)| *row as usize));
    }
    rows.sort_unstable();
    rows.dedup();
    rows.retain(|row| *row < count);
    rows
}

/// A forward problem over the rows of a region, for `dataflow::solve`: each row is a block, its edges are the rows
/// it reaches (a `Leave` is no edge), and its seed is the state an entry row starts with.
struct RowFlow<F> {
    edges: Vec<Vec<usize>>,
    seeds: Vec<Option<Held>>,
    step: F,
}

impl<F: Fn(usize, &Held) -> Held> quench_runtime::dataflow::Dataflow for RowFlow<F> {
    type State = Held;

    fn blocks(&self) -> usize {
        self.edges.len()
    }

    fn edges(&self, block: usize) -> &[usize] {
        &self.edges[block]
    }

    fn entry_state(&self, block: usize) -> Option<Held> {
        self.seeds[block].clone()
    }

    fn join(&self, into: &mut Held, incoming: &Held) -> bool {
        meet(into, incoming)
    }

    fn transfer(&mut self, block: usize, state: &Held) -> Held {
        (self.step)(block, state)
    }
}

/// The state on entry to each row of a forward analysis over the rows: `seed` gives an entry row's state and `step`
/// the state after a row from the state before it. Where paths meet, `meet` keeps the slots all of them hold. `None`
/// marks a row control does not reach.
fn solve(span: &Span, decoded: &[Decoded], ending: Ending, entries: &[(u32, u32)], seed: impl Fn(usize) -> Held,
    step: impl Fn(usize, &Held) -> Held) -> Vec<Option<Held>> {
    let count = decoded.len();
    let mut seeds: Vec<Option<Held>> = vec![None; count];
    for row in entry_rows(span, count, ending, entries) {
        seeds[row] = Some(seed(row));
    }
    let edges: Vec<Vec<usize>> = decoded.iter().enumerate().map(|(k, row)| {
        row.successors(k, count, span).filter_map(|next| match next {
            Next::Row(n) => Some(n),
            Next::Leave(_) => None,
        }).collect()
    }).collect();
    quench_runtime::dataflow::solve(&mut RowFlow { edges, seeds, step })
}

/// The int32 state an entry at `row` holds: the slots of its live webs whose home is an int32 (a MemInt slot or an
/// int32 pin), which the entry holds as an int32 word.
fn int_entry_state(span: &Span, homes: &Homes, webs: &Webs, row: usize) -> Held {
    let mut held = Held::with_capacity(homes.orig.len().max(1));
    for web in webs.live_at(span, row as u32) {
        if matches!(homes.place(web as Register), Place::MemInt | Place::Pin(_)) {
            held.insert(usize::from(homes.orig[web]));
        }
    }
    held
}

/// Whether a numeric kind's result is an int32 on every operand the row continues on: the bitwise, shift and
/// `Math.imul`/`Math.clz32` kinds, whose results are ToInt32 values. Unsigned shift, addition, subtraction and the
/// no-overflow kinds may yield a number no int32 holds.
fn yields_int32(kind: Kind) -> bool {
    matches!(kind, Kind::And | Kind::Or | Kind::Xor | Kind::Shl | Kind::Shr | Kind::Imul | Kind::Clz32)
}

/// The int32 state after `row` from `held`. A copy or constant keeps its source's int32-ness; a row `yields_int32`
/// writes an int32; anything else writes an unknown value.
fn int_transfer(homes: &Homes, row: &Decoded, held: &Held) -> Held {
    let slot = |web: Register| usize::from(homes.orig[usize::from(web)]);
    let source = |source: Option<Source>| match source {
        Some(Source::Register(web)) => held.contains(slot(web)),
        Some(Source::Constant(word) | Source::Value(word)) => Value::from_bits(word).as_int().is_some(),
        None => false,
    };
    let value = if row.heap.is_some() || row.step.is_some() {
        false
    } else if matches!(row.kind, Kind::Move | Kind::Const) {
        source(row.a)
    } else {
        yields_int32(row.kind)
    };
    let mut out = held.clone();
    for written in [row.dst, row.also].into_iter().flatten() {
        out.remove(slot(written));
    }
    if let (true, Some(dst)) = (value, row.dst) {
        out.insert(slot(dst));
    }
    out
}

/// The state after `row` from `held`. A copy keeps its source's Number-ness; a numeric row's result is a
/// Number where the row leaves on any other operand; anything else writes an unknown value.
fn transfer(homes: &Homes, row: &Decoded, kinds: Option<&OperandKinds>, held: &Held) -> Held {
    let slot = |web: Register| usize::from(homes.orig[usize::from(web)]);
    let source = |source: Option<Source>| match source {
        Some(Source::Register(web)) => held.contains(slot(web)),
        Some(Source::Constant(word) | Source::Value(word)) => Value::from_bits(word).as_number().is_some(),
        None => false,
    };
    let value = if row.heap.is_some() || row.step.is_some() {
        false
    } else if matches!(row.kind, Kind::Move | Kind::Const) {
        source(row.a)
    } else {
        leaves_on_other(row.kind) && variant_of(row.kind, kinds) != Variant::Concat
    };
    let mut out = held.clone();
    for written in [row.dst, row.also].into_iter().flatten() {
        out.remove(slot(written));
    }
    if let (true, Some(dst)) = (value, row.dst) {
        out.insert(slot(dst));
    }
    out
}

/// Meets `other` into `known`, keeping only the slots both hold; whether `known` changed.
fn meet(known: &mut Held, other: &Held) -> bool {
    let lost: Vec<usize> = known.iter().filter(|slot| !other.contains(*slot)).collect();
    for slot in &lost {
        known.remove(*slot);
    }
    !lost.is_empty()
}

/// Which entry guards of Mem slots a region keeps (`entry_rows`), by row and frame slot.
///
/// A Number guard checks the entry value of its slot. Its check is redundant where no row reads that value
/// natively before writing the slot (`entry_reads`). `kept` slots keep theirs whatever the rows read: a declared
/// Number parameter's rows rely on its guard, and an inlined frame's copies run at a leave.
pub(crate) struct EntryReads {
    /// The number-register slots (`Span::number_registers`), the only ones a Number guard checks.
    numbers: Held,
    kept: Held,
    /// Per row, the slots whose entry value a native read reaches before a write, on some path from the row.
    live: Vec<Held>,
}

impl EntryReads {
    /// Whether the entry at row `row` emits the Number guard of frame slot `slot`. A row past the analysis keeps it.
    pub(crate) fn guards(&self, row: usize, slot: usize) -> bool {
        self.numbers.contains(slot) && (self.kept.contains(slot) || self.live.get(row).is_none_or(|live| live.contains(slot)))
    }
}

/// Whether the row checks each operand it reads, so it reads no fact an entry guard provides: a bare numeric
/// row (`Decoded::number`, nothing else set) whose stencil leaves on any other operand (`leaves_on_other`), and
/// which is no `Concat` variant.
pub(crate) fn self_checking(row: &Decoded, kinds: Option<&OperandKinds>) -> bool {
    let bare = row.heap.is_none() && row.step.is_none() && row.global.is_none() && row.base.is_none() && row.also.is_none()
        && row.atom.is_none() && row.folded.is_none() && row.instanceof.is_none() && row.lexical.is_none() && row.add.is_none()
        && row.name.is_none() && row.ic.is_none() && row.declined.is_none() && row.holder.is_none() && row.cell.is_none() && !row.class_read
        && !row.proven && !row.checked && !row.admitted && !row.recheck;
    bare && leaves_on_other(row.kind) && variant_of(row.kind, kinds) != Variant::Concat
}

/// The slots of `from` in `into`.
fn union(into: &mut Held, from: &Held) {
    for slot in from.iter() {
        into.insert(slot);
    }
}

/// The entry reads of a region's rows: a backward analysis of the slots a native read reaches before a write.
/// A self-checking row reads nothing an entry must prove; any other row reads all its registers, and a row writes
/// its destinations, which end the entry value there.
pub(crate) fn entry_reads(span: &Span, decoded: &[Decoded], homes: &Homes, kinds: &[Option<OperandKinds>], kept: &[Register]) -> EntryReads {
    let count = decoded.len();
    let capacity = homes.orig.len().max(1);
    let slot = |web: Register| usize::from(homes.orig[usize::from(web)]);
    let mut live: Vec<Held> = vec![Held::with_capacity(capacity); count];
    let mut changed = true;
    while changed {
        changed = false;
        for k in (0..count).rev() {
            let row = &decoded[k];
            let mut before = Held::with_capacity(capacity);
            for next in row.successors(k, count, span) {
                if let Next::Row(n) = next {
                    union(&mut before, &live[n]);
                }
            }
            for written in [row.dst, row.also].into_iter().flatten() {
                before.remove(slot(written));
            }
            if !self_checking(row, kinds.get(k).and_then(Option::as_ref)) {
                for read in row.registers() {
                    before.insert(slot(read));
                }
            }
            if !before.iter().eq(live[k].iter()) {
                live[k] = before;
                changed = true;
            }
        }
    }
    EntryReads { numbers: slots(capacity, span.number_registers().iter().copied()), kept: slots(capacity, kept.iter().copied()), live }
}

/// The set of the frame slots `registers` name.
fn slots(capacity: usize, registers: impl Iterator<Item = Register>) -> Held {
    let mut held = Held::with_capacity(capacity);
    for register in registers {
        held.insert(usize::from(register));
    }
    held
}
