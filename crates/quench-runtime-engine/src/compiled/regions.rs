//! The alias region and mutability of every native memory access: the one place a
//! `quench_runtime::staged::Region` becomes Cranelift `MemFlagsData`. Cranelift keeps its alias regions in
//! the function, numbered in insertion order, so every function declares `Region::ALL` in order
//! (`declare_regions`, called where a function is compiled) and a region's handle is its number.

#[allow(unused_imports)]
use crate::ext_traits::*;
use std::collections::BTreeMap;
use cranelift_codegen::ir::{self, AliasRegion, AliasRegionData, MemFlagsData};

use quench_runtime::staged::region::{Access, Mutability, Region};
use quench_runtime::staged::elements::BackingAccess;
use quench_runtime::staged::Effects;

/// The flags of a trusted (aligned, non-trapping) access: tagged with its region so Cranelift
/// reorders it past accesses to other regions, and where nothing changes the memory while code
/// runs, readonly and movable so it hoists and merges freely.
pub(crate) fn flags(access: Access) -> MemFlagsData {
    let tagged = trusted(AliasRegion::from_u32(access.region as u32));
    match access.mutability {
        Mutability::Mutable => tagged,
        Mutability::Readonly => tagged.with_readonly().with_can_move(),
    }
}

/// The flags of a trusted, mutable access in `region`, a handle `declare_field_regions` made.
fn trusted(region: AliasRegion) -> MemFlagsData {
    MemFlagsData::new().with_notrap().with_aligned().with_alias_region(Some(region))
}

/// What a load of a region reads, as the unit's effect row proves it (`LoadFact::of_unit`): the one place a load's fact
/// picks its access, and with it its flags (`flags`).
#[derive(Clone, Copy)]
pub(crate) enum LoadFact {
    /// The word may change while the code runs: the region's mutable access.
    Mutable(Region),
    /// The unit writes the region nowhere and runs no code that may write it, move records or collect, so the word is
    /// fixed for the run: the region's readonly access.
    Fixed(Region),
}

impl LoadFact {
    /// The fact of a load of `region` in a unit whose whole effect row is `effects` (`Unit::effects`).
    pub(crate) fn of_unit(effects: Effects, region: Region) -> Self {
        if effects.writes_region(region) || effects.may_move_or_call() { Self::Mutable(region) } else { Self::Fixed(region) }
    }

    /// The access the fact names.
    pub(crate) fn access(self) -> Access {
        match self {
            Self::Mutable(region) => region.mutable(),
            Self::Fixed(region) => region.readonly(),
        }
    }
}

/// The access of an array's backing words in a unit whose effect row is `effects`: the header (the record's payload
/// word, `Region::Payload`) and the length (`Region::Elements`), each fixed where the unit proves it.
pub(crate) fn backing_access(effects: Effects) -> BackingAccess {
    BackingAccess { header: LoadFact::of_unit(effects, Region::Payload).access(), length: LoadFact::of_unit(effects, Region::Elements).access() }
}

/// The access of the array record table's base word (`JsStepDispatch::head.array_record_base`): readonly in a unit where
/// arrays stay put (`arrays_fixed`, `Lowering::arrays_stay_put`), so its loads hoist and merge; mutable otherwise.
pub(crate) fn record_base(arrays_fixed: bool) -> Access {
    if arrays_fixed { Region::RecordBases.readonly() } else { Region::RecordBases.mutable() }
}

/// Every word an array access reads, each decided once per unit: the record table's base (`record`) and the backing's
/// header and length (`backing`).
#[derive(Clone, Copy)]
pub(crate) struct ArrayAccess {
    pub(crate) record: Access,
    pub(crate) backing: BackingAccess,
}

/// The accesses of an array access in a unit whose effect row is `effects` and whose arrays stay put iff `arrays_fixed`.
pub(crate) fn array_access(effects: Effects, arrays_fixed: bool) -> ArrayAccess {
    ArrayAccess { record: record_base(arrays_fixed), backing: backing_access(effects) }
}

/// The (class, atom) a named property is keyed by: the proven class of its receiver and its atom.
pub(crate) type FieldKey = (u32, u32);

/// The flags of a mutable property-value access keyed by `field`: its own field region when the
/// function declared one (`declare_field_regions`), else `Region::ObjectSlots`.
pub(crate) fn property_flags(field: Option<AliasRegion>) -> MemFlagsData {
    trusted(field.unwrap_or(AliasRegion::from_u32(Region::ObjectSlots as u32)))
}

/// Declares every region in `function` unless it already names its own (the Wasm tier numbers
/// its regions itself), so that `flags` handles are valid.
pub(crate) fn declare_regions(function: &mut ir::Function) {
    if function.dfg.alias_regions.len() != 0 {
        return;
    }
    for region in Region::ALL {
        let handle = function.dfg.alias_regions.insert(AliasRegionData {
            user_id: *region as u32,
            description: format!("{region:?}").into(),
        });
        debug_assert_eq!(handle.as_u32(), *region as u32, "regions are declared in order");
    }
}

