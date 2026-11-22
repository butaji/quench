//! The alias region and mutability of every native memory access: the one place a
//! `quench_runtime::staged::Region` becomes Cranelift `MemFlagsData`. Cranelift keeps its alias regions in
//! the function, numbered in insertion order, so every function declares `Region::ALL` in order
//! (`declare_regions`, called where a function is compiled) and a region's handle is its number.

#[allow(unused_imports)]
use crate::ext_traits::*;
use cranelift_codegen::ir::{self, AliasRegion, AliasRegionData, MemFlagsData};

use quench_runtime::staged::region::{Access, Mutability, Region};

/// The flags of a trusted (aligned, non-trapping) access: tagged with its region so Cranelift
/// reorders it past accesses to other regions, and where nothing changes the memory while code
/// runs, readonly and movable so it hoists and merges freely.
pub(crate) fn flags(access: Access) -> MemFlagsData {
    let tagged = MemFlagsData::new().with_notrap().with_aligned()
        .with_alias_region(Some(AliasRegion::from_u32(access.region as u32)));
    match access.mutability {
        Mutability::Mutable => tagged,
        Mutability::Readonly => tagged.with_readonly().with_can_move(),
    }
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
