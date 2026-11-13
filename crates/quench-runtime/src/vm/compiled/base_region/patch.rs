//! Re-patching an inline-cache entry's shape and slot in place (`quench-bjit/stencils/ic_rows.rs`).
//!
//! A site whose chain holds one own-field entry (a get, or a store) may meet another receiver
//! shape: the row misses, the instruction leaves to the interpreter, and the site's cache then
//! holds that one shape. Instead of retiring the code, the entry's two operands, the shape id
//! and the slot's byte offset, are rewritten where the stitcher put them. Each is a 32-bit field
//! (`Imm32`) of the published code, found from per-row offsets recorded at link time and written
//! through the code heap's write view (`CodeMemory::write_imm32`); the executable view is never
//! writable.
//!
//! The patch keeps the code valid: the entry's slot type is the one the row was built for (a
//! different type declines), the new load's slot observation is subscribed before the write, and
//! the region's shape dependency (`OsrFacts::shape_dependencies`) retires it at a compaction
//! whatever shape a patch names. A site is patched at most `MAX_REPATCHES_PER_SITE` times, a
//! state machine `SiteState` that ends in `Generic`: the site then stays a miss that leaves at
//! the instruction (a shape that keeps changing is not monomorphic).
use super::base_rows::{get_entry_holes, set_entry_holes, EntryOperands, CACHE_ENTRY_HOLE, GET_ENTRY_OPERANDS, SET_ENTRY_OPERANDS};
use super::cache::{takes_get, takes_set};
use super::*;
use crate::typed_ir::Type;
use quench_bjit::Imm32;

/// The most times one site's entry is re-patched before the site goes generic: a site that has
/// changed shape this often is polymorphic over time, and each patch costs a leave and a write.
pub(in crate::vm) const MAX_REPATCHES_PER_SITE: u8 = 4;

/// A planned row that is a site's only entry and may be re-patched: what its record keeps.
#[derive(Clone, Copy)]
pub(super) struct SiteEntry {
    pc: u32,
    store: bool,
    ty: Type,
    /// The shape the entry names now: a re-patch to it is no change (a leave that was no shape
    /// miss, a type guard's, say, must not use a patch).
    shape: u32,
}

impl SiteEntry {
    /// The entry of `load` at `pc`, where it is the chain's only one and an own field.
    pub(super) fn of(pc: u32, store: bool, load: &NativeFieldLoad, chain: usize) -> Option<Self> {
        (chain == 1 && load.holder.is_none()).then_some(Self { pc, store, ty: load.ty, shape: load.shape })
    }

    fn operands(&self) -> EntryOperands {
        if self.store { SET_ENTRY_OPERANDS } else { GET_ENTRY_OPERANDS }
    }
}

/// Where a patchable entry's operands are in the published code: the 32-bit fields (offsets
/// from the code's start) that hold its shape and its slot offset.
struct Fields {
    shape: Vec<Imm32>,
    slot: Vec<Imm32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SiteState {
    /// Patched `used` times so far.
    Patching { used: u8 },
    /// Past the bound: the site misses to the interpreter for good.
    Generic,
}

impl SiteState {
    fn after_patch(self) -> Self {
        match self {
            Self::Patching { used } if used + 1 < MAX_REPATCHES_PER_SITE => Self::Patching { used: used + 1 },
            _ => Self::Generic,
        }
    }
}

struct Site {
    entry: SiteEntry,
    fields: Fields,
    state: SiteState,
}

/// What a re-patch request came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::vm) enum Repatch {
    /// The entry now names the new shape and slot; the instruction may run again in the code.
    Patched,
    /// The site has used its patches (`MAX_REPATCHES_PER_SITE`), now or earlier.
    Generic,
    /// The site or load is not one a patch keeps valid: no site there, a chain, a holder read,
    /// another slot type, the shape it names already, or a dependency that no longer holds.
    Declined,
}

/// The re-patchable sites of one built region, ascending by PC.
#[derive(Default)]
pub(in crate::vm) struct CacheSites(Vec<Site>);

impl CacheSites {
    /// The sites of `body` (the region's rows after its `first` entry rows), given the absolute
    /// code offset each row of the program starts at. An entry whose shape or slot is not a
    /// 32-bit field of its stencil is left out.
    pub(super) fn collect(body: &[&Planned], first: usize, row_offsets: &[usize]) -> Self {
        let mut sites = Vec::new();
        for (k, row) in body.iter().enumerate() {
            let Some(entry) = row.patch else { continue };
            let start = row_offsets[first + k] as u32;
            let operands = entry.operands();
            let fields = |operand: usize| -> Vec<Imm32> {
                let hole = (CACHE_ENTRY_HOLE + operand) as u32;
                row.stencil.imm32.iter().filter(|field| field.index == hole).map(|field| Imm32 { offset: field.offset + start, ..*field }).collect()
            };
            let fields = Fields { shape: fields(operands.shape), slot: fields(operands.slot) };
            if !fields.shape.is_empty() && !fields.slot.is_empty() {
                sites.push(Site { entry, fields, state: SiteState::Patching { used: 0 } });
            }
        }
        Self(sites)
    }

