use super::*;
use super::super::declared::EntryGuard;

/// An entry row: its stencil and hole values, and where its miss goes. A Number guard's miss is its
/// taken successor; a declared test's (`Heap::Declared`) is its exit 0. Either leaves to the failed entry.
pub(in super::super) struct EntryRow {
    pub(in super::super) stencil: &'static Stencil,
    pub(in super::super) holes: [u64; HOLES],
    declared: bool,
    /// The register a Number guard checks (a Mem slot or a pinned double); `None` for any other row.
    pub(in super::super) number: Option<Register>,
    /// The register an int32 guard checks (a pinned int32 or an int slot); `None` for any other row.
    pub(in super::super) int32: Option<Register>,
    /// The unboxing row a pinned guard runs in the proven-arguments entry (`UNBOX` or `UNBOX_INT`, the guard's tag test left out);
    /// `None` for a slot guard, which writes nothing and that entry skips.
    pub(in super::super) unbox: Option<&'static Stencil>,
}

/// The bit of an int32 guard in a guard mask (`RowEntries::numbers`): a Number guard of parameter `r` sets bit `r`,
/// an int32 guard of `r` bit `INT32_BIT + r`. A register past `INT32_BIT` sets every bit, so no call covers it.
pub(in super::super) const INT32_BIT: u32 = u64::BITS / 2;

impl EntryRow {
    /// Whether the row is a Number or int32 guard, which the proven-arguments entry starts past.
    pub(in super::super) fn guards(&self) -> bool {
        self.number.is_some() || self.int32.is_some()
    }
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

/// The bit of a guard of `register` in a guard mask (`INT32_BIT`).
fn guard_bit(register: Register, int32: bool) -> u64 {
    let at = (if int32 { INT32_BIT } else { 0 }) + u32::from(register);
    if usize::from(register) < INT32_BIT as usize { 1 << at } else { u64::MAX }
}

/// The parameter registers the Number and int32 guards of `entry` check, as a mask (`RowEntries::numbers`).
pub(in super::super) fn guard_mask(entry: &[EntryRow]) -> u64 {
    let numbers = entry.iter().filter_map(|row| row.number).map(|register| guard_bit(register, false));
    let int32s = entry.iter().filter_map(|row| row.int32).map(|register| guard_bit(register, true));
    numbers.chain(int32s).fold(0, |mask, bit| mask | bit)
}

/// A row of a slot's entry guard (`kind`: `Guard`, or `TagInit` for a dead int slot) into `dst`, its source the frame slot at
/// `offset`; `register` is the orig register it checks. A pinned destination also gets its unboxing row (`Kind::Unbox`, `Kind::UnboxInt`).
fn number_guard(kind: Kind, dst: Place, offset: u64, register: Register) -> Result<EntryRow, String> {
    let row = |kind| numeric::Row { kind, dst, a: Place::Mem, b: Place::Mem, speculate: true };
    let unbox = match dst {
        Place::Pin(_) => Some(numeric::stencil(row(Kind::UnboxInt))?),
        Place::PinF(_) => Some(numeric::stencil(row(Kind::Unbox))?),
        _ => None,
    };
    let int32 = matches!(dst, Place::Pin(_) | Place::MemInt);
    let guarded = (kind == Kind::Guard).then_some(register);
    Ok(EntryRow {
        stencil: numeric::stencil(row(kind))?,
        holes: padded_holes(if kind == Kind::TagInit { [offset, 0, 0] } else { [0, offset, 0] }),
        declared: false,
        number: guarded.filter(|_| !int32),
        int32: guarded.filter(|_| int32),
        unbox,
    })
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
    let orig = |web: Register| layout.homes.orig[usize::from(web)];
    let slot = |web: Register| u64::from(orig(web)) * SLOT;
    let mut rows = Vec::new();
    if let Some(realm) = layout.realm {
        let row = HeapRow { heap: Heap::Realm, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
        rows.push(EntryRow { stencil: base_rows::stencil(row)?, holes: padded_holes([0, realm.offset, realm.global]), declared: false, number: None, int32: None, unbox: None });
    }
    for (pin, web) in layout.homes.pins.iter().filter(|(_, web)| entered.contains(&usize::from(*web))) {
        rows.push(number_guard(Kind::Guard, Place::Pin(*pin as u8), slot(*web), orig(*web))?);
    }
    for (pin, web) in layout.homes.floats.iter().filter(|(_, web)| entered.contains(&usize::from(*web))) {
        rows.push(number_guard(Kind::Guard, Place::PinF(*pin as u8), slot(*web), orig(*web))?);
    }
    for web in &layout.homes.slots {
        let live = entered.contains(&usize::from(*web));
        rows.push(number_guard(if live { Kind::Guard } else { Kind::TagInit }, Place::MemInt, slot(*web), orig(*web))?);
    }
    for web in entered.iter().map(|web| *web as Register) {
        if layout.homes.place(web) == Place::Mem && layout.entry_reads.guards(at, usize::from(orig(web))) {
            rows.push(number_guard(Kind::Guard, Place::Mem, slot(web), orig(web))?);
        }
    }
    let test = HeapRow { heap: Heap::Declared, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    for EntryGuard { register, languages } in &layout.guards {
        rows.push(EntryRow { stencil: base_rows::stencil(test)?, holes: padded_holes([u64::from(*register) * SLOT, u64::from(*languages), 0]), declared: true, number: None, int32: None, unbox: None });
    }
    let prototype = HeapRow { heap: Heap::Prototype, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    for proto in &layout.prototypes {
        let receiver = u64::from(proto.shape) | (u64::from(proto.link) << numeric::site_bits::HELD_LINK_AT);
        rows.push(EntryRow { stencil: base_rows::stencil(prototype)?, holes: padded_holes([u64::from(proto.register) * SLOT, receiver, 0]), declared: true, number: None, int32: None, unbox: None });
    }
    for proto in &layout.shapes {
        let heap = if proto.flat { Heap::ShapeFlat } else { Heap::Shape };
        let shape = HeapRow { heap, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
        rows.push(EntryRow { stencil: base_rows::stencil(shape)?, holes: padded_holes([u64::from(proto.register) * SLOT, u64::from(proto.shape), 0]), declared: true, number: None, int32: None, unbox: None });
    }
    if layout.stores {
        let stores = HeapRow { heap: Heap::Stores, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
        rows.push(EntryRow { stencil: base_rows::stencil(stores)?, holes: padded_holes([0, 0, 0]), declared: true, number: None, int32: None, unbox: None });
    }
    let identity = HeapRow { heap: Heap::CaptureIdentity, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    for (slot, callee) in &layout.identities {
        rows.push(EntryRow { stencil: base_rows::stencil(identity)?, holes: padded_holes([u64::from(*slot), u64::from(*slot) * SLOT, *callee]), declared: true, number: None, int32: None, unbox: None });
    }
    let (mut numbers, rest): (Vec<EntryRow>, Vec<EntryRow>) = rows.into_iter().partition(EntryRow::guards);
    numbers.extend(rest);
    Ok(numbers)
}
