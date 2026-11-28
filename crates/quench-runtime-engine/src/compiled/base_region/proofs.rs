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

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// What one row runs knowing: the registers whose value was proved a direct object, by the register that
/// defined the value (a copy's proof is its source's), the subset an access in the body proved (`established`,
/// so `objects` minus `established` is the entry seed's alone), the copies still holding their source's value, and the
/// class-typed receivers whose exact shape and prototype link hold here: the entry proved them, or a held read checked
/// them, and no row since may change an object's shape (`may_reshape`). `flats`: the loop-invariant receivers whose entry
/// proved their exact shape in flat form too, held and killed with `shapes`. `loop_held`: every loop-invariant receiver whose
/// entry checked its exact shape (`flats` among them), killed with `shapes`; the only shapes an access may leave unkilled
/// (`shape_hit`). `stores`: the entry checked that the run stores
/// specialized (`guard::stores`), and no row since has run code that could rewrite that word (`runs_code`): a store the
/// proofs admit needs no test of its own. `released`: a call on some path cleared `stores`, and no store re-read the word
/// since (`Decoded::recheck`).
#[derive(Clone, Default, PartialEq)]
struct Proved {
    objects: Vec<Register>,
    established: Vec<Register>,
    copies: Vec<(Register, Register)>,
    shapes: Vec<Register>,
    flats: Vec<Register>,
    loop_held: Vec<Register>,
    stores: bool,
    released: bool,
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
        self.flats.retain(|held| *held != register);
        self.loop_held.retain(|held| *held != register);
        self.copies.retain(|(copy, source)| *copy != register && *source != register);
    }
}

/// Whether `row` may change an object's exact shape or prototype link, or run user code that may: an adding, deleting or
/// defining store, a call, a helper step, a read that may run a getter (anything but the held reads and typed or prototype
/// checks), an `instanceof` or `@@hasInstance`, a native's holder call, a checked conversion (`valueOf`). A row that only
/// leaves the body on a miss (`Guard`, `Leave`) or is a numeric row over Numbers, a move or a jump keeps the shape proofs.
fn may_reshape(row: &Decoded) -> bool {
    let reads_alone = row.heap.is_some_and(|heap| matches!(heap, Heap::Held | Heap::ThisHeld | Heap::Declared | Heap::Prototype | Heap::Class | Heap::Realm
        | Heap::TypedLength | Heap::TypedIndex(_) | Heap::TypedStore(_)));
    (row.heap.is_some() && !reads_alone) || row.step.is_some_and(|step| !matches!(step, Step::Guard(_) | Step::Leave | Step::Allocate))
        || row.add.is_some() || row.instanceof.is_some() || row.holder.is_some() || row.global.is_some()
        || row.declined.is_some() || row.checked
}

/// Whether `row` may rewrite the run's stores word: code that may run in a nested run (a call, a helper, an `instanceof`,
/// a checked conversion, a declined store's step). An allocation runs no guest code and rewrites no object, so it clears
/// nothing.
fn clears_stores(row: &Decoded) -> bool {
    (row.makes_calls() && row.step != Some(Step::Allocate)) || row.instanceof.is_some() || row.checked || row.declined.is_some()
}

