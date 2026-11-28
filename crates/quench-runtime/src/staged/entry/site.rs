//! The site-table reads and stores of an IC entry: the chains, the site tables and the own reads.

use super::*;

/// A field read through two cache entries: the first names a value the second entry's site reads from.
#[inline(always)]
pub fn site_chain<S: Stage>(stage: &mut S, (entries, functions): (S::Word, S::Word), object: S::Word, (first, second): (S::Imm, S::Imm), miss: S::Miss) -> Option<S::Word> {
    site_tables(stage, entries, miss)?;
    let middle = site_read_in(stage, (entries, functions), object, first, miss)?;
    site_read_in(stage, (entries, functions), middle, second, miss)
}

/// A store of `value` through the own cache entry `offset` bytes into the table `entries`: the
/// record's shape is the entry's, the gate admits the write where it is `gated`, and the slot is
/// plain. A value the caller proved immediate and of the slot's observed type is not `gated`: it
/// needs no barrier log and widens no observation (`slot_store::gate_open`).
///
/// A live cell's class key fixes its shape, so a store through an own entry names no record and needs no
/// record hop: the class key is the one guard (`class_matches`), and a shape-keyed entry matches no cell and
/// leaves to the VM. `proven` names a record's exact shape, which no live cell
/// carries as a word, and no caller proves one, so a store that carries one leaves. A gated store asks the
/// class space's gate (`slot_store::int_over_int_gate`), which refuses every store that may widen a tracked
/// slot type; an ungated store is one whose value the slot's type holds.
#[inline(always)]
pub fn site_store<S: Stage>(stage: &mut S, entries: S::Word, object: S::Word, offset: S::Imm,
    (value, gated, proven): (S::Word, bool, Option<S::Imm>), miss: S::Miss) -> Option<()> {
    site_tables(stage, entries, miss)?;
    let direct = super::super::value::is_direct_object(stage, object);
    stage.guard(direct, miss)?;
    let entry = stage.add_imm(entries, offset);
    let own = own_entry(stage, entry);
    stage.guard(own, miss)?;
    let granule = super::super::value::direct_object_id(stage, object);
    let cell = super::super::object::cell_at_granule(stage, granule);
    class_matches(stage, cell, entry, miss)?;
    if proven.is_some() {
        let never = stage.cond_of(false);
        stage.guard(never, miss)?;
    }
    let slot = entry_slot(stage, entry);
    let open = if gated {
        let old = cell_field(stage, cell, slot);
        super::super::slot_store::int_over_int_gate(stage, (StagedRecordSpace::Class, granule, old), value)
    } else { stage.immediate_store_open() };
    stage.guard(open, miss)?;
    class_store(stage, cell, slot, value);
    Some(())
}

/// Guards that the field cache table `entries` (the running program's, as `Vm::program_field_entries`
/// hands it) is present.
#[inline(always)]
fn site_tables<S: Stage>(stage: &mut S, entries: S::Word, miss: S::Miss) -> Option<()> {
    let present = stage.ne_imm(entries, 0u32);
    stage.guard(present, miss)
}

/// The data value the entry at `offset` bytes into `entries` holds for `object`, own or held by its class
/// (`direct_read_in`), or for a function (`record_read_in`, which reads the function's own value). `None`
/// (leaving at `miss`) where the word is neither a direct object nor a function, or the entry is no entry for it.
#[inline(always)]
fn site_read_in<S: Stage>(stage: &mut S, (entries, functions): (S::Word, S::Word), object: S::Word, offset: S::Imm, miss: S::Miss) -> Option<S::Word> {
    let entry = stage.add_imm(entries, offset);
    let direct = super::super::value::is_direct_object(stage, object);
    stage.choose_leaving(direct, #[inline(always)] |stage| direct_read_in(stage, object, entry, miss), #[inline(always)] |stage| {
        let record = function_object(stage, functions, object, miss)?;
        record_read_in(stage, record, entry, miss)
    })
}

/// The object of the function `object` names, in the function record table `functions`
/// (`NATIVE_FUNCTION_LAYOUT.object_offset`). `None` (leaving at `miss`) for any other word.
#[inline(always)]
fn function_object<S: Stage>(stage: &mut S, functions: S::Word, object: S::Word, miss: S::Miss) -> Option<S::Word> {
    let function = super::super::value::is_direct_function(stage, object);
    stage.guard(function, miss)?;
    let present = stage.ne_imm(functions, 0u32);
    stage.guard(present, miss)?;
    let id = super::super::value::direct_object_id(stage, object);
    let offset = stage.mul_imm(id, super::super::layout_offsets::NATIVE_FUNCTION_LAYOUT.record_size as u64);
    let record = stage.add(functions, offset);
    Some(stage.add_imm(record, super::super::layout_offsets::NATIVE_FUNCTION_LAYOUT.object_offset as u64))
}

/// `site_read_in` of an own entry alone: `None` (leaving at `miss`) for an entry that names a
/// prototype holder, which the interpreter's own-field handlers (`Vm::primary_own_field_hit`) do
/// not answer, since they keep the receiver's slot as their hint.
#[inline(always)]
fn site_own_in<S: Stage>(stage: &mut S, entries: S::Word, object: S::Word, offset: S::Imm, miss: S::Miss) -> Option<S::Word> {
    let direct = super::super::value::is_direct_object(stage, object);
    stage.guard(direct, miss)?;
    let entry = stage.add_imm(entries, offset);
    let own = own_entry(stage, entry);
    stage.guard(own, miss)?;
    direct_own_value(stage, object, entry, miss)
}

/// `site_read` of an own entry alone (`site_own_in`).
#[inline(always)]
pub fn site_own_read<S: Stage>(stage: &mut S, entries: S::Word, object: S::Word, offset: S::Imm, miss: S::Miss) -> Option<S::Word> {
    site_tables(stage, entries, miss)?;
    site_own_in(stage, entries, object, offset, miss)
}

/// `site_chain` of own entries alone (`site_own_in`).
#[inline(always)]
pub fn site_own_chain<S: Stage>(stage: &mut S, entries: S::Word, object: S::Word, (first, second): (S::Imm, S::Imm), miss: S::Miss) -> Option<S::Word> {
    site_tables(stage, entries, miss)?;
    let middle = site_own_in(stage, entries, object, first, miss)?;
    site_own_in(stage, entries, middle, second, miss)
}

/// A field read through the cache entry `offset` bytes into the field cache table `entries` of the
/// object `object` names: the data value the entry holds (`site_read_in`), or `None` (leaving at `miss`)
/// where the table is absent too. The offset is the row's only per-site operand, so a site is read as
/// the interpreter's cached read reads it.
#[inline(always)]
pub fn site_read<S: Stage>(stage: &mut S, (entries, functions): (S::Word, S::Word), object: S::Word, offset: S::Imm, miss: S::Miss) -> Option<S::Word> {
    site_tables(stage, entries, miss)?;
    site_read_in(stage, (entries, functions), object, offset, miss)
}
