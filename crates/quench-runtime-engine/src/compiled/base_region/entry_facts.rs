//! Where a heap row's recorded facts come from: a loop region's entry record, through which its
//! arrays are read as dense backings or as typed elements, or nowhere (`Facts`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::tier_surface::TypedArrayKind;
use quench_runtime::staged::typed_elements::NativeKind;

/// How a loop region's arrays are viewed on entry: not at all (each access proves its array's word), as
/// dense arrays' backings or as typed arrays' elements, which the entry record holds (`ElementViews`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Views {
    Frame,
    Dense,
    /// Typed arrays of this native kind, which the entry proved.
    Typed(TypedArrayKind),
}

impl Views {
    /// The views of `region`'s arrays its entry found as `views`. Hole-free double backings the
    /// region never writes are a `Float64Array`'s raw elements, which the entry record holds
    /// (`loop_tier::loop_arrays_in_lane`): a read is a bounds test and one load.
    pub(crate) fn of(views: ElementViews, region: &LoopRegion) -> Self {
        match views {
            ElementViews::Dense => Self::Dense,
            ElementViews::Typed(Some(kind)) if NativeKind::new(kind).is_some() => Self::Typed(kind),
            ElementViews::Double if region.arrays.iter().all(|array| !array.written) => Self::Typed(TypedArrayKind::Float64),
            // A written double backing takes stores of any value: its rows read it through its record.
            ElementViews::Typed(_) | ElementViews::Double => Self::Frame,
        }
    }
}

/// Where a heap row's recorded facts come from: a loop region's entry record (its arrays read
/// through it where the views are not `Frame`), or nowhere, as a whole function's rows read the heap
/// by its tables (an array word's record, a field's cache entry).
#[derive(Clone, Copy)]
pub(crate) enum Facts {
    /// A loop region: `views` say how its arrays are read, `writes` where it
    /// stores a field through a row, which may stale a field value the entry record holds
    /// (`stores_field`), so its field reads are the site's, live. A region that makes a call by a
    /// row (`calls_in_place`), whose callee may change anything the entry recorded, reads its
    /// arrays through their records (`Views::Frame`), its fields through the site's entry, and
    /// its captures live (`osr_lane::bind_captures`), as the interpreter does.
    Entry { views: Views, writes: bool },
    Site,
}

/// The entry-record byte offset of the array `register` holds through the region, which an
/// indexing row reads the backing of from the record; 0 where the array is looked up per access.
fn entry_array(region: &LoopRegion, facts: Facts, register: Register) -> u64 {
    // A row's hole is a u32 patch: an offset past it is looked up per access instead.
    match facts {
        Facts::Entry { views: Views::Dense | Views::Typed(_), .. } => region.array_entry_offset(register).filter(|offset| u32::try_from(*offset).is_ok()).unwrap_or(0),
        Facts::Entry { views: Views::Frame, .. } | Facts::Site => 0,
    }
}

/// The row of an element access of the array `register` under `facts`, over the typed row of the kind the
/// entry viewed the array as (`typed`) where it did, else the dense `heap` row: with the record offset of its
/// entry words (0 where it has none).
pub(crate) fn element_heap(region: &LoopRegion, facts: Facts, register: Register, (dense, typed): (Heap, fn(u8) -> Heap)) -> (Heap, u64) {
    let hole = entry_array(region, facts, register);
    match facts {
        Facts::Entry { views: Views::Typed(kind), .. } if hole != 0 => (typed(kind as u8), hole),
        _ => (dense, hole),
    }
}
