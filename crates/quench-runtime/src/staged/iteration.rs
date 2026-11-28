//! The fast arms of the key and iterator opcodes over a register frame: what a register's own
//! word proves, so a tier that holds it pays no call. Where the word does not prove it the
//! operation leaves at the miss edge, and the full operation (`Vm::value_step`,
//! `Vm::require_iterator_result`) decides: the interpreter runs the fast arm first on
//! `Concrete` (`dispatch_fast::lane_frame`), the b-jit runs it as a stencil row ahead of the
//! helper step that is its slow arm.

use super::region::Region;
use super::field_cache_layout::CLASS_KEY;
use super::{Concrete, Frame, Stage, Step, elements, value};
use crate::bytecode::Register;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;

/// Continue where register `register` holds an object, which `RequireIteratorResult` accepts: a
/// direct object or array record by its own word, else any other object kind the frame's
/// `object_like` answers (the VM's `is_object_like`). Any other value, a primitive, leaves at `miss`
/// and the general step throws.
#[inline(always)]
pub fn require_iterator_result<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, register: Register,
    _miss: S::Miss) -> Step<S::Word, F::Control> {
    let word = frame.tagged(stage, register);
    let object = value::is_direct_object(stage, word);
    let array = value::is_direct_array(stage, word);
    let record = stage.any(object, array);
    match stage.choose_leaving(record, |_| Some(word), #[inline(always)] |stage| frame.object_like(stage, word).map(|()| word)) {
        Some(word) => Step::Value(word),
        None => Step::Unresolved,
    }
}

/// `destination` defined with register `source`'s word where it is a direct string, which is
/// its own property key; any other value leaves at `miss`, where an integer, a symbol or an
/// object takes the full conversion.
#[inline(always)]
pub fn to_property_key<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    source: Register, miss: S::Miss) -> Step<S::Word, F::Control> {
    let word = frame.tagged(stage, source);
    let string = value::is_direct_string(stage, word);
    match stage.guard(string, miss) {
        Some(()) => Step::Value(frame.move_register(stage, destination, source)),
        None => Step::Unresolved,
    }
}

/// The elements of a for-in state (`Vm::for_in_state`), a private array the loop steps: the object
/// whose keys were listed, the snapshot array of those keys and the cursor, the int index of the
/// key the next step reads; last, the array of each snapshot key's slot in the listed shape (an
/// int, or `undefined` for a key no slot read answers: an index key, or any key of a shape with
/// accessors), or `undefined` where the keys came from the general walk. The memo of the snapshot's validity follows them as two raw words, which
/// read as Numbers to the collector: the shape the keys were listed for and the collection epoch of
/// the listing (`FOR_IN_NO_MEMO` where the keys came from the general walk, whose snapshot holds for
/// no shape). In that epoch (which no reused handle or compacted shape shares) any object of that
/// shape that keeps its keys in its shape alone, as a plain object does, has exactly the snapshot's
/// keys, each an own enumerable property.
pub const FOR_IN_OBJECT: usize = 0;
pub const FOR_IN_KEYS: usize = 1;
pub const FOR_IN_CURSOR: usize = 2;
pub const FOR_IN_SHAPE: usize = 3;
pub const FOR_IN_EPOCH: usize = 4;
pub const FOR_IN_SLOTS: usize = 5;
pub const FOR_IN_STATE_LEN: usize = 6;
/// The word of a state whose snapshot has no memo: no raw shape or epoch equals it, as it names a heap tag.
pub const FOR_IN_NO_MEMO: u64 = crate::value::TAG_UNDEFINED;

/// The word of element `index` of a for-in state whose first value `values` addresses.
#[inline(always)]
fn state_word<S: Stage>(stage: &mut S, values: S::Word, index: usize) -> S::Word {
    stage.load_u64(Region::Elements.mutable(), values, (index * std::mem::size_of::<crate::Value>()) as i32)
}

/// Whether the memo word `listed` (`FOR_IN_SHAPE`) is a live class cell's key (`Vm::own_receiver_key`): its
/// `CLASS_KEY` bit is set, which no record's shape carries; the empty memo word `FOR_IN_NO_MEMO` takes the
/// cell branch too and matches no class, so it leaves at its guard.
#[inline(always)]
fn listed_keys_class<S: Stage>(stage: &mut S, listed: S::Word) -> S::Cond {
    let bit = stage.and_imm(listed, u64::from(CLASS_KEY));
    stage.ne_imm(bit, 0u32)
}

/// Guards that the raw word `held` is `expected`.
#[inline(always)]
fn same_word<S: Stage>(stage: &mut S, held: S::Word, expected: S::Word, miss: S::Miss) -> Option<()> {
    let moved = stage.sub(held, expected);
    let same = stage.eq_imm(moved, 0u32);
    stage.guard(same, miss)
}

