//! The rows of named field reads and stores through their cache sites.

use super::*;

/// The byte offset of the field cache entry of `site` in the program's table.
fn entry_offset(site: u16) -> u64 {
    u64::from(site) * crate::vm::NATIVE_FIELD_CACHE_LAYOUT.entry_size as u64
}

/// The folded form of a field read at cache `site`: where the site proves exactly one load, held by
/// the receiver's immediate prototype, whose value the holder holds now (`InheritedConstant`), the
/// receiver it stands on and the value. A change of a value any prototype of the holder's shape holds
/// retires the code (`Body::holders`).
fn folded_read(known: Known, site: u16) -> Option<(Folded, u64)> {
    let reads = known.reads?;
    let loads = reads(site);
    let [load] = loads.as_slice() else { return None };
    let inherited = load.inherited?;
    let [(link, value)] = inherited.values() else { return None };
    (!load.function_shape).then_some((Folded { shape: load.shape, link: *link, holder: load.holder? }, value.to_bits()))
}

/// The row of a named field read of a register (or of the receiver, `this`, which the payload
/// holds) through the site's cache entry, or a nested lookup's two (`FieldSite`: `base.first.second`,
/// with no sink field); a read the prototype holds is folded to its value where the code may hold
/// that (`folded_read`).
fn site_row(known: Known, instruction: WideInstruction) -> Option<Decoded> {
    let (base, first, second) = match instruction.field_lookup()? {
        FieldLookup::Atom { base, cache_site, .. } => (base, cache_site, None),
        FieldLookup::Site(index) => {
            let site = known.program.field_sites.get(index).filter(|site| site.sink.is_none())?;
            (site.base, site.first.1, site.second.map(|(_, cache_site)| cache_site))
        }
    };
    let object = if base == FieldBase::THIS { None } else { Some(base.register_index()?) };
    Some(cached_read(known, (object, first, second), instruction.result_register()))
}

/// The row that reads cache site `first` (and, chained, `second`) of the object in register `object`, or of the receiver where there is
/// none, into the register `dst`: folded to the prototype's value where the site proves exactly that (`folded_read`).
fn cached_read(known: Known, (object, first, second): (Option<Register>, u16, Option<u16>), dst: Register) -> Decoded {
    let folded = second.is_none().then(|| folded_read(known, first)).flatten();
    let heap = match (object.is_none(), second.is_some(), folded.is_some()) {
        (false, false, false) => Heap::Site,
        (false, true, _) => Heap::SiteChain,
        (true, false, false) => Heap::ThisSite,
        (true, true, _) => Heap::ThisChain,
        (false, false, true) => Heap::Held,
        (true, false, true) => Heap::ThisHeld,
    };
    let hole = entry_offset(first) | second.map_or(0, |second| entry_offset(second) << numeric::library::CHAIN_SECOND_AT);
    let row = Decoded::heap(heap, Some(dst), object.map(Source::Register), None);
    match folded {
        Some((folded, value)) => Decoded { hole: value, folded: Some(folded), ..row },
        None => {
            let load = (heap == Heap::Site).then(|| own_flat_load(known, first)).flatten();
            Decoded {
                hole,
                slot: second.is_none().then(|| own_slot_type(known, first)).flatten(),
                ic: load.map(ic_hole),
                typed: load.and_then(number_slot),
                ..row
            }
        }
    }
}

/// The row of a field the loop's entry recorded: its value, which the loop does not write (`Facts::Entry`).
fn recorded_field(region: &LoopRegion, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    let index = *region.field_reads().get(&(pc - region.header))?;
    let hole = crate::vm::compiled::osr_loop::field_word(region, index) as u64 * SLOT;
    let row = Decoded::heap(Heap::Field, Some(instruction.result_register()), None, None);
    (!region.is_dynamic_field(index) && u32::try_from(hole).is_ok()).then_some(Decoded { hole, ..row })
}

