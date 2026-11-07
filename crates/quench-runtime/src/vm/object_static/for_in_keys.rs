//! for-in's key walk: the key snapshot a loop takes (`Vm::for_in_start`), the state
//! `Op::ForInNext` steps through it (`Vm::for_in_next`), and the shape memo that lets a step
//! skip its key's lookup (`ForInOwnKeys`).

use super::*;

/// The key snapshot a for-in last took from a shape (`Vm::own_for_in_keys`): the internal
/// array of that shape's own enumerable keys, with the shape and the collection epoch. In
/// the same epoch (which no reused handle or compacted shape shares), any receiver with that
/// shape and no side descriptors has exactly these keys, each an own enumerable property, so
/// a later loop over such a receiver reuses the array, and a step of a loop walking it over
/// such a receiver needs no lookup (`Vm::for_in_snapshot_current`). No script reaches the
/// array, so loops share it.
pub(in crate::vm) struct ForInOwnKeys {
    shape: u32,
    epoch: u64,
    keys: Value,
}

impl ForInOwnKeys {
    /// The snapshot array, while it holds the keys of `shape` in `epoch`.
    fn keys(&self, shape: u32, epoch: u64) -> Option<Value> {
        (self.shape == shape && self.epoch == epoch).then_some(self.keys)
    }
}

/// A for-in state's elements: the (boxed) object, its key snapshot and the cursor, the
/// index of the snapshot key the next step reads.
const OBJECT: usize = 0;
const KEYS: usize = 1;
const CURSOR: usize = 2;

impl<H: Host> Vm<H> {
    /// The state a for-in over `source` steps (`Op::ForInNext`): its object and key snapshot,
    /// at the first key. `null` and `undefined` list no key.
    pub(in crate::vm) fn for_in_start(&mut self, p: &ResidualProgram, source: Value) -> Result<Value, JsError> {
        if source.is_null() || source.is_undefined() {
            let keys = self.internal_array(Vec::new());
            return Ok(self.internal_array(vec![Value::UNDEFINED, keys, Value::integer(0)]));
        }
        let object = self.box_object(source)?;
        let root = self.heap.root(object);
        let keys = self.object_for_in_keys(p, object);
        let object = self.heap.root_value(root).expect("rooted for-in object");
        self.heap.release_root(root);
        Ok(self.internal_array(vec![object, keys?, Value::integer(0)]))
    }

    /// The next key of the for-in `state` that is still an enumerable property of its
    /// object, past any deleted or made non-enumerable since the snapshot
    /// (EnumerateObjectProperties); `None` once the snapshot is exhausted.
    pub(in crate::vm) fn for_in_next(&mut self, p: &ResidualProgram, state: Value) -> Result<Option<Value>, JsError> {
        while let Some((object, keys, key, cursor)) = self.for_in_peek(state) {
            self.for_in_advance(state, cursor);
            if self.for_in_snapshot_current(object, keys) {
                return Ok(Some(key));
            }
            let root = self.heap.root(key);
            let enumerable = self.object_for_in_key_is_enumerable(p, object, key);
            let key = self.heap.root_value(root).expect("rooted for-in key");
            self.heap.release_root(root);
            if enumerable? {
                return Ok(Some(key));
            }
        }
        Ok(None)
    }

    /// The state's object, snapshot, key at the cursor and cursor; `None` past the last key.
    #[inline(always)]
    fn for_in_peek(&self, state: Value) -> Option<(Value, Value, Value, usize)> {
        let elements = self.heap.array(state)?.elements();
        let cursor = usize::try_from(elements.get(CURSOR)?.as_int()?).ok()?;
        let (object, keys) = (elements[OBJECT], elements[KEYS]);
        let key = *self.heap.array(keys)?.elements().get(cursor)?;
        Some((object, keys, key, cursor))
    }

    /// Moves the state's cursor past `cursor`.
    #[inline(always)]
    fn for_in_advance(&mut self, state: Value, cursor: usize) {
        let next = Value::integer(i32::try_from(cursor + 1).expect("for-in snapshot fits an index"));
        let array = self.heap.array_mut(state).expect("for-in state");
        if !array.replace_in_place(CURSOR, next) {
            array.elements_mut().make_mut().set(CURSOR, next);
        }
    }

    /// Whether `keys` is the snapshot of `object`'s current shape (`ForInOwnKeys`) while it
    /// stores no attributes beside it: then each of its keys is still an own enumerable
    /// property of `object`, as a step asks, without a lookup.
    #[inline(always)]
    fn for_in_snapshot_current(&self, object: Value, keys: Value) -> bool {
        let Some(listed) = self.for_in_own_keys.as_ref() else { return false };
        self.object_data(object)
            .filter(|data| !data.has_side_descriptors())
            .and_then(|data| listed.keys(data.shape(), self.heap.collection_epoch()))
            == Some(keys)
    }

    /// `shape_for_in_keys` of a receiver none of whose prototypes has an enumerable string
    /// key, as most have not (class methods are not enumerable): a prototype then neither
    /// adds a key nor shadows one the receiver lists first, so the keys are the receiver's
    /// own enumerable ones, read from the shape memo (`shape_enumerable_slots`) and
    /// snapshotted once per shape (`ForInOwnKeys`). `None` leaves any other chain to the walk.
    pub(super) fn own_for_in_keys(&mut self, receiver: Value) -> Option<Value> {
        let slots = self.shape_enumerable_slots(receiver)?;
        let mut prototype = self.object_data(receiver)?.proto();
        while !prototype.is_null() {
            if !self.shape_enumerable_slots(prototype)?.is_empty() {
                return None;
            }
            prototype = self.object_data(prototype)?.proto();
        }
        let (shape, epoch) = (self.object_data(receiver)?.shape(), self.heap.collection_epoch());
        if let Some(keys) = self.for_in_own_keys.as_ref().and_then(|listed| listed.keys(shape, epoch)) {
            return Some(keys);
        }
        let keys: Vec<Value> = slots.iter().map(|&(atom, _)| self.atom_string(atom)).collect();
        let keys = self.internal_array(keys);
        self.for_in_own_keys = Some(ForInOwnKeys { shape, epoch, keys });
        Some(keys)
    }

    /// An array of `elements` that only the runtime holds.
    pub(super) fn internal_array(&mut self, elements: Vec<Value>) -> Value {
        self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(Self::empty_object(self.array_proto), elements),
        })
    }
}
