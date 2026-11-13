//! The fast arms of the key and iterator opcodes over a register frame: what a register's own
//! word proves, so a tier that holds it pays no call. Where the word does not prove it the
//! operation leaves at the miss edge, and the full operation (`Vm::value_step`,
//! `Vm::require_iterator_result`) decides: the interpreter runs the fast arm first on
//! `Concrete` (`dispatch_fast::lane_frame`), the b-jit runs it as a stencil row ahead of the
//! helper step that is its slow arm.

use super::{Frame, Stage, Step, value};
use crate::bytecode::Register;

/// Continue where register `register` holds a direct object or array record, which is an
/// object and so an iterator result `RequireIteratorResult` accepts; any other value leaves at
/// `miss`, where another object kind passes and a primitive throws.
#[inline(always)]
pub(crate) fn require_iterator_result<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, register: Register,
    miss: S::Miss) -> Step<S::Word, F::Control> {
    let word = frame.tagged(stage, register);
    let object = value::is_direct_object(stage, word);
    let array = value::is_direct_array(stage, word);
    let record = stage.any(object, array);
    match stage.guard(record, miss) {
        Some(()) => Step::Value(word),
        None => Step::Unresolved,
    }
}

/// `destination` defined with register `source`'s word where it is a direct string, which is
/// its own property key; any other value leaves at `miss`, where an integer, a symbol or an
/// object takes the full conversion.
#[inline(always)]
pub(crate) fn to_property_key<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    source: Register, miss: S::Miss) -> Step<S::Word, F::Control> {
    let word = frame.tagged(stage, source);
    let string = value::is_direct_string(stage, word);
    match stage.guard(string, miss) {
        Some(()) => Step::Value(frame.move_register(stage, destination, source)),
        None => Step::Unresolved,
    }
}
//! The fast arms of the key and iterator opcodes over a register frame: what a register's own
//! word proves, so a tier that holds it pays no call. Where the word does not prove it the
//! operation leaves at the miss edge, and the full operation (`Vm::value_step`,
//! `Vm::require_iterator_result`) decides: the interpreter runs the fast arm first on
//! `Concrete` (`dispatch_fast::lane_frame`), the b-jit runs it as a stencil row ahead of the
//! helper step that is its slow arm.

use super::{Concrete, Frame, Stage, Step, elements, entry, value};
use crate::bytecode::Register;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;

/// Continue where register `register` holds a direct object or array record, which is an
/// object and so an iterator result `RequireIteratorResult` accepts; any other value leaves at
/// `miss`, where another object kind passes and a primitive throws.
#[inline(always)]
pub(crate) fn require_iterator_result<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, register: Register,
    miss: S::Miss) -> Step<S::Word, F::Control> {
    let word = frame.tagged(stage, register);
    let object = value::is_direct_object(stage, word);
    let array = value::is_direct_array(stage, word);
    let record = stage.any(object, array);
    match stage.guard(record, miss) {
        Some(()) => Step::Value(word),
        None => Step::Unresolved,
    }
}

/// `destination` defined with register `source`'s word where it is a direct string, which is
/// its own property key; any other value leaves at `miss`, where an integer, a symbol or an
/// object takes the full conversion.
#[inline(always)]
pub(crate) fn to_property_key<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register,
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
/// key the next step reads. The memo of the snapshot's validity follows them as two raw words, which
/// read as Numbers to the collector: the shape the keys were listed for and the collection epoch of
/// the listing (`FOR_IN_NO_MEMO` where the keys came from the general walk, whose snapshot holds for
/// no shape). In that epoch (which no reused handle or compacted shape shares) any object of that
/// shape that keeps its keys in its shape alone, as a plain object does, has exactly the snapshot's
/// keys, each an own enumerable property.
pub(crate) const FOR_IN_OBJECT: usize = 0;
pub(crate) const FOR_IN_KEYS: usize = 1;
pub(crate) const FOR_IN_CURSOR: usize = 2;
pub(crate) const FOR_IN_SHAPE: usize = 3;
pub(crate) const FOR_IN_EPOCH: usize = 4;
pub(crate) const FOR_IN_STATE_LEN: usize = 5;
/// The word of a state whose snapshot has no memo: no raw shape or epoch equals it, as it names a heap tag.
pub(crate) const FOR_IN_NO_MEMO: u64 = crate::value::TAG_UNDEFINED;