/// The row of a field read: an array's `length`, the value a loop's entry recorded, or the site's
/// cache entry (`site_row`), which also answers a loop's read its entry did not record, or that
/// reads through a register the loop redefines.
pub(super) fn field_row(region: &LoopRegion, facts: Facts, known: Known, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    if let Some(FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup()
        && known.names_length(atom)
    {
        let register = base.register_index()?;
        let (heap, hole) = element_heap(region, facts, register, (Heap::Length, |_| Heap::TypedLength));
        let row = Decoded::heap(heap, Some(instruction.result_register()), Some(Source::Register(register)), None);
        return Some(Decoded { hole: if heap == Heap::TypedLength { hole } else { 0 }, ..row });
    }
    match facts {
        Facts::Entry { writes: false, .. } => recorded_field(region, pc, instruction).or_else(|| site_row(known, instruction)),
        Facts::Entry { writes: true, .. } | Facts::Site => site_row(known, instruction),
    }
}

/// The row of a named field store to a register's object, or to `this`, through the site's cache
/// entry (`entry::site_store`), where the store may run in place (`Known::stores_in_place`).
pub(super) fn field_store_row(known: Known, instruction: WideInstruction) -> Option<Decoded> {
    let atom = instruction.atom_index();
    if !known.stores_in_place(atom) || instruction.returns_from_frame() || instruction.writes_current_this() || adds::of(known, instruction).is_some() {
        return None;
    }
    let (heap, base) = match instruction.field_store_register() {
        Some(register) => (Heap::SiteStore, Some(register)),
        None => (Heap::ThisStore, None),
    };
    let value = Some(Source::Register(instruction.register_a()));
    let site = instruction.cache_site_index();
    // A store of a Number into a slot that holds Numbers tries its inline-cache row first (`Decoded::ic`).
    // A raw-double slot takes no tagged write: neither row writes its binary64 word, so it stores through the gate.
    let ic = (heap == Heap::SiteStore).then(|| own_flat_load(known, site).filter(|load| !load.raw_double && stores_number(load.ty)).map(ic_hole)).flatten();
    let slot = own_slot_load(known, site).filter(|load| !load.raw_double);
    Some(Decoded { base, hole: entry_offset(site), atom: Some(atom), slot: slot.map(|load| load.ty), slot_shape: slot.map(|load| load.shape), ic, ..Decoded::heap(heap, None, value, None) })
}

/// Whether an inline-cache store row may write a Number into a slot of observed type `ty`: the
/// slot holds every Number, so the write widens nothing, or it holds Numbers only, where an int32
/// slot takes int32 words alone (`Heap::IcSet { int }`).
fn stores_number(ty: crate::typed_ir::Type) -> bool {
    // A slot that holds no int32 (no live value yet) admits none: the int32 row's write would widen it.
    crate::typed_ir::Type::INT32.is_subtype(ty)
        && (crate::typed_ir::Type::NUMBER.is_subtype(ty) || ty.is_subtype(crate::typed_ir::Type::NUMBER))
}

/// The one own load cache `site` proves: what an
/// inline-cache row (`Heap::IcGet`, `Heap::IcSet`) patches into its code.
fn own_flat_load(known: Known, site: u16) -> Option<crate::vm::compiled::object_slots::NativeFieldLoad> {
    let loads = known.reads?(site);
    let [load] = loads[..] else { return None };
    // The row reads and writes either storage form, as the record's inline flag says.
    (load.holder.is_none() && load.valid() && load.inherited.is_none() && !load.function_shape).then_some(load)
}

/// The dependency that proves the one own flat entry's slot holds Numbers: its observed type is a Number
/// type the code may hold (`NativeFieldLoad::slot_dependency`), and its words are tagged, so each is an
/// int word or a double whatever the receiver's storage form. A raw-double slot (`raw_double`) is left
/// to the tested read: its words are binary64 bits, which a non-canonical NaN may leave outside the
/// double tag range.
fn number_slot(load: crate::vm::compiled::object_slots::NativeFieldLoad) -> Option<crate::code_dependencies::SlotDependency> {
    (!load.raw_double && load.ty.is_subtype(crate::typed_ir::Type::NUMBER)).then(|| load.slot_dependency()).flatten()
}

/// An inline-cache row's entry operands in one word: the exact shape below 32 bits, the slot's byte
/// offset in the flat run above (`base_region::plan::site_holes` splits them).
fn ic_hole(load: crate::vm::compiled::object_slots::NativeFieldLoad) -> u64 {
    u64::from(load.shape) | (u64::from(load.slot) * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as u64) << u32::BITS
}

/// The observed type of the own slot cache `site` stores to, where it proves exactly one own load.
fn own_slot_type(known: Known, site: u16) -> Option<crate::typed_ir::Type> {
    own_slot_load(known, site).map(|load| load.ty)
}

/// The one own load cache `site` proves, whose shape and slot type a store's admission stands on.
fn own_slot_load(known: Known, site: u16) -> Option<crate::vm::compiled::object_slots::NativeFieldLoad> {
    let [load] = known.reads?(site)[..] else { return None };
    (load.holder.is_none() && load.valid()).then_some(load)
}

/// Whether `instruction` is a field store a row makes (`field_store_row`).
pub(in crate::vm::compiled::base_region) fn stores_field(known: Known, instruction: WideInstruction) -> bool {
    instruction.result_flags() == 0
        && matches!(crate::vm::compiled::decoded(instruction), Some(crate::staged::Operation::SetField { .. }))
        && field_store_row(known, instruction).is_some()
}
