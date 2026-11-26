//! The heap frame of a stencil's operand places: the element callouts (`ElementFrame`) and the
//! array operations' frame answers (`Frame::direct_array`, `array_length`, `typed_length`,
//! `array_append`) over `Places`. The dense rows read the array records the dispatch payload holds
//! and prove each access's array (`elements::prove_view`); a frame keeps no view, so nothing is
//! cached between rows. An operation the places cannot decide leaves at the row's miss edge.
//!
//! The four `Frame` answers are inherent methods here (`heap_*`), because `Frame<Concrete> for
//! Places` is the one impl in `places.rs`; that impl delegates to them (see the report).

use crate::bytecode::{Operand, Register};
use crate::heap_rows::{payload, stage_site, word_at};
use crate::layout::{JS_STEP_ARRAY_CHAIN_INDEX_FREE_ADDRESS, JS_STEP_ARRAY_MARK_BLOCKS, JS_STEP_ARRAY_MARK_BLOCK_COUNT, JS_STEP_ARRAY_RECORD_BASE, JS_STEP_BUFFER_RELOCATIONS, JS_STEP_TYPED_VIEW_DIRECTORY, JS_STEP_TYPED_VIEW_PAGES, JS_STEP_LOOP_ENTRY, JS_STEP_STORE_WATCH, LOOP_ENTRY_ARRAY_COUNT_AT, STORE_WATCH_COUNTS};
use crate::numeric::{HOLE_A, HOLE_B};
use crate::places::on_place;
use crate::typed_ir::Representation;
use crate::places::{LEFT, RIGHT, Places, index_of};
use crate::staged::{Concrete, ElementFrame, Frame, elements, entry, value};
use crate::staged::barrier::{counted_store_unlogged, MarkList};
use crate::staged::element_rows::ElementView;
use crate::staged::typed_view::{TypedRecords, ViewAccess, prove_typed_view};
use crate::words::{int_word, is_double, is_int};

/// The table of array records the dispatch payload holds, where one is reserved.
#[inline(always)]
fn record_table(payload_base: usize) -> Option<u64> {
    let records = word_at(payload_base, JS_STEP_ARRAY_RECORD_BASE);
    (records != 0).then_some(records)
}

impl ElementFrame<Concrete> for Places {
    /// The array record table the activation's dispatch payload holds.
    #[inline(always)]
    fn array_records(&mut self, _stage: &mut Concrete) -> Option<u64> {
        record_table(payload(&self.s))
    }

    /// The int32 index of the key register: the place's int32 (`places::index_of`), as its low
    /// half; a key that is no int32 leaves at the miss edge.
    #[inline(always)]
    fn index_key(&mut self, _stage: &mut Concrete, key: Operand, _miss: ()) -> Option<u64> {
        let place = self.place(key.register_index()?);
        let index = on_place!(place, index_of(&self.s))?;
        Some(u64::from(index as u32))
    }

    /// The direct array's write barrier admits `value` unlogged (`barrier::counted_store_unlogged`
    /// over the array records' mark list): the rule of the interpreter's `Heap::array_store_unlogged`
    /// and the u-jit's `store_gate`. Any other store is the VM's.
    #[inline(always)]
    fn array_store_gate(&mut self, stage: &mut Concrete, base: u64, value: u64, miss: ()) -> Option<()> {
        let list = MarkList { blocks: JS_STEP_ARRAY_MARK_BLOCKS, count: JS_STEP_ARRAY_MARK_BLOCK_COUNT };
        counted_store_unlogged(stage, payload(&self.s) as u64, (JS_STEP_STORE_WATCH, STORE_WATCH_COUNTS), list,
            base, value, miss)
    }