/// The word of element `index` of a for-in state whose first value `values` addresses.
#[inline(always)]
fn state_word<S: Stage>(stage: &mut S, values: S::Word, index: usize) -> S::Word {
    stage.load_u64(values, (index * std::mem::size_of::<crate::Value>()) as i32)
}

/// Guards that the raw word `held` is `expected`.
#[inline(always)]
fn same_word<S: Stage>(stage: &mut S, held: S::Word, expected: S::Word, miss: S::Miss) -> Option<()> {
    let moved = stage.sub(held, expected);
    let same = stage.eq_imm(moved, 0u32);
    stage.guard(same, miss)
}

/// The next key of the for-in `state` (a direct array among the array records at `arrays`) where its
/// memo proves the object, a direct plain object among the object records at `objects`, still has the
/// snapshot's keys: the state is a writable dense array of its own, the memo's epoch is `epoch` and its
/// shape the object's exact shape, and the cursor, an int word, names a key. The key is the answer and
/// the cursor moves past it (an int in place, `elements::replace_element`); `None` (leaving at `miss`,
/// before any write) otherwise, where the full step decides: any other object, a deleted or
/// non-enumerable key, and an exhausted snapshot, which jumps.
#[inline(always)]
pub(crate) fn for_in_next<S: Stage>(stage: &mut S, (arrays, objects): (S::Word, S::Word), state: S::Word, epoch: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let backing = elements::array_backing(stage, arrays, state, miss)?;
    elements::writable_backing(stage, backing, miss)?;
    let length = stage.constant(FOR_IN_STATE_LEN as u64);
    let short = stage.ult(backing.length, length);
    let whole = stage.not(short);
    stage.guard(whole, miss)?;
    let remembered = state_word(stage, backing.values, FOR_IN_EPOCH);
    same_word(stage, remembered, epoch, miss)?;
    let present = stage.ne_imm(objects, 0u32);
    stage.guard(present, miss)?;
    let object = state_word(stage, backing.values, FOR_IN_OBJECT);
    let (_, record) = entry::direct_record(stage, objects, object, miss)?;
    let exact = stage.load_u32(record, OBJECT.auxiliary_offset as i32);
    let listed = state_word(stage, backing.values, FOR_IN_SHAPE);
    same_word(stage, exact, listed, miss)?;
    let payload = stage.load_u64(record, OBJECT.payload_offset as i32);
    let plain = super::object::plain_payload(stage, payload);
    stage.guard(plain, miss)?;
    let cursor = state_word(stage, backing.values, FOR_IN_CURSOR);
    let int_tag = stage.constant(crate::value::ValueLayout::INT_TAG);
    let index = stage.sub(cursor, int_tag);
    let wide = stage.ugt_imm(index, u64::from(u32::MAX));
    let narrow = stage.not(wide);
    stage.guard(narrow, miss)?;
    let keys = state_word(stage, backing.values, FOR_IN_KEYS);
    let snapshot = elements::array_backing(stage, arrays, keys, miss)?;
    let (_, key) = elements::dense_element(stage, snapshot.values, snapshot.length, index, miss)?;
    let next = stage.add_imm(cursor, 1u32);
    let address = stage.add_imm(backing.values, (FOR_IN_CURSOR * std::mem::size_of::<crate::Value>()) as u64);
    elements::replace_element(stage, backing.values, address, cursor, next);
    Some(key)
}

