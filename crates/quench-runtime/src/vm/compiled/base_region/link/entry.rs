use super::*;
use super::super::declared::EntryGuard;

/// An entry row: its stencil and hole values, and where its miss goes. A Number guard's miss is its
/// taken successor; a declared test's (`Heap::Declared`) is its exit 0. Either leaves to the failed entry.
pub(in super::super) struct EntryRow {
    pub(in super::super) stencil: &'static Stencil,
    pub(in super::super) holes: [u64; HOLES],
    declared: bool,
    /// The register a Number guard checks (its orig register); `None` for any other row.
    pub(in super::super) number: Option<Register>,
}

impl EntryRow {
    /// The taken successor of the row: the failed entry's leave, unless its miss is an exit.
    pub(in super::super) fn taken(&self, failed: usize) -> Option<usize> {
        (!self.declared).then_some(failed)
    }

    /// The exits of the row: exit 0 to the failed entry's leave, for a declared test.
    pub(in super::super) fn exits(&self, failed: usize) -> Vec<Target> {
        if self.declared { vec![Target::Row(failed)] } else { Vec::new() }
    }
}

/// The bit of `register` in a guard mask (`RowEntries::numbers`): a register past the mask's width sets every bit, so
/// no call covers it.
fn register_bit(register: Register) -> u64 {
    if usize::from(register) < u64::BITS as usize { 1 << register } else { u64::MAX }
}

/// The parameter registers the Number guards of `entry` check, as a mask (`RowEntries::numbers`).
pub(in super::super) fn guard_mask(entry: &[EntryRow]) -> u64 {
    entry.iter().filter_map(|row| row.number).fold(0, |mask, register| mask | register_bit(register))
}

/// The entry rows: the Number guards first, then the rest (the realm, each entry tag and check). Each pinned register
/// live at the header (or, for a resume stub, at its PC; `entered` are the webs live there) loads its int32, each int
/// slot is checked (or, dead there, tagged), and each other Number register live there is checked, so a loop whose
/// values are no Numbers is never entered. The guards come first so a body's proven-arguments entry starts past them
/// (`EntryRow::number`); the rest only test or write slots no Number guard reads, so their order does not change what
/// they check. Then each declared parameter's entry test (`layout.guards`), and each class-typed parameter's receiver
/// test (`layout.prototypes`: exact shape and prototype link).
/// `at` is the row the entry enters at (0 for the body's entry, a stub's PC for a resume stub): a Mem
/// slot's Number guard is emitted only where `layout.entry_reads` says a row reads its entry value.
pub(in super::super) fn entry_rows(layout: &Layout, entered: &[usize], at: usize) -> Result<Vec<EntryRow>, String> {
    let guard = |kind, dst, offset: u64, register: Register| {
        numeric::stencil(numeric::Row { kind, dst, a: Place::Mem, b: Place::Mem, speculate: true }).map(|stencil| EntryRow {
            stencil,
            holes: padded_holes(if kind == Kind::TagInit { [offset, 0, 0] } else { [0, offset, 0] }),
            declared: false,
            number: (kind == Kind::Guard).then_some(register),
        })
    };
    let orig = |web: Register| layout.homes.orig[usize::from(web)];
    let slot = |web: Register| u64::from(orig(web)) * SLOT;
    let mut rows = Vec::new();
    if let Some(realm) = layout.realm {
        let row = HeapRow { heap: Heap::Realm, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
        rows.push(EntryRow { stencil: base_rows::stencil(row)?, holes: padded_holes([0, realm.offset, realm.global]), declared: false, number: None });
    }
    for (pin, web) in layout.homes.pins.iter().filter(|(_, web)| entered.contains(&usize::from(*web))) {
        rows.push(guard(Kind::Guard, Place::Pin(*pin as u8), slot(*web), orig(*web))?);
    }
    for (pin, web) in layout.homes.floats.iter().filter(|(_, web)| entered.contains(&usize::from(*web))) {
        rows.push(guard(Kind::Guard, Place::PinF(*pin as u8), slot(*web), orig(*web))?);
    }
    for web in &layout.homes.slots {
        let live = entered.contains(&usize::from(*web));
        rows.push(guard(if live { Kind::Guard } else { Kind::TagInit }, Place::MemInt, slot(*web), orig(*web))?);
    }
    for web in entered.iter().map(|web| *web as Register) {
        if layout.homes.place(web) == Place::Mem && layout.entry_reads.guards(at, usize::from(orig(web))) {
            rows.push(guard(Kind::Guard, Place::Mem, slot(web), orig(web))?);
        }
    }
    let test = HeapRow { heap: Heap::Declared, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    for EntryGuard { register, languages } in &layout.guards {
        rows.push(EntryRow { stencil: base_rows::stencil(test)?, holes: padded_holes([u64::from(*register) * SLOT, u64::from(*languages), 0]), declared: true, number: None });
    }
    let prototype = HeapRow { heap: Heap::Prototype, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    for proto in &layout.prototypes {
        let receiver = u64::from(proto.shape) | (u64::from(proto.link) << numeric::library::HELD_LINK_AT);
        rows.push(EntryRow { stencil: base_rows::stencil(prototype)?, holes: padded_holes([u64::from(proto.register) * SLOT, receiver, 0]), declared: true, number: None });
    }
    for proto in &layout.shapes {
        let heap = if proto.flat { Heap::ShapeFlat } else { Heap::Shape };
        let shape = HeapRow { heap, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
        rows.push(EntryRow { stencil: base_rows::stencil(shape)?, holes: padded_holes([u64::from(proto.register) * SLOT, u64::from(proto.shape), 0]), declared: true, number: None });
    }
    if layout.stores {
        let stores = HeapRow { heap: Heap::Stores, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
        rows.push(EntryRow { stencil: base_rows::stencil(stores)?, holes: padded_holes([0, 0, 0]), declared: true, number: None });
    }
    let identity = HeapRow { heap: Heap::CaptureIdentity, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    for (slot, callee) in &layout.identities {
        rows.push(EntryRow { stencil: base_rows::stencil(identity)?, holes: padded_holes([u64::from(*slot), *callee, 0]), declared: true, number: None });
    }
    let (mut numbers, rest): (Vec<EntryRow>, Vec<EntryRow>) = rows.into_iter().partition(|row| row.number.is_some());
    numbers.extend(rest);
    Ok(numbers)
}