    /// Makes the entry at `pc` name `load`'s shape and slot. `subscribe` takes the new load's
    /// dependencies and says whether they hold; it runs after every check and before the first
    /// write. `write` stores a 32-bit field at a code offset (the code heap's write view).
    pub(in crate::vm) fn repatch(&mut self, pc: u32, atom: crate::bytecode::Atom, load: &NativeFieldLoad, subscribe: impl FnOnce() -> bool, mut write: impl FnMut(usize, u32)) -> Repatch {
        let Ok(at) = self.0.binary_search_by_key(&pc, |site| site.entry.pc) else { return Repatch::Declined };
        let site = &mut self.0[at];
        if site.state == SiteState::Generic {
            return Repatch::Generic;
        }
        let takes = if site.entry.store { takes_set(load) } else { takes_get(load) };
        if !takes || load.holder.is_some() || load.ty != site.entry.ty || load.shape == site.entry.shape || !subscribe() {
            return Repatch::Declined;
        }
        let operands = site.entry.operands();
        let holes = if site.entry.store { set_entry_holes(atom, load) } else { get_entry_holes(load) };
        for (fields, operand) in [(&site.fields.shape, operands.shape), (&site.fields.slot, operands.slot)] {
            fields.iter().for_each(|field| write(field.offset as usize, field.value(holes[operand] as u32)));
        }
        site.entry.shape = load.shape;
        site.state = site.state.after_patch();
        Repatch::Patched
    }

    /// The patches the site at `pc` may still take, or none where it is no site or is generic.
    #[cfg(test)]
    fn remaining(&self, pc: u32) -> u8 {
        match self.0.iter().find(|site| site.entry.pc == pc).map(|site| site.state) {
            Some(SiteState::Patching { used }) => MAX_REPATCHES_PER_SITE - used,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_bjit::{Stencil, HOLES};

    const PC: u32 = 9;
    const ROW_START: usize = 40;

    fn load(shape: u32, slot: u16) -> NativeFieldLoad {
        NativeFieldLoad { shape, holder: None, slot, storage_len: 4, storage: Some(crate::heap::PlainPropertyStorage::Arena), ty: Type::NUMBER, inherited: None, function_shape: false }
    }

    /// A get row whose shape is a field at 3 and whose slot is a field at 11 (and a frame
    /// offset field at 7, which a patch must not touch).
    fn get_row() -> Planned {
        let field = |offset, index| Imm32 { offset, index, negate: false, addend: 0 };
        let imm32 = vec![field(3, CACHE_ENTRY_HOLE as u32), field(7, 0), field(11, CACHE_ENTRY_HOLE as u32 + 1)];
        let stencil = Box::leak(Box::new(Stencil { code: vec![0; 32], relocs: Vec::new(), imm32, next_jump: None, tail_len: 0, raw_len: 32 }));
        let patch = SiteEntry::of(PC, false, &load(5, 0), 1);
        Planned { stencil, node: None, holes: [0; HOLES], target: None, exit_pc: Some(PC), pre: Vec::new(), post: Vec::new(), patch }
    }

    fn sites() -> CacheSites {
        let row = get_row();
        let offsets = [0, ROW_START];
        CacheSites::collect(&[&row], 1, &offsets)
    }

    #[test]
    fn the_shape_and_slot_offsets_are_recorded_at_the_rows_absolute_position() {
        let sites = sites();
        let fields = &sites.0[0].fields;
        let at = |fields: &[Imm32]| fields.iter().map(|field| field.offset as usize).collect::<Vec<_>>();
        assert_eq!((at(&fields.shape), at(&fields.slot)), (vec![ROW_START + 3], vec![ROW_START + 11]));
    }

    #[test]
    fn a_chain_or_a_holder_entry_is_no_site() {
        assert!(SiteEntry::of(PC, false, &load(5, 0), 2).is_none());
        let holder = NativeFieldLoad { holder: Some(8), ..load(5, 0) };
        assert!(SiteEntry::of(PC, false, &holder, 1).is_none());
    }

    #[test]
    fn a_patch_writes_the_new_shape_and_slot_only() {
        let mut sites = sites();
        let mut written = Vec::new();
        let outcome = sites.repatch(PC, 0, &load(6, 2), || true, |at, value| written.push((at, value)));
        let slot = 2 * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as u32;
        assert_eq!((outcome, written), (Repatch::Patched, vec![(ROW_START + 3, 6), (ROW_START + 11, slot)]));
    }

    #[test]
    fn a_site_goes_generic_after_the_bound() {
        let mut sites = sites();
        for used in 0..MAX_REPATCHES_PER_SITE {
            assert_eq!(sites.remaining(PC), MAX_REPATCHES_PER_SITE - used);
            assert_eq!(sites.repatch(PC, 0, &load(10 + u32::from(used), 1), || true, |_, _| {}), Repatch::Patched);
        }
        let mut writes = 0;
        assert_eq!(sites.repatch(PC, 0, &load(99, 1), || true, |_, _| writes += 1), Repatch::Generic);
        assert_eq!((writes, sites.remaining(PC)), (0, 0));
    }

    #[test]
    fn a_load_the_entry_cannot_take_declines_without_writing() {
        let mut sites = sites();
        let mut writes = 0;
        let widened = NativeFieldLoad { ty: Type::ANY, ..load(6, 1) };
        assert_eq!(sites.repatch(PC, 0, &widened, || true, |_, _| writes += 1), Repatch::Declined);
        assert_eq!(sites.repatch(PC, 0, &load(6, 1), || false, |_, _| writes += 1), Repatch::Declined);
        assert_eq!(sites.repatch(PC + 1, 0, &load(6, 1), || true, |_, _| writes += 1), Repatch::Declined);
        assert_eq!(sites.repatch(PC, 0, &load(5, 1), || true, |_, _| writes += 1), Repatch::Declined);
        assert_eq!((writes, sites.remaining(PC)), (0, MAX_REPATCHES_PER_SITE));
    }
}