/// The backing of the private array `word` of a state (its snapshot, `FOR_IN_KEYS`, or once
/// `current_state` proved the memo, its slots, `FOR_IN_SLOTS`): only `Vm::for_in_state` stores
/// those words, always a direct array no script reaches, so only the record's encoding is
/// still a question (`elements::array_backing` without its value test).
#[inline(always)]
fn private_backing<S: Stage>(stage: &mut S, arrays: S::Word, word: S::Word, miss: S::Miss) -> Option<elements::Backing<S::Word>> {
    let record = elements::array_record(stage, arrays, word);
    let backing = elements::record_backing(stage, record);
    let encoded = stage.ugt_imm(backing.length, u64::from(!crate::heap::ENCODED_LEN));
    let wide = stage.not(encoded);
    stage.guard(wide, miss)?;
    Some(backing)
}

/// The state `state` of a `for-in` whose snapshot is whole and current: its collection epoch is
/// `epoch` and the object it lists still has the shape it listed, as a plain record. A step that
/// writes the cursor (`WRITE`) also proves the state's backing its own (`writable_backing`); a
/// read of the state needs no such proof.
#[inline(always)]
fn current_state<S: Stage, const WRITE: bool>(stage: &mut S, arrays: S::Word, state: S::Word, epoch: S::Word,
    miss: S::Miss) -> Option<elements::Backing<S::Word>> {
    let backing = elements::array_backing(stage, arrays, state, miss)?;
    if WRITE { elements::writable_backing(stage, backing, miss)?; }
    let length = stage.constant(FOR_IN_STATE_LEN as u64);
    let short = stage.ult(backing.length, length);
    let whole = stage.not(short);
    stage.guard(whole, miss)?;
    let remembered = state_word(stage, backing.values, FOR_IN_EPOCH);
    same_word(stage, remembered, epoch, miss)?;
    let object = state_word(stage, backing.values, FOR_IN_OBJECT);
    let listed = state_word(stage, backing.values, FOR_IN_SHAPE);
    let keyed = listed_keys_class(stage, listed);
    stage.choose_leaving(keyed, #[inline(always)] |stage| {
        // A live class cell: its class key is the listed key, which fixes its shape and link, so the one
        // compare validates the memo (no record hop, and no payload test: a class cell's fields are one run).
        let direct = value::is_direct_object(stage, object);
        stage.guard(direct, miss)?;
        let granule = value::direct_object_id(stage, object);
        let cell = super::object::cell_at_granule(stage, granule);
        let class = super::object::cell_class(stage, cell);
        let key = stage.add_imm(class, u64::from(CLASS_KEY));
        same_word(stage, key, listed, miss)?;
        Some(stage.constant(0))
    }, #[inline(always)] |stage| {
        super::guard::no_record(stage, miss)?;
        None
    })?;
    Some(backing)
}

/// The next key of the for-in `state` (a direct array among the array records at `arrays`) where its
/// memo proves the object, a direct plain object among the object records at `objects`, still has the
/// snapshot's keys: the state is a writable dense array of its own, the memo's epoch is `epoch` and its
/// shape the object's exact shape, and the cursor, an int word, names a key. The key is the answer and
/// the cursor moves past it (an int in place, `elements::replace_element`); `None` (leaving at `miss`,
/// before any write) otherwise, where the full step decides: any other object, a deleted or
/// non-enumerable key, and an exhausted snapshot, which jumps.
#[inline(always)]
pub fn for_in_next<S: Stage>(stage: &mut S, arrays: S::Word, state: S::Word, epoch: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let backing = current_state::<S, true>(stage, arrays, state, epoch, miss)?;
    let cursor = state_word(stage, backing.values, FOR_IN_CURSOR);
    let int_tag = stage.constant(crate::value::ValueLayout::INT_TAG);
    let index = stage.sub(cursor, int_tag);
    let wide = stage.ugt_imm(index, u64::from(u32::MAX));
    let narrow = stage.not(wide);
    stage.guard(narrow, miss)?;
    let keys = state_word(stage, backing.values, FOR_IN_KEYS);
    let snapshot = private_backing(stage, arrays, keys, miss)?;
    let (_, key) = elements::dense_element(stage, snapshot.values, snapshot.length, index, miss)?;
    let next = stage.add_imm(cursor, 1u32);
    let address = stage.add_imm(backing.values, (FOR_IN_CURSOR * std::mem::size_of::<crate::Value>()) as u64);
    elements::replace_element(stage, backing.values, address, cursor, next);
    Some(key)
}

#[cfg(test)]
mod tests;
#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