/// Declares one alias region per (class, atom) key after `Region::ALL` (`declare_regions` runs
/// first), and returns each key's handle. A key is split off `Region::ObjectSlots` only under the
/// function's soundness rule (`retag_field_regions`): a store to one key never kills a load of
/// another, so every property access must be keyed, which the caller checks before it asks.
pub(crate) fn declare_field_regions(function: &mut ir::Function, keys: impl IntoIterator<Item = FieldKey>) -> BTreeMap<FieldKey, AliasRegion> {
    debug_assert_eq!(function.dfg.alias_regions.len(), Region::ALL.len(), "the regions are declared first");
    keys.into_iter().map(|(class, atom)| {
        let user_id = function.dfg.alias_regions.len() as u32;
        let handle = function.dfg.alias_regions.insert(AliasRegionData {
            user_id,
            description: format!("field {class}.{atom}").into(),
        });
        ((class, atom), handle)
    }).collect()
}

/// The soundness rule of the field regions. Cranelift's alias analysis forwards a load past a
/// store to another region, so a property access left in `Region::ObjectSlots` beside keyed
/// ones would be missed by them: if any load or store of the function still names
/// `Region::ObjectSlots` (a property access with an unknown class or a computed key, or a staged
/// helper that names the region itself), every keyed access goes back to `Region::ObjectSlots`,
/// and the function's property values stay one region. Calls observe every region already, so
/// they stay barriers either way.
pub(crate) fn retag_field_regions(function: &mut ir::Function, fields: &BTreeMap<FieldKey, AliasRegion>) {
    if fields.is_empty() { return; }
    let object_slots = AliasRegion::from_u32(Region::ObjectSlots as u32);
    let mut keyed = Vec::new();
    let mut unkeyed = false;
    for index in 0..function.dfg.num_insts() {
        let inst = ir::Inst::new(index);
        let Some(memflags) = function.dfg.insts[inst].memflags() else { continue };
        match function.dfg.mem_flags[memflags].alias_region() {
            Some(region) if region == object_slots => unkeyed = true,
            Some(region) if fields.values().any(|field| *field == region) => keyed.push(inst),
            _ => {}
        }
    }
    if !unkeyed { return; }
    for inst in keyed {
        let Some(memflags) = function.dfg.insts[inst].memflags() else { continue };
        let data = function.dfg.mem_flags[memflags].with_alias_region(Some(object_slots));
        let retagged = function.dfg.mem_flags.insert_unchecked(data);
        if let Some(slot) = function.dfg.insts[inst].memflags_mut() { *slot = retagged; }
    }
}

macro_rules! region_flags {
    ($($(#[$doc:meta])* $name:ident => $region:ident . $mutability:ident),* $(,)?) => {
        $($(#[$doc])* pub(crate) fn $name() -> MemFlagsData { flags(Region::$region.$mutability()) })*
    };
}

region_flags! {
    /// An activation's register or local slot, or a call record.
    frame => Frame.mutable,
    /// An object record's exact shape word.
    shape_word => ShapeWord.mutable,
    /// A word of the step dispatch, call context or result.
    dispatch => Dispatch.mutable,
    /// Heap bookkeeping (`Region::HeapMeta`): a cache entry, the property arena's header.
    heap_meta => HeapMeta.mutable,
    /// A per-site feedback counter.
    miss_counter => MissCounters.mutable,
    /// A word of the frozen constant pool.
    constant => Constants.readonly,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_access_is_trusted_and_tagged_with_its_region_number() {
        for region in Region::ALL {
            for access in [region.mutable(), region.readonly()] {
                let data = flags(access);
                assert!(data.notrap() && data.aligned());
                assert_eq!(data.alias_region(), Some(AliasRegion::from_u32(*region as u32)));
            }
        }
    }

    #[test]
    fn only_a_readonly_access_may_move() {
        for region in Region::ALL {
            let (mutable, readonly) = (flags(region.mutable()), flags(region.readonly()));
            assert!(!mutable.readonly() && !mutable.can_move());
            assert!(readonly.readonly() && readonly.can_move());
        }
    }

    #[test]
    fn the_record_base_is_readonly_only_where_arrays_stay_put() {
        let (fixed, moving) = (flags(record_base(true)), flags(record_base(false)));
        assert!(fixed.readonly() && fixed.can_move() && fixed.notrap() && fixed.aligned());
        assert!(!moving.readonly() && !moving.can_move());
        assert_eq!(fixed.alias_region(), moving.alias_region());
    }

    #[test]
    fn a_function_declares_each_region_at_its_number() {
        let mut function = ir::Function::new();
        declare_regions(&mut function);
        assert_eq!(function.dfg.alias_regions.len(), Region::ALL.len());
        for region in Region::ALL {
            let handle = AliasRegion::from_u32(*region as u32);
            assert_eq!(function.dfg.alias_regions[handle].user_id, *region as u32);
        }
    }
}