    /// The VM's store where no replacement answers it, as far as a row takes it: a hole of a direct
    /// array filled in place (`elements::fill_hole`) while the held index-free chain proof covers the
    /// array's prototype and the write barrier admits `value` unlogged. Any other store leaves.
    #[inline(always)]
    fn index_set_host(&mut self, stage: &mut Concrete, base: u64, key: u64, value: u64, miss: ()) -> Option<()> {
        value::is_direct_array(stage, base).then_some(())?;
        self.array_store_gate(stage, base, value, miss)?;
        let payload = payload(&self.s);
        let records = record_table(payload)?;
        let chain = word_at(word_at(payload, JS_STEP_ARRAY_CHAIN_INDEX_FREE_ADDRESS) as usize, 0);
        elements::fill_hole(stage, records, chain, (base, key, value), miss)
    }

    /// A typed array's element reads deliver the element's Value word (`typed_read_value`), as the rows' places hold.
    #[inline(always)]
    fn typed_values(&self) -> bool { true }

    /// A typed array this access proves the view of (`prove_typed_view`, any native kind, read from the payload's
    /// view records): the same element row then reads or stores its element in place. A direct array, or a
    /// value no view proof admits, keeps no view: the dense proof takes it.
    #[inline(always)]
    fn kept_view(&mut self, stage: &mut Concrete, base: u64, writable: bool) -> Option<ElementView<u64>> {
        if value::is_direct_array(stage, base) { return None; }
        let payload = payload(&self.s);
        let records = TypedRecords { directory: word_at(payload, JS_STEP_TYPED_VIEW_DIRECTORY), pages: word_at(payload, JS_STEP_TYPED_VIEW_PAGES),
            relocations: word_at(payload, JS_STEP_BUFFER_RELOCATIONS) };
        (records.directory != 0).then_some(())?;
        prove_typed_view(stage, records, base, ViewAccess { kind: None, writable }, ()).map(ElementView::Typed)
    }
}

impl Places {
    /// `Frame::direct_array`: whether the register `source` holds a direct array.
    #[inline(always)]
    pub(crate) fn heap_direct_array(&mut self, stage: &mut Concrete, source: Register) -> Option<()> {
        let base = self.tagged(stage, source);
        value::is_direct_array(stage, base).then_some(())
    }

    /// `Frame::array_length`: the length of the direct array in `source`, as its int32 word, where
    /// it has a plain dense backing (`elements::array_length`).
    #[inline(always)]
    pub(crate) fn heap_array_length(&mut self, stage: &mut Concrete, source: Register) -> Option<(u64, Representation)> {
        let records = record_table(payload(&self.s))?;
        let base = self.tagged(stage, source);
        let length = elements::array_length(stage, records, base, ())?;
        Some((length, Representation::Tagged))
    }

    /// `Frame::typed_length`: the count of the typed array the loop's entry proved, whose entry
    /// words lie at the site the row's stage names (hole 1 for the `TypedLength` row), as an int32.
    /// The entry proves the register's array, so `source` is not read.
    #[inline(always)]
    pub(crate) fn heap_typed_length(&mut self, stage: &mut Concrete, _source: Register) -> Option<(u64, Representation)> {
        let offset = stage_site(stage);
        let record = word_at(payload(&self.s), JS_STEP_LOOP_ENTRY);
        let length = entry::entry_word(stage, record, offset + LOOP_ENTRY_ARRAY_COUNT_AT);
        Some((int_word(length as u32 as i32), Representation::Tagged))
    }

    /// `Frame::array_append`: appends the Number `value` to the array in `array` where its backing
    /// is writable and its capacity has room (`elements::append_in_room`). A heap value, which the
    /// barrier logs, is the VM's append.
    #[inline(always)]
    pub(crate) fn heap_array_append(&mut self, stage: &mut Concrete, array: Register, value: u64) -> Option<()> {
        if !(is_int(value) || is_double(value)) { return None; }
        let records = record_table(payload(&self.s))?;
        let base = self.tagged(stage, array);
        let backing = elements::array_backing(stage, records, base, ())?;
        elements::writable_backing(stage, backing, ())?;
        elements::append_in_room(stage, backing, value, ())?;
        Some(())
    }
}
