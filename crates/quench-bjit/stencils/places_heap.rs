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
use crate::layout::{JS_STEP_ARRAY_RECORD_BASE, JS_STEP_LOOP_ENTRY, JS_STEP_STORE_WATCH, LOOP_ENTRY_ARRAY_COUNT_AT, STORE_WATCH_COUNTS};
use crate::numeric::{HOLE_A, HOLE_B};
use crate::typed_ir::Representation;
use crate::places::{LEFT, RIGHT, Places, index_of};
use crate::staged::{Concrete, ElementFrame, Frame, elements, entry, value};
use crate::words::{int_word, is_double, is_int};

/// The table of array records the dispatch payload holds, where one is reserved.
#[inline(always)]
fn record_table(payload_base: usize) -> Option<u64> {
    let records = word_at(payload_base, JS_STEP_ARRAY_RECORD_BASE);
    (records != 0).then_some(records)
}

impl<const D: u8, const A: u8, const B: u8> ElementFrame<Concrete> for Places<D, A, B> {
    /// The array record table the activation's dispatch payload holds.
    #[inline(always)]
    fn array_records(&mut self, _stage: &mut Concrete) -> Option<u64> {
        record_table(payload(&self.s))
    }

    /// The int32 index of the key register: the place's int32 (`places::index_of`), as its low
    /// half; a key that is no int32 leaves at the miss edge.
    #[inline(always)]
    fn index_key(&mut self, _stage: &mut Concrete, key: Operand, _miss: ()) -> Option<u64> {
        let index = match key.register_index()? {
            LEFT => index_of::<A, HOLE_A>(&self.s),
            RIGHT => index_of::<B, HOLE_B>(&self.s),
            _ => index_of::<D, 0>(&self.s),
        }?;
        Some(u64::from(index as u32))
    }

    /// A store's value needs no barrier log where it is a Number: an int word or a double, while
    /// old records keep no counts (`Stage::immediate_store_open`, read from the payload's gate word).
    /// Any other value (a heap value, which the barrier logs) is the VM's store.
    #[inline(always)]
    fn array_store_gate(&mut self, _stage: &mut Concrete, _base: u64, value: u64, _miss: ()) -> Option<()> {
        let uncounted = word_at(payload(&self.s), JS_STEP_STORE_WATCH) & STORE_WATCH_COUNTS == 0;
        (uncounted && (is_int(value) || is_double(value))).then_some(())
    }
}

impl<const D: u8, const A: u8, const B: u8> Places<D, A, B> {
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
