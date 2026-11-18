//! Receivers proved direct objects once per value. A field access whose site proves one own flat entry
//! (`Decoded::ic`) leaves its receiver register holding a direct object on every edge out of it: its
//! inline-cache row's hit proved it, and a miss runs the site row and then the proof row, which leaves
//! the code unless the receiver is one (`plan::ic_plan`). A register keeps the value until it is written, so
//! a later access of it, or of a copy of it (`dst = src`), reached on every path through such an access,
//! needs neither the family test nor the record table test: one compare of the record's auxiliary word with
//! the entry's shape proves the shape and flat plain values (`Heap::IcKnownGet`, `Heap::IcKnownSet`), as a C
//! struct access over a tagged union costs. A loop's header joins the entry, where nothing is proved, and the
//! code is not resumed at a row some proof reaches (`Decoded::relies`): a resume skips the access.

use super::*;

/// What one row runs knowing: the registers whose value was proved a direct object, by the register that
/// defined the value (a copy's proof is its source's), and the copies still holding their source's value.
#[derive(Clone, Default, PartialEq)]
struct Proved {
    objects: Vec<Register>,
    copies: Vec<(Register, Register)>,
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
        self.copies.retain(|(copy, source)| *copy != register && *source != register);
    }
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
    if let Some(register) = receiver(row) {
        let root = proved.root(register);
        if !proved.objects.contains(&root) {
            proved.objects.push(root);
        }
    }
    let source = copied(row).map(|source| proved.root(source));
    for written in [row.dst, row.also].into_iter().flatten() {
        proved.write(written);
    }
    if let (Some(dst), Some(source)) = (row.dst, source) && dst != source {
        proved.copies.push((dst, source));
    }
    proved
}

/// The meet of two paths' proofs: what both hold.
fn meet(a: &Proved, b: &Proved) -> Proved {
    Proved {
        objects: a.objects.iter().filter(|held| b.objects.contains(held)).copied().collect(),
        copies: a.copies.iter().filter(|copy| b.copies.contains(copy)).copied().collect(),
    }
}

/// Marks each reached row whose receiver every path proves a direct object (`Decoded::proven`), and
/// each row some proof reaches (`Decoded::relies`), which the code must not be resumed at.
pub(super) fn prove(span: &Span, rows: &mut [Decoded], reached: &Bits) {
    let count = rows.len();
    // `None`: no path reaches the row yet (the top of the meet).
    let mut before: Vec<Option<Proved>> = vec![None; count];
    if count == 0 {
        return;
    }
    before[0] = Some(Proved::default());
    let mut pending = vec![0];
    while let Some(k) = pending.pop() {
        let Some(held) = before[k].clone() else { continue };
        let out = after(&rows[k], &held);
        for next in rows[k].successors(k, count, span) {
            let Next::Row(next) = next else { continue };
            // The first row is the region's entry: nothing is proved there, whatever the back edge holds.
            if next == 0 {
                continue;
            }
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
        row.relies = !held.objects.is_empty();
        row.proven = receiver(row).is_some_and(|register| held.holds(register));
    }
}
