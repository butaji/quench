//! Field accesses through the inline caches their sites observed (`NativeFieldLoad`): each
//! entry is a stencil row (`quench-bjit/stencils/ic_rows.rs`) whose shape and slot are holes the
//! stitcher patches, and a site of several entries is a chain of rows, a miss falling to the
//! next and a hit branching past them. The last entry's miss leaves at the instruction, which
//! the interpreter runs. A shape guard is per execution; what the code embeds besides is the
//! shape ids and the slots' types, which `OsrFacts::of_loads` turns into the code's dependencies
//! (a compaction that renumbers shapes, a holder's departure, a slot widened past its observed
//! type retire it).
use super::base_rows::{cache_holes, get_entry_holes, set_entry_holes, CACHE_ENTRY_HOLE};
use super::*;
use crate::staged::Form;
use crate::typed_ir::Type;
use quench_bjit::HOLES;

/// The most entries of one site a chain holds: V8 inlines 4 maps per access. A site with more
/// leaves at the instruction past them.
const MAX_CACHE_ENTRIES: usize = 4;

/// The entries a field access takes, in the order the site observed them.
#[derive(Clone)]
pub(super) struct Cache {
    pub(super) atom: crate::bytecode::Atom,
    pub(super) loads: Vec<NativeFieldLoad>,
}

/// Whether a get takes the entry: a plan the layout admits, a receiver that is an object record
/// (a function record's own fields are not read here), and a flat own slot or one a prototype
/// holds (a holder that keeps its values inline misses at run time).
fn takes_get(load: &NativeFieldLoad) -> bool {
    load.valid() && !load.function_shape && (load.holder.is_some() || Form::of(load.storage, load.storage_len) == Form::Arena)
}

/// Whether a store takes the entry: an own flat slot whose observed type a stored Number keeps:
/// no compiled read of the slot assumed more (`SlotDependency`), or an int32 slot, which the row
/// guards (`Heap::CacheSetInt`).
fn takes_set(load: &NativeFieldLoad) -> bool {
    takes_get(load) && load.holder.is_none() && store_heap(load).is_some()
}

/// The store row an entry's slot type admits.
fn store_heap(load: &NativeFieldLoad) -> Option<Heap> {
    match load.ty {
        ty if ty == Type::ANY || ty == Type::NUMBER => Some(Heap::CacheSet),
        ty if ty == Type::INT32 => Some(Heap::CacheSetInt),
        _ => None,
    }
}

/// `GetField` of a register receiver the region redefines, through the entries `loads` observed.
pub(super) fn cached_get(instruction: WideInstruction, loads: &[NativeFieldLoad]) -> Option<Decoded> {
    let crate::bytecode::FieldLookup::Atom { atom, base, .. } = instruction.field_lookup()? else { return None };
    let receiver = base.register_index()?;
    let loads: Vec<NativeFieldLoad> = loads.iter().filter(|load| takes_get(load)).take(MAX_CACHE_ENTRIES).copied().collect();
    let row = Decoded::heap(Heap::CacheGet, Some(instruction.result_register()), Some(Source::Register(receiver)), None);
    (!loads.is_empty()).then_some(Decoded { cache: Some(Cache { atom, loads }), ..row })
}

/// A named field store to a register receiver through the entries the region's reads of the same
/// field observed. The region reads every field of that name through caches too (a snapshot of
/// the entry record would go stale at the store).
pub(super) fn cached_set(region: &LoopRegion, instruction: WideInstruction, loads: &[Vec<NativeFieldLoad>]) -> Option<Decoded> {
    if instruction.writes_current_this() || instruction.returns_from_frame() {
        return None;
    }
    let (receiver, atom) = (instruction.field_store_register()?, instruction.atom_index());
    let named = || region.fields.iter().enumerate().filter(|(_, field)| field.atom == atom);
    if !named().all(|(index, _)| region.is_dynamic_field(index)) {
        return None;
    }
    let observed = named().filter(|(_, field)| field.receiver == FieldReceiver::Register(receiver)).filter_map(|(index, _)| loads.get(index)).flatten();
    let loads: Vec<NativeFieldLoad> = observed.filter(|load| takes_set(load)).take(MAX_CACHE_ENTRIES).copied().collect();
    let row = Decoded::heap(Heap::CacheSet, None, Some(Source::Register(instruction.register_a())), None);
    (!loads.is_empty()).then_some(Decoded { base: Some(receiver), cache: Some(Cache { atom, loads }), ..row })
}

/// One entry's row and holes. `operands` are the row's frame operands, the first two holes.
fn entry_row(decoded: &Decoded, cache: &Cache, load: &NativeFieldLoad, (dst, a): (Place, Place), chained: bool, operands: [u64; CACHE_ENTRY_HOLE]) -> Result<(HeapRow, [u64; HOLES]), String> {
    let (heap, entry) = match decoded.heap {
        Some(Heap::CacheSet) => (store_heap(load).ok_or("a store entry the slot's type refuses")?, set_entry_holes(cache.atom, load)),
        _ if load.holder.is_some() => (Heap::CacheHolderGet, get_entry_holes(load)),
        _ => (Heap::CacheGet, get_entry_holes(load)),
    };
    let row = if matches!(heap, Heap::CacheSet | Heap::CacheSetInt) {
        HeapRow { heap, dst: Place::Mem, a, b: Place::Mem, entry: chained }
    } else {
        HeapRow { heap, dst, a: Place::Mem, b: Place::Mem, entry: chained }
    };
    Ok((row, cache_holes(operands, entry)))
}

/// The rows of a cache access at `pc`: one per entry, each but the last handing a miss to the next
/// and a hit to the instruction after this one. `operands` are the destination's, the first
/// operand's and the second's (place, hole).
pub(super) fn cache_plan(decoded: &Decoded, cache: &Cache, homes: &Homes, pc: u32, [(dst, dst_hole), (a, a_hole), _]: [(Place, u64); 3]) -> Result<Planned, String> {
    let store = decoded.heap == Some(Heap::CacheSet);
    let base = decoded.base.map_or(0, |register| u64::from(homes.orig[usize::from(register)]) * SLOT);
    let operands = if store { [base, a_hole] } else { [dst_hole, a_hole] };
    let mut parts = Vec::with_capacity(cache.loads.len());
    for (k, load) in cache.loads.iter().enumerate() {
        let chained = k + 1 < cache.loads.len();
        let (row, holes) = entry_row(decoded, cache, load, (dst, a), chained, operands)?;
        let target = chained.then_some(pc + 1);
        parts.push(Planned { stencil: base_rows::stencil(row)?, holes, target, exit_pc: Some(pc), pre: Vec::new(), post: Vec::new() });
    }
    let mut last = parts.pop().ok_or("a cache access with no entry")?;
    last.pre = parts;
    Ok(last)
}