/// The interpreter's reading of `for_in_next`: the key the step over `state` yields, among the array
/// records at `arrays` and object records at `objects`, in collection epoch `epoch`; `None` leaves the
/// step to the VM's full one, which also advances nothing the row did not.
#[inline(always)]
pub(crate) fn concrete_for_in_next(arrays: *const crate::heap::ArrayObject, objects: *const crate::heap::Object, state: crate::Value,
    epoch: u64) -> Option<crate::Value> {
    let mut stage = Concrete::new(std::ptr::null());
    for_in_next(&mut stage, (arrays as u64, objects as u64), state.to_bits(), epoch, ()).map(crate::Value::from_bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{ArrayElements, ArrayObject, Cell, Heap, Object};
    use crate::Value;

    /// A heap with a plain object of two properties (shapes 1 and 2) and the snapshot array of its two
    /// keys; the object's exact shape is the first answer.
    fn listed() -> (Heap, Value, Value, u32) {
        let mut heap = Heap::new();
        for shape in 1..=2 {
            heap.register_property_shape(shape, shape as usize, None);
        }
        let object = heap.alloc(Cell::Object(Object::new(Value::NULL)));
        heap.property_add(object, Value::integer(1), 1);
        heap.property_add(object, Value::integer(2), 2);
        let shape = heap.object(object).expect("an object").exact_shape();
        let snapshot = ArrayElements::from_slice(&[Value::integer(10), Value::integer(20)]);
        let keys = heap.alloc(Cell::Array { object: ArrayObject::fresh(Value::NULL, snapshot) });
        (heap, object, keys, shape)
    }

    fn state(heap: &mut Heap, (object, keys): (Value, Value), memo: [u64; 2]) -> Value {
        let elements = [object, keys, Value::integer(0), Value::from_bits(memo[0]), Value::from_bits(memo[1])];
        heap.alloc(Cell::Array { object: ArrayObject::fresh(Value::NULL, ArrayElements::from_slice(&elements)) })
    }

    fn step(heap: &Heap, state: Value, epoch: u64) -> Option<Value> {
        concrete_for_in_next(heap.array_record_base().expect("array records"), heap.object_record_base().expect("object records"), state, epoch)
    }

    fn cursor(heap: &Heap, state: Value) -> Value {
        heap.array(state).expect("a state").elements()[FOR_IN_CURSOR]
    }

    /// A state whose memo is the object's shape and the current epoch yields its keys in order and moves
    /// its cursor; at the end of the snapshot the row leaves for the full step, which jumps.
    #[test]
    fn a_state_whose_memo_holds_steps_through_its_keys() {
        let (mut heap, object, keys, shape) = listed();
        let epoch = 7;
        let state = state(&mut heap, (object, keys), [u64::from(shape), epoch]);
        assert_eq!(step(&heap, state, epoch), Some(Value::integer(10)));
        assert_eq!(cursor(&heap, state), Value::integer(1));
        assert_eq!(step(&heap, state, epoch), Some(Value::integer(20)));
        assert_eq!(step(&heap, state, epoch), None, "the snapshot is exhausted");
        assert_eq!(cursor(&heap, state), Value::integer(2), "a leave moves nothing");
    }

    /// A state whose memo is another epoch or shape, or none, or whose object has changed shape, leaves
    /// before its cursor moves.
    #[test]
    fn a_state_whose_memo_does_not_hold_leaves_before_writing() {
        let (mut heap, object, keys, shape) = listed();
        let epoch = 7;
        let held = (object, keys);
        let stale = state(&mut heap, held, [u64::from(shape), epoch]);
        assert_eq!(step(&heap, stale, epoch + 1), None, "a collection passed");
        let unmemoed = state(&mut heap, held, [FOR_IN_NO_MEMO; 2]);
        assert_eq!(step(&heap, unmemoed, epoch), None, "the keys came from the general walk");
        let other_shape = state(&mut heap, held, [u64::from(shape) + 1, epoch]);
        assert_eq!(step(&heap, other_shape, epoch), None, "the object is not of the listed shape");
        heap.object_mut(object).expect("an object").mark_as_prototype();
        let prototype = state(&mut heap, held, [u64::from(shape), epoch]);
        assert_eq!(step(&heap, prototype, epoch), None, "an object with extras keeps keys outside its shape");
        for state in [stale, unmemoed, other_shape, prototype] {
            assert_eq!(cursor(&heap, state), Value::integer(0));
        }
    }
}
