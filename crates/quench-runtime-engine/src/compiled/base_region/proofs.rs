//! Class-typed receivers' shapes. The entry checks a class-typed receiver's live class cell (`declared::class_prototypes`),
//! which fixes its exact shape and prototype link. A held read of that receiver (`Heap::Held`, `Decoded::class_read`) then
//! becomes a constant load on every path where no row since may change the shape or link: the prototype's value is the
//! constant the entry's class test justifies. Rows that run user code or reshape an object end the proof (`may_reshape`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// What one row runs knowing: the class-typed receivers whose exact shape and prototype link hold here, by register. The
/// entry checked them, or a held read did, and no row since may change an object's shape (`may_reshape`).
#[derive(Clone, Default, PartialEq)]
struct Proved {
    shapes: Vec<Register>,
}

/// Whether `row` may change an object's exact shape or prototype link, or run user code that may: an adding, deleting or
/// defining store, a call, a helper step, a read that may run a getter (anything but the held reads and typed or prototype
/// checks), an `instanceof` or `@@hasInstance`, a native's holder call, a checked conversion (`valueOf`). A row that only
/// leaves the body on a miss (`Guard`, `Leave`) or is a numeric row over Numbers, a move or a jump keeps the shape proofs.
fn may_reshape(row: &Decoded) -> bool {
    let reads_alone = row.heap.is_some_and(|heap| matches!(heap, Heap::Held | Heap::ThisHeld | Heap::Declared | Heap::Class | Heap::Realm
        | Heap::TypedLength | Heap::TypedIndex(_) | Heap::TypedStore(_)));
    (row.heap.is_some() && !reads_alone) || row.step.is_some_and(|step| !matches!(step, Step::Guard(_) | Step::Leave | Step::Allocate))
        || row.add.is_some() || row.holder.is_some() || row.global.is_some()
        || row.declined.is_some() || row.checked
}

/// What holds after `row`, given what held before it.
fn after(row: &Decoded, before: &Proved) -> Proved {
    let mut proved = before.clone();
    if may_reshape(row) {
        proved.shapes.clear();
    }
    // A held read checked its receiver's shape and link, which hold after it (the check left the body otherwise).
    if row.class_read && let Some(register) = row.a.and_then(register_of) && !proved.shapes.contains(&register) {
        proved.shapes.push(register);
    }
    proved
}

/// The meet of two paths' proofs: what both hold.
fn meet(a: &Proved, b: &Proved) -> Proved {
    Proved { shapes: a.shapes.iter().filter(|held| b.shapes.contains(held)).copied().collect() }
}

/// The rows of `from`'s successors that are rows of the region.
fn next_rows(rows: &[Decoded], span: &Span, from: usize) -> Vec<usize> {
    rows[from].successors(from, rows.len(), span).filter_map(|next| match next {
        Next::Row(k) => Some(k),
        Next::Leave(_) => None,
    }).collect()
}

/// Marks each reached row a class-typed receiver's held read whose shape and link hold at the row (`constant_read`). `shaped`
/// holds the class-typed receivers the entry checked for exact shape and prototype link (`declared::class_prototypes`): the
/// entry test (`Heap::Class`) fixes them, so the seed holds at the entry and every row until one may change the shape.
pub(crate) fn prove(span: &Span, rows: &mut [Decoded], reached: &Bits, shaped: &[Register]) {
    let count = rows.len();
    // `None`: no path reaches the row yet (the top of the meet).
    let mut before: Vec<Option<Proved>> = vec![None; count];
    if count == 0 {
        return;
    }
    before[0] = Some(Proved { shapes: shaped.to_vec() });
    let mut pending = vec![0];
    while let Some(k) = pending.pop() {
        let Some(held) = before[k].clone() else { continue };
        let out = after(&rows[k], &held);
        for next in rows[k].successors(k, count, span) {
            let Next::Row(next) = next else { continue };
            // The first row is the region's entry, where the entry seed holds. A back edge into it joins like any
            // other edge: a row since may have reshaped a class-typed receiver the seed proved (`may_reshape`), and
            // the seed's shape is then no longer held on the next iteration.
            let joined = match &before[next] {
                None => out.clone(),
                Some(known) => meet(known, &out),
            };
            if before[next].as_ref() != Some(&joined) {
                before[next] = Some(joined);
                pending.push(next);
            }
        }
    }
    settle(rows, reached, &mut before);
}

/// Each reached row's constant reads, from the `before` of the fixpoint.
fn settle(rows: &mut [Decoded], reached: &Bits, before: &mut [Option<Proved>]) {
    for (k, row) in rows.iter_mut().enumerate() {
        let held = before[k].take().unwrap_or_default();
        if !reached.contains(k) {
            continue;
        }
        if row.class_read && let Some(register) = row.a.and_then(register_of) && held.shapes.contains(&register) {
            constant_read(row);
        }
    }
}

/// A class receiver's held read whose shape and link hold at the row (`Proved::shapes`): the prototype's value as a
/// constant load of its tagged word. A Number value keeps the held read, a constant Number being no tagged word a row
/// writes as it is.
fn constant_read(row: &mut Decoded) {
    if Value::from_bits(row.hole).as_number().is_some() {
        return;
    }
    let value = std::mem::take(&mut row.hole);
    row.heap = None;
    row.kind = Kind::Move;
    row.a = Some(Source::Value(value));
    row.class_read = false;
}