/// The receiver register an inline-cache access names.
pub(crate) fn receiver(row: &Decoded) -> Option<Register> {
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

/// Whether `row` is an inline-cache access the plan runs with its cache row (`Decoded::takes_ic_row`) whose receiver `held`
/// proves an object of a loop receiver's shape (`loop_held`). Such a row cannot reshape: its cache hit compares the shape
/// the entry checked, and the site row on a miss reads or writes an existing data slot. Only loop receivers qualify: their
/// cache entry's shape is the one every read of the register saw (`osr_compile::loop_shapes`). A class receiver's entry shape
/// is not recorded in the row, so it gets no exemption.
fn shape_hit(row: &Decoded, held: &Proved) -> bool {
    row.takes_ic_row() && receiver(row).is_some_and(|register| held.holds(register) && held.loop_held.contains(&register))
}

/// What holds after `row`, given what held before it.
fn after(row: &Decoded, before: &Proved) -> Proved {
    let mut proved = before.clone();
    if may_reshape(row) {
        if !shape_hit(row, before) {
            proved.shapes.clear();
            proved.flats.clear();
            proved.loop_held.clear();
        } else if matches!(row.heap, Some(Heap::SiteStore)) {
            // A store writes a slot a folded prototype read (`constant_read`) may hold, so the class shapes go; the
            // loop receivers' flat shapes stay, the write changing no shape or form.
            proved.shapes.clear();
        }
    }
    if clears_stores(row) {
        proved.stores = false;
        proved.released |= row.makes_calls();
    }
    // A recheck re-reads the stores word before the store, so the flag holds after it.
    if recheck_store(row, before) {
        proved.stores = true;
        proved.released = false;
    }
    established(&mut proved, row);
    let source = copied(row).map(|source| proved.root(source));
    for written in [row.dst, row.also].into_iter().flatten() {
        proved.write(written);
    }
    allocated(&mut proved, row);
    typed_object(&mut proved, row);
    if let (Some(dst), Some(source)) = (row.dst, source) && dst != source {
        proved.copies.push((dst, source));
    }
    // A held read checked its receiver's shape and link, which hold after it (the check left the body otherwise).
    if row.class_read && let Some(register) = row.a.and_then(register_of) && !proved.shapes.contains(&register) {
        proved.shapes.push(register);
    }
    proved
}

/// The receiver an inline-cache access names is a direct object after it, established by the body.
fn established(proved: &mut Proved, row: &Decoded) {
    if let Some(register) = receiver(row) {
        let root = proved.root(register);
        if !proved.objects.contains(&root) {
            proved.objects.push(root);
        }
        if !proved.established.contains(&root) {
            proved.established.push(root);
        }
    }
}

/// The destination of a typed object read (`Decoded::typed` of the object family) is a direct object after it: the
/// inline-cache row's hit delivers a word the slot's dependency holds to be one, and the site row's path tests it
/// (`plan::ic_plan`). The rows after it then test no family for it, as for a receiver the access proved.
fn typed_object(proved: &mut Proved, row: &Decoded) {
    let Some(dst) = row.dst.filter(|_| row.typed.is_some_and(|dependency| dependency.ty == quench_runtime::typed_ir::Type::DIRECT_OBJECT_FAMILY)) else { return };
    if !proved.objects.contains(&dst) {
        proved.objects.push(dst);
    }
    if !proved.established.contains(&dst) {
        proved.established.push(dst);
    }
}

/// The proof an allocation row leaves: the object it defines is a direct object, as the record `Step::Allocate` makes is,
/// so the rows after it test no family for it. Its exact shape is not proved here (the literal site's shape is a fact the
/// code would have to depend on).
fn allocated(proved: &mut Proved, row: &Decoded) {
    if let Some(dst) = row.dst.filter(|_| row.step == Some(Step::Allocate)) {
        proved.objects.push(dst);
        proved.established.push(dst);
    }
}

/// The meet of two paths' proofs: what both hold.
fn meet(a: &Proved, b: &Proved) -> Proved {
    Proved {
        objects: a.objects.iter().filter(|held| b.objects.contains(held)).copied().collect(),
        established: a.established.iter().filter(|held| b.established.contains(held)).copied().collect(),
        copies: a.copies.iter().filter(|copy| b.copies.contains(copy)).copied().collect(),
        shapes: a.shapes.iter().filter(|held| b.shapes.contains(held)).copied().collect(),
        flats: a.flats.iter().filter(|held| b.flats.contains(held)).copied().collect(),
        loop_held: a.loop_held.iter().filter(|held| b.loop_held.contains(held)).copied().collect(),
        stores: a.stores && b.stores,
        released: a.released || b.released,
    }
}

/// Whether `row`, reached with `held`, is a store the inline-cache known-set row takes: its receiver `held` proves a direct
/// object, and it names no second register and is not declined (so no step runs in its place).
fn admitted_store(row: &Decoded, held: &Proved) -> bool {
    matches!(row.heap, Some(Heap::SiteStore)) && row.also.is_none() && row.declined.is_none() && receiver(row).is_some_and(|register| held.holds(register))
}

/// Whether `row`, reached with `held`, re-reads the stores word before it stores (`Decoded::recheck`): an admitted store
/// whose flag a call cleared.
fn recheck_store(row: &Decoded, held: &Proved) -> bool {
    !held.stores && held.released && admitted_store(row, held)
}

/// The rows of `from`'s successors that are rows of the region.
fn next_rows(rows: &[Decoded], span: &Span, from: usize) -> Vec<usize> {
    rows[from].successors(from, rows.len(), span).filter_map(|next| match next {
        Next::Row(k) => Some(k),
        Next::Leave(_) => None,
    }).collect()
}

/// Whether a proven store follows `from` before the next row that may rewrite the stores word: the re-read at `from`
/// then admits it too, so the re-read is worth its test.
fn admits_after(rows: &[Decoded], span: &Span, before: &[Option<Proved>], from: usize) -> bool {
    let mut seen = Bits::default();
    let mut pending = next_rows(rows, span, from);
    while let Some(k) = pending.pop() {
        if seen.contains(k) {
            continue;
        }
        seen.insert(k);
        let Some(held) = before[k].as_ref() else { continue };
        if admitted_store(&rows[k], held) {
            return true;
        }
        if !clears_stores(&rows[k]) {
            pending.extend(next_rows(rows, span, k));
        }
    }
    false
}

/// The rows that re-read the stores word before they store (`Decoded::recheck`): each store `recheck_store` takes, where
/// a proven store follows it before the next kill (`admits_after`). Other rows are false.
fn rechecks(rows: &[Decoded], span: &Span, before: &[Option<Proved>]) -> Vec<bool> {
    (0..rows.len()).map(|k| before[k].as_ref().is_some_and(|held| recheck_store(&rows[k], held)) && admits_after(rows, span, before, k)).collect()
}

/// Marks each reached row whose receiver every path proves a direct object (`Decoded::proven`), and
/// each row a proof the body established reaches (`Decoded::relies`), which the code must not be resumed at.
/// `entry` holds the registers the entry proves plain direct objects and the function never writes
/// (`declared::exact_objects`): they are proved at every row, from the entry on, and a resume stub
/// re-establishes them with the declared entry rows. `shaped` holds the class-typed receivers the entry checked for
/// exact shape and prototype link (`declared::class_prototypes`): a held read of one becomes a constant load on every
/// path where no row since may change the shape (`may_reshape`). `flat` holds the loop-invariant receivers the entry checked
/// in flat form too (`Known::shapes`): a proven access of one of them is `Decoded::flat`, with no compare. Each proven store the run-level stores test admits, with no
/// call since the entry, is marked `Decoded::admitted`; a store a call cleared the test for re-reads the word (`Decoded::recheck`)
/// where a proven store follows it before the next such clear.
pub(crate) fn prove(span: &Span, rows: &mut [Decoded], reached: &Bits, entry: &[Register], shaped: &[Register], loops: &[Register], flat: &[Register]) {
    let count = rows.len();
    // `None`: no path reaches the row yet (the top of the meet).
    let mut before: Vec<Option<Proved>> = vec![None; count];
    if count == 0 {
        return;
    }
    // The entry tests the run's stores (`Heap::Stores`) wherever a row is admitted, so the seed holds it.
    before[0] = Some(Proved { objects: entry.to_vec(), shapes: shaped.to_vec(), flats: flat.to_vec(), loop_held: loops.to_vec(), stores: true, ..Proved::default() });
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
    settle(rows, span, reached, &mut before);
}

/// Each reached row's proofs, from the `before` of the fixpoint: its receiver, its re-read (`Decoded::recheck`), its
/// admission, and the constant reads the shapes settle.
fn settle(rows: &mut [Decoded], span: &Span, reached: &Bits, before: &mut [Option<Proved>]) {
    let rechecked = rechecks(rows, span, before);
    for (k, row) in rows.iter_mut().enumerate() {
        let held = before[k].take().unwrap_or_default();
        if !reached.contains(k) {
            continue;
        }
        row.relies = !held.established.is_empty();
        row.proven = receiver(row).is_some_and(|register| held.holds(register));
        row.flat = row.proven && receiver(row).is_some_and(|register| held.flats.contains(&register));
        row.recheck = rechecked[k];
        row.admitted = row.proven && matches!(row.heap, Some(Heap::SiteStore)) && (held.stores || row.recheck);
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
