//! Receivers proved direct objects once per value. A field access whose site proves one own flat entry
//! (`Decoded::ic`) leaves its receiver register holding a direct object on every edge out of it: its
//! inline-cache row's hit proved it, and a miss runs the site row and then the proof row, which leaves
//! the code unless the receiver is one (`plan::ic_plan`). A register keeps the value until it is written, so
//! a later access of it, or of a copy of it (`dst = src`), reached on every path through such an access,
//! needs neither the family test nor the record table test: one compare of the record's auxiliary word with
//! the entry's shape proves the shape and flat plain values (`Heap::IcKnownGet`, `Heap::IcKnownSet`), as a C
//! struct access over a tagged union costs. A loop's header joins the entry, where nothing is proved, and the
//! code is not resumed at a row a proof the body established reaches (`Decoded::relies`): a resume skips the
//! access. A proof the entry seed alone reaches needs no such gate: a resume stub re-runs the declared entry rows.

use super::*;

/// What one row runs knowing: the registers whose value was proved a direct object, by the register that
/// defined the value (a copy's proof is its source's), the subset an access in the body proved (`established`,
/// so `objects` minus `established` is the entry seed's alone), the copies still holding their source's value, and the
/// class-typed receivers whose exact shape and prototype link hold here: the entry proved them, or a held read checked
/// them, and no row since may change an object's shape (`may_reshape`).
#[derive(Clone, Default, PartialEq)]
struct Proved {
    objects: Vec<Register>,
    established: Vec<Register>,
    copies: Vec<(Register, Register)>,
    shapes: Vec<Register>,
}

impl Proved {
    /// The register whose value `register` holds: its source where it is a live copy.
    fn root(&self, register: Register) -> Register {
        self.copies.iter().find(|(copy, _)| *copy == register).map_or(register, |(_, source)| *source)
    }

    fn holds(&self, register: Register) -> bool {
        self.objects.contains(&self.root(register))
    }

    /// `register` written: its proof, its copy and the copies of its old value go.
    fn write(&mut self, register: Register) {
        self.objects.retain(|held| *held != register);
        self.established.retain(|held| *held != register);
        self.shapes.retain(|held| *held != register);
        self.copies.retain(|(copy, source)| *copy != register && *source != register);
    }
}

/// Whether `row` may change an object's exact shape or prototype link, or run user code that may: an adding, deleting or
/// defining store, a call, a helper step, a read that may run a getter (anything but the held reads and typed or prototype
/// checks), an `instanceof` or `@@hasInstance`, a native's holder call, a checked conversion (`valueOf`). A row that only
/// leaves the body on a miss (`Guard`, `Leave`) or is a numeric row over Numbers, a move or a jump keeps the shape proofs.
fn may_reshape(row: &Decoded) -> bool {
    let reads_alone = row.heap.is_some_and(|heap| matches!(heap, Heap::Held | Heap::ThisHeld | Heap::Declared | Heap::Prototype | Heap::Realm
        | Heap::TypedLength | Heap::TypedIndex(_) | Heap::TypedStore(_)));
    (row.heap.is_some() && !reads_alone) || row.step.is_some_and(|step| !matches!(step, Step::Guard(_) | Step::Leave))
        || row.add.is_some() || row.name.is_some() || row.instanceof.is_some() || row.holder.is_some() || row.global.is_some()
        || row.declined.is_some() || row.checked
}

/// The receiver register an inline-cache access names.
pub(super) fn receiver(row: &Decoded) -> Option<Register> {
    row.ic?;
    match row.heap? {
        Heap::Site => row.a.and_then(register_of),
        Heap::SiteStore => row.base,
        _ => None,
    }
}

/// The register a plain move copies into its destination.
fn copied(row: &Decoded) -> Option<Register> {
    (row.kind == Kind::Move && row.heap.is_none() && row.step.is_none() && !row.checked).then(|| row.a.and_then(register_of)).flatten()
}

/// What holds after `row`, given what held before it.
fn after(row: &Decoded, before: &Proved) -> Proved {
    let mut proved = before.clone();
    if may_reshape(row) {
        proved.shapes.clear();
    }
    if let Some(register) = receiver(row) {
        let root = proved.root(register);
        if !proved.objects.contains(&root) {
            proved.objects.push(root);
        }
        if !proved.established.contains(&root) {
            proved.established.push(root);
        }
    }
    let source = copied(row).map(|source| proved.root(source));
    for written in [row.dst, row.also].into_iter().flatten() {
        proved.write(written);
    }
    if let (Some(dst), Some(source)) = (row.dst, source) && dst != source {
        proved.copies.push((dst, source));
    }
    // A held read checked its receiver's shape and link, which hold after it (the check left the body otherwise).
    if row.class_read && let Some(register) = row.a.and_then(register_of) && !proved.shapes.contains(&register) {
        proved.shapes.push(register);
    }
    proved
}

/// The meet of two paths' proofs: what both hold.
fn meet(a: &Proved, b: &Proved) -> Proved {
    Proved {
        objects: a.objects.iter().filter(|held| b.objects.contains(held)).copied().collect(),
        established: a.established.iter().filter(|held| b.established.contains(held)).copied().collect(),
        copies: a.copies.iter().filter(|copy| b.copies.contains(copy)).copied().collect(),
        shapes: a.shapes.iter().filter(|held| b.shapes.contains(held)).copied().collect(),
    }
}

/// Marks each reached row whose receiver every path proves a direct object (`Decoded::proven`), and
/// each row a proof the body established reaches (`Decoded::relies`), which the code must not be resumed at.
/// `entry` holds the registers the entry proves plain direct objects and the function never writes
/// (`declared::exact_objects`): they are proved at every row, from the entry on, and a resume stub
/// re-establishes them with the declared entry rows. `shaped` holds the class-typed receivers the entry checked for
/// exact shape and prototype link (`declared::class_prototypes`): a held read of one becomes a constant load on every
/// path where no row since may change the shape (`may_reshape`).
pub(super) fn prove(span: &Span, rows: &mut [Decoded], reached: &Bits, entry: &[Register], shaped: &[Register]) {
    let count = rows.len();
    // `None`: no path reaches the row yet (the top of the meet).
    let mut before: Vec<Option<Proved>> = vec![None; count];
    if count == 0 {
        return;
    }
    before[0] = Some(Proved { objects: entry.to_vec(), shapes: shaped.to_vec(), ..Proved::default() });
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
    for (k, row) in rows.iter_mut().enumerate() {
        let held = before[k].take().unwrap_or_default();
        if !reached.contains(k) {
            continue;
        }
        row.relies = !held.established.is_empty();
        row.proven = receiver(row).is_some_and(|register| held.holds(register));
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
