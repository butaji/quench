//! The alias region and mutability of every native memory access: the one place a
//! `crate::staged::Region` becomes Cranelift `MemFlagsData`. Cranelift keeps its alias regions in
//! the function, numbered in insertion order, so every function declares `Region::ALL` in order
//! (`declare_regions`, called where a function is compiled) and a region's handle is its number.

use cranelift_codegen::ir::{self, AliasRegion, AliasRegionData, MemFlagsData};

use crate::staged::region::{Access, Mutability, Region};

/// The flags of a trusted (aligned, non-trapping) access: tagged with its region so Cranelift
/// reorders it past accesses to other regions, and where nothing changes the memory while code
/// runs, readonly and movable so it hoists and merges freely.
pub(in crate::vm) fn flags(access: Access) -> MemFlagsData {
    let tagged = MemFlagsData::new().with_notrap().with_aligned()
        .with_alias_region(Some(AliasRegion::from_u32(access.region as u32)));
    match access.mutability {
        Mutability::Mutable => tagged,
        Mutability::Readonly => tagged.with_readonly().with_can_move(),
    }
}

/// Declares every region in `function` unless it already names its own (the Wasm tier numbers
/// its regions itself), so that `flags` handles are valid.
pub(in crate::vm) fn declare_regions(function: &mut ir::Function) {
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

macro_rules! region_flags {
    ($($(#[$doc:meta])* $name:ident => $region:ident . $mutability:ident),* $(,)?) => {
        $($(#[$doc])* pub(in crate::vm) fn $name() -> MemFlagsData { flags(Region::$region.$mutability()) })*
    };
}

region_flags! {
    /// An activation's register or local slot, or a call record.
    frame => Frame.mutable,
    /// A property value of an object.
    object_slots => ObjectSlots.mutable,
    /// An object record's exact shape word.
    shape_word => ShapeWord.mutable,
    /// A dense array element.
    elements => Elements.mutable,
    /// A record's payload word, or a narrow value packed in it.
    payload => Payload.mutable,
    /// A word of the step dispatch, call context or result.
    dispatch => Dispatch.mutable,
    /// A per-site feedback counter.
    miss_counter => MissCounters.mutable,
    /// A word of the loop entry record: written before the run, never during it.
    loop_entry => LoopEntry.readonly,
    /// A word of the frozen constant pool.
    constant => Constants.readonly,
}
